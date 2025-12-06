// Built-in modules for Nexus

mod async_status;
mod command;
mod file;
mod package;
mod service;
mod shell;
pub mod template;
mod user;

pub use async_status::AsyncStatusModule;
pub use command::CommandModule;
pub use file::FileModule;
pub use package::PackageModule;
pub use service::ServiceModule;
pub use shell::ShellModule;
pub use template::TemplateEngine;
pub use user::UserModule;

use async_trait::async_trait;

use crate::executor::{Connection, ExecutionContext, LocalConnection, SshConnection, TaskOutput};
use crate::output::errors::{ModuleError, NexusError};
use crate::parser::ast::ModuleCall;
use crate::runtime::evaluate_expression;

/// Wrapper for different connection types
pub enum AnyConnection {
    Ssh(SshConnection),
    Local(LocalConnection),
}

impl AnyConnection {
    /// Get the underlying connection as a trait object
    pub fn as_connection(&self) -> &dyn Connection {
        match self {
            AnyConnection::Ssh(conn) => conn,
            AnyConnection::Local(conn) => conn,
        }
    }
}

/// Trait for module implementations
#[async_trait]
pub trait Module: Send + Sync {
    /// Module name
    fn name(&self) -> &'static str;

    /// Execute the module
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError>;
}

/// Module executor that dispatches to the appropriate module
pub struct ModuleExecutor {
    package: PackageModule,
    service: ServiceModule,
    file: FileModule,
    command: CommandModule,
    shell: ShellModule,
    user: UserModule,
}

impl ModuleExecutor {
    pub fn new() -> Self {
        ModuleExecutor {
            package: PackageModule::new(),
            service: ServiceModule::new(),
            file: FileModule::new(),
            command: CommandModule::new(),
            shell: ShellModule::new(),
            user: UserModule::new(),
        }
    }

    /// Execute a module call
    pub async fn execute(
        &self,
        module_call: &ModuleCall,
        ctx: &ExecutionContext,
        conn: &AnyConnection,
    ) -> Result<TaskOutput, NexusError> {
        match module_call {
            ModuleCall::Package { name, state } => {
                let name_val = evaluate_expression(name, ctx)?;
                self.package
                    .execute_with_params(ctx, conn.as_connection(), &name_val.to_string(), *state)
                    .await
            }

            ModuleCall::Service {
                name,
                state,
                enabled,
            } => {
                let name_val = evaluate_expression(name, ctx)?;
                self.service
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &name_val.to_string(),
                        *state,
                        *enabled,
                    )
                    .await
            }

            ModuleCall::File {
                path,
                state,
                source,
                content,
                owner,
                group,
                mode,
            } => {
                let path_val = evaluate_expression(path, ctx)?;
                let source_val = source
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let content_val = content
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let owner_val = owner
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let group_val = group
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let mode_val = mode
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                self.file
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &path_val.to_string(),
                        *state,
                        source_val.as_ref().map(|v| v.to_string()),
                        content_val.as_ref().map(|v| v.to_string()),
                        owner_val.as_ref().map(|v| v.to_string()),
                        group_val.as_ref().map(|v| v.to_string()),
                        mode_val.as_ref().map(|v| v.to_string()),
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

                self.command
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &cmd_val.to_string(),
                        creates_val.as_ref().map(|v| v.to_string()),
                        removes_val.as_ref().map(|v| v.to_string()),
                    )
                    .await
            }

