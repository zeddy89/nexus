// SSH connection management with pooling

use async_trait::async_trait;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use dashmap::DashMap;
use ssh2::{KeyboardInteractivePrompt, Session};

use super::Connection;
use crate::inventory::Host;
use crate::output::errors::NexusError;

/// Type of connection to use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    Ssh,
    Local,
}

/// SSH connection pool for reusing connections
pub struct ConnectionPool {
    connections: DashMap<String, Vec<PooledConnection>>,
    max_per_host: usize,
    connect_timeout: Duration,
    command_timeout: Duration,
    default_user: Option<String>,
    private_key_path: Option<String>,
    password: Option<String>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        ConnectionPool {
            connections: DashMap::new(),
            max_per_host: 5,
            connect_timeout: Duration::from_secs(30),
            command_timeout: Duration::from_secs(300),
            default_user: None,
            private_key_path: None,
            password: None,
        }
    }

    pub fn with_max_per_host(mut self, max: usize) -> Self {
        self.max_per_host = max;
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    pub fn with_default_user(mut self, user: String) -> Self {
        self.default_user = Some(user);
        self
    }

    pub fn with_private_key(mut self, path: String) -> Self {
        self.private_key_path = Some(path);
        self
    }

    pub fn with_password(mut self, password: String) -> Self {
        self.password = Some(password);
        self
    }

    /// Get a connection to a host (from pool or create new)
    /// Note: This will not be used for local hosts - use get_any_connection instead
    pub fn get(&self, host: &Host) -> Result<SshConnection, NexusError> {
        let key = host.ssh_target();

        // Try to get from pool
        if let Some(mut conns) = self.connections.get_mut(&key) {
            while let Some(conn) = conns.pop() {
                if conn.is_valid() {
                    return Ok(SshConnection { inner: conn });
                }
            }
        }

        // Create new connection
        let conn = self.connect(host)?;
        Ok(SshConnection { inner: conn })
    }

    /// Get the appropriate connection type for a host (SSH or local)
    pub fn get_connection_type(&self, host: &Host) -> ConnectionType {
        if host.is_local() {
            ConnectionType::Local
        } else {
            ConnectionType::Ssh
        }
    }

    /// Return a connection to the pool
    pub fn return_connection(&self, conn: PooledConnection, key: String) {
        if !conn.is_valid() {
            return;
        }

        let mut conns = self.connections.entry(key).or_default();
        if conns.len() < self.max_per_host {
            conns.push(conn);
        }
    }

    /// Create a new SSH connection
    fn connect(&self, host: &Host) -> Result<PooledConnection, NexusError> {
        let address = format!("{}:{}", host.address, host.port);

        // TCP connection with timeout
        let tcp = TcpStream::connect_timeout(
            &address.parse().map_err(|e| NexusError::Ssh {
                host: host.name.clone(),
                message: format!("Invalid address: {}", e),
                suggestion: Some("Check the host address format".to_string()),
            })?,
            self.connect_timeout,
        )
        .map_err(|e| NexusError::Ssh {
            host: host.name.clone(),
            message: format!("Connection failed: {}", e),
            suggestion: ssh_connection_suggestion(&e),
        })?;

        // SSH session
        let mut session = Session::new().map_err(|e| NexusError::Ssh {
            host: host.name.clone(),
            message: format!("Failed to create SSH session: {}", e),
            suggestion: None,
        })?;

        session.set_tcp_stream(tcp);
        session.set_timeout(self.connect_timeout.as_millis() as u32);

        session.handshake().map_err(|e| NexusError::Ssh {
            host: host.name.clone(),
            message: format!("SSH handshake failed: {}", e),
            suggestion: Some("Check SSH service is running on the target".to_string()),
        })?;

        // Authentication
        let user = if host.user.is_empty() {
            self.default_user
                .clone()
                .or_else(|| std::env::var("USER").ok())
                .unwrap_or_else(|| "root".to_string())
        } else {
            host.user.clone()
        };

        // Try SSH agent first
        let mut authenticated = false;

        if let Ok(mut agent) = session.agent() {
            if agent.connect().is_ok() {
                agent.list_identities().ok();
                for identity in agent.identities().unwrap_or_default() {
                    if agent.userauth(&user, &identity).is_ok() {
                        authenticated = true;
                        break;
                    }
                }
            }
        }

        // Try private key file
        if !authenticated {
            let key_paths = self
                .private_key_path
                .iter()
                .map(|p| p.to_string())
                .chain(
                    [
                        dirs::home_dir()
                            .map(|h| h.join(".ssh/id_ed25519").to_string_lossy().to_string()),
                        dirs::home_dir()
                            .map(|h| h.join(".ssh/id_rsa").to_string_lossy().to_string()),
                    ]
                    .into_iter()
                    .flatten(),
                )
                .collect::<Vec<_>>();

            for key_path in key_paths {
                if Path::new(&key_path).exists()
                    && session
                        .userauth_pubkey_file(&user, None, Path::new(&key_path), None)
                        .is_ok()
                {
                    authenticated = true;
                    break;
                }
            }
        }

        // Try password authentication
        if !authenticated {
            if let Some(ref password) = self.password {
                // First try standard password auth
                if session.userauth_password(&user, password).is_ok() {
                    authenticated = true;
                } else {
                    // Fall back to keyboard-interactive auth (used by some PAM configurations)
                    let mut prompter = PasswordPrompter(password.clone());
                    if session
                        .userauth_keyboard_interactive(&user, &mut prompter)
                        .is_ok()
                    {
                        authenticated = true;
                    }
                }
            }
        }

        if !authenticated {
            return Err(NexusError::Ssh {
                host: host.name.clone(),
                message: "Authentication failed".to_string(),
                suggestion: Some(
                    "Ensure SSH key is added to agent, specify --private-key, or use --ask-pass for password auth".to_string(),
                ),
            });
        }

        Ok(PooledConnection {
            session,
            host_name: host.name.clone(),
        })
    }

    /// Close all connections
    pub fn close_all(&self) {
        self.connections.clear();
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

/// A pooled SSH connection
pub struct PooledConnection {
    session: Session,
    host_name: String,
}

impl PooledConnection {
    /// Check if the connection is still valid
    pub fn is_valid(&self) -> bool {
        self.session.authenticated()
    }

    /// Execute a command on this connection
    pub fn exec(&self, command: &str) -> Result<CommandResult, NexusError> {
        let mut channel = self.session.channel_session().map_err(|e| {
            // CRITICAL BUG FIX: Detect timeout and connection errors
            // These errors mean the connection is bad and should not be reused
            let is_connection_error = e.to_string().contains("timeout")
                || e.to_string().contains("Connection")
                || e.to_string().contains("Broken pipe");

            NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!(
                    "Failed to open channel{}: {}",
                    if is_connection_error {
                        " (connection error)"
                    } else {
                        ""
                    },
                    e
                ),
                suggestion: if is_connection_error {
                    Some("Connection will be discarded due to error".to_string())
                } else {
                    None
                },
            }
        })?;

        channel.exec(command).map_err(|e| {
            let is_timeout = e.to_string().contains("timeout");
            NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!(
                    "Failed to execute command{}: {}",
                    if is_timeout { " (timeout)" } else { "" },
                    e
                ),
                suggestion: if is_timeout {
                    Some("Command timed out. Connection will be discarded.".to_string())
                } else {
                    None
                },
            }
        })?;

        let mut stdout = String::new();
        let mut stderr = String::new();

        channel.read_to_string(&mut stdout).ok();
        channel.stderr().read_to_string(&mut stderr).ok();

        channel.wait_close().ok();
        let exit_code = channel.exit_status().unwrap_or(-1);

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Execute a command with streaming output
    pub fn exec_streaming<F, G>(
        &self,
        command: &str,
        mut on_stdout: F,
        mut on_stderr: G,
    ) -> Result<i32, NexusError>
    where
        F: FnMut(&[u8]),
        G: FnMut(&[u8]),
    {
        let mut channel = self
            .session
            .channel_session()
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to open channel: {}", e),
                suggestion: None,
            })?;

        channel.exec(command).map_err(|e| NexusError::Ssh {
            host: self.host_name.clone(),
            message: format!("Failed to execute command: {}", e),
            suggestion: None,
        })?;

        // Set non-blocking
        self.session.set_blocking(false);

        let mut stdout_buf = [0u8; 4096];
        let mut stderr_buf = [0u8; 4096];

        loop {
            let mut activity = false;

            // Read stdout
            match channel.read(&mut stdout_buf) {
                Ok(0) => {}
                Ok(n) => {
                    on_stdout(&stdout_buf[..n]);
                    activity = true;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => break,
            }

            // Read stderr
            match channel.stderr().read(&mut stderr_buf) {
                Ok(0) => {}
                Ok(n) => {
                    on_stderr(&stderr_buf[..n]);
                    activity = true;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => break,
            }

            // Check if channel is done
            if channel.eof() {
                break;
            }

            if !activity {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        self.session.set_blocking(true);
        channel.wait_close().ok();
        Ok(channel.exit_status().unwrap_or(-1))
    }

    /// Upload a file via SFTP
    pub fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<(), NexusError> {
        let sftp = self.session.sftp().map_err(|e| NexusError::Ssh {
            host: self.host_name.clone(),
            message: format!("Failed to open SFTP: {}", e),
            suggestion: None,
        })?;

        let content = std::fs::read(local_path).map_err(|e| NexusError::Io {
            message: format!("Failed to read local file: {}", e),
            path: Some(local_path.to_path_buf()),
        })?;

        let mut remote_file = sftp
            .create(Path::new(remote_path))
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to create remote file: {}", e),
                suggestion: None,
            })?;

        remote_file
            .write_all(&content)
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to write remote file: {}", e),
                suggestion: None,
            })?;

        Ok(())
    }

    /// Write content to a remote file
    pub fn write_file(&self, remote_path: &str, content: &[u8]) -> Result<(), NexusError> {
        let sftp = self.session.sftp().map_err(|e| NexusError::Ssh {
            host: self.host_name.clone(),
            message: format!("Failed to open SFTP: {}", e),
            suggestion: None,
        })?;

        let mut remote_file = sftp
            .create(Path::new(remote_path))
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to create remote file: {}", e),
                suggestion: None,
            })?;

        remote_file
            .write_all(content)
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to write remote file: {}", e),
                suggestion: None,
            })?;

        Ok(())
    }

    /// Read a remote file
    pub fn read_file(&self, remote_path: &str) -> Result<Vec<u8>, NexusError> {
        let sftp = self.session.sftp().map_err(|e| NexusError::Ssh {
            host: self.host_name.clone(),
            message: format!("Failed to open SFTP: {}", e),
            suggestion: None,
        })?;

        let mut remote_file = sftp
            .open(Path::new(remote_path))
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to open remote file: {}", e),
                suggestion: None,
            })?;

        let mut content = Vec::new();
        remote_file
            .read_to_end(&mut content)
            .map_err(|e| NexusError::Ssh {
                host: self.host_name.clone(),
                message: format!("Failed to read remote file: {}", e),
                suggestion: None,
            })?;

        Ok(content)
    }
}

