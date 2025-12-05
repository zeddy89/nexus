use super::ansible_parser::AnsibleTask;
use super::nexus_writer;
use super::{ConversionIssue, ConversionResult, Converter, IssueSeverity};
use crate::output::errors::NexusError;
use std::fs;
use std::path::{Path, PathBuf};

/// Represents an Ansible role structure
pub struct AnsibleRole {
    pub name: String,
    pub path: PathBuf,
    pub tasks_dir: Option<PathBuf>,
    pub handlers_dir: Option<PathBuf>,
    pub templates_dir: Option<PathBuf>,
    pub files_dir: Option<PathBuf>,
    pub vars_dir: Option<PathBuf>,
    pub defaults_dir: Option<PathBuf>,
    pub meta_file: Option<PathBuf>,
}

impl AnsibleRole {
    /// Detect and parse a role from a directory
    pub fn from_path(path: &Path) -> Option<Self> {
        if !path.is_dir() {
            return None;
        }

        let name = path.file_name()?.to_string_lossy().to_string();

        // Check if this looks like a role (has tasks/ or meta/)
        let tasks_dir = path.join("tasks");
        let meta_file = path.join("meta/main.yml");

        if !tasks_dir.exists() && !meta_file.exists() {
            return None;
        }

        Some(AnsibleRole {
            name,
            path: path.to_path_buf(),
            tasks_dir: if tasks_dir.exists() {
                Some(tasks_dir)
            } else {
                None
            },
            handlers_dir: {
                let h = path.join("handlers");
                if h.exists() {
                    Some(h)
                } else {
                    None
                }
            },
            templates_dir: {
                let t = path.join("templates");
                if t.exists() {
                    Some(t)
                } else {
                    None
                }
            },
            files_dir: {
                let f = path.join("files");
                if f.exists() {
                    Some(f)
                } else {
                    None
                }
            },
            vars_dir: {
                let v = path.join("vars");
                if v.exists() {
                    Some(v)
                } else {
                    None
                }
            },
            defaults_dir: {
                let d = path.join("defaults");
                if d.exists() {
                    Some(d)
                } else {
                    None
                }
            },
            meta_file: if meta_file.exists() {
                Some(meta_file)
            } else {
                None
            },
        })
    }
}

/// Role converter that handles Ansible role to Nexus role conversion
pub struct RoleConverter<'a> {
    converter: &'a Converter,
}

impl<'a> RoleConverter<'a> {
    pub fn new(converter: &'a Converter) -> Self {
        Self { converter }
    }

    /// Convert an Ansible role to Nexus format
    pub fn convert_role(
        &self,
        role: &AnsibleRole,
        output_dir: &Path,
    ) -> Result<RoleConversionResult, NexusError> {
        let mut result = RoleConversionResult::new(&role.name);

        // Create output directory
        let role_output = output_dir.join(&role.name);
        if !self.converter.options.dry_run {
            fs::create_dir_all(&role_output).map_err(|e| NexusError::Io {
                message: format!("Failed to create role directory: {}", e),
                path: Some(role_output.clone()),
            })?;
        }

        // Convert tasks
        if let Some(tasks_dir) = &role.tasks_dir {
            self.convert_tasks_dir(tasks_dir, &role_output.join("tasks"), &mut result)?;
        }

        // Convert handlers
        if let Some(handlers_dir) = &role.handlers_dir {
            self.convert_handlers_dir(handlers_dir, &role_output, &mut result)?;
        }

        // Copy templates (optionally convert)
        if let Some(templates_dir) = &role.templates_dir {
            self.copy_templates(templates_dir, &role_output.join("templates"), &mut result)?;
        }

        // Copy files directory as-is
        if let Some(files_dir) = &role.files_dir {
            self.copy_files(files_dir, &role_output.join("files"), &mut result)?;
        }

        // Merge vars and defaults
        if role.vars_dir.is_some() || role.defaults_dir.is_some() {
            self.merge_vars(
                role.defaults_dir.as_deref(),
                role.vars_dir.as_deref(),
                &role_output.join("vars.yml"),
                &mut result,
            )?;
        }

        // Convert meta
        if let Some(meta_file) = &role.meta_file {
            self.convert_meta(meta_file, &role_output.join("meta.yml"), &mut result)?;
        }

        result.output_path = Some(role_output);
        Ok(result)
    }

