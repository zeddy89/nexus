use serde_yaml::Value;
use std::collections::HashMap;

/// Maps Ansible modules to Nexus smart actions
pub struct ModuleMapper {
    mappings: HashMap<&'static str, ModuleMapping>,
}

#[derive(Clone)]
pub struct ModuleMapping {
    pub nexus_module: &'static str,
    pub nexus_action: Option<&'static str>,
    pub arg_converter: fn(&Value) -> Result<ModuleConversionResult, String>,
}

#[derive(Debug, Clone)]
pub struct ModuleConversionResult {
    pub action_line: String,
    pub additional_lines: Vec<String>,
    pub warnings: Vec<String>,
}

impl ModuleMapper {
    pub fn new() -> Self {
        let mut mappings = HashMap::new();

        // Package managers → package:
        mappings.insert(
            "yum",
            ModuleMapping {
                nexus_module: "package",
                nexus_action: None,
                arg_converter: convert_package_module,
            },
        );
        mappings.insert(
            "dnf",
            ModuleMapping {
                nexus_module: "package",
                nexus_action: None,
                arg_converter: convert_package_module,
            },
        );
        mappings.insert(
            "apt",
            ModuleMapping {
                nexus_module: "package",
                nexus_action: None,
                arg_converter: convert_package_module,
            },
        );
        mappings.insert(
            "package",
            ModuleMapping {
                nexus_module: "package",
                nexus_action: None,
                arg_converter: convert_package_module,
            },
        );

        // Service management
        mappings.insert(
            "service",
            ModuleMapping {
                nexus_module: "service",
                nexus_action: None,
                arg_converter: convert_service_module,
            },
        );
        mappings.insert(
            "systemd",
            ModuleMapping {
                nexus_module: "service",
                nexus_action: None,
                arg_converter: convert_service_module,
            },
        );

        // File operations
        mappings.insert(
            "copy",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("copy"),
                arg_converter: convert_copy_module,
            },
        );
        mappings.insert(
            "template",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("template"),
                arg_converter: convert_template_module,
            },
        );
        mappings.insert(
            "file",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: None,
                arg_converter: convert_file_module,
            },
        );
        mappings.insert(
            "stat",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("stat"),
                arg_converter: convert_stat_module,
            },
        );
        mappings.insert(
            "lineinfile",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("line"),
                arg_converter: convert_lineinfile_module,
            },
        );
        mappings.insert(
            "blockinfile",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("block"),
                arg_converter: convert_blockinfile_module,
            },
        );
        mappings.insert(
            "get_url",
            ModuleMapping {
                nexus_module: "file",
                nexus_action: Some("download"),
                arg_converter: convert_get_url_module,
            },
        );

        // User/group management
        mappings.insert(
            "user",
            ModuleMapping {
                nexus_module: "user",
                nexus_action: None,
                arg_converter: convert_user_module,
            },
        );
        mappings.insert(
            "group",
            ModuleMapping {
                nexus_module: "group",
                nexus_action: None,
                arg_converter: convert_group_module,
            },
        );

        // Commands
        mappings.insert(
            "command",
            ModuleMapping {
                nexus_module: "command",
                nexus_action: None,
                arg_converter: convert_command_module,
            },
        );
        mappings.insert(
            "shell",
            ModuleMapping {
                nexus_module: "shell",
                nexus_action: None,
                arg_converter: convert_shell_module,
            },
        );
        mappings.insert(
            "raw",
            ModuleMapping {
                nexus_module: "raw",
                nexus_action: None,
                arg_converter: convert_raw_module,
            },
        );

        // Git
        mappings.insert(
            "git",
            ModuleMapping {
                nexus_module: "git",
                nexus_action: None,
                arg_converter: convert_git_module,
            },
        );

        // HTTP/URI
        mappings.insert(
            "uri",
            ModuleMapping {
                nexus_module: "http",
                nexus_action: None,
                arg_converter: convert_uri_module,
            },
        );

        // Debug/logging
        mappings.insert(
            "debug",
            ModuleMapping {
                nexus_module: "log",
                nexus_action: None,
                arg_converter: convert_debug_module,
            },
        );
        mappings.insert(
            "fail",
            ModuleMapping {
                nexus_module: "fail",
                nexus_action: None,
                arg_converter: convert_fail_module,
            },
        );
        mappings.insert(
            "assert",
            ModuleMapping {
                nexus_module: "assert",
                nexus_action: None,
                arg_converter: convert_assert_module,
            },
        );

        // Variables
        mappings.insert(
            "set_fact",
            ModuleMapping {
                nexus_module: "set",
                nexus_action: None,
                arg_converter: convert_set_fact_module,
            },
        );
        mappings.insert(
            "include_vars",
            ModuleMapping {
                nexus_module: "vars",
                nexus_action: None,
                arg_converter: convert_include_vars_module,
            },
        );

        // Include/import
        mappings.insert(
            "include_tasks",
            ModuleMapping {
                nexus_module: "include",
                nexus_action: None,
                arg_converter: convert_include_tasks_module,
            },
        );
        mappings.insert(
            "import_tasks",
            ModuleMapping {
                nexus_module: "import",
                nexus_action: None,
                arg_converter: convert_import_tasks_module,
            },
        );

        // Meta tasks
        mappings.insert(
            "meta",
            ModuleMapping {
                nexus_module: "meta",
                nexus_action: None,
                arg_converter: convert_meta_module,
            },
        );

        // Wait operations
        mappings.insert(
            "wait_for",
            ModuleMapping {
                nexus_module: "wait",
                nexus_action: None,
                arg_converter: convert_wait_for_module,
            },
        );

        // Pause execution
        mappings.insert(
            "pause",
            ModuleMapping {
                nexus_module: "pause",
                nexus_action: None,
                arg_converter: convert_pause_module,
            },
        );

        // Dynamic inventory (not supported)
        mappings.insert(
            "add_host",
            ModuleMapping {
                nexus_module: "add_host",
                nexus_action: None,
                arg_converter: convert_add_host_module,
            },
        );
        mappings.insert(
            "group_by",
            ModuleMapping {
                nexus_module: "group_by",
                nexus_action: None,
                arg_converter: convert_group_by_module,
            },
        );

        // Script execution
        mappings.insert(
            "script",
            ModuleMapping {
                nexus_module: "script",
                nexus_action: None,
                arg_converter: convert_script_module,
            },
        );

        // Expect (interactive - limited support)
        mappings.insert(
            "expect",
            ModuleMapping {
                nexus_module: "expect",
                nexus_action: None,
                arg_converter: convert_expect_module,
            },
        );

        Self { mappings }
    }

    /// Convert an Ansible module invocation to Nexus format
    pub fn convert(
        &self,
        module_name: &str,
        args: &Value,
    ) -> Result<ModuleConversionResult, String> {
        if let Some(mapping) = self.mappings.get(module_name) {
            (mapping.arg_converter)(args)
        } else {
            // Unknown module - flag for manual review
            Ok(ModuleConversionResult {
                action_line: format!(
                    "# TODO: Manual conversion needed for '{}' module",
                    module_name
                ),
                additional_lines: vec![format!(
                    "# Original: {}: {}",
                    module_name,
                    serde_yaml::to_string(args).unwrap_or_default()
                )],
                warnings: vec![format!("Unknown module: {}", module_name)],
            })
        }
    }

    /// Check if a module is supported
    pub fn is_supported(&self, module_name: &str) -> bool {
        self.mappings.contains_key(module_name)
    }

    /// Get list of all supported modules
    pub fn supported_modules(&self) -> Vec<&str> {
        self.mappings.keys().copied().collect()
    }
}

