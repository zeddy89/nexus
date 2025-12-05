mod ansible_parser;
mod expression;
mod module_mapper;
mod nexus_writer;
mod report;
mod role_converter;

pub use expression::ExpressionConverter;
pub use module_mapper::{ModuleConversionResult, ModuleMapper, ModuleMapping};
pub use report::{ConversionIssue, ConversionReport, ConversionResult, IssueSeverity};
pub use role_converter::{AnsibleRole, RoleConversionResult, RoleConverter};

use crate::output::errors::NexusError;
use ansible_parser::{parse_playbook, AnsiblePlay, AnsiblePlaybook, AnsibleTask};
use std::fs;
use std::path::{Path, PathBuf};

/// Type alias for play conversion result: (output, (total, converted, review), issues)
type PlayConversionResult =
    Result<(String, (usize, usize, usize), Vec<ConversionIssue>), NexusError>;

/// Normalize Ansible module names by stripping collection prefixes (FQCN).
/// Converts "ansible.builtin.dnf" -> "dnf", "ansible.posix.sysctl" -> "sysctl", etc.
fn normalize_module_name(name: &str) -> String {
    // Common Ansible collection prefixes to strip
    const COLLECTION_PREFIXES: &[&str] = &[
        "ansible.builtin.",
        "ansible.posix.",
        "ansible.netcommon.",
        "ansible.utils.",
        "ansible.windows.",
        "community.general.",
        "community.mysql.",
        "community.postgresql.",
        "community.docker.",
    ];

    for prefix in COLLECTION_PREFIXES {
        if let Some(stripped) = name.strip_prefix(prefix) {
            return stripped.to_string();
        }
    }

    // If no known prefix, return as-is
    name.to_string()
}

/// Options for conversion
#[derive(Debug, Clone, Default)]
pub struct ConversionOptions {
    pub dry_run: bool,
    pub interactive: bool,
    pub convert_all: bool,
    pub include_inventory: bool,
    pub include_templates: bool,
    pub keep_jinja2: bool,
    pub strict: bool,
    pub quiet: bool,
    pub verbose: bool,
}

/// Main converter that orchestrates the conversion process
pub struct Converter {
    pub(crate) options: ConversionOptions,
    expression_converter: ExpressionConverter,
    module_mapper: ModuleMapper,
}

impl Converter {
    pub fn new(options: ConversionOptions) -> Self {
        Self {
            options,
            expression_converter: ExpressionConverter::new(),
            module_mapper: ModuleMapper::new(),
        }
    }

    /// Convert a single file or directory
    pub fn convert(
        &self,
        source: &Path,
        output: Option<&Path>,
    ) -> Result<ConversionReport, NexusError> {
        if source.is_dir() {
            self.convert_directory(source, output)
        } else {
            self.convert_file(source, output)
        }
    }

    /// Assess a directory without converting
    pub fn assess(&self, source: &Path) -> Result<ConversionReport, NexusError> {
        // TODO: Implement assessment mode
        let mut report = ConversionReport::new(source.to_path_buf());
        report.assessment_only = true;
        Ok(report)
    }

    fn convert_file(
        &self,
        source: &Path,
        output: Option<&Path>,
    ) -> Result<ConversionReport, NexusError> {
        let mut report = ConversionReport::new(source.to_path_buf());

        // Parse the Ansible playbook
        let playbook = parse_playbook(source)?;

        // Convert the playbook
        let (converted_content, file_result) = self.convert_playbook(&playbook, source)?;

        // Determine output path
        let output_path = if let Some(out) = output {
            out.to_path_buf()
        } else {
            nexus_writer::generate_output_path(source, None)
        };

        let mut file_result = file_result;
        file_result.output_path = Some(output_path.clone());

        // Write if not dry run
        if !self.options.dry_run {
            nexus_writer::write_nexus_playbook(&output_path, &converted_content)?;
        }

        report.output = Some(output_path);
        report.add_file_result(file_result);

        Ok(report)
    }

