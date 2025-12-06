// File module - manage files, directories, and permissions

use async_trait::async_trait;
use std::path::Path;

use super::Module;
use crate::executor::{Connection, ExecutionContext, SshConnection, TaskOutput};
use crate::output::diff::generate_unified_diff;
use crate::output::errors::{ModuleError, NexusError};
use crate::parser::ast::FileState;

pub struct FileModule;

impl Default for FileModule {
    fn default() -> Self {
        Self::new()
    }
}

impl FileModule {
    pub fn new() -> Self {
        FileModule
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute_with_params(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
        state: FileState,
        source: Option<String>,
        content: Option<String>,
        owner: Option<String>,
        group: Option<String>,
        mode: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        // Check mode
        if ctx.check_mode {
            let action = match state {
                FileState::File => {
                    if source.is_some() {
                        format!("copy to {}", path)
                    } else if content.is_some() {
                        format!("create/update file {}", path)
                    } else {
                        format!("ensure file {} exists", path)
                    }
                }
                FileState::Directory => format!("create directory {}", path),
                FileState::Link => format!(
                    "create symlink {} -> {}",
                    path,
                    source.as_deref().unwrap_or("?")
                ),
                FileState::Absent => format!("remove {}", path),
                FileState::Touch => format!("touch {}", path),
            };
            let mut msg = format!("Would {}", action);
            if owner.is_some() || group.is_some() || mode.is_some() {
                msg.push_str(" with");
                if let Some(ref o) = owner {
                    msg.push_str(&format!(" owner={}", o));
                }
                if let Some(ref g) = group {
                    msg.push_str(&format!(" group={}", g));
                }
                if let Some(ref m) = mode {
                    msg.push_str(&format!(" mode={}", m));
                }
            }

            // Check if there would be changes in check mode
            let mut _has_changes = false;
            let mut diff_str = String::new();

            if state == FileState::File {
                if let Some(ref content_str) = content {
                    // Check if file exists and content differs
                    let exists = conn
                        .exec(&format!("test -f {}", shell_quote(path)))
                        .await?
                        .success();
                    let old_content = if exists {
                        conn.read_file(path).await.ok()
                    } else {
                        None
                    };

                    // Determine if there are changes
                    _has_changes = if let Some(ref old) = old_content {
                        old != content_str
                    } else {
                        // File doesn't exist, so it would be created
                        true
                    };

                    // Generate diff if diff_mode is enabled and there are changes
                    if ctx.diff_mode && _has_changes {
                        diff_str = if let Some(old) = old_content {
                            generate_unified_diff(
                                &old,
                                content_str,
                                &format!("{} (before)", path),
                                &format!("{} (after)", path),
                            )
                        } else {
                            // New file - show all content as additions
                            generate_unified_diff(
                                "",
                                content_str,
                                "/dev/null",
                                &format!("{} (new)", path),
                            )
                        };
                    }
                } else if source.is_some() {
                    // For source files, assume there might be changes in check mode
                    _has_changes = true;
                } else {
                    // Just ensuring file exists
                    let exists = conn
                        .exec(&format!("test -f {}", shell_quote(path)))
                        .await?
                        .success();
                    _has_changes = !exists;
                }
            } else {
                // For other states, assume there might be changes
                _has_changes = true;
            }

            let mut output = if _has_changes {
                TaskOutput::changed().with_stdout(msg)
            } else {
                TaskOutput::success().with_stdout(format!("{} (no changes)", msg))
            };

            if !diff_str.is_empty() {
                output = output.with_diff(diff_str);
            }

            return Ok(output);
        }

        match state {
            FileState::File => {
                self.ensure_file(ctx, conn, path, source, content, owner, group, mode)
                    .await
            }
            FileState::Directory => {
                self.ensure_directory(ctx, conn, path, owner, group, mode)
                    .await
            }
            FileState::Link => {
                if let Some(src) = source {
                    self.ensure_link(ctx, conn, path, &src).await
                } else {
                    Err(NexusError::Module(Box::new(ModuleError {
                        module: "file".to_string(),
                        task_name: String::new(),
                        host: conn.host_name().to_string(),
                        message: "Link requires 'source' parameter".to_string(),
                        stderr: None,
                        suggestion: Some("Add source: /path/to/target".to_string()),
                    })))
                }
            }
            FileState::Absent => self.ensure_absent(ctx, conn, path).await,
            FileState::Touch => self.touch_file(ctx, conn, path, owner, group, mode).await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn ensure_file(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
        source: Option<String>,
        content: Option<String>,
        owner: Option<String>,
        group: Option<String>,
        mode: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        let mut changed = false;
        let mut output_lines = Vec::new();
        let mut diff_output: Option<String> = None;

        // Check if file exists
        let exists = conn
            .exec(&format!("test -f {}", shell_quote(path)))
            .await?
            .success();

        // Handle content
        if let Some(content) = content {
            // Read old content for diff generation
            let old_content = if exists && ctx.diff_mode {
                conn.read_file(path).await.ok()
            } else {
                None
            };

            // Check if content is different (if file exists)
            let needs_update = if exists {
                let current = conn.read_file(path).await.ok();
                current.as_deref() != Some(&content)
            } else {
                true
            };

            if needs_update {
                // Generate diff before writing
                if ctx.diff_mode {
                    if let Some(old) = old_content {
                        diff_output = Some(generate_unified_diff(
                            &old,
                            &content,
                            &format!("{} (before)", path),
                            &format!("{} (after)", path),
                        ));
                    } else if !exists {
                        // New file - show all content as additions
                        diff_output = Some(generate_unified_diff(
                            "",
                            &content,
                            "/dev/null",
                            &format!("{} (new)", path),
                        ));
                    }
                }

                // Create parent directory if needed
                if let Some(parent) = Path::new(path).parent() {
                    let cmd = format!("mkdir -p {}", shell_quote(parent.to_str().unwrap()));
                    conn.exec(&ctx.wrap_command(&cmd)).await?;
                }

                // If sudo is enabled, use tee to write file (SFTP can't use sudo)
                if ctx.sudo {
                    // Use base64 encoding to safely transfer content through shell
                    // Write to temp file first, then atomically move to final location
                    let encoded = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        content.as_bytes(),
                    );
                    let temp_path = format!("{}.nexus-tmp-{}", path, std::process::id());
                    let cmd = format!(
                        "echo {} | base64 -d > {} && mv {} {}",
                        encoded,
                        shell_quote(&temp_path),
                        shell_quote(&temp_path),
                        shell_quote(path)
                    );
                    let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                    if !result.success() {
                        // Clean up temp file on failure
                        let _ = conn
                            .exec(&ctx.wrap_command(&format!("rm -f {}", shell_quote(&temp_path))))
                            .await;
                        return Err(NexusError::Module(Box::new(ModuleError {
                            module: "file".to_string(),
                            task_name: String::new(),
                            host: conn.host_name().to_string(),
                            message: format!("Failed to write file {}", path),
                            stderr: Some(result.stderr),
                            suggestion: None,
                        })));
                    }
                } else {
                    conn.write_file(path, &content).await?;
                }
                changed = true;
                output_lines.push(format!(
                    "{} file {}",
                    if exists { "Updated" } else { "Created" },
                    path
                ));
            }
        } else if let Some(source) = source {
            // Copy from local source
            let local_path = Path::new(&source);
            if !local_path.exists() {
                return Err(NexusError::Io {
                    message: format!("Source file not found: {}", source),
                    path: Some(local_path.to_path_buf()),
                });
            }

            // Check if content differs
            let local_content =
                std::fs::read_to_string(local_path).map_err(|e| NexusError::Io {
                    message: format!("Failed to read source file: {}", e),
                    path: Some(local_path.to_path_buf()),
                })?;

            // Read old content for diff generation
            let old_content = if exists && ctx.diff_mode {
                conn.read_file(path).await.ok()
            } else {
                None
            };

            let needs_update = if exists {
                let remote_content = conn.read_file(path).await.ok();
                remote_content.as_deref() != Some(&local_content)
            } else {
                true
            };

            if needs_update {
                // Generate diff before writing
                if ctx.diff_mode {
                    if let Some(old) = old_content {
                        diff_output = Some(generate_unified_diff(
                            &old,
                            &local_content,
                            &format!("{} (before)", path),
                            &format!("{} (after)", path),
                        ));
                    } else if !exists {
                        // New file - show all content as additions
                        diff_output = Some(generate_unified_diff(
                            "",
                            &local_content,
                            "/dev/null",
                            &format!("{} (new)", path),
                        ));
                    }
                }

                if let Some(parent) = Path::new(path).parent() {
                    let cmd = format!("mkdir -p {}", shell_quote(parent.to_str().unwrap()));
                    conn.exec(&ctx.wrap_command(&cmd)).await?;
                }

                // Write file content via Connection trait
                conn.write_file(path, &local_content).await?;
                changed = true;
                output_lines.push(format!("Copied {} to {}", source, path));
            }
        }

        // Set permissions
        if let Some(ref m) = mode {
            let current_mode = get_file_mode(conn, path).await?;
            if current_mode.as_deref() != Some(m.as_str()) {
                let cmd = format!("chmod {} {}", m, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    return Err(NexusError::Module(Box::new(ModuleError {
                        module: "file".to_string(),
                        task_name: String::new(),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to set mode on {}", path),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })));
                }
                changed = true;
                output_lines.push(format!("Set mode {} on {}", m, path));
            }
        }

        // Set ownership - check if it needs to change first
        if owner.is_some() || group.is_some() {
            let ownership = match (&owner, &group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => unreachable!(),
            };

            // Get current ownership to check if change is needed
            let current_ownership_result = conn
                .exec(&format!(
                    "stat -c '%U:%G' {} 2>/dev/null || stat -f '%Su:%Sg' {}",
                    shell_quote(path),
                    shell_quote(path)
                ))
                .await?;

            let needs_change = !current_ownership_result.success()
                || current_ownership_result.stdout.trim() != ownership;

            if needs_change {
                let cmd = format!("chown {} {}", ownership, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if !result.success() {
                    return Err(NexusError::Module(Box::new(ModuleError {
                        module: "file".to_string(),
                        task_name: String::new(),
                        host: conn.host_name().to_string(),
                        message: format!("Failed to set ownership on {}", path),
                        stderr: Some(result.stderr),
                        suggestion: None,
                    })));
                }
                changed = true;
                output_lines.push(format!("Set ownership {} on {}", ownership, path));
            }
        }

        let mut output = if changed {
            TaskOutput::changed()
        } else {
            TaskOutput::success()
        };

        output = output.with_stdout(output_lines.join("\n"));

        if let Some(diff) = diff_output {
            output = output.with_diff(diff);
        }

        Ok(output)
    }

    async fn ensure_directory(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
        owner: Option<String>,
        group: Option<String>,
        mode: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        let mut changed = false;
        let mut output_lines = Vec::new();

        // Check if directory exists
        let exists = conn
            .exec(&format!("test -d {}", shell_quote(path)))
            .await?
            .success();

        if !exists {
            let cmd = format!("mkdir -p {}", shell_quote(path));
            let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
            if !result.success() {
                return Err(NexusError::Module(Box::new(ModuleError {
                    module: "file".to_string(),
                    task_name: String::new(),
                    host: conn.host_name().to_string(),
                    message: format!("Failed to create directory {}", path),
                    stderr: Some(result.stderr),
                    suggestion: None,
                })));
            }
            changed = true;
            output_lines.push(format!("Created directory {}", path));
        }

        // Set mode - check if it needs to change first
        if let Some(ref m) = mode {
            let current_mode = get_file_mode(conn, path).await?;
            if current_mode.as_deref() != Some(m.as_str()) {
                let cmd = format!("chmod {} {}", m, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if result.success() {
                    changed = true;
                    output_lines.push(format!("Set mode {} on {}", m, path));
                }
            }
        }

        // Set ownership - check if it needs to change first
        if owner.is_some() || group.is_some() {
            let ownership = match (&owner, &group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => unreachable!(),
            };

            // Get current ownership to check if change is needed
            let current_ownership_result = conn
                .exec(&format!(
                    "stat -c '%U:%G' {} 2>/dev/null || stat -f '%Su:%Sg' {}",
                    shell_quote(path),
                    shell_quote(path)
                ))
                .await?;

            let needs_change = !current_ownership_result.success()
                || current_ownership_result.stdout.trim() != ownership;

            if needs_change {
                let cmd = format!("chown {} {}", ownership, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if result.success() {
                    changed = true;
                    output_lines.push(format!("Set ownership {} on {}", ownership, path));
                }
            }
        }

        let output = if changed {
            TaskOutput::changed()
        } else {
            TaskOutput::success()
        };

        Ok(output.with_stdout(output_lines.join("\n")))
    }

    async fn ensure_link(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
        target: &str,
    ) -> Result<TaskOutput, NexusError> {
        // Check current state
        let result = conn
            .exec(&format!("readlink {}", shell_quote(path)))
            .await?;
        let current_target = if result.success() {
            Some(result.stdout.trim().to_string())
        } else {
            None
        };

        if current_target.as_deref() == Some(target) {
            return Ok(TaskOutput::success()
                .with_stdout(format!("Link {} already points to {}", path, target)));
        }

        // Remove existing if different
        if current_target.is_some() {
            let cmd = format!("rm -f {}", shell_quote(path));
            conn.exec(&ctx.wrap_command(&cmd)).await?;
        }

        // Create link
        let cmd = format!("ln -s {} {}", shell_quote(target), shell_quote(path));
        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
        if !result.success() {
            return Err(NexusError::Module(Box::new(ModuleError {
                module: "file".to_string(),
                task_name: String::new(),
                host: conn.host_name().to_string(),
                message: format!("Failed to create link {} -> {}", path, target),
                stderr: Some(result.stderr),
                suggestion: None,
            })));
        }

        Ok(TaskOutput::changed().with_stdout(format!("Created link {} -> {}", path, target)))
    }

    async fn ensure_absent(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
    ) -> Result<TaskOutput, NexusError> {
        // Check if exists
        let exists = conn
            .exec(&format!("test -e {}", shell_quote(path)))
            .await?
            .success();

        if !exists {
            return Ok(TaskOutput::success().with_stdout(format!("{} does not exist", path)));
        }

        let cmd = format!("rm -rf {}", shell_quote(path));
        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
        if !result.success() {
            return Err(NexusError::Module(Box::new(ModuleError {
                module: "file".to_string(),
                task_name: String::new(),
                host: conn.host_name().to_string(),
                message: format!("Failed to remove {}", path),
                stderr: Some(result.stderr),
                suggestion: None,
            })));
        }

        Ok(TaskOutput::changed().with_stdout(format!("Removed {}", path)))
    }

    async fn touch_file(
        &self,
        ctx: &ExecutionContext,
        conn: &dyn Connection,
        path: &str,
        owner: Option<String>,
        group: Option<String>,
        mode: Option<String>,
    ) -> Result<TaskOutput, NexusError> {
        let mut output_lines = Vec::new();

        let exists = conn
            .exec(&format!("test -f {}", shell_quote(path)))
            .await?
            .success();

        let cmd = format!("touch {}", shell_quote(path));
        let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
        if !result.success() {
            return Err(NexusError::Module(Box::new(ModuleError {
                module: "file".to_string(),
                task_name: String::new(),
                host: conn.host_name().to_string(),
                message: format!("Failed to touch {}", path),
                stderr: Some(result.stderr),
                suggestion: None,
            })));
        }

        // Touch always modifies the file (creates or updates timestamp)
        let mut changed = true;
        if !exists {
            output_lines.push(format!("Created {}", path));
        } else {
            output_lines.push(format!("Updated timestamp on {}", path));
        }

        // Set mode - check if it needs to change first
        if let Some(ref m) = mode {
            let current_mode = get_file_mode(conn, path).await?;
            if current_mode.as_deref() != Some(m.as_str()) {
                let cmd = format!("chmod {} {}", m, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if result.success() {
                    changed = true;
                    output_lines.push(format!("Set mode {} on {}", m, path));
                }
            }
        }

        // Set ownership - check if it needs to change first
        if owner.is_some() || group.is_some() {
            let ownership = match (&owner, &group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => unreachable!(),
            };

            // Get current ownership to check if change is needed
            let current_ownership_result = conn
                .exec(&format!(
                    "stat -c '%U:%G' {} 2>/dev/null || stat -f '%Su:%Sg' {}",
                    shell_quote(path),
                    shell_quote(path)
                ))
                .await?;

            let needs_change = !current_ownership_result.success()
                || current_ownership_result.stdout.trim() != ownership;

            if needs_change {
                let cmd = format!("chown {} {}", ownership, shell_quote(path));
                let result = conn.exec(&ctx.wrap_command(&cmd)).await?;
                if result.success() {
                    changed = true;
                    output_lines.push(format!("Set ownership {} on {}", ownership, path));
                }
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
impl Module for FileModule {
    fn name(&self) -> &'static str {
        "file"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        unreachable!()
    }
}

/// Get the mode of a file
async fn get_file_mode(conn: &dyn Connection, path: &str) -> Result<Option<String>, NexusError> {
    let result = conn
        .exec(&format!("stat -c '%a' {} 2>/dev/null", shell_quote(path)))
        .await?;
    if result.success() {
        Ok(Some(result.stdout.trim().to_string()))
    } else {
        Ok(None)
    }
}

/// Shell-quote a string for safe use in commands
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
