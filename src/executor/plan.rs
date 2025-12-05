// Execution plan generator - Terraform-style planning for Nexus

use std::sync::Arc;
use std::time::Duration;

use crate::executor::ExecutionContext;
use crate::inventory::Inventory;
use crate::modules::{AnyConnection, ModuleExecutor};
use crate::output::errors::NexusError;
use crate::parser::ast::{
    FileState, ModuleCall, PackageState, Playbook, ServiceState, Task, TaskOrBlock, UserState,
};
use crate::runtime::evaluate_expression;

/// Type of change being planned
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Create,      // +
    Remove,      // -
    Modify,      // ~
    NoChange,    // ✓
    Unknown,     // ?
    Conditional, // ? (depends on runtime condition)
}

impl ChangeType {
    pub fn symbol(&self) -> &'static str {
        match self {
            ChangeType::Create => "+",
            ChangeType::Remove => "-",
            ChangeType::Modify => "~",
            ChangeType::NoChange => "✓",
            ChangeType::Unknown => "?",
            ChangeType::Conditional => "?",
        }
    }
}

/// A planned change for a single task
#[derive(Debug, Clone)]
pub struct PlannedChange {
    pub task_name: String,
    pub module: String,
    pub change_type: ChangeType,
    pub current_state: Option<String>,
    pub desired_state: Option<String>,
    pub diff: Option<String>,
    pub is_dangerous: bool,
    pub danger_reason: Option<String>,
}

impl PlannedChange {
    /// Create a signature for grouping identical changes
    pub fn signature(&self) -> String {
        format!("{}-{:?}-{}", self.task_name, self.change_type, self.module)
    }
}

/// Plan for a single host
#[derive(Debug)]
pub struct HostPlan {
    pub host: String,
    pub changes: Vec<PlannedChange>,
    pub estimated_duration: Duration,
}

impl HostPlan {
    /// Get a signature for grouping identical host plans
    pub fn signature(&self) -> String {
        self.changes
            .iter()
            .map(|c| c.signature())
            .collect::<Vec<_>>()
            .join("|")
    }
}

/// Complete execution plan
#[derive(Debug)]
pub struct ExecutionPlan {
    pub playbook: String,
    pub host_plans: Vec<HostPlan>,
    pub total_tasks: usize,
    pub creates: usize,
    pub modifies: usize,
    pub removes: usize,
    pub no_changes: usize,
    pub warnings: usize,
    pub estimated_duration: Duration,
}

impl ExecutionPlan {
    /// Create a new execution plan from host plans
    pub fn new(playbook: String, host_plans: Vec<HostPlan>) -> Self {
        let mut creates = 0;
        let mut modifies = 0;
        let mut removes = 0;
        let mut no_changes = 0;
        let mut warnings = 0;
        let mut total_tasks = 0;

        for host_plan in &host_plans {
            for change in &host_plan.changes {
                total_tasks += 1;
                match change.change_type {
                    ChangeType::Create => creates += 1,
                    ChangeType::Modify => modifies += 1,
                    ChangeType::Remove => removes += 1,
                    ChangeType::NoChange => no_changes += 1,
                    ChangeType::Unknown | ChangeType::Conditional => {}
                }
                if change.is_dangerous {
                    warnings += 1;
                }
            }
        }

        let estimated_duration = host_plans
            .iter()
            .map(|hp| hp.estimated_duration)
            .max()
            .unwrap_or(Duration::from_secs(0));

        ExecutionPlan {
            playbook,
            host_plans,
            total_tasks,
            creates,
            modifies,
            removes,
            no_changes,
            warnings,
            estimated_duration,
        }
    }
}

/// Plan generator
#[allow(dead_code)]
pub struct PlanGenerator {
    module_executor: ModuleExecutor,
}

impl PlanGenerator {
    pub fn new() -> Self {
        PlanGenerator {
            module_executor: ModuleExecutor::new(),
        }
    }