    fn convert_directory(
        &self,
        source: &Path,
        output: Option<&Path>,
    ) -> Result<ConversionReport, NexusError> {
        let mut report = ConversionReport::new(source.to_path_buf());

        if let Some(out) = output {
            report.output = Some(out.to_path_buf());
        }

        // First, check for Ansible roles in the directory
        let roles = find_ansible_roles(source)?;

        if !roles.is_empty() {
            // If we found roles, convert them
            let role_output_dir = output
                .map(|o| o.join("nexus-roles"))
                .unwrap_or_else(|| source.join("nexus-roles"));

            let role_converter = role_converter::RoleConverter::new(self);

            for role in &roles {
                match role_converter.convert_role(role, &role_output_dir) {
                    Ok(role_result) => {
                        report.total_roles += 1;
                        // Add each file result from the role to the report
                        for file_result in role_result.file_results {
                            report.add_file_result(file_result);
                        }

                        // Add role-level issues
                        if !role_result.issues.is_empty() {
                            let mut dummy_result = ConversionResult::new(role.path.clone());
                            for issue in role_result.issues {
                                dummy_result.add_issue(issue);
                            }
                            report.add_file_result(dummy_result);
                        }
                    }
                    Err(e) => {
                        let mut result = ConversionResult::new(role.path.clone());
                        result.success = false;
                        result.add_issue(ConversionIssue::error(format!(
                            "Failed to convert role '{}': {}",
                            role.name, e
                        )));
                        report.add_file_result(result);
                    }
                }
            }
        }

        // Find all YAML files (excluding role directories we already processed)
        let yaml_files = find_yaml_files(source)?;

        for yaml_file in yaml_files {
            // Skip files that are inside role directories we already converted
            let skip_file = roles.iter().any(|role| yaml_file.starts_with(&role.path));
            if skip_file {
                continue;
            }

            // Skip files that don't look like playbooks
            if !is_likely_playbook(&yaml_file) {
                continue;
            }

            let relative_path = yaml_file.strip_prefix(source).unwrap_or(&yaml_file);
            let output_path = if let Some(out) = output {
                let mut new_path = out.join(relative_path);
                new_path.set_extension("nx.yml");
                Some(new_path)
            } else {
                None
            };

            match self.convert_single_file(&yaml_file, output_path.as_deref()) {
                Ok(result) => {
                    report.total_playbooks += 1;
                    report.add_file_result(result);
                }
                Err(e) => {
                    let mut result = ConversionResult::new(yaml_file);
                    result.success = false;
                    result.add_issue(ConversionIssue::error(format!("Failed to convert: {}", e)));
                    report.add_file_result(result);
                }
            }
        }

        Ok(report)
    }

