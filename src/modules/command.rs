// Command module - execute arbitrary shell commands

use async_trait::async_trait;

use super::Module;
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::NexusError;

pub struct CommandModule;

impl Default for CommandModule {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandModule {
    pub fn new() -> Self {
        CommandModule
    }

    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        command: &str,
        creates: Option<String>,
        removes: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        // Helper function to safely quote shell arguments
        fn shell_quote(s: &str) -> String {
            format!("'{}'", s.replace('\'', "'\\''"))
        }

        // Check mode
        if ctx.check_mode {
            let mut msg = format!("Would execute command: {}", command);
            if let Some(ref c) = creates {
                msg.push_str(&format!(" (creates: {})", c));
            }
            if let Some(ref r) = removes {
                msg.push_str(&format!(" (removes: {})", r));
            }
            return Ok(TaskOutput::changed().with_stdout(msg));
        }

        // Check 'creates' condition - skip if file exists
        if let Some(ref creates_path) = creates {
            let exists = conn
                .exec(&format!("test -e {}", shell_quote(creates_path)))
                .await?
                .success();
            if exists {
                return Ok(TaskOutput::success()
                    .with_stdout(format!("Skipped - {} already exists", creates_path)));
            }
        }

        // Check 'removes' condition - skip if file doesn't exist
        if let Some(ref removes_path) = removes {
            let exists = conn
                .exec(&format!("test -e {}", shell_quote(removes_path)))
                .await?
                .success();
            if !exists {
                return Ok(TaskOutput::success()
                    .with_stdout(format!("Skipped - {} does not exist", removes_path)));
            }
        }

        // Wrap command with sudo if needed
        let final_command = ctx.wrap_command(command);

        // Execute the command
        let result = conn.exec(&final_command).await?;

        if result.success() {
            Ok(TaskOutput::changed()
                .with_stdout(result.stdout)
                .with_stderr(result.stderr))
        } else {
            // Return failure but include output
            let mut output =
                TaskOutput::failed(format!("Command exited with code {}", result.exit_code));
            output.stdout = result.stdout;
            output.stderr = result.stderr;
            output.exit_code = result.exit_code;
            Ok(output)
        }
    }

    /// Execute with streaming output (for long-running commands)
    pub async fn execute_streaming(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        command: &str,
        on_stdout: Box<dyn Fn(String) + Send + Sync>,
        on_stderr: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<TaskOutput, NexusError> {
        if ctx.check_mode {
            return Ok(TaskOutput::changed().with_stdout(format!("Would run: {}", command)));
        }

        // Wrap command with sudo if needed
        let final_command = ctx.wrap_command(command);

        let result = conn
            .exec_streaming(&final_command, on_stdout, on_stderr)
            .await?;

        if result.exit_code == 0 {
            Ok(TaskOutput::changed())
        } else {
            Ok(TaskOutput::failed(format!(
                "Command exited with code {}",
                result.exit_code
            )))
        }
    }
}

#[async_trait]
impl Module for CommandModule {
    fn name(&self) -> &'static str {
        "command"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        unreachable!()
    }
}

/// Shell command builder for safer command construction
#[allow(dead_code)]
pub struct ShellCommand {
    parts: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
}

#[allow(dead_code)]
impl ShellCommand {
    pub fn new(cmd: &str) -> Self {
        ShellCommand {
            parts: vec![cmd.to_string()],
            env: Vec::new(),
            cwd: None,
        }
    }

    pub fn arg(mut self, arg: &str) -> Self {
        self.parts.push(shell_quote(arg));
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for arg in args {
            self.parts.push(shell_quote(arg.as_ref()));
        }
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    pub fn cwd(mut self, dir: &str) -> Self {
        self.cwd = Some(dir.to_string());
        self
    }

    pub fn build(&self) -> String {
        let mut cmd = String::new();

        // Add environment variables
        for (k, v) in &self.env {
            cmd.push_str(&format!("{}={} ", shell_quote(k), shell_quote(v)));
        }

        // Add directory change if specified
        if let Some(ref dir) = self.cwd {
            cmd.push_str(&format!("cd {} && ", shell_quote(dir)));
        }

        // Add the command and arguments
        cmd.push_str(&self.parts.join(" "));

        cmd
    }
}

#[allow(dead_code)]
fn shell_quote(s: &str) -> String {
    // If the string only contains safe characters, return as-is
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '/' || c == '.')
    {
        return s.to_string();
    }

    // Otherwise, quote it
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote() {
        assert_eq!(shell_quote("hello"), "hello");
        assert_eq!(shell_quote("hello world"), "'hello world'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_command_builder() {
        let cmd = ShellCommand::new("echo").arg("hello").arg("world").build();
        assert_eq!(cmd, "echo hello world");

        let cmd = ShellCommand::new("ls").cwd("/tmp").arg("-la").build();
        assert_eq!(cmd, "cd /tmp && ls -la");

        let cmd = ShellCommand::new("npm")
            .env("NODE_ENV", "production")
            .arg("install")
            .build();
        assert_eq!(cmd, "NODE_ENV=production npm install");
    }
}