impl Default for ModuleMapper {
    fn default() -> Self {
        Self::new()
    }
}

// Helper to get string from yaml value
fn get_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| {
        // Try as boolean first
        if let Some(b) = v.as_bool() {
            return Some(b);
        }
        // Handle Ansible's yes/no strings
        if let Some(s) = v.as_str() {
            match s.to_lowercase().as_str() {
                "yes" | "true" | "on" | "1" => Some(true),
                "no" | "false" | "off" | "0" => Some(false),
                _ => None,
            }
        } else {
            None
        }
    })
}

// Helper to escape content for shell/YAML safety
fn escape_content(content: &str) -> String {
    content
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
        .replace("'", "\\'")
}

// === Module converters ===

fn convert_package_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // Handle 'list' parameter (dnf list updates, yum list installed, etc.)
    // This is a query operation, convert to shell command
    if let Some(list_type) = get_str(args, "list") {
        return Ok(ModuleConversionResult {
            action_line: format!(
                "shell: dnf list {} -q 2>/dev/null || yum list {} -q 2>/dev/null",
                list_type, list_type
            ),
            additional_lines: vec![],
            warnings: vec![
                "Converted package list to shell command - output format may differ".to_string(),
            ],
        });
    }

    // Handle update_cache for apt - check if update_cache is present and no name is provided
    if let Some(update_cache) = get_bool(args, "update_cache") {
        let has_name = get_str(args, "name").is_some()
            || args.get("name").and_then(|v| v.as_sequence()).is_some();
        if update_cache && !has_name {
            return Ok(ModuleConversionResult {
                action_line: "package: update".to_string(),
                additional_lines: vec![],
                warnings: vec![],
            });
        }
    }

    // Get name - handle both string and array formats
    let name = if let Some(name_str) = get_str(args, "name") {
        name_str
    } else if let Some(name_array) = args.get("name").and_then(|v| v.as_sequence()) {
        name_array
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        return Err("Missing 'name' in package module".to_string());
    };

    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action = match state.as_str() {
        "present" | "installed" | "latest" => "install",
        "absent" | "removed" => "remove",
        _ => "install",
    };

    let mut options = Vec::new();
    if state == "latest" {
        options.push("--upgrade".to_string());
    }

    // Add support for additional package parameters
    if get_bool(args, "install_weak_deps") == Some(false) {
        options.push("--no-weak-deps".to_string());
    }
    if get_bool(args, "allow_downgrade") == Some(true) {
        options.push("--allow-downgrade".to_string());
    }
    if get_bool(args, "autoremove") == Some(true) {
        options.push("--autoremove".to_string());
    }

    let action_line = if options.is_empty() {
        format!("package: {} {}", action, name)
    } else {
        format!("package: {} {} {}", action, name, options.join(" "))
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_service_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").ok_or("Missing 'name' in service module")?;
    let state = get_str(args, "state");
    let enabled = get_bool(args, "enabled");

    // Improved match logic to handle all cases properly
    let action = match (state.as_deref(), enabled) {
        // State with enabled combinations
        (Some("started"), Some(true)) => format!("service: enable {} --now", name),
        (Some("started"), Some(false)) => format!("service: start {}", name),
        (Some("started"), None) => format!("service: start {}", name),
        (Some("stopped"), Some(false)) => format!("service: disable {} --now", name),
        (Some("stopped"), Some(true)) => {
            // Stop service but keep it enabled (unusual but valid)
            format!("service: stop {}", name)
        }
        (Some("stopped"), None) => format!("service: stop {}", name),
        (Some("restarted"), Some(true)) => {
            format!("service: enable {} && service: restart {}", name, name)
        }
        (Some("restarted"), _) => format!("service: restart {}", name),
        (Some("reloaded"), Some(true)) => {
            format!("service: enable {} && service: reload {}", name, name)
        }
        (Some("reloaded"), _) => format!("service: reload {}", name),
        // No state specified, only enabled
        (None, Some(true)) => format!("service: enable {}", name),
        (None, Some(false)) => format!("service: disable {}", name),
        // No state, no enabled - default to status check
        (None, None) => format!("service: status {}", name),
        // Catch any other states
        (Some(_), _) => format!("service: status {}", name),
    };

    Ok(ModuleConversionResult {
        action_line: action,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_copy_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let src = get_str(args, "src");
    let dest = get_str(args, "dest").ok_or("Missing 'dest' in copy module")?;
    let content = get_str(args, "content");

    let mut options = Vec::new();
    if let Some(owner) = get_str(args, "owner") {
        options.push(format!("--owner {}", owner));
    }
    if let Some(group) = get_str(args, "group") {
        options.push(format!("--group {}", group));
    }
    if let Some(mode) = get_str(args, "mode") {
        options.push(format!("--mode {}", mode));
    }

    let action_line = if let Some(src) = src {
        format!("file: copy {} {} {}", src, dest, options.join(" "))
            .trim()
            .to_string()
    } else if let Some(content) = content {
        // Use proper content escaping for all special characters
        format!(
            "file: write {} --content \"{}\" {}",
            dest,
            escape_content(&content),
            options.join(" ")
        )
        .trim()
        .to_string()
    } else {
        return Err("copy module requires 'src' or 'content'".to_string());
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_template_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let src = get_str(args, "src").ok_or("Missing 'src' in template module")?;
    let dest = get_str(args, "dest").ok_or("Missing 'dest' in template module")?;

    let mut options = Vec::new();
    if let Some(owner) = get_str(args, "owner") {
        options.push(format!("--owner {}", owner));
    }
    if let Some(mode) = get_str(args, "mode") {
        options.push(format!("--mode {}", mode));
    }

    // Add support for backup parameter
    if get_bool(args, "backup") == Some(true) {
        options.push("--backup".to_string());
    }

    // Add support for newline_sequence parameter
    if let Some(newline) = get_str(args, "newline_sequence") {
        let newline_flag = match newline.as_str() {
            "\\n" | "LF" => "--newline lf",
            "\\r\\n" | "CRLF" => "--newline crlf",
            "\\r" | "CR" => "--newline cr",
            _ => "--newline lf",
        };
        options.push(newline_flag.to_string());
    }

    let action_line = format!("file: template {} {} {}", src, dest, options.join(" "))
        .trim()
        .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_file_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let path = get_str(args, "path")
        .or_else(|| get_str(args, "dest"))
        .ok_or("Missing 'path' in file module")?;
    let state = get_str(args, "state").unwrap_or_else(|| "file".to_string());

    let action_line = match state.as_str() {
        "directory" => {
            let mut opts = Vec::new();
            if let Some(owner) = get_str(args, "owner") {
                opts.push(format!("--owner {}", owner));
            }
            if let Some(mode) = get_str(args, "mode") {
                opts.push(format!("--mode {}", mode));
            }
            format!("file: mkdir {} {}", path, opts.join(" "))
                .trim()
                .to_string()
        }
        "absent" => format!("file: delete {}", path),
        "link" => {
            let src = get_str(args, "src").ok_or("Missing 'src' for symlink")?;
            format!("file: link {} {}", src, path)
        }
        "touch" => format!("file: touch {}", path),
        _ => {
            // When state is "file" or not specified, check for mode/owner changes
            let mut opts = Vec::new();
            let has_owner = get_str(args, "owner").is_some();
            let has_group = get_str(args, "group").is_some();
            let has_mode = get_str(args, "mode").is_some();

            if let Some(owner) = get_str(args, "owner") {
                opts.push(format!("--owner {}", owner));
            }
            if let Some(group) = get_str(args, "group") {
                opts.push(format!("--group {}", group));
            }
            if let Some(mode) = get_str(args, "mode") {
                opts.push(format!("--mode {}", mode));
            }

            // Clearer output: if mode/owner/group are set, it's a permission change
            if has_mode || has_owner || has_group {
                format!("file: setperms {} {}", path, opts.join(" "))
                    .trim()
                    .to_string()
            } else {
                // No permissions specified - just stat the file
                format!("file: stat {}", path)
            }
        }
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_stat_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let path = get_str(args, "path").ok_or("Missing 'path' in stat module")?;

    Ok(ModuleConversionResult {
        action_line: format!("file: stat {}", path),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_lineinfile_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let path = get_str(args, "path")
        .or_else(|| get_str(args, "dest"))
        .ok_or("Missing 'path' in lineinfile module")?;
    let line = get_str(args, "line");
    let regexp = get_str(args, "regexp");
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());
    let backrefs = get_bool(args, "backrefs").unwrap_or(false);

    let action_line = if state == "absent" {
        if let Some(regexp) = regexp {
            format!("file: line {} --remove --regexp \"{}\"", path, regexp)
        } else {
            "# TODO: lineinfile absent requires regexp".to_string()
        }
    } else if let Some(line) = line {
        if let Some(regexp) = regexp {
            if backrefs {
                // When backrefs is true, the line can contain \1, \2, etc. for capture groups
                format!(
                    "file: line {} \"{}\" --regexp \"{}\" --backrefs",
                    path, line, regexp
                )
            } else {
                format!("file: line {} \"{}\" --regexp \"{}\"", path, line, regexp)
            }
        } else {
            if backrefs {
                // backrefs without regexp doesn't make sense
                return Ok(ModuleConversionResult {
                    action_line: format!("file: line {} \"{}\"", path, line),
                    additional_lines: vec![],
                    warnings: vec!["backrefs parameter ignored without regexp".to_string()],
                });
            }
            format!("file: line {} \"{}\"", path, line)
        }
    } else {
        "# TODO: lineinfile requires 'line' parameter".to_string()
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_blockinfile_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let path = get_str(args, "path").ok_or("Missing 'path' in blockinfile module")?;
    let block = get_str(args, "block").unwrap_or_default();
    let marker = get_str(args, "marker");

    let mut opts = Vec::new();
    if let Some(marker) = marker {
        opts.push(format!("--marker \"{}\"", marker));
    }

    let action_line = format!(
        "file: block {} \"{}\" {}",
        path,
        block.replace("\"", "\\\""),
        opts.join(" ")
    )
    .trim()
    .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_get_url_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let url = get_str(args, "url").ok_or("Missing 'url' in get_url module")?;
    let dest = get_str(args, "dest").ok_or("Missing 'dest' in get_url module")?;

    let mut opts = Vec::new();
    if let Some(mode) = get_str(args, "mode") {
        opts.push(format!("--mode {}", mode));
    }
    if let Some(checksum) = get_str(args, "checksum") {
        opts.push(format!("--checksum \"{}\"", checksum));
    }

    // Add support for timeout parameter
    if let Some(timeout) = args.get("timeout") {
        let timeout_str = match timeout {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => timeout.as_u64().map(|n| n.to_string()).unwrap_or_default(),
        };
        if !timeout_str.is_empty() {
            opts.push(format!("--timeout {}", timeout_str));
        }
    }

    // Add support for force parameter
    if get_bool(args, "force") == Some(true) {
        opts.push("--force".to_string());
    }

    // Add support for headers parameter
    if let Some(headers) = args.get("headers") {
        if let Some(headers_map) = headers.as_mapping() {
            for (key, value) in headers_map {
                if let (Some(k), Some(v)) = (key.as_str(), value.as_str()) {
                    opts.push(format!("--header \"{}:{}\"", k, v));
                }
            }
        }
    }

    // Add support for validate_certs parameter
    if get_bool(args, "validate_certs") == Some(false) {
        opts.push("--no-verify".to_string());
    }

    let action_line = format!("file: download {} {} {}", url, dest, opts.join(" "))
        .trim()
        .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_user_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").ok_or("Missing 'name' in user module")?;
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action = if state == "absent" {
        "remove"
    } else {
        "create"
    };

    let mut opts = Vec::new();
    if let Some(groups) = get_str(args, "groups") {
        opts.push(format!("--groups {}", groups));
    }
    if let Some(shell) = get_str(args, "shell") {
        opts.push(format!("--shell {}", shell));
    }
    if let Some(home) = get_str(args, "home") {
        opts.push(format!("--home {}", home));
    }
    if let Some(uid) = args.get("uid") {
        let uid_str = match uid {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => uid.as_u64().map(|n| n.to_string()).unwrap_or_default(),
        };
        opts.push(format!("--uid {}", uid_str));
    }

    // Add support for password parameter
    if let Some(password) = get_str(args, "password") {
        opts.push(format!("--password \"{}\"", password));
    }

    // Add support for update_password parameter
    if let Some(update_pw) = get_str(args, "update_password") {
        match update_pw.as_str() {
            "always" => opts.push("--update-password always".to_string()),
            "on_create" => opts.push("--update-password on_create".to_string()),
            _ => {}
        }
    }

    // Add support for append parameter (for groups)
    if get_bool(args, "append") == Some(true) {
        opts.push("--append-groups".to_string());
    }

    // Add support for expires parameter
    if let Some(expires) = args.get("expires") {
        let expires_str = match expires {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => expires.as_i64().map(|n| n.to_string()).unwrap_or_default(),
        };
        if !expires_str.is_empty() {
            opts.push(format!("--expires {}", expires_str));
        }
    }

    let action_line = format!("user: {} {} {}", action, name, opts.join(" "))
        .trim()
        .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_group_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").ok_or("Missing 'name' in group module")?;
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action = if state == "absent" {
        "remove"
    } else {
        "create"
    };

    let action_line = format!("group: {} {}", action, name);

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_command_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // Handle both string form and dict form
    let cmd = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "cmd")
            .or_else(|| get_str(args, "_raw_params"))
            .ok_or("Missing command")?
    };

    let mut opts = Vec::new();
    if let Some(chdir) = get_str(args, "chdir") {
        opts.push(format!("--chdir {}", chdir));
    }
    if let Some(creates) = get_str(args, "creates") {
        opts.push(format!("--creates {}", creates));
    }

    let action_line = if opts.is_empty() {
        format!("command: {}", cmd)
    } else {
        format!("command: {} {}", cmd, opts.join(" "))
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_shell_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let cmd = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "cmd")
            .or_else(|| get_str(args, "_raw_params"))
            .ok_or("Missing shell command")?
    };

    let action_line = format!("shell: {}", cmd);

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_raw_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let cmd = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "_raw_params").ok_or("Missing raw command")?
    };

    Ok(ModuleConversionResult {
        action_line: format!("raw: {}", cmd),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_git_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let repo = get_str(args, "repo").ok_or("Missing 'repo' in git module")?;
    let dest = get_str(args, "dest").ok_or("Missing 'dest' in git module")?;

    let mut opts = Vec::new();
    if let Some(version) = get_str(args, "version") {
        opts.push(format!("--branch {}", version));
    }
    if get_bool(args, "force") == Some(true) {
        opts.push("--force".to_string());
    }

    let action_line = format!("git: clone {} {} {}", repo, dest, opts.join(" "))
        .trim()
        .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_uri_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let url = get_str(args, "url").ok_or("Missing 'url' in uri module")?;
    let method = get_str(args, "method").unwrap_or_else(|| "GET".to_string());

    let mut opts = Vec::new();
    if let Some(body) = get_str(args, "body") {
        opts.push(format!("--body \"{}\"", body));
    }
    if let Some(status_code) = args.get("status_code") {
        let code_str = match status_code {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => status_code
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_default(),
        };
        opts.push(format!("--expect-status {}", code_str));
    }

    let action_line = format!("http: {} {} {}", method.to_lowercase(), url, opts.join(" "))
        .trim()
        .to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_debug_module(args: &Value) -> Result<ModuleConversionResult, String> {
    if let Some(msg) = get_str(args, "msg") {
        Ok(ModuleConversionResult {
            action_line: format!("log: \"{}\"", msg),
            additional_lines: vec![],
            warnings: vec![],
        })
    } else if let Some(var) = get_str(args, "var") {
        Ok(ModuleConversionResult {
            action_line: format!("log: ${{{}}}", var),
            additional_lines: vec![],
            warnings: vec![],
        })
    } else {
        Ok(ModuleConversionResult {
            action_line: "log: debug".to_string(),
            additional_lines: vec![],
            warnings: vec![],
        })
    }
}

fn convert_fail_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let msg = get_str(args, "msg").unwrap_or_else(|| "Task failed".to_string());

    Ok(ModuleConversionResult {
        action_line: format!("fail: \"{}\"", msg),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_assert_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // TODO: Handle 'that' array of conditions
    let msg = get_str(args, "fail_msg").or_else(|| get_str(args, "msg"));

    let warnings = vec!["Assert conditions need manual review".to_string()];

    let action_line = if let Some(msg) = msg {
        format!("assert: true --msg \"{}\"  # TODO: convert conditions", msg)
    } else {
        "assert: true  # TODO: convert conditions".to_string()
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings,
    })
}

fn convert_set_fact_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let mut lines = Vec::new();

    if let Some(map) = args.as_mapping() {
        for (key, value) in map {
            if let Some(key_str) = key.as_str() {
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    _ => serde_yaml::to_string(value)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                lines.push(format!("set: {} = {}", key_str, value_str));
            }
        }
    }

    let action_line = lines
        .first()
        .cloned()
        .unwrap_or_else(|| "set: # TODO: convert".to_string());
    let additional_lines = if lines.len() > 1 {
        lines[1..].to_vec()
    } else {
        vec![]
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines,
        warnings: vec![],
    })
}

fn convert_include_vars_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let file = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "file")
            .or_else(|| get_str(args, "dir"))
            .ok_or("Missing file in include_vars")?
    };

    Ok(ModuleConversionResult {
        action_line: format!("vars: {}", file),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_include_tasks_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let file = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "file").ok_or("Missing file in include_tasks")?
    };

    Ok(ModuleConversionResult {
        action_line: format!("include: {}", file),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_import_tasks_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let file = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "file").ok_or("Missing file in import_tasks")?
    };

    Ok(ModuleConversionResult {
        action_line: format!("import: {}", file),
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_meta_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // Meta module can be a string or have a 'free_form' parameter
    let action = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "free_form")
            .or_else(|| get_str(args, "_raw_params"))
            .unwrap_or_else(|| "noop".to_string())
    };

    let action_line = match action.as_str() {
        "flush_handlers" => "meta: flush_handlers".to_string(),
        "end_play" => "meta: end_play".to_string(),
        "end_host" => "meta: end_host".to_string(),
        "clear_facts" => "meta: clear_facts".to_string(),
        "refresh_inventory" => "meta: refresh_inventory".to_string(),
        "noop" => "meta: noop".to_string(),
        other => format!("meta: {}", other),
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_wait_for_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let host = get_str(args, "host");
    let port = args.get("port").and_then(|v| match v {
        Value::Number(n) => n.as_u64().map(|n| n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    });
    let state = get_str(args, "state").unwrap_or_else(|| "started".to_string());
    let timeout = args.get("timeout").and_then(|v| match v {
        Value::Number(n) => n.as_u64().map(|n| n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    });
    let delay = args.get("delay").and_then(|v| match v {
        Value::Number(n) => n.as_u64().map(|n| n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    });
    let path = get_str(args, "path");

    let mut warnings = vec![];

    // If waiting for a port
    if let (Some(port_val), Some(host_val)) = (&port, &host) {
        let mut opts = Vec::new();
        if let Some(timeout_val) = timeout {
            opts.push(format!("--timeout {}", timeout_val));
        }
        if let Some(delay_val) = delay {
            opts.push(format!("--delay {}", delay_val));
        }

        let action_line = if state == "stopped" || state == "absent" {
            format!(
                "shell: while nc -z {} {} 2>/dev/null; do sleep 1; done {}",
                host_val,
                port_val,
                opts.join(" ")
            )
        } else {
            format!(
                "shell: while ! nc -z {} {} 2>/dev/null; do sleep 1; done {}",
                host_val,
                port_val,
                opts.join(" ")
            )
        };

        warnings.push("wait_for converted to shell command - may need adjustment".to_string());

        Ok(ModuleConversionResult {
            action_line,
            additional_lines: vec![],
            warnings,
        })
    } else if let Some(path_val) = path {
        // Waiting for a file/path
        let action_line = if state == "absent" {
            format!("shell: while [ -e {} ]; do sleep 1; done", path_val)
        } else {
            format!("shell: while [ ! -e {} ]; do sleep 1; done", path_val)
        };

        warnings.push("wait_for path converted to shell command".to_string());

        Ok(ModuleConversionResult {
            action_line,
            additional_lines: vec![],
            warnings,
        })
    } else if let Some(port_val) = port {
        // Port without host defaults to localhost
        let action_line = format!(
            "shell: while ! nc -z localhost {} 2>/dev/null; do sleep 1; done",
            port_val
        );

        warnings.push("wait_for converted to shell command - may need adjustment".to_string());

        Ok(ModuleConversionResult {
            action_line,
            additional_lines: vec![],
            warnings,
        })
    } else {
        // Generic timeout wait
        if let Some(timeout_val) = timeout {
            Ok(ModuleConversionResult {
                action_line: format!("pause: {}", timeout_val),
                additional_lines: vec![],
                warnings: vec!["wait_for timeout converted to pause".to_string()],
            })
        } else {
            Ok(ModuleConversionResult {
                action_line: "# TODO: wait_for requires host/port or path".to_string(),
                additional_lines: vec![],
                warnings: vec!["wait_for missing required parameters".to_string()],
            })
        }
    }
}

fn convert_pause_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let seconds = args.get("seconds").and_then(|v| match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    });
    let minutes = args.get("minutes").and_then(|v| match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    });
    let prompt = get_str(args, "prompt");

    let action_line = if let Some(prompt_text) = prompt {
        format!("pause: prompt \"{}\"", prompt_text.replace("\"", "\\\""))
    } else if let Some(mins) = minutes {
        format!("pause: {}m", mins)
    } else if let Some(secs) = seconds {
        format!("pause: {}s", secs)
    } else {
        // No duration specified - wait for user input
        "pause: prompt \"Press enter to continue\"".to_string()
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_add_host_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").unwrap_or_else(|| "unknown".to_string());
    let groups = get_str(args, "groups");

    let mut comment_lines = vec![
        "# TODO: add_host not supported in Nexus".to_string(),
        format!("# Original: add_host name={}", name),
    ];

    if let Some(groups_val) = groups {
        comment_lines.push(format!("# Groups: {}", groups_val));
    }

    Ok(ModuleConversionResult {
        action_line: comment_lines[0].clone(),
        additional_lines: comment_lines[1..].to_vec(),
        warnings: vec![
            "add_host is not supported - dynamic inventory needs manual conversion".to_string(),
        ],
    })
}

fn convert_group_by_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let key = get_str(args, "key").unwrap_or_else(|| "unknown".to_string());

    Ok(ModuleConversionResult {
        action_line: "# TODO: group_by not supported in Nexus".to_string(),
        additional_lines: vec![format!("# Original: group_by key={}", key)],
        warnings: vec![
            "group_by is not supported - dynamic grouping needs manual conversion".to_string(),
        ],
    })
}

