// Service module - manage systemd services

use async_trait::async_trait;

use super::Module;
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::{ModuleError, NexusError};
use crate::parser::ast::ServiceState;

pub struct ServiceModule;

impl Default for ServiceModule {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceModule {
    pub fn new() -> Self {
        ServiceModule
    }

    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
        state: ServiceState,
        enabled: Option<bool>,
    ) -> Result<TaskOutput, NexusError> {
        // Check mode
        if ctx.check_mode {
            let mut msg = format!("Would {} service: {}", state_action(&state), name);
            if let Some(en) = enabled {
                msg.push_str(&format!(", enabled={}", en));
            }
            return Ok(TaskOutput::changed().with_stdout(msg));
        }

        let mut changed = false;
        let mut output_lines = Vec::new();

        // Get current state
        let current_state = get_service_state(conn, name).await?;

        // Handle state changes
        match state {
            ServiceState::Running => {
                if !current_state.running {
                    let cmd = format!("systemctl start {}", name);
                    let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                    if !result.success() {
                        return Err(NexusError::Module(Box::new(ModuleError {
                            module: "service".to_string(),
                            task_name: format!("Start {}", name),
                            host: conn.host_name().to_string(),
                            message: format!("Failed to start service {}", name),
                            stderr: Some(result.stderr),
                            suggestion: Some(
                                "Check service logs with: journalctl -u ".to_string() + name,
                            ),
                        })));
                    }
                    changed = true;
                    output_lines.push(format!("Started service {}", name));
                } else {
                    output_lines.push(format!("Service {} is already running", name));
                }
            }

            ServiceState::Stopped => {
                if current_state.running {
                    let cmd = format!("systemctl stop {}", name);
                    let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                    if !result.success() {
                        return Err(NexusError::Module(Box::new(ModuleError {
                            module: "service".to_string(),
                            task_name: format!("Stop {}", name),
                            host: conn.host_name().to_string(),
                            message: format!("Failed to stop service {}", name),
                            stderr: Some(result.stderr),
                            suggestion: None,
                        })));
                    }
                    changed = true;
                    output_lines.push(format!("Stopped service {}", name));
                } else {
                    output_lines.push(format!("Service {} is already stopped", name));
                }
            }

            ServiceState::Restarted => {
                let cmd = format!("systemctl restart {}", name);
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    return Err(NexusError::Module(Box::new(ModuleError {
                        module: "service".to_string(),
                        task_name: format!("Restart {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to restart service {}", name),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })));
                }
                changed = true;
                output_lines.push(format!("Restarted service {}", name));
            }

            ServiceState::Reloaded => {
                let cmd = format!("systemctl reload {}", name);
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    // Try reload-or-restart as fallback
                    let cmd2 = format!("systemctl reload-or-restart {}", name);
                    let result2 = conn.exec(&ctx.wrap_command(&cmd2)).await?;
                    if !result2.success() {
                        return Err(NexusError::Module(Box::new(ModuleError {
                            module: "service".to_string(),
                            task_name: format!("Reload {}", name),
                            host: conn.host_name().to_string(),
                            message: format!("Failed to reload service {}", name),
                            stderr: Some(result.stderr),
                            suggestion: Some("Service may not support reload".to_string()),
                        })));
                    }
                }
                changed = true;
                output_lines.push(format!("Reloaded service {}", name));
            }
        }

        // Handle enabled state
        if let Some(should_enable) = enabled {
            if should_enable && !current_state.enabled {
                let cmd = format!("systemctl enable {}", name);
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    return Err(NexusError::Module(Box::new(ModuleError {
                        module: "service".to_string(),
                        task_name: format!("Enable {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to enable service {}", name),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })));
                }
                changed = true;
                output_lines.push(format!("Enabled service {}", name));
            } else if !should_enable && current_state.enabled {
                let cmd = format!("systemctl disable {}", name);
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    return Err(NexusError::Module(Box::new(ModuleError {
                        module: "service".to_string(),
                        task_name: format!("Disable {}", name),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to disable service {}", name),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })));
                }
                changed = true;
                output_lines.push(format!("Disabled service {}", name));
            }
        }

        let output = if changed {
            TaskOutput::changed()
        } else {
            TaskOutput::success()
        };

        Ok(output.with_stdout(output_lines.join("\n")))
    }
}

#[async_trait]
impl Module for ServiceModule {
    fn name(&self) -> &'static str {
        "service"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        unreachable!()
    }
}

/// Current state of a service
struct ServiceStateInfo {
    running: bool,
    enabled: bool,
}

/// Get the current state of a service
async fn get_service_state(
    conn: &dyn Connection,
    name: &str,
) -> Result<ServiceStateInfo, NexusError> {
    // Check if running
    let active_result = conn
        .exec(&format!("systemctl is-active {} 2>/dev/null || true", name))
        .await?;
    let running = active_result.stdout.trim() == "active";

    // Check if enabled
    let enabled_result = conn
        .exec(&format!(
            "systemctl is-enabled {} 2>/dev/null || true",
            name
        ))
        .await?;
    let enabled = enabled_result.stdout.trim() == "enabled";

    Ok(ServiceStateInfo { running, enabled })
}

fn state_action(state: &ServiceState) -> &'static str {
    match state {
        ServiceState::Running => "start",
        ServiceState::Stopped => "stop",
        ServiceState::Restarted => "restart",
        ServiceState::Reloaded => "reload",
    }
}