    /// Generate an execution plan for a playbook
    pub async fn generate_plan(
        &self,
        playbook: &Playbook,
        inventory: &Inventory,
        ssh_config: SshConfig,
        limit: Option<&str>,
    ) -> Result<ExecutionPlan, NexusError> {
        let mut hosts = inventory.get_hosts(&playbook.hosts);

        // Apply limit filter if specified
        if let Some(limit_pattern) = limit {
            hosts.retain(|host| {
                // Match against hostname or pattern
                if limit_pattern.contains('*') || limit_pattern.contains('?') {
                    // Wildcard pattern matching
                    let pattern = limit_pattern.replace('*', ".*").replace('?', ".");
                    if let Ok(re) = regex::Regex::new(&format!("^{}$", pattern)) {
                        re.is_match(&host.name)
                    } else {
                        host.name == limit_pattern
                    }
                } else if limit_pattern.contains(',') {
                    // Comma-separated list of hosts
                    limit_pattern
                        .split(',')
                        .map(str::trim)
                        .any(|h| h == host.name)
                } else {
                    // Exact match
                    host.name == limit_pattern
                }
            });
        }

        let mut host_plans = Vec::new();

        // Create connection pool
        let mut pool =
            crate::executor::ConnectionPool::new().with_connect_timeout(Duration::from_secs(30));

        if let Some(user) = &ssh_config.user {
            pool = pool.with_default_user(user.clone());
        }
        if let Some(password) = &ssh_config.password {
            pool = pool.with_password(password.clone());
        }
        if let Some(key) = &ssh_config.private_key {
            pool = pool.with_private_key(key.clone());
        }

        for host in hosts {
            // Get connection from pool
            let ssh_conn = pool.get(host)?;
            let conn = AnyConnection::Ssh(ssh_conn);

            // Generate changes for each task
            let mut changes = Vec::new();
            let mut total_duration = Duration::from_secs(0);

            for task_or_block in &playbook.tasks {
                if let TaskOrBlock::Task(task) = task_or_block {
                    // Create execution context for planning
                    let ctx = ExecutionContext::new(Arc::new(host.clone()), playbook.vars.clone());

                    // Check state for this task
                    let planned = self.check_task_state(task, &ctx, &conn, &host.name).await?;

                    if let Some(change) = planned {
                        total_duration += estimate_task_duration(&task.module, change.change_type);
                        changes.push(change);
                    }
                }
            }

            host_plans.push(HostPlan {
                host: host.name.clone(),
                changes,
                estimated_duration: total_duration,
            });
        }

        Ok(ExecutionPlan::new(playbook.source_file.clone(), host_plans))
    }

    /// Check the state for a single task
    async fn check_task_state(
        &self,
        task: &Task,
        ctx: &ExecutionContext,
        conn: &AnyConnection,
        host: &str,
    ) -> Result<Option<PlannedChange>, NexusError> {
        // Check when condition first - if it depends on registered vars, mark as conditional
        if let Some(ref when_expr) = task.when {
            match evaluate_expression(when_expr, ctx) {
                Ok(result) => {
                    if !result.is_truthy() {
                        // Condition is false - task will be skipped
                        return Ok(None);
                    }
                }
                Err(e) => {
                    // If the error is about a missing variable (likely a registered var),
                    // mark this task as conditional since we can't evaluate it at plan time
                    if e.to_string().contains("Variable not found") {
                        return Ok(Some(PlannedChange {
                            task_name: task.name.clone(),
                            module: task.module.module_name().to_string(),
                            change_type: ChangeType::Conditional,
                            current_state: None,
                            desired_state: Some("depends on runtime condition".to_string()),
                            diff: None,
                            is_dangerous: false,
                            danger_reason: None,
                        }));
                    }
                    return Err(e);
                }
            }
        }

        // Try to check the actual module state, handling missing variables gracefully
        let change = match self.check_module_state(task, ctx, conn, host).await {
            Ok(change) => change,
            Err(e) => {
                // If expression evaluation fails due to missing variable,
                // mark as conditional
                if e.to_string().contains("Variable not found") {
                    PlannedChange {
                        task_name: task.name.clone(),
                        module: task.module.module_name().to_string(),
                        change_type: ChangeType::Conditional,
                        current_state: None,
                        desired_state: Some("uses runtime variables".to_string()),
                        diff: None,
                        is_dangerous: false,
                        danger_reason: None,
                    }
                } else {
                    return Err(e);
                }
            }
        };

        Ok(Some(change))
    }