            ModuleCall::Shell {
                command,
                chdir,
                creates,
                removes,
            } => {
                let cmd_val = evaluate_expression(command, ctx)?;
                let chdir_val = chdir
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let creates_val = creates
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let removes_val = removes
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                self.shell
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &cmd_val.to_string(),
                        chdir_val.as_ref().map(|v| v.to_string()),
                        creates_val.as_ref().map(|v| v.to_string()),
                        removes_val.as_ref().map(|v| v.to_string()),
                    )
                    .await
            }

            ModuleCall::User {
                name,
                state,
                uid,
                gid,
                groups,
                shell,
                home,
                create_home,
            } => {
                let name_val = evaluate_expression(name, ctx)?;
                let uid_val = uid
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let gid_val = gid
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let shell_val = shell
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let home_val = home
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                let groups_val: Result<Vec<_>, _> = groups
                    .iter()
                    .map(|e| evaluate_expression(e, ctx).map(|v| v.to_string()))
                    .collect();

                self.user
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &name_val.to_string(),
                        *state,
                        uid_val.as_ref().and_then(|v| v.as_i64()).map(|i| i as u32),
                        gid_val.as_ref().and_then(|v| v.as_i64()).map(|i| i as u32),
                        groups_val?,
                        shell_val.as_ref().map(|v| v.to_string()),
                        home_val.as_ref().map(|v| v.to_string()),
                        *create_home,
                    )
                    .await
            }

            ModuleCall::RunFunction { name, args: _ } => {
                // Function execution is handled by the runtime
                Err(NexusError::Runtime {
                    function: Some(name.clone()),
                    message: "Function execution not yet implemented".to_string(),
                    suggestion: None,
                })
            }

            ModuleCall::Template {
                src,
                dest,
                owner,
                group,
                mode,
            } => {
                let src_val = evaluate_expression(src, ctx)?;
                let dest_val = evaluate_expression(dest, ctx)?;
                let owner_val = owner
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let group_val = group
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;
                let mode_val = mode
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                // Check mode - skip template rendering and just report intent
                if ctx.check_mode {
                    let mut msg = format!("Would deploy template {} to {}", src_val, dest_val);
                    if owner_val.is_some() || group_val.is_some() || mode_val.is_some() {
                        msg.push_str(" with");
                        if let Some(ref o) = owner_val {
                            msg.push_str(&format!(" owner={}", o));
                        }
                        if let Some(ref g) = group_val {
                            msg.push_str(&format!(" group={}", g));
                        }
                        if let Some(ref m) = mode_val {
                            msg.push_str(&format!(" mode={}", m));
                        }
                    }
                    return Ok(TaskOutput::changed().with_stdout(msg));
                }

                // Read and process the template file
                let src_string = src_val.to_string();
                let src_path = std::path::Path::new(&src_string);
                if !src_path.exists() {
                    return Err(NexusError::Io {
                        message: format!("Template file not found: {}", src_val),
                        path: Some(src_path.to_path_buf()),
                    });
                }

                // Render template with context
                let mut engine = TemplateEngine::new();
                // Add the template's directory to search paths for includes
                if let Some(parent) = src_path.parent() {
                    engine.add_search_path(parent.to_string_lossy().to_string());
                }

                let rendered = engine.render_file(src_path, ctx)?;

                // Write the rendered content to the destination
                self.file
                    .execute_with_params(
                        ctx,
                        conn.as_connection(),
                        &dest_val.to_string(),
                        crate::parser::ast::FileState::File,
                        None, // Don't use source, we have content
                        Some(rendered),
                        owner_val.as_ref().map(|v| v.to_string()),
                        group_val.as_ref().map(|v| v.to_string()),
                        mode_val.as_ref().map(|v| v.to_string()),
                    )
                    .await
            }

            ModuleCall::Facts { categories } => {
                use crate::executor::facts::{FactCategory, FactGatherer};
                use std::collections::HashMap;

                // Check mode - skip fact gathering and just report intent
                if ctx.check_mode {
                    let msg = format!("Would gather facts: {}", categories.join(", "));
                    return Ok(TaskOutput::success().with_stdout(msg));
                }

                // Parse category strings into FactCategory enum
                let fact_categories: Vec<FactCategory> = categories
                    .iter()
                    .filter_map(|cat| match cat.to_lowercase().as_str() {
                        "system" => Some(FactCategory::System),
                        "hardware" => Some(FactCategory::Hardware),
                        "network" => Some(FactCategory::Network),
                        "mounts" => Some(FactCategory::Mounts),
                        "packages" => Some(FactCategory::Packages),
                        "services" => Some(FactCategory::Services),
                        "environment" => Some(FactCategory::Environment),
                        "all" => Some(FactCategory::All),
                        _ => None,
                    })
                    .collect();

                // If no valid categories or empty, gather all
                let cats_to_gather = if fact_categories.is_empty() {
                    vec![FactCategory::All]
                } else {
                    fact_categories
                };

                // Gather facts - currently only supported for SSH connections
                let facts = match conn {
                    AnyConnection::Ssh(ssh_conn) => {
                        FactGatherer::gather(ssh_conn, &cats_to_gather)?
                    }
                    AnyConnection::Local(_) => {
                        // TODO: Implement local fact gathering
                        return Err(NexusError::Runtime {
                            function: Some("facts".to_string()),
                            message: "Fact gathering not yet implemented for local connections"
                                .to_string(),
                            suggestion: Some("Use SSH connection for fact gathering".to_string()),
                        });
                    }
                };

                // Convert facts to Ansible-compatible names and store in context
                let mut ansible_facts = HashMap::new();
                for (key, value) in &facts {
                    // Map internal fact names to ansible_* names
                    let ansible_key = match key.as_str() {
                        "hostname" => "ansible_hostname",
                        "hostname_short" => "ansible_hostname_short",
                        "os_family" => "ansible_os_family",
                        "os_name" => "ansible_distribution",
                        "os_version" => "ansible_distribution_version",
                        "kernel_version" => "ansible_kernel",
                        "architecture" => "ansible_architecture",
                        "cpu_count" => "ansible_processor_count",
                        "memory_total_mb" => "ansible_memtotal_mb",
                        "default_ipv4" => "ansible_default_ipv4_address",
                        "interfaces" => "ansible_interfaces",
                        _ => key.as_str(),
                    };
                    ansible_facts.insert(ansible_key.to_string(), value.clone());
                }

                // Store all facts in context variables
                for (key, value) in ansible_facts {
                    ctx.set_var(&key, value);
                }

                // Create output
                let fact_count = facts.len();
                let mut output =
                    TaskOutput::success().with_stdout(format!("Gathered {} facts", fact_count));

                // Store the facts in the output data
                output.data = facts;

                Ok(output)
            }

            ModuleCall::Log { message } => {
                let msg = evaluate_expression(message, ctx)?;
                Ok(TaskOutput::success().with_stdout(msg.to_string()))
            }

            ModuleCall::Set { name, value } => {
                let val = evaluate_expression(value, ctx)?;
                ctx.set_var(name, val.clone());
                Ok(TaskOutput::success().with_stdout(format!("Set {} = {}", name, val)))
            }

            ModuleCall::Fail { message } => {
                let msg = evaluate_expression(message, ctx)?;
                Err(NexusError::Runtime {
                    function: Some("fail".to_string()),
                    message: msg.to_string(),
                    suggestion: None,
                })
            }

            ModuleCall::Assert { condition, message } => {
                use crate::parser::ast::Value;
                let cond = evaluate_expression(condition, ctx)?;
                let is_true = match &cond {
                    Value::Bool(b) => *b,
                    Value::String(s) => !s.is_empty() && s != "false" && s != "0",
                    Value::Int(i) => *i != 0,
                    Value::Null => false,
                    _ => true,
                };

                if is_true {
                    Ok(TaskOutput::success().with_stdout("Assertion passed"))
                } else {
                    let msg = if let Some(m) = message {
                        evaluate_expression(m, ctx)?.to_string()
                    } else {
                        "Assertion failed".to_string()
                    };
                    Err(NexusError::Runtime {
                        function: Some("assert".to_string()),
                        message: msg,
                        suggestion: None,
                    })
                }
            }

            ModuleCall::Raw { command } => {
                let cmd = evaluate_expression(command, ctx)?;
                // Raw executes without shell wrapping
                self.command
                    .execute_with_params(ctx, conn.as_connection(), &cmd.to_string(), None, None)
                    .await
            }

            ModuleCall::Git {
                repo,
                dest,
                version,
                force,
            } => {
                let repo_val = evaluate_expression(repo, ctx)?;
                let dest_val = evaluate_expression(dest, ctx)?;
                let version_val = version
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                // Helper function to quote strings safely for shell
                fn shell_quote(s: &str) -> String {
                    format!("'{}'", s.replace('\'', "'\\''"))
                }

                // Check if destination exists and is a git repo
                let dest_quoted = shell_quote(&dest_val.to_string());
                let check_exists = conn
                    .as_connection()
                    .exec(&format!("test -d {}/.git", dest_quoted))
                    .await?
                    .success();

                if check_exists {
                    // Repo exists - check if we need to update it
                    let current_remote = conn
                        .as_connection()
                        .exec(&format!(
                            "cd {} && git config --get remote.origin.url",
                            dest_quoted
                        ))
                        .await?;

                    let remote_matches = current_remote.success()
                        && current_remote.stdout.trim() == repo_val.to_string();

                    if !remote_matches && !force.unwrap_or(false) {
                        return Err(NexusError::Module(Box::new(ModuleError {
                            module: "git".to_string(),
                            task_name: String::new(),
                            host: conn.as_connection().host_name().to_string(),
                            message: format!(
                                "Directory {} exists but is not the expected repo. Use force=true to overwrite",
                                dest_val
                            ),
                            stderr: None,
                            suggestion: Some("Set force: true to remove and re-clone".to_string()),
                        })));
                    }

                    // Check version if specified
                    if let Some(ref v) = version_val {
                        let current_branch = conn
                            .as_connection()
                            .exec(&format!(
                                "cd {} && git rev-parse --abbrev-ref HEAD",
                                dest_quoted
                            ))
                            .await?;

                        let version_str = v.to_string();
                        if current_branch.success() && current_branch.stdout.trim() == version_str {
                            // Already at correct version
                            return Ok(TaskOutput::success().with_stdout(format!(
                                "Repository {} already at version {}",
                                dest_val, version_str
                            )));
                        }

                        // Need to checkout the version
                        let checkout_cmd = format!(
                            "cd {} && git fetch origin && git checkout {}",
                            dest_quoted,
                            shell_quote(&version_str)
                        );
                        let result = conn
                            .as_connection()
                            .exec(&ctx.wrap_command(&checkout_cmd))
                            .await?;

                        if result.success() {
                            return Ok(TaskOutput::changed().with_stdout(format!(
                                "Checked out version {} in {}",
                                version_str, dest_val
                            )));
                        } else {
                            return Err(NexusError::Module(Box::new(ModuleError {
                                module: "git".to_string(),
                                task_name: String::new(),
                                host: conn.as_connection().host_name().to_string(),
                                message: format!("Failed to checkout version {}", version_str),
                                stderr: Some(result.stderr),
                                suggestion: None,
                            })));
                        }
                    } else {
                        // No version specified, repo exists - just update it
                        let pull_cmd = format!("cd {} && git pull", dest_quoted);
                        let result = conn
                            .as_connection()
                            .exec(&ctx.wrap_command(&pull_cmd))
                            .await?;

                        if result.success() {
                            // Check if anything was updated
                            let already_up_to_date = result.stdout.contains("Already up to date")
                                || result.stdout.contains("Already up-to-date");

                            if already_up_to_date {
                                return Ok(TaskOutput::success().with_stdout(format!(
                                    "Repository {} already up to date",
                                    dest_val
                                )));
                            } else {
                                return Ok(TaskOutput::changed()
                                    .with_stdout(format!("Updated repository {}", dest_val)));
                            }
                        }
                    }
                }

                // Repo doesn't exist or force=true - clone it
                let repo_quoted = shell_quote(&repo_val.to_string());
                let mut cmd = format!("git clone {}", repo_quoted);
                if let Some(ref v) = version_val {
                    cmd.push_str(&format!(" --branch {}", shell_quote(&v.to_string())));
                }

                if force.unwrap_or(false) && check_exists {
                    // For force, remove dest first then clone
                    cmd = format!("rm -rf {} && {}", dest_quoted, cmd);
                }

                cmd.push_str(&format!(" {}", dest_quoted));

                self.shell
                    .execute_with_params(ctx, conn.as_connection(), &cmd, None, None, None)
                    .await
            }

            ModuleCall::Http {
                url,
                method,
                body,
                headers: _,
            } => {
                let url_val = evaluate_expression(url, ctx)?;
                let body_val = body
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                // Build curl command
                let method_str = method.as_deref().unwrap_or("GET");
                let mut cmd = format!("curl -s -X {} '{}'", method_str, url_val);
                if let Some(ref b) = body_val {
                    cmd.push_str(&format!(" -d '{}'", b));
                }

                self.shell
                    .execute_with_params(ctx, conn.as_connection(), &cmd, None, None, None)
                    .await
            }

            ModuleCall::Group { name, state, gid } => {
                let name_val = evaluate_expression(name, ctx)?;
                let gid_val = gid
                    .as_ref()
                    .map(|e| evaluate_expression(e, ctx))
                    .transpose()?;

                let cmd = match state {
                    crate::parser::ast::UserState::Present => {
                        let mut c = format!("groupadd {}", name_val);
                        if let Some(ref g) = gid_val {
                            c.push_str(&format!(" -g {}", g));
                        }
                        // Add || true to handle already exists case
                        format!("{} 2>/dev/null || grep -q '^{}:' /etc/group", c, name_val)
                    }
                    crate::parser::ast::UserState::Absent => {
                        format!("groupdel {} 2>/dev/null || true", name_val)
                    }
                };

                self.shell
                    .execute_with_params(ctx, conn.as_connection(), &cmd, None, None, None)
                    .await
            }
        }
    }
}