    fn convert_tasks_dir(
        &self,
        tasks_dir: &Path,
        output_dir: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        if !self.converter.options.dry_run {
            fs::create_dir_all(output_dir).map_err(|e| NexusError::Io {
                message: format!("Failed to create tasks directory: {}", e),
                path: Some(output_dir.to_path_buf()),
            })?;
        }

        for entry in fs::read_dir(tasks_dir).map_err(|e| NexusError::Io {
            message: format!("Failed to read tasks directory: {}", e),
            path: Some(tasks_dir.to_path_buf()),
        })? {
            let entry = entry.map_err(|e| NexusError::Io {
                message: format!("Failed to read entry: {}", e),
                path: Some(tasks_dir.to_path_buf()),
            })?;
            let path = entry.path();

            if path.is_file() && is_yaml_file(&path) {
                let filename = path.file_stem().unwrap_or_default().to_string_lossy();
                let output_file = output_dir.join(format!("{}.nx.yml", filename));

                match self.convert_task_file(&path, &output_file) {
                    Ok(file_result) => {
                        result.tasks_converted += 1;
                        result.file_results.push(file_result);
                    }
                    Err(e) => {
                        result.add_issue(ConversionIssue::error(format!(
                            "Failed to convert {}: {}",
                            path.display(),
                            e
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn convert_task_file(
        &self,
        source: &Path,
        output: &Path,
    ) -> Result<ConversionResult, NexusError> {
        // Read the tasks file (it's a list of tasks, not a full playbook)
        let content = fs::read_to_string(source).map_err(|e| NexusError::Io {
            message: format!("Failed to read {}: {}", source.display(), e),
            path: Some(source.to_path_buf()),
        })?;

        let mut result = ConversionResult::new(source.to_path_buf());
        result.output_path = Some(output.to_path_buf());

        // Parse as a list of tasks
        let tasks: Vec<AnsibleTask> = serde_yaml::from_str(&content).map_err(|e| {
            NexusError::Parse(Box::new(crate::output::errors::ParseError {
                kind: crate::output::errors::ParseErrorKind::InvalidYaml,
                message: format!("Failed to parse tasks file: {}", e),
                file: Some(source.display().to_string()),
                line: None,
                column: None,
                suggestion: Some("Ensure the tasks file is valid YAML".to_string()),
            }))
        })?;

        let mut output_content = String::new();

        for task in &tasks {
            // For now, just serialize back to YAML with .nx extension
            // In the future, this would call converter.convert_task(task)
            let task_yaml = serde_yaml::to_string(task).map_err(|e| {
                NexusError::Parse(Box::new(crate::output::errors::ParseError {
                    kind: crate::output::errors::ParseErrorKind::InvalidYaml,
                    message: format!("Failed to serialize task: {}", e),
                    file: Some(source.display().to_string()),
                    line: None,
                    column: None,
                    suggestion: None,
                }))
            })?;

            output_content.push_str("- ");
            output_content.push_str(&task_yaml);

            result.tasks_total += 1;
            result.tasks_converted += 1;

            // Add info about tasks that might need review
            if task.module_args.is_empty() {
                result.add_issue(ConversionIssue::info(format!(
                    "Task '{}' has no module arguments",
                    task.name.as_deref().unwrap_or("unnamed")
                )));
            }
        }

        if !self.converter.options.dry_run {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).ok();
            }
            nexus_writer::write_nexus_playbook(output, &output_content)?;
        }

        Ok(result)
    }

    fn convert_handlers_dir(
        &self,
        handlers_dir: &Path,
        role_output: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        let main_handlers = handlers_dir.join("main.yml");
        if main_handlers.exists() {
            let output = role_output.join("handlers.nx.yml");
            match self.convert_task_file(&main_handlers, &output) {
                Ok(file_result) => {
                    result.handlers_converted = true;
                    result.file_results.push(file_result);
                }
                Err(e) => {
                    result.add_issue(ConversionIssue::error(format!(
                        "Failed to convert handlers: {}",
                        e
                    )));
                }
            }
        }
        Ok(())
    }

    fn copy_templates(
        &self,
        templates_dir: &Path,
        output_dir: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        if self.converter.options.dry_run {
            return Ok(());
        }

        fs::create_dir_all(output_dir).map_err(|e| NexusError::Io {
            message: format!("Failed to create templates directory: {}", e),
            path: Some(output_dir.to_path_buf()),
        })?;

        copy_dir_recursive(templates_dir, output_dir)?;
        result.templates_copied = true;

        // If converting templates (not keeping Jinja2), convert them
        if self.converter.options.include_templates && !self.converter.options.keep_jinja2 {
            result.add_issue(ConversionIssue::info(
                "Template conversion enabled but not yet implemented".to_string(),
            ));
        }

        Ok(())
    }

    fn copy_files(
        &self,
        files_dir: &Path,
        output_dir: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        if self.converter.options.dry_run {
            return Ok(());
        }

        fs::create_dir_all(output_dir).map_err(|e| NexusError::Io {
            message: format!("Failed to create files directory: {}", e),
            path: Some(output_dir.to_path_buf()),
        })?;

        copy_dir_recursive(files_dir, output_dir)?;
        result.files_copied = true;

        Ok(())
    }

    fn merge_vars(
        &self,
        defaults_dir: Option<&Path>,
        vars_dir: Option<&Path>,
        output: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        let mut merged_vars = serde_yaml::Mapping::new();

        // Load defaults first (lower priority)
        if let Some(defaults) = defaults_dir {
            let main_file = defaults.join("main.yml");
            if main_file.exists() {
                if let Ok(content) = fs::read_to_string(&main_file) {
                    if let Ok(vars) = serde_yaml::from_str::<serde_yaml::Mapping>(&content) {
                        for (k, v) in vars {
                            merged_vars.insert(k, v);
                        }
                    }
                }
            }
        }

        // Load vars (higher priority, overwrites defaults)
        if let Some(vars) = vars_dir {
            let main_file = vars.join("main.yml");
            if main_file.exists() {
                if let Ok(content) = fs::read_to_string(&main_file) {
                    if let Ok(vars) = serde_yaml::from_str::<serde_yaml::Mapping>(&content) {
                        for (k, v) in vars {
                            merged_vars.insert(k, v);
                        }
                    }
                }
            }
        }

        if !merged_vars.is_empty() && !self.converter.options.dry_run {
            let yaml_content = serde_yaml::to_string(&merged_vars).unwrap_or_default();
            fs::write(output, yaml_content).map_err(|e| NexusError::Io {
                message: format!("Failed to write vars file: {}", e),
                path: Some(output.to_path_buf()),
            })?;
            result.vars_merged = true;
        }

        Ok(())
    }

    fn convert_meta(
        &self,
        meta_file: &Path,
        output: &Path,
        result: &mut RoleConversionResult,
    ) -> Result<(), NexusError> {
        if self.converter.options.dry_run {
            return Ok(());
        }

        // For now, just copy meta file as-is
        if let Ok(content) = fs::read_to_string(meta_file) {
            fs::write(output, content).map_err(|e| NexusError::Io {
                message: format!("Failed to write meta file: {}", e),
                path: Some(output.to_path_buf()),
            })?;
            result.meta_converted = true;
        }

        Ok(())
    }
}

/// Result of converting a role
#[derive(Debug)]
pub struct RoleConversionResult {
    pub name: String,
    pub output_path: Option<PathBuf>,
    pub tasks_converted: usize,
    pub handlers_converted: bool,
    pub templates_copied: bool,
    pub files_copied: bool,
    pub vars_merged: bool,
    pub meta_converted: bool,
    pub issues: Vec<ConversionIssue>,
    pub file_results: Vec<ConversionResult>,
}

impl RoleConversionResult {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            output_path: None,
            tasks_converted: 0,
            handlers_converted: false,
            templates_copied: false,
            files_copied: false,
            vars_merged: false,
            meta_converted: false,
            issues: Vec::new(),
            file_results: Vec::new(),
        }
    }

    pub fn add_issue(&mut self, issue: ConversionIssue) {
        self.issues.push(issue);
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| matches!(i.severity, IssueSeverity::Error))
    }
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yml") | Some("yaml")
    )
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), NexusError> {
    fs::create_dir_all(dst).map_err(|e| NexusError::Io {
        message: format!("Failed to create directory: {}", e),
        path: Some(dst.to_path_buf()),
    })?;

    for entry in fs::read_dir(src).map_err(|e| NexusError::Io {
        message: format!("Failed to read directory: {}", e),
        path: Some(src.to_path_buf()),
    })? {
        let entry = entry.map_err(|e| NexusError::Io {
            message: format!("Failed to read entry: {}", e),
            path: Some(src.to_path_buf()),
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| NexusError::Io {
                message: format!("Failed to copy file: {}", e),
                path: Some(src_path.clone()),
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ansible_role_detection() {
        // This test would need a test fixtures directory
        // For now, just test that None is returned for non-directories
        assert!(AnsibleRole::from_path(Path::new("/nonexistent")).is_none());
    }
}