    fn convert_single_file(
        &self,
        source: &Path,
        output: Option<&Path>,
    ) -> Result<ConversionResult, NexusError> {
        let playbook = parse_playbook(source)?;
        let (converted_content, mut file_result) = self.convert_playbook(&playbook, source)?;

        let output_path = if let Some(out) = output {
            out.to_path_buf()
        } else {
            nexus_writer::generate_output_path(source, None)
        };

        file_result.output_path = Some(output_path.clone());

        if !self.options.dry_run {
            // Create parent directories if needed
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|e| NexusError::Io {
                    message: format!("Failed to create directory: {}", e),
                    path: Some(parent.to_path_buf()),
                })?;
            }
            nexus_writer::write_nexus_playbook(&output_path, &converted_content)?;
        }

        Ok(file_result)
    }

    fn convert_playbook(
        &self,
        playbook: &AnsiblePlaybook,
        source: &Path,
    ) -> Result<(String, ConversionResult), NexusError> {
        let mut result = ConversionResult::new(source.to_path_buf());
        let mut output = String::new();

        for play in &playbook.plays {
            let (play_output, play_tasks, play_issues) = self.convert_play(play)?;
            output.push_str(&play_output);
            output.push_str("\n---\n");

            result.tasks_total += play_tasks.0;
            result.tasks_converted += play_tasks.1;
            result.tasks_need_review += play_tasks.2;

            for issue in play_issues {
                result.add_issue(issue);
            }
        }

        Ok((output.trim_end_matches("\n---\n").to_string(), result))
    }

    fn convert_play(&self, play: &AnsiblePlay) -> PlayConversionResult {
        let mut output = String::new();
        let mut issues = Vec::new();
        let mut total_tasks = 0;
        let mut converted_tasks = 0;
        let mut review_tasks = 0;

        // Play name
        if let Some(name) = &play.name {
            output.push_str(&format!("name: {}\n", name));
        }

        // Hosts
        output.push_str(&format!("hosts: {}\n", play.hosts));

        // Become -> sudo
        if play.r#become == Some(true) {
            output.push_str("sudo: true\n");
            if let Some(ref user) = play.become_user {
                output.push_str(&format!("sudo_user: {}\n", user));
            }
        }

        // Variables
        if !play.vars.is_empty() {
            output.push_str("\nvars:\n");
            for (key, value) in &play.vars {
                let yaml_value = serde_yaml::to_string(value).unwrap_or_default();
                output.push_str(&format!("  {}: {}", key, yaml_value));
            }
        }

        // Tasks
        if !play.tasks.is_empty() || play.gather_facts == Some(true) {
            output.push_str("\ntasks:\n");

            // Add gather_facts task if enabled
            if play.gather_facts == Some(true) {
                output.push_str("  - name: Gather facts\n");
                output.push_str("    facts: all\n\n");
                total_tasks += 1;
                converted_tasks += 1;
            }

            for task in &play.tasks {
                let (task_output, task_issues, needs_review) = self.convert_task(task)?;
                output.push_str(&task_output);

                total_tasks += 1;
                if needs_review {
                    review_tasks += 1;
                } else {
                    converted_tasks += 1;
                }
                issues.extend(task_issues);
            }
        }

        // Handlers
        if !play.handlers.is_empty() {
            output.push_str("\nhandlers:\n");
            for handler in &play.handlers {
                let (handler_output, handler_issues, _) = self.convert_task(handler)?;
                output.push_str(&handler_output);
                issues.extend(handler_issues);
            }
        }

        Ok((output, (total_tasks, converted_tasks, review_tasks), issues))
    }

    fn convert_task(
        &self,
        task: &AnsibleTask,
    ) -> Result<(String, Vec<ConversionIssue>, bool), NexusError> {
        let mut output = String::new();
        let mut issues = Vec::new();
        let mut needs_review = false;

        // Task name
        output.push_str("  - ");
        if let Some(name) = &task.name {
            output.push_str(&format!("name: {}\n    ", name));
        }

        // Find the module being used
        let mut module_name = None;
        let mut module_args = None;

        // Common modules we should check for
        let known_modules = [
            "yum",
            "dnf",
            "apt",
            "package",
            "service",
            "systemd",
            "copy",
            "template",
            "file",
            "stat",
            "lineinfile",
            "blockinfile",
            "user",
            "group",
            "command",
            "shell",
            "raw",
            "git",
            "get_url",
            "uri",
            "debug",
            "fail",
            "assert",
            "set_fact",
            "include_vars",
            "include_tasks",
            "import_tasks",
        ];

        // First check for short module names directly
        for module in &known_modules {
            if let Some(args) = task.module_args.get(*module) {
                module_name = Some(module.to_string());
                module_args = Some(args.clone());
                break;
            }
        }

        // If not found, check for FQCN (ansible.builtin.*, ansible.posix.*, etc.)
        if module_name.is_none() {
            for (key, args) in &task.module_args {
                let normalized = normalize_module_name(key);
                if known_modules.contains(&normalized.as_str()) {
                    module_name = Some(normalized);
                    module_args = Some(args.clone());
                    break;
                }
            }
        }

        // Convert the module
        if let (Some(name), Some(args)) = (module_name, module_args) {
            match self.module_mapper.convert(&name, &args) {
                Ok(conv_result) => {
                    output.push_str(&conv_result.action_line);
                    output.push('\n');

                    for line in &conv_result.additional_lines {
                        output.push_str(&format!("    {}\n", line));
                    }

                    for warning in conv_result.warnings {
                        issues.push(ConversionIssue::warning(warning));
                        needs_review = true;
                    }
                }
                Err(e) => {
                    output.push_str(&format!("# TODO: {}\n", e));
                    issues.push(ConversionIssue::error(e));
                    needs_review = true;
                }
            }
        } else {
            // Unknown module
            let unknown_modules: Vec<_> = task.module_args.keys().collect();
            if !unknown_modules.is_empty() {
                let first_module = unknown_modules[0];
                output.push_str(&format!("# TODO: Unknown module '{}'\n", first_module));
                issues.push(ConversionIssue::warning(format!(
                    "Unknown module: {}",
                    first_module
                )));
                needs_review = true;
            }
        }

        // When condition
        if let Some(when) = &task.when_condition {
            let when_str = match when {
                serde_yaml::Value::String(s) => s.clone(),
                other => serde_yaml::to_string(other).unwrap_or_default(),
            };
            let converted = self.expression_converter.convert_condition(&when_str);
            output.push_str(&format!("    when: {}\n", converted.output));
        }

        // Register
        if let Some(register) = &task.register {
            output.push_str(&format!("    register: {}\n", register));
        }

        // Notify
        if let Some(notify) = &task.notify {
            let notify_str = match notify {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Sequence(seq) => seq
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => String::new(),
            };
            if !notify_str.is_empty() {
                output.push_str(&format!("    notify: {}\n", notify_str));
            }
        }

        // Loop
        if let Some(loop_expr) = &task.loop_expr {
            let loop_str = match loop_expr {
                serde_yaml::Value::String(s) => {
                    let converted = self.expression_converter.convert_string(s);
                    converted.output
                }
                other => serde_yaml::to_string(other)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            };
            output.push_str(&format!("    loop: {}\n", loop_str));
        } else if let Some(with_items) = &task.with_items {
            let items_str = match with_items {
                serde_yaml::Value::String(s) => {
                    let converted = self.expression_converter.convert_string(s);
                    converted.output
                }
                other => serde_yaml::to_string(other)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            };
            output.push_str(&format!("    loop: {}\n", items_str));
        }

        // Tags
        if let Some(tags) = &task.tags {
            let tags_str = match tags {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Sequence(seq) => seq
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => String::new(),
            };
            if !tags_str.is_empty() {
                output.push_str(&format!("    tags: [{}]\n", tags_str));
            }
        }

        // changed_when
        if let Some(changed_when) = &task.changed_when {
            let expr = match changed_when {
                serde_yaml::Value::Bool(b) => b.to_string(),
                serde_yaml::Value::String(s) => {
                    let converted = self.expression_converter.convert_condition(s);
                    converted.output
                }
                _ => serde_yaml::to_string(changed_when).unwrap_or_default(),
            };
            output.push_str(&format!("    changed_when: {}\n", expr));
        }

        // failed_when
        if let Some(failed_when) = &task.failed_when {
            let expr = match failed_when {
                serde_yaml::Value::Bool(b) => b.to_string(),
                serde_yaml::Value::String(s) => {
                    let converted = self.expression_converter.convert_condition(s);
                    converted.output
                }
                _ => serde_yaml::to_string(failed_when).unwrap_or_default(),
            };
            output.push_str(&format!("    failed_when: {}\n", expr));
        }

        // ignore_errors
        if let Some(ignore_errors) = task.ignore_errors {
            if ignore_errors {
                output.push_str("    ignore_errors: true\n");
            }
        }

        // become -> sudo (task level)
        if task.r#become == Some(true) {
            output.push_str("    sudo: true\n");
            if let Some(ref user) = task.become_user {
                output.push_str(&format!("    sudo_user: {}\n", user));
            }
        }

        output.push('\n');

        Ok((output, issues, needs_review))
    }
}