/// RAII wrapper for pooled connections
pub struct SshConnection {
    inner: PooledConnection,
}

impl SshConnection {
    pub fn exec(&self, command: &str) -> Result<CommandResult, NexusError> {
        self.inner.exec(command)
    }

    pub fn exec_streaming<F, G>(
        &self,
        command: &str,
        on_stdout: F,
        on_stderr: G,
    ) -> Result<i32, NexusError>
    where
        F: FnMut(&[u8]),
        G: FnMut(&[u8]),
    {
        self.inner.exec_streaming(command, on_stdout, on_stderr)
    }

    pub fn upload_file(&self, local: &Path, remote: &str) -> Result<(), NexusError> {
        self.inner.upload_file(local, remote)
    }

    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<(), NexusError> {
        self.inner.write_file(path, content)
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, NexusError> {
        self.inner.read_file(path)
    }

    pub fn host_name(&self) -> &str {
        &self.inner.host_name
    }
}

/// Result of executing a command
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

fn ssh_connection_suggestion(e: &std::io::Error) -> Option<String> {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            Some("Ensure SSH service is running on the target host".to_string())
        }
        std::io::ErrorKind::TimedOut => {
            Some("Check network connectivity and firewall rules".to_string())
        }
        std::io::ErrorKind::PermissionDenied => {
            Some("Check SSH key permissions and authentication".to_string())
        }
        _ => None,
    }
}

