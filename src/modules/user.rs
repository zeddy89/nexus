// User module - manage system users

use async_trait::async_trait;

use super::Module;
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::{ModuleError, NexusError};
use crate::parser::ast::UserState;

pub struct UserModule;

impl Default for UserModule {
    fn default() -> Self {
        Self::new()
    }
}

impl UserModule {
    pub fn new() -> Self {
        UserModule
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
        state: UserState,
        uid: Option<u32>,
        gid: Option<u32>,
        groups: Vec<String>,
        shell: Option<String>,
        home: Option<String>,
        create_home: Option<bool>,
    ) -> Result<TaskOutput, NexusError> {
        // Check mode
        if ctx.check_mode {
            let action = match state {
                UserState::Present => "create/update",
                UserState::Absent => "remove",
            };
            return Ok(
                TaskOutput::changed().with_stdout(format!("Would {} user: {}", action, name))
            );
        }

        // Check if user exists
        let user_exists = conn
            .exec(&format!("id {} >/dev/null 2>&1", name))
            .await?
            .success();

        match state {
            UserState::Present => {
                if user_exists {
                    self.update_user(ctx, conn, name, uid, gid, groups, shell, home)
                        .await
                } else {
                    self.create_user(ctx, conn, name, uid, gid, groups, shell, home, create_home)
                        .await
                }
            }
            UserState::Absent => {
                if user_exists {
                    self.remove_user(ctx, conn, name).await
                } else {
                    Ok(TaskOutput::success().with_stdout(format!("User {} does not exist", name)))
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_user(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
        uid: Option<u32>,
        gid: Option<u32>,
        groups: Vec<String>,
        shell: Option<String>,
        home: Option<String>,
        create_home: Option<bool>,
    ) -> Result<TaskOutput, NexusError> {
        let mut cmd = format!("useradd {}", name);

        if let Some(u) = uid {
            cmd.push_str(&format!(" -u {}", u));
        }

        if let Some(g) = gid {
            cmd.push_str(&format!(" -g {}", g));
        }

        if !groups.is_empty() {
            cmd.push_str(&format!(" -G {}", groups.join(",")));
        }

        if let Some(s) = shell {
            cmd.push_str(&format!(" -s {}", s));
        }

        if let Some(h) = home {
            cmd.push_str(&format!(" -d {}", h));
        }

        if create_home.unwrap_or(true) {
            cmd.push_str(" -m");
        } else {
            cmd.push_str(" -M");
        }

        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;

        if result.success() {
            Ok(TaskOutput::changed().with_stdout(format!("Created user {}", name)))
        } else {
            Err(NexusError::Module(Box::new(ModuleError {
                module: "user".to_string(),
                task_name: format!("Create user {}", name),
                host: conn.host_name().to_string(),
                message: format!("Failed to create user {}", name),
                stderr: Some(result.stderr),
                suggestion: None,
            })))
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_user(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
        uid: Option<u32>,
        gid: Option<u32>,
        groups: Vec<String>,
        shell: Option<String>,
        home: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        let mut changes = Vec::new();

        // Get current user info
        let current = self.get_user_info(conn, name).await?;

        // Build usermod command if changes needed
        let mut cmd = format!("usermod {}", name);
        let mut has_changes = false;

        if let Some(u) = uid {
            if current.uid != u {
                cmd.push_str(&format!(" -u {}", u));
                changes.push(format!("UID: {} -> {}", current.uid, u));
                has_changes = true;
            }
        }

        if let Some(g) = gid {
            if current.gid != g {
                cmd.push_str(&format!(" -g {}", g));
                changes.push(format!("GID: {} -> {}", current.gid, g));
                has_changes = true;
            }
        }

        if !groups.is_empty() {
            // Check if groups are different
            let current_groups: std::collections::HashSet<_> = current.groups.iter().collect();
            let desired_groups: std::collections::HashSet<_> = groups.iter().collect();

            if current_groups != desired_groups {
                cmd.push_str(&format!(" -G {}", groups.join(",")));
                changes.push(format!("Groups: {:?} -> {:?}", current.groups, groups));
                has_changes = true;
            }
        }

        if let Some(s) = shell {
            if current.shell != s {
                cmd.push_str(&format!(" -s {}", s));
                changes.push(format!("Shell: {} -> {}", current.shell, s));
                has_changes = true;
            }
        }

        if let Some(h) = home {
            if current.home != h {
                cmd.push_str(&format!(" -d {}", h));
                changes.push(format!("Home: {} -> {}", current.home, h));
                has_changes = true;
            }
        }

        if !has_changes {
            return Ok(TaskOutput::success().with_stdout(format!("User {} is up to date", name)));
        }

        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;

        if result.success() {
            Ok(TaskOutput::changed().with_stdout(format!(
                "Updated user {}: {}",
                name,
                changes.join(", ")
            )))
        } else {
            Err(NexusError::Module(Box::new(ModuleError {
                module: "user".to_string(),
                task_name: format!("Update user {}", name),
                host: conn.host_name().to_string(),
                message: format!("Failed to update user {}", name),
                stderr: Some(result.stderr),
                suggestion: None,
            })))
        }
    }

    async fn remove_user(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        name: &str,
    ) -> Result<TaskOutput, NexusError> {
        let cmd = format!("userdel -r {}", name);
        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;

        if result.success() {
            Ok(TaskOutput::changed().with_stdout(format!("Removed user {}", name)))
        } else {
            Err(NexusError::Module(Box::new(ModuleError {
                module: "user".to_string(),
                task_name: format!("Remove user {}", name),
                host: conn.host_name().to_string(),
                message: format!("Failed to remove user {}", name),
                stderr: Some(result.stderr),
                suggestion: None,
            })))
        }
    }

    async fn get_user_info(
        &self,
        conn: &dyn Connection,
        name: &str,
    ) -> Result<UserInfo, NexusError> {
        // Get passwd entry
        let result = conn.exec(&format!("getent passwd {}", name)).await?;
        if !result.success() {
            return Err(NexusError::Module(Box::new(ModuleError {
                module: "user".to_string(),
                task_name: String::new(),
                host: conn.host_name().to_string(),
                message: format!("User {} not found", name),
                stderr: None,
                suggestion: None,
            })));
        }

        // Parse passwd format: name:x:uid:gid:gecos:home:shell
        let parts: Vec<&str> = result.stdout.trim().split(':').collect();
        if parts.len() < 7 {
            return Err(NexusError::Module(Box::new(ModuleError {
                module: "user".to_string(),
                task_name: String::new(),
                host: conn.host_name().to_string(),
                message: "Invalid passwd entry".to_string(),
                stderr: None,
                suggestion: None,
            })));
        }

        let uid: u32 = parts[2].parse().unwrap_or(0);
        let gid: u32 = parts[3].parse().unwrap_or(0);
        let home = parts[5].to_string();
        let shell = parts[6].to_string();

        // Get groups
        let groups_result = conn.exec(&format!("groups {}", name)).await?;
        let groups: Vec<String> = if groups_result.success() {
            groups_result
                .stdout
                .trim()
                .split(':')
                .next_back()
                .unwrap_or("")
                .split_whitespace()
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };

        Ok(UserInfo {
            uid,
            gid,
            home,
            shell,
            groups,
        })
    }
}

struct UserInfo {
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
    groups: Vec<String>,
}

#[async_trait]
impl Module for UserModule {
    fn name(&self) -> &'static str {
        "user"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        unreachable!()
    }
}
