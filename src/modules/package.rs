// Package module - install/remove/update packages

use async_trait::async_trait;

use super::{detect_package_manager, Module, PackageManager};
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::{NexusError, ModuleError};
use crate::parser::ast::PackageState;

pub struct PackageModule {
    cached_manager: std::sync::RwLock<Option<PackageManager>>,
}

impl Default for PackageModule {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageModule {
    pub fn new() -> Self {
        PackageModule {
            cached_manager: std::sync::RwLock::new(None),
        }
    }

    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
        state: PackageState,
    ) -> Result<TaskOutput, NexusError> {
        // Check mode - just report what would happen
        if ctx.check_mode {
            let action = match state {
                PackageState::Installed => "install",
                PackageState::Latest => "upgrade to latest version of",
                PackageState::Absent => "remove",
            };
            return Ok(TaskOutput::changed()
                .with_stdout(format!("Would {} package: {}", action, name)));
        }

        // Detect package manager (cached)
        let cached = *self.cached_manager.read().unwrap();
        let manager = if let Some(m) = cached {
            m
        } else {
            let m = detect_package_manager(conn).await?;
            *self.cached_manager.write().unwrap() = Some(m);
            m
        };

        // Check current state
        let check_cmd = manager.check_installed_cmd(name);
        let is_installed = conn.exec(&check_cmd).await?.success();

        match state {
            PackageState::Installed => {
                if is_installed {
                    return Ok(TaskOutput::success()
                        .with_stdout(format!("Package {} is already installed", name)));
                }

                let cmd = manager.install_cmd(name);
                let final_cmd = ctx.wrap_command(&cmd);
                let result = conn.exec(&final_cmd).await?;

                if result.success() {
                    Ok(TaskOutput::changed()
                        .with_stdout(result.stdout)
                        .with_stderr(result.stderr))
                } else {
                    Err(NexusError::Module(Box::new(ModuleError {
                        module: "package".to_string(),
                        task_name: format!("Install {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to install package {}", name),
                        stderr: Some(result.stderr),
                        suggestion: Some("Check package name and repository configuration".to_string()),
                    })))
                }
            }

            PackageState::Latest => {
                let cmd = manager.update_cmd(name);
                let final_cmd = ctx.wrap_command(&cmd);
                let result = conn.exec(&final_cmd).await?;

                if result.success() {
                    // Check if anything was actually updated
                    let changed = !result.stdout.contains("Nothing to do")
                        && !result.stdout.contains("0 upgraded");

                    if changed {
                        Ok(TaskOutput::changed()
                            .with_stdout(result.stdout)
                            .with_stderr(result.stderr))
                    } else {
                        Ok(TaskOutput::success()
                            .with_stdout(format!("Package {} is already at latest version", name)))
                    }
                } else {
                    Err(NexusError::Module(Box::new(ModuleError {
                        module: "package".to_string(),
                        task_name: format!("Update {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to update package {}", name),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })))
                }
            }

            PackageState::Absent => {
                if !is_installed {
                    return Ok(TaskOutput::success()
                        .with_stdout(format!("Package {} is not installed", name)));
                }

                let cmd = manager.remove_cmd(name);
                let final_cmd = ctx.wrap_command(&cmd);
                let result = conn.exec(&final_cmd).await?;

                if result.success() {
                    Ok(TaskOutput::changed()
                        .with_stdout(result.stdout)
                        .with_stderr(result.stderr))
                } else {
                    Err(NexusError::Module(Box::new(ModuleError {
                        module: "package".to_string(),
                        task_name: format!("Remove {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to remove package {}", name),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })))
                }
            }
        }
    }
}

#[async_trait]
impl Module for PackageModule {
    fn name(&self) -> &'static str {
        "package"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        // This is called via execute_with_params
        unreachable!()
    }
}

#[allow(dead_code)]
fn state_action(state: &PackageState) -> &'static str {
    match state {
        PackageState::Installed => "install",
        PackageState::Latest => "update",
        PackageState::Absent => "remove",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_manager_commands() {
        let apt = PackageManager::Apt;
        assert!(apt.install_cmd("nginx").contains("apt-get install"));
        assert!(apt.check_installed_cmd("nginx").contains("dpkg"));

        let dnf = PackageManager::Dnf;
        assert!(dnf.install_cmd("nginx").contains("dnf install"));
        assert!(dnf.check_installed_cmd("nginx").contains("rpm"));
    }
}
