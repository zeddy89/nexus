use std::collections::HashMap;
use serde_yaml::Value;

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
        mappings.insert("yum", ModuleMapping {
            nexus_module: "package",
            nexus_action: None,
            arg_converter: convert_package_module,
        });
        mappings.insert("dnf", ModuleMapping {
            nexus_module: "package",
            nexus_action: None,
            arg_converter: convert_package_module,
        });
        mappings.insert("apt", ModuleMapping {
            nexus_module: "package",
            nexus_action: None,
            arg_converter: convert_package_module,
        });
        mappings.insert("package", ModuleMapping {
            nexus_module: "package",
            nexus_action: None,
            arg_converter: convert_package_module,
        });

        // Service management
        mappings.insert("service", ModuleMapping {
            nexus_module: "service",
            nexus_action: None,
            arg_converter: convert_service_module,
        });
        mappings.insert("systemd", ModuleMapping {
            nexus_module: "service",
            nexus_action: None,
            arg_converter: convert_service_module,
        });

        // File operations
        mappings.insert("copy", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("copy"),
            arg_converter: convert_copy_module,
        });
        mappings.insert("template", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("template"),
            arg_converter: convert_template_module,
        });
        mappings.insert("file", ModuleMapping {
            nexus_module: "file",
            nexus_action: None,
            arg_converter: convert_file_module,
        });
        mappings.insert("stat", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("stat"),
            arg_converter: convert_stat_module,
        });
        mappings.insert("lineinfile", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("line"),
            arg_converter: convert_lineinfile_module,
        });
        mappings.insert("blockinfile", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("block"),
            arg_converter: convert_blockinfile_module,
        });
        mappings.insert("get_url", ModuleMapping {
            nexus_module: "file",
            nexus_action: Some("download"),
            arg_converter: convert_get_url_module,
        });

        // User/group management
        mappings.insert("user", ModuleMapping {
            nexus_module: "user",
            nexus_action: None,
            arg_converter: convert_user_module,
        });
        mappings.insert("group", ModuleMapping {
            nexus_module: "group",
            nexus_action: None,
            arg_converter: convert_group_module,
        });

        // Commands
        mappings.insert("command", ModuleMapping {
            nexus_module: "command",
            nexus_action: None,
            arg_converter: convert_command_module,
        });
        mappings.insert("shell", ModuleMapping {
            nexus_module: "shell",
            nexus_action: None,
            arg_converter: convert_shell_module,
        });
        mappings.insert("raw", ModuleMapping {
            nexus_module: "raw",
            nexus_action: None,
            arg_converter: convert_raw_module,
        });

        // Git
        mappings.insert("git", ModuleMapping {
            nexus_module: "git",
            nexus_action: None,
            arg_converter: convert_git_module,
        });

        // HTTP/URI
        mappings.insert("uri", ModuleMapping {
            nexus_module: "http",
            nexus_action: None,
            arg_converter: convert_uri_module,
        });

        // Debug/logging
        mappings.insert("debug", ModuleMapping {
            nexus_module: "log",
            nexus_action: None,
            arg_converter: convert_debug_module,
        });
        mappings.insert("fail", ModuleMapping {
            nexus_module: "fail",
            nexus_action: None,
            arg_converter: convert_fail_module,
        });
        mappings.insert("assert", ModuleMapping {
            nexus_module: "assert",
            nexus_action: None,
            arg_converter: convert_assert_module,
        });

        // Variables
        mappings.insert("set_fact", ModuleMapping {
            nexus_module: "set",
            nexus_action: None,
            arg_converter: convert_set_fact_module,
        });
        mappings.insert("include_vars", ModuleMapping {
            nexus_module: "vars",
            nexus_action: None,
            arg_converter: convert_include_vars_module,
        });

        // Include/import
        mappings.insert("include_tasks", ModuleMapping {
            nexus_module: "include",
            nexus_action: None,
            arg_converter: convert_include_tasks_module,
        });
        mappings.insert("import_tasks", ModuleMapping {
            nexus_module: "import",
            nexus_action: None,
            arg_converter: convert_import_tasks_module,
        });

        Self { mappings }
    }

    /// Convert an Ansible module invocation to Nexus format
    pub fn convert(&self, module_name: &str, args: &Value) -> Result<ModuleConversionResult, String> {
        if let Some(mapping) = self.mappings.get(module_name) {
            (mapping.arg_converter)(args)
        } else {
            // Unknown module - flag for manual review
            Ok(ModuleConversionResult {
                action_line: format!("# TODO: Manual conversion needed for '{}' module", module_name),
                additional_lines: vec![
                    format!("# Original: {}: {}", module_name, serde_yaml::to_string(args).unwrap_or_default()),
                ],
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
    value.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
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

// === Module converters ===

fn convert_package_module(args: &Value) -> Result<ModuleConversionResult, String> {
    // Handle update_cache for apt - check if update_cache is present and no name is provided
    if let Some(update_cache) = get_bool(args, "update_cache") {
        let has_name = get_str(args, "name").is_some() || args.get("name").and_then(|v| v.as_sequence()).is_some();
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
        name_array.iter()
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

    let action = match (state.as_deref(), enabled) {
        (Some("started"), Some(true)) => format!("service: enable {} --now", name),
        (Some("started"), Some(false)) => format!("service: start {}", name),
        (Some("started"), None) => format!("service: start {}", name),
        (Some("stopped"), Some(false)) => format!("service: disable {} --now", name),
        (Some("stopped"), _) => format!("service: stop {}", name),
        (Some("restarted"), _) => format!("service: restart {}", name),
        (Some("reloaded"), _) => format!("service: reload {}", name),
        (None, Some(true)) => format!("service: enable {}", name),
        (None, Some(false)) => format!("service: disable {}", name),
        _ => format!("service: status {}", name),
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
        format!("file: copy {} {} {}", src, dest, options.join(" ")).trim().to_string()
    } else if let Some(content) = content {
        format!("file: write {} --content \"{}\" {}", dest, content.replace("\"", "\\\""), options.join(" ")).trim().to_string()
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

    let action_line = format!("file: template {} {} {}", src, dest, options.join(" ")).trim().to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_file_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let path = get_str(args, "path").or_else(|| get_str(args, "dest")).ok_or("Missing 'path' in file module")?;
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
            format!("file: mkdir {} {}", path, opts.join(" ")).trim().to_string()
        }
        "absent" => format!("file: delete {}", path),
        "link" => {
            let src = get_str(args, "src").ok_or("Missing 'src' for symlink")?;
            format!("file: link {} {}", src, path)
        }
        "touch" => format!("file: touch {}", path),
        _ => {
            let mut opts = Vec::new();
            if let Some(owner) = get_str(args, "owner") {
                opts.push(format!("--owner {}", owner));
            }
            if let Some(mode) = get_str(args, "mode") {
                opts.push(format!("--mode {}", mode));
            }
            if opts.is_empty() {
                format!("file: stat {}", path)
            } else {
                format!("file: chmod {} {}", path, opts.join(" ")).trim().to_string()
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
    let path = get_str(args, "path").or_else(|| get_str(args, "dest")).ok_or("Missing 'path' in lineinfile module")?;
    let line = get_str(args, "line");
    let regexp = get_str(args, "regexp");
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action_line = if state == "absent" {
        if let Some(regexp) = regexp {
            format!("file: line {} --remove --regexp \"{}\"", path, regexp)
        } else {
            "# TODO: lineinfile absent requires regexp".to_string()
        }
    } else if let Some(line) = line {
        if let Some(regexp) = regexp {
            format!("file: line {} \"{}\" --regexp \"{}\"", path, line, regexp)
        } else {
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

    let action_line = format!("file: block {} \"{}\" {}", path, block.replace("\"", "\\\""), opts.join(" ")).trim().to_string();

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

    let action_line = format!("file: download {} {} {}", url, dest, opts.join(" ")).trim().to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_user_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").ok_or("Missing 'name' in user module")?;
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action = if state == "absent" { "remove" } else { "create" };

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

    let action_line = format!("user: {} {} {}", action, name, opts.join(" ")).trim().to_string();

    Ok(ModuleConversionResult {
        action_line,
        additional_lines: vec![],
        warnings: vec![],
    })
}

fn convert_group_module(args: &Value) -> Result<ModuleConversionResult, String> {
    let name = get_str(args, "name").ok_or("Missing 'name' in group module")?;
    let state = get_str(args, "state").unwrap_or_else(|| "present".to_string());

    let action = if state == "absent" { "remove" } else { "create" };

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
        get_str(args, "cmd").or_else(|| get_str(args, "_raw_params")).ok_or("Missing command")?
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
        get_str(args, "cmd").or_else(|| get_str(args, "_raw_params")).ok_or("Missing shell command")?
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

    let action_line = format!("git: clone {} {} {}", repo, dest, opts.join(" ")).trim().to_string();

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
            _ => status_code.as_u64().map(|n| n.to_string()).unwrap_or_default(),
        };
        opts.push(format!("--expect-status {}", code_str));
    }

    let action_line = format!("http: {} {} {}", method.to_lowercase(), url, opts.join(" ")).trim().to_string();

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
                    _ => serde_yaml::to_string(value).unwrap_or_default().trim().to_string(),
                };
                lines.push(format!("set: {} = {}", key_str, value_str));
            }
        }
    }

    let action_line = lines.first().cloned().unwrap_or_else(|| "set: # TODO: convert".to_string());
    let additional_lines = if lines.len() > 1 { lines[1..].to_vec() } else { vec![] };

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
        get_str(args, "file").or_else(|| get_str(args, "dir")).ok_or("Missing file in include_vars")?
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
}