    /// Check the module-specific state
    async fn check_module_state(
        &self,
        task: &Task,
        ctx: &ExecutionContext,
        conn: &AnyConnection,
        host: &str,
    ) -> Result<PlannedChange, NexusError> {
        match &task.module {
            ModuleCall::Package { name, state } => {
                let name_val = evaluate_expression(name, ctx)?;
                self.check_package_state(conn, host, &name_val.to_string(), *state, &task.name)
                    .await
            }

            ModuleCall::Service {
                name,
                state,
                enabled,
            } => {
                let name_val = evaluate_expression(name, ctx)?;
                self.check_service_state(
                    conn,
                    host,
                    &name_val.to_string(),
                    *state,
                    *enabled,
                    &task.name,
                )
                .await
            }

            ModuleCall::File {
                path,
                state,
                content,
                ..
            } => {
                let path_val = evaluate_expression(path, ctx)?;
                let content_val = content
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                self.check_file_state(
                    conn,
                    host,
                    &path_val.to_string(),
                    *state,
                    content_val.as_ref().map(|v| v.to_string()),
                    &task.name,
                )
                .await
            }

            ModuleCall::Command {
                cmd,
                creates,
                removes,
            } => {
                let cmd_val = evaluate_expression(cmd, ctx)?;
                let creates_val = creates
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let removes_val = removes
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                self.check_command_state(
                    conn,
                    host,
                    &cmd_val.to_string(),
                    creates_val.as_ref().map(|v| v.to_string()),
                    removes_val.as_ref().map(|v| v.to_string()),
                    &task.name,
                )
                .await
            }

            ModuleCall::User { name, state, .. } => {
                let name_val = evaluate_expression(name, ctx)?;
                self.check_user_state(conn, host, &name_val.to_string(), *state, &task.name)
                    .await
            }

            _ => {
                // For other modules, return unknown
                Ok(PlannedChange {
                    task_name: task.name.clone(),
                    module: "unknown".to_string(),
                    change_type: ChangeType::Unknown,
                    current_state: None,
                    desired_state: None,
                    diff: None,
                    is_dangerous: false,
                    danger_reason: None,
                })
            }
        }
    }

    /// Check package state
    async fn check_package_state(
        &self,
        conn: &AnyConnection,
        _host: &str,
        name: &str,
        state: PackageState,
        task_name: &str,
    ) -> Result<PlannedChange, NexusError> {
        let connection = conn.as_connection();

        // Detect package manager
        let manager = crate::modules::detect_package_manager(connection).await?;
        let check_cmd = manager.check_installed_cmd(name);
        let is_installed = connection.exec(&check_cmd).await?.success();

        let (change_type, current, desired) = match state {
            PackageState::Installed => {
                if is_installed {
                    (
                        ChangeType::NoChange,
                        Some("installed".to_string()),
                        Some("installed".to_string()),
                    )
                } else {
                    (
                        ChangeType::Create,
                        Some("not installed".to_string()),
                        Some("installed".to_string()),
                    )
                }
            }
            PackageState::Latest => {
                if is_installed {
                    (
                        ChangeType::Modify,
                        Some("current version".to_string()),
                        Some("latest version".to_string()),
                    )
                } else {
                    (
                        ChangeType::Create,
                        Some("not installed".to_string()),
                        Some("latest version".to_string()),
                    )
                }
            }
            PackageState::Absent => {
                if is_installed {
                    (
                        ChangeType::Remove,
                        Some("installed".to_string()),
                        Some("removed".to_string()),
                    )
                } else {
                    (
                        ChangeType::NoChange,
                        Some("not installed".to_string()),
                        Some("not installed".to_string()),
                    )
                }
            }
        };

        Ok(PlannedChange {
            task_name: task_name.to_string(),
            module: "package".to_string(),
            change_type,
            current_state: current,
            desired_state: desired,
            diff: None,
            is_dangerous: false,
            danger_reason: None,
        })
    }