/// Simple home directory lookup
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// Helper for keyboard-interactive authentication
struct PasswordPrompter(String);

impl KeyboardInteractivePrompt for PasswordPrompter {
    fn prompt<'a>(
        &mut self,
        _username: &str,
        _instructions: &str,
        prompts: &[ssh2::Prompt<'a>],
    ) -> Vec<String> {
        // Return the password for each prompt (typically just one "Password:" prompt)
        prompts.iter().map(|_| self.0.clone()).collect()
    }
}

// Implement Connection trait for SshConnection
#[async_trait]
impl Connection for SshConnection {
    async fn exec(&self, cmd: &str) -> Result<CommandResult, NexusError> {
        // SSH operations are blocking, so we run them in a blocking task
        let result = self.inner.exec(cmd)?;
        Ok(result)
    }

    async fn exec_streaming(
        &self,
        cmd: &str,
        on_stdout: Box<dyn Fn(String) + Send + Sync>,
        on_stderr: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<CommandResult, NexusError> {
        // Convert byte callbacks to string callbacks
        let stdout_callback = |bytes: &[u8]| {
            if let Ok(s) = std::str::from_utf8(bytes) {
                on_stdout(s.to_string());
            }
        };

        let stderr_callback = |bytes: &[u8]| {
            if let Ok(s) = std::str::from_utf8(bytes) {
                on_stderr(s.to_string());
            }
        };

        let exit_code = self
            .inner
            .exec_streaming(cmd, stdout_callback, stderr_callback)?;

        Ok(CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String, NexusError> {
        let bytes = self.inner.read_file(path)?;
        String::from_utf8(bytes).map_err(|e| NexusError::Io {
            message: format!("File is not valid UTF-8: {}", e),
            path: Some(std::path::PathBuf::from(path)),
        })
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), NexusError> {
        self.inner.write_file(path, content.as_bytes())
    }

    fn host_name(&self) -> &str {
        self.inner.host_name.as_str()
    }
}