impl Default for ModuleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect the package manager on a system
pub async fn detect_package_manager(conn: &dyn Connection) -> Result<PackageManager, NexusError> {
    // Check for various package managers
    let checks = [
        ("which dnf 2>/dev/null", PackageManager::Dnf),
        ("which yum 2>/dev/null", PackageManager::Yum),
        ("which apt-get 2>/dev/null", PackageManager::Apt),
        ("which zypper 2>/dev/null", PackageManager::Zypper),
        ("which pacman 2>/dev/null", PackageManager::Pacman),
        ("which apk 2>/dev/null", PackageManager::Apk),
    ];

    for (cmd, manager) in checks {
        let result = conn.exec(cmd).await?;
        if result.success() && !result.stdout.trim().is_empty() {
            return Ok(manager);
        }
    }

    Err(NexusError::Module(Box::new(ModuleError {
        module: "package".to_string(),
        task_name: String::new(),
        host: conn.host_name().to_string(),
        message: "Could not detect package manager".to_string(),
        stderr: None,
        suggestion: Some("Ensure the system has a supported package manager".to_string()),
    })))
}

/// Supported package managers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Dnf,
    Yum,
    Apt,
    Zypper,
    Pacman,
    Apk,
}

impl PackageManager {
    pub fn install_cmd(&self, package: &str) -> String {
        match self {
            PackageManager::Dnf => format!("dnf install -y {}", package),
            PackageManager::Yum => format!("yum install -y {}", package),
            PackageManager::Apt => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                package
            ),
            PackageManager::Zypper => format!("zypper install -y {}", package),
            PackageManager::Pacman => format!("pacman -S --noconfirm {}", package),
            PackageManager::Apk => format!("apk add {}", package),
        }
    }

    pub fn remove_cmd(&self, package: &str) -> String {
        match self {
            PackageManager::Dnf => format!("dnf remove -y {}", package),
            PackageManager::Yum => format!("yum remove -y {}", package),
            PackageManager::Apt => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get remove -y {}",
                package
            ),
            PackageManager::Zypper => format!("zypper remove -y {}", package),
            PackageManager::Pacman => format!("pacman -R --noconfirm {}", package),
            PackageManager::Apk => format!("apk del {}", package),
        }
    }

    pub fn update_cmd(&self, package: &str) -> String {
        match self {
            PackageManager::Dnf => format!("dnf upgrade -y {}", package),
            PackageManager::Yum => format!("yum update -y {}", package),
            PackageManager::Apt => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get upgrade -y {}",
                package
            ),
            PackageManager::Zypper => format!("zypper update -y {}", package),
            PackageManager::Pacman => format!("pacman -Syu --noconfirm {}", package),
            PackageManager::Apk => format!("apk upgrade {}", package),
        }
    }

    pub fn check_installed_cmd(&self, package: &str) -> String {
        match self {
            PackageManager::Dnf | PackageManager::Yum => {
                format!("rpm -q {} >/dev/null 2>&1", package)
            }
            PackageManager::Apt => {
                format!("dpkg -l {} 2>/dev/null | grep -q '^ii'", package)
            }
            PackageManager::Zypper => {
                format!("rpm -q {} >/dev/null 2>&1", package)
            }
            PackageManager::Pacman => {
                format!("pacman -Q {} >/dev/null 2>&1", package)
            }
            PackageManager::Apk => {
                format!("apk info -e {} >/dev/null 2>&1", package)
            }
        }
    }
}