/// Find all YAML files in a directory recursively
fn find_yaml_files(dir: &Path) -> Result<Vec<PathBuf>, NexusError> {
    let mut files = Vec::new();

    if dir.is_file() {
        if is_yaml_file(dir) {
            files.push(dir.to_path_buf());
        }
        return Ok(files);
    }

    for entry in fs::read_dir(dir).map_err(|e| NexusError::Io {
        message: format!("Failed to read directory: {}", e),
        path: Some(dir.to_path_buf()),
    })? {
        let entry = entry.map_err(|e| NexusError::Io {
            message: format!("Failed to read entry: {}", e),
            path: Some(dir.to_path_buf()),
        })?;
        let path = entry.path();

        if path.is_dir() {
            // Skip common non-playbook directories
            let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
            if dir_name.starts_with('.') || dir_name == "node_modules" || dir_name == "venv" {
                continue;
            }
            files.extend(find_yaml_files(&path)?);
        } else if is_yaml_file(&path) {
            files.push(path);
        }
    }

    Ok(files)
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yml") | Some("yaml")
    )
}

fn is_likely_playbook(path: &Path) -> bool {
    // Skip files that are clearly not playbooks
    let filename = path.file_name().unwrap_or_default().to_string_lossy();

    // Skip inventory files
    if filename == "inventory" || filename == "hosts" {
        return false;
    }

    // Skip requirements files
    if filename.starts_with("requirements") {
        return false;
    }

    // Skip galaxy files
    if filename.starts_with("galaxy") {
        return false;
    }

    // Try to read and detect if it's a playbook
    if let Ok(content) = fs::read_to_string(path) {
        // Ansible playbooks start with a list (plays) that have 'hosts:' key
        content.contains("hosts:") && (content.contains("tasks:") || content.contains("roles:"))
    } else {
        false
    }
}