    /// Check service state
    async fn check_service_state(
        &self,
        conn: &AnyConnection,
        _host: &str,
        name: &str,
        state: ServiceState,
        _enabled: Option<bool>,
        task_name: &str,
    ) -> Result<PlannedChange, NexusError> {
        let connection = conn.as_connection();

        // Check if running
        let active_result = connection
            .exec(&format!("systemctl is-active {} 2>/dev/null || true", name))
            .await?;
        let is_running = active_result.stdout.trim() == "active";

        // Check if enabled
        let enabled_result = connection
            .exec(&format!(
                "systemctl is-enabled {} 2>/dev/null || true",
                name
            ))
            .await?;
        let _is_enabled = enabled_result.stdout.trim() == "enabled";

        let (change_type, current, desired, is_dangerous, danger_reason) = match state {
            ServiceState::Running => {
                if is_running {
                    (
                        ChangeType::NoChange,
                        Some("running".to_string()),
                        Some("running".to_string()),
                        false,
                        None,
                    )
                } else {
                    (
                        ChangeType::Modify,
                        Some("stopped".to_string()),
                        Some("running".to_string()),
                        false,
                        None,
                    )
                }
            }
            ServiceState::Stopped => {
                if !is_running {
                    (
                        ChangeType::NoChange,
                        Some("stopped".to_string()),
                        Some("stopped".to_string()),
                        false,
                        None,
                    )
                } else {
                    (
                        ChangeType::Modify,
                        Some("running".to_string()),
                        Some("stopped".to_string()),
                        true,
                        Some("Service will be stopped".to_string()),
                    )
                }
            }
            ServiceState::Restarted => (
                ChangeType::Modify,
                Some("current state".to_string()),
                Some("restarted".to_string()),
                true,
                Some("Service will restart - may cause downtime".to_string()),
            ),
            ServiceState::Reloaded => (
                ChangeType::Modify,
                Some("current config".to_string()),
                Some("reloaded config".to_string()),
                false,
                None,
            ),
        };

        Ok(PlannedChange {
            task_name: task_name.to_string(),
            module: "service".to_string(),
            change_type,
            current_state: current,
            desired_state: desired,
            diff: None,
            is_dangerous,
            danger_reason,
        })
    }

    /// Check file state
    async fn check_file_state(
        &self,
        conn: &AnyConnection,
        _host: &str,
        path: &str,
        state: FileState,
        content: Option<String>,
        task_name: &str,
    ) -> Result<PlannedChange, NexusError> {
        let connection = conn.as_connection();
        let shell_path = format!("'{}'", path.replace('\'', "'\\''"));

        let exists = connection
            .exec(&format!("test -f {}", shell_path))
            .await?
            .success();

        let (change_type, current, desired, diff) = match state {
            FileState::File => {
                if let Some(ref new_content) = content {
                    if exists {
                        // Read current content
                        let old_content = connection.read_file(path).await.ok();
                        if old_content.as_deref() == Some(new_content.as_str()) {
                            (
                                ChangeType::NoChange,
                                Some("exists with correct content".to_string()),
                                Some("exists with correct content".to_string()),
                                None,
                            )
                        } else {
                            // Generate diff
                            let diff_str = old_content.map(|old| {
                                crate::output::diff::generate_unified_diff(
                                    &old,
                                    new_content,
                                    &format!("{} (current)", path),
                                    &format!("{} (desired)", path),
                                )
                            });

                            (
                                ChangeType::Modify,
                                Some("exists with different content".to_string()),
                                Some("updated content".to_string()),
                                diff_str,
                            )
                        }
                    } else {
                        (
                            ChangeType::Create,
                            Some("does not exist".to_string()),
                            Some("will be created".to_string()),
                            None,
                        )
                    }
                } else if exists {
                    (
                        ChangeType::NoChange,
                        Some("exists".to_string()),
                        Some("exists".to_string()),
                        None,
                    )
                } else {
                    (
                        ChangeType::Create,
                        Some("does not exist".to_string()),
                        Some("will be created".to_string()),
                        None,
                    )
                }
            }
            FileState::Directory => {
                let is_dir = connection
                    .exec(&format!("test -d {}", shell_path))
                    .await?
                    .success();
                if is_dir {
                    (
                        ChangeType::NoChange,
                        Some("directory exists".to_string()),
                        Some("directory exists".to_string()),
                        None,
                    )
                } else {
                    (
                        ChangeType::Create,
                        Some("does not exist".to_string()),
                        Some("directory will be created".to_string()),
                        None,
                    )
                }
            }
            FileState::Absent => {
                if exists {
                    (
                        ChangeType::Remove,
                        Some("exists".to_string()),
                        Some("will be removed".to_string()),
                        None,
                    )
                } else {
                    (
                        ChangeType::NoChange,
                        Some("does not exist".to_string()),
                        Some("does not exist".to_string()),
                        None,
                    )
                }
            }
            _ => (ChangeType::Unknown, None, None, None),
        };

        Ok(PlannedChange {
            task_name: task_name.to_string(),
            module: "file".to_string(),
            change_type,
            current_state: current,
            desired_state: desired,
            diff,
            is_dangerous: false,
            danger_reason: None,
        })
    }

