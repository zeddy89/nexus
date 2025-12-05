// Shell module - execute commands through /bin/sh -c
// This allows shell features like variable expansion, pipes, and redirects

use async_trait::async_trait;

use super::Module;
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::NexusError;

pub struct ShellModule;

impl Default for ShellModule {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellModule {
    pub fn new() -> Self {
        ShellModule
    }

    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        command: &str,
        chdir: Option<String>,
        creates: Option<String>,
        removes: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        // Check mode
        if ctx.check_mode {
            let mut msg = format!("Would execute shell command: {}", command);
            if let Some(ref dir) = chdir {
                msg.push_str(&format!(" (chdir: {})", dir));
            }
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
                .exec(&format!("test -e '{}'", creates_path))
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
                .exec(&format!("test -e '{}'", removes_path))
                .await?
                .success();
            if !exists {
                return Ok(TaskOutput::success()
                    .with_stdout(format!("Skipped - {} does not exist", removes_path)));
            }
        }

        // Build the shell command - execute through /bin/sh -c
        let mut shell_cmd = String::new();

        // Add chdir if specified
        if let Some(ref dir) = chdir {
            // Change directory first
            shell_cmd.push_str(&format!("cd '{}' && ", dir.replace('\'', "'\\'''")));
        }

        // Wrap the command in /bin/sh -c to allow shell features
        // Escape single quotes in the command
        let escaped_command = command.replace('\'', "'\\''");
        shell_cmd.push_str(&format!("/bin/sh -c '{}'", escaped_command));

        // Wrap command with sudo if needed
        let final_command = ctx.wrap_command(&shell_cmd);

        // Execute the command
        let result = conn.exec(&final_command).await?;

        if result.success() {
            Ok(TaskOutput::changed()
                .with_stdout(result.stdout)
                .with_stderr(result.stderr))
        } else {
            // Return failure but include output
            let mut output = TaskOutput::failed(format!(
                "Shell command exited with code {}",
                result.exit_code
            ));
            output.stdout = result.stdout;
            output.stderr = result.stderr;
            output.exit_code = result.exit_code;
            Ok(output)
        }
    }
}

#[async_trait]
impl Module for ShellModule {
    fn name(&self) -> &'static str {
        "shell"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote() {
        // Test escaping single quotes
        let command = "echo 'hello world'";
        let escaped = command.replace('\'', "'\\''");
        assert_eq!(escaped, "echo '\\''hello world'\\''");
    }

    #[test]
    fn test_shell_module_creation() {
        let module = ShellModule::new();
        assert_eq!(module.name(), "shell");
    }
}