fn convert_script_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // Script can be a string or have a 'cmd' or '_raw_params' parameter
    let script_path = if args.is_string() {
        args.as_str().unwrap().to_string()
    } else {
        get_str(args, "cmd")
            .or_else(|| get_str(args, "_raw_params"))
            .or_else(|| get_str(args, "script"))
            .ok_or("Missing script path")?
    };

    let mut opts = Vec::new();
    if let Some(chdir) = get_str(args, "chdir") {
        opts.push(format!("--chdir {}", chdir));
    }
    if let Some(creates) = get_str(args, "creates") {
        opts.push(format!("--creates {}", creates));
    }

    let action_line = if opts.is_empty() {
        format!("script: {}", script_path)
    } else {
        format!("script: {} {}", script_path, opts.join(" "))
    };

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_expect_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let command = get_str(args, "command").unwrap_or_else(|| "unknown".to_string());
    let responses = args.get("responses");

    let mut comment_lines = vec![
        "# TODO: expect module (interactive) - manual conversion required".to_string(),
        format!("# Command: {}", command),
    ];

    if let Some(resp) = responses {
        comment_lines.push(format!(
            "# Responses: {}",
            serde_yaml::to_string(resp).unwrap_or_default().trim()
        ));
    }

    let warnings = vec![
        "expect module requires interactive input - not well supported".to_string(),
        "Consider using shell with heredoc or expect script".to_string(),
    ];

    Ok(ModuleConversionResult {
        action_line: comment_lines[0].clone(),
        additional_lines: comment_lines[1..].to_vec(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str;

    #[test]
    fn test_package_install() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("name: nginx\nstate: present").unwrap();
        let result = mapper.convert("yum", &args).unwrap();
        assert_eq!(result.action_line, "package: install nginx");
    }

    #[test]
    fn test_package_install_array() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("name: [curl, wget, vim]").unwrap();
        let result = mapper.convert("apt", &args).unwrap();
        assert_eq!(result.action_line, "package: install curl wget vim");
    }

    #[test]
    fn test_package_update_cache() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("update_cache: yes").unwrap();
        let result = mapper.convert("apt", &args).unwrap();
        assert_eq!(result.action_line, "package: update");
    }

    #[test]
    fn test_service_enable() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("name: nginx\nstate: started\nenabled: true").unwrap();
        let result = mapper.convert("service", &args).unwrap();
        assert_eq!(result.action_line, "service: enable nginx --now");
    }

    #[test]
    fn test_service_disable_and_stop() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("name: nginx\nstate: stopped\nenabled: false").unwrap();
        let result = mapper.convert("service", &args).unwrap();
        assert_eq!(result.action_line, "service: disable nginx --now");
    }

    #[test]
    fn test_stat_module() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("path: /etc/config.conf").unwrap();
        let result = mapper.convert("stat", &args).unwrap();
        assert_eq!(result.action_line, "file: stat /etc/config.conf");
    }

    #[test]
    fn test_file_directory() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("path: /opt/app\nstate: directory\nowner: app").unwrap();
        let result = mapper.convert("file", &args).unwrap();
        assert!(result.action_line.contains("file: mkdir /opt/app"));
        assert!(result.action_line.contains("--owner app"));
    }

    #[test]
    fn test_meta_flush_handlers() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("flush_handlers").unwrap();
        let result = mapper.convert("meta", &args).unwrap();
        assert_eq!(result.action_line, "meta: flush_handlers");
    }

    #[test]
    fn test_meta_end_play() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("free_form: end_play").unwrap();
        let result = mapper.convert("meta", &args).unwrap();
        assert_eq!(result.action_line, "meta: end_play");
    }

    #[test]
    fn test_wait_for_port() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("host: localhost\nport: 8080\ntimeout: 300").unwrap();
        let result = mapper.convert("wait_for", &args).unwrap();
        assert!(result.action_line.contains("nc -z localhost 8080"));
        assert!(result.warnings.len() > 0);
    }

    #[test]
    fn test_wait_for_path() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("path: /tmp/ready\nstate: present").unwrap();
        let result = mapper.convert("wait_for", &args).unwrap();
        assert!(result.action_line.contains("while [ ! -e /tmp/ready ]"));
    }

    #[test]
    fn test_pause_seconds() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("seconds: 30").unwrap();
        let result = mapper.convert("pause", &args).unwrap();
        assert_eq!(result.action_line, "pause: 30s");
    }

    #[test]
    fn test_pause_minutes() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("minutes: 5").unwrap();
        let result = mapper.convert("pause", &args).unwrap();
        assert_eq!(result.action_line, "pause: 5m");
    }

    #[test]
    fn test_pause_prompt() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("prompt: \"Press any key to continue\"").unwrap();
        let result = mapper.convert("pause", &args).unwrap();
        assert_eq!(
            result.action_line,
            "pause: prompt \"Press any key to continue\""
        );
    }

    #[test]
    fn test_script_simple() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("/path/to/script.sh").unwrap();
        let result = mapper.convert("script", &args).unwrap();
        assert_eq!(result.action_line, "script: /path/to/script.sh");
    }

    #[test]
    fn test_script_with_options() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("cmd: /path/to/script.sh\nchdir: /tmp").unwrap();
        let result = mapper.convert("script", &args).unwrap();
        assert!(result.action_line.contains("script: /path/to/script.sh"));
        assert!(result.action_line.contains("--chdir /tmp"));
    }

    #[test]
    fn test_add_host_unsupported() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("name: newhost\ngroups: webservers").unwrap();
        let result = mapper.convert("add_host", &args).unwrap();
        assert!(result.action_line.contains("TODO"));
        assert!(result.warnings.len() > 0);
        assert!(result.warnings[0].contains("not supported"));
    }

    #[test]
    fn test_group_by_unsupported() {
        let mapper = ModuleMapper::new();
        let args: Value = from_str("key: ansible_os_family").unwrap();
        let result = mapper.convert("group_by", &args).unwrap();
        assert!(result.action_line.contains("TODO"));
        assert!(result.warnings.len() > 0);
    }

    #[test]
    fn test_expect_unsupported() {
        let mapper = ModuleMapper::new();
        let args: Value =
            from_str("command: passwd user\nresponses:\n  \"(?i)password\": \"secret\"").unwrap();
        let result = mapper.convert("expect", &args).unwrap();
        assert!(result.action_line.contains("TODO"));
        assert!(result.warnings.len() > 0);
        assert!(result.warnings[0].contains("interactive"));
    }

    #[test]
    fn test_all_new_modules_registered() {
        let mapper = ModuleMapper::new();

        // Test that all new modules are registered
        assert!(
            mapper.is_supported("meta"),
            "meta module should be supported"
        );
        assert!(
            mapper.is_supported("wait_for"),
            "wait_for module should be supported"
        );
        assert!(
            mapper.is_supported("pause"),
            "pause module should be supported"
        );
        assert!(
            mapper.is_supported("add_host"),
            "add_host module should be supported"
        );
        assert!(
            mapper.is_supported("group_by"),
            "group_by module should be supported"
        );
        assert!(
            mapper.is_supported("script"),
            "script module should be supported"
        );
        assert!(
            mapper.is_supported("expect"),
            "expect module should be supported"
        );

        // Verify the mapper returns the correct count
        let supported = mapper.supported_modules();
        assert!(
            supported.len() >= 34,
            "Should have at least 34 supported modules, got {}",
            supported.len()
        );
    }
}