    /// Check command state
    async fn check_command_state(
        &self,
        conn: &AnyConnection,
        _host: &str,
        command: &str,
        creates: Option<String>,
        removes: Option<String>,
        task_name: &str,
    ) -> Result<PlannedChange, NexusError> {
        let connection = conn.as_connection();

        // Detect dangerous patterns
        let dangerous_patterns = ["rm -rf", "reboot", "shutdown", "dd if=", "mkfs", "fdisk"];
        let is_dangerous = dangerous_patterns
            .iter()
            .any(|pattern| command.contains(pattern));

        let danger_reason = if is_dangerous {
            Some("Command contains potentially destructive operations".to_string())
        } else {
            None
        };

        // Check creates/removes conditions
        let (change_type, current, desired) = if let Some(ref creates_path) = creates {
            let exists = connection
                .exec(&format!("test -e '{}'", creates_path))
                .await?
                .success();
            if exists {
                (
                    ChangeType::NoChange,
                    Some(format!("{} already exists", creates_path)),
                    Some(format!("{} exists (skip)", creates_path)),
                )
            } else {
                (
                    ChangeType::Modify,
                    Some(format!("{} does not exist", creates_path)),
                    Some("will execute command".to_string()),
                )
            }
        } else if let Some(ref removes_path) = removes {
            let exists = connection
                .exec(&format!("test -e '{}'", removes_path))
                .await?
                .success();
            if exists {
                (
                    ChangeType::Modify,
                    Some(format!("{} exists", removes_path)),
                    Some("will execute command".to_string()),
                )
            } else {
                (
                    ChangeType::NoChange,
                    Some(format!("{} does not exist", removes_path)),
                    Some(format!("{} does not exist (skip)", removes_path)),
                )
            }
        } else {
            // No creates/removes - will always run
            (
                ChangeType::Modify,
                Some("command will execute".to_string()),
                Some("command will execute".to_string()),
            )
        };

        Ok(PlannedChange {
            task_name: task_name.to_string(),
            module: "command".to_string(),
            change_type,
            current_state: current,
            desired_state: desired,
            diff: None,
            is_dangerous,
            danger_reason,
        })
    }

    /// Check user state
    async fn check_user_state(
        &self,
        conn: &AnyConnection,
        _host: &str,
        name: &str,
        state: UserState,
        task_name: &str,
    ) -> Result<PlannedChange, NexusError> {
        let connection = conn.as_connection();

        let user_exists = connection
            .exec(&format!("id {} >/dev/null 2>&1", name))
            .await?
            .success();

        let (change_type, current, desired) = match state {
            UserState::Present => {
                if user_exists {
                    (
                        ChangeType::NoChange,
                        Some("user exists".to_string()),
                        Some("user exists".to_string()),
                    )
                } else {
                    (
                        ChangeType::Create,
                        Some("user does not exist".to_string()),
                        Some("user will be created".to_string()),
                    )
                }
            }
            UserState::Absent => {
                if user_exists {
                    (
                        ChangeType::Remove,
                        Some("user exists".to_string()),
                        Some("user will be removed".to_string()),
                    )
                } else {
                    (
                        ChangeType::NoChange,
                        Some("user does not exist".to_string()),
                        Some("user does not exist".to_string()),
                    )
                }
            }
        };

        Ok(PlannedChange {
            task_name: task_name.to_string(),
            module: "user".to_string(),
            change_type,
            current_state: current,
            desired_state: desired,
            diff: None,
            is_dangerous: false,
            danger_reason: None,
        })
    }
}

impl Default for PlanGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// SSH configuration for planning
pub struct SshConfig {
    pub user: Option<String>,
    pub password: Option<String>,
    pub private_key: Option<String>,
}

/// Estimate task duration based on module type and change state
fn estimate_task_duration(module: &ModuleCall, change_type: ChangeType) -> Duration {
    // Base estimate per module type (for actual changes)
    let base_secs = match module {
        ModuleCall::Package { .. } => 30,
        ModuleCall::Service { .. } => 5,
        ModuleCall::File { .. } => 2,
        ModuleCall::Command { .. } => 10,
        ModuleCall::User { .. } => 3,
        ModuleCall::Template { .. } => 3,
        ModuleCall::Facts { .. } => 15,
        _ => 5,
    };

    // Adjust based on change type
    let adjusted_secs = match change_type {
        // No change - just verification, very fast
        ChangeType::NoChange => 1,
        // Actual changes - full estimate
        ChangeType::Create | ChangeType::Modify | ChangeType::Remove => base_secs,
        // Conditional/Unknown - may or may not run, use 50%
        ChangeType::Conditional | ChangeType::Unknown => base_secs / 2,
    };

    Duration::from_secs(adjusted_secs)
}