/// Find all Ansible roles in a directory
fn find_ansible_roles(dir: &Path) -> Result<Vec<role_converter::AnsibleRole>, NexusError> {
    let mut roles = Vec::new();

    // Check if the directory itself is a role
    if let Some(role) = role_converter::AnsibleRole::from_path(dir) {
        roles.push(role);
        return Ok(roles);
    }

    // Look for a "roles" subdirectory (common Ansible structure)
    let roles_dir = dir.join("roles");
    if roles_dir.is_dir() {
        for entry in fs::read_dir(&roles_dir).map_err(|e| NexusError::Io {
            message: format!("Failed to read roles directory: {}", e),
            path: Some(roles_dir.clone()),
        })? {
            let entry = entry.map_err(|e| NexusError::Io {
                message: format!("Failed to read entry: {}", e),
                path: Some(roles_dir.clone()),
            })?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(role) = role_converter::AnsibleRole::from_path(&path) {
                    roles.push(role);
                }
            }
        }
    }

    // Also check for roles in the current directory (less common but possible)
    for entry in fs::read_dir(dir).map_err(|e| NexusError::Io {
        message: format!("Failed to read directory: {}", e),
        path: Some(dir.to_path_buf()),
    })? {
        let entry = entry.map_err(|e| NexusError::Io {
            message: format!("Failed to read entry: {}", e),
            path: Some(dir.to_path_buf()),
        })?;
        let path = entry.path();

        // Skip the "roles" directory as we already processed it
        if path == roles_dir {
            continue;
        }

        if path.is_dir() {
            let dir_name = path.file_name().unwrap_or_default().to_string_lossy();

            // Skip hidden and common non-role directories
            if dir_name.starts_with('.')
                || dir_name == "group_vars"
                || dir_name == "host_vars"
                || dir_name == "inventory"
                || dir_name == "playbooks"
                || dir_name == "nexus-roles"
            {
                continue;
            }

            if let Some(role) = role_converter::AnsibleRole::from_path(&path) {
                roles.push(role);
            }
        }
    }

    Ok(roles)
}
