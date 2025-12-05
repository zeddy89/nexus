// Local command execution without SSH

use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::{CommandResult, Connection};
use crate::output::errors::NexusError;

/// Local connection for executing commands on localhost
pub struct LocalConnection {
    host_name: String,
}

impl LocalConnection {
    /// Create a new local connection
    pub fn new(host_name: impl Into<String>) -> Self {
        LocalConnection {
            host_name: host_name.into(),
        }
    }

    /// Check if a host should use local connection
    pub fn should_use_local(host_name: &str) -> bool {
        host_name == "localhost" || host_name == "127.0.0.1" || host_name == "::1"
    }
}

#[async_trait]
impl Connection for LocalConnection {
    async fn exec(&self, cmd: &str) -> Result<CommandResult, NexusError> {
        // Execute command using sh -c
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
            .map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to execute local command: {}", e),
                suggestion: Some("Check that 'sh' is available on the system".to_string()),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn exec_streaming(
        &self,
        cmd: &str,
        on_stdout: Box<dyn Fn(String) + Send + Sync>,
        on_stderr: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<CommandResult, NexusError> {
        // Execute command with streaming output
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to spawn local command: {}", e),
                suggestion: Some("Check that 'sh' is available on the system".to_string()),
            })?;

        let stdout_handle = child.stdout.take().ok_or_else(|| NexusError::Runtime {
            function: None,
            message: "Failed to capture stdout".to_string(),
            suggestion: None,
        })?;

        let stderr_handle = child.stderr.take().ok_or_else(|| NexusError::Runtime {
            function: None,
            message: "Failed to capture stderr".to_string(),
            suggestion: None,
        })?;

        // Spawn tasks to read stdout and stderr
        let stdout_task = tokio::spawn(async move {
            let mut stdout_full = String::new();
            let mut reader = BufReader::new(stdout_handle);

            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        on_stdout(line.clone());
                        stdout_full.push_str(&line);
                    }
                    Err(_) => break,
                }
            }

            stdout_full
        });

        let stderr_task = tokio::spawn(async move {
            let mut stderr_full = String::new();
            let mut reader = BufReader::new(stderr_handle);

            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        on_stderr(line.clone());
                        stderr_full.push_str(&line);
                    }
                    Err(_) => break,
                }
            }

            stderr_full
        });

        // Wait for command to complete
        let status = child.wait().await.map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to wait for command: {}", e),
            suggestion: None,
        })?;

        // Collect output
        let stdout = stdout_task.await.map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to read stdout: {}", e),
            suggestion: None,
        })?;

        let stderr = stderr_task.await.map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to read stderr: {}", e),
            suggestion: None,
        })?;

        let exit_code = status.code().unwrap_or(-1);

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String, NexusError> {
        // Read file using tokio::fs
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| NexusError::Io {
                message: format!("Failed to read file: {}", e),
                path: Some(std::path::PathBuf::from(path)),
            })
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), NexusError> {
        // Write file using tokio::fs
        tokio::fs::write(path, content)
            .await
            .map_err(|e| NexusError::Io {
                message: format!("Failed to write file: {}", e),
                path: Some(std::path::PathBuf::from(path)),
            })
    }

    fn host_name(&self) -> &str {
        &self.host_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_exec() {
        let conn = LocalConnection::new("localhost");
        let result = conn.exec("echo 'hello world'").await.unwrap();

        assert!(result.success());
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn test_local_exec_failure() {
        let conn = LocalConnection::new("localhost");
        let result = conn.exec("exit 1").await.unwrap();

        assert!(!result.success());
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_should_use_local() {
        assert!(LocalConnection::should_use_local("localhost"));
        assert!(LocalConnection::should_use_local("127.0.0.1"));
        assert!(LocalConnection::should_use_local("::1"));
        assert!(!LocalConnection::should_use_local("example.com"));
        assert!(!LocalConnection::should_use_local("192.168.1.1"));
    }

    #[tokio::test]
    async fn test_file_operations() {
        use tempfile::NamedTempFile;

        let conn = LocalConnection::new("localhost");
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // Write file
        conn.write_file(path, "test content").await.unwrap();

        // Read file
        let content = conn.read_file(path).await.unwrap();
        assert_eq!(content, "test content");
    }
}
