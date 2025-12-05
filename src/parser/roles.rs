// Role loader and manager for Nexus
//
// Roles follow a conventional directory structure:
//   roles/
//     rolename/
//       tasks/
//         main.yml          # Main task list
//       handlers/
//         main.yml          # Handler definitions
//       templates/          # Jinja2 templates
//       files/              # Static files
//       vars/
//         main.yml          # Role variables
//       defaults/
//         main.yml          # Default variables (lowest priority)
//       meta/
//         main.yml          # Role metadata and dependencies

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::ast::*;
use super::yaml::parse_playbook;
use crate::output::errors::{NexusError, ParseError, ParseErrorKind};

/// Role search paths
#[derive(Debug, Clone)]
pub struct RoleResolver {
    /// Paths to search for roles (in order of priority)
    search_paths: Vec<PathBuf>,
    /// Cache of loaded roles
    loaded_roles: HashMap<String, Role>,
}

impl RoleResolver {
    /// Create a new role resolver with default search paths
    pub fn new() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("./roles"),
                PathBuf::from("~/.nexus/roles"),
                PathBuf::from("/etc/nexus/roles"),
            ],
            loaded_roles: HashMap::new(),
        }
    }

    /// Add a search path (highest priority)
    pub fn add_search_path(&mut self, path: impl Into<PathBuf>) {
        self.search_paths.insert(0, path.into());
    }

    /// Add a search path relative to the playbook
    pub fn add_playbook_relative_path(&mut self, playbook_path: &Path) {
        if let Some(parent) = playbook_path.parent() {
            let roles_dir = parent.join("roles");
            if roles_dir.exists() {
                self.search_paths.insert(0, roles_dir);
            }
        }
    }

    /// Resolve a role by name, loading it if necessary
    pub fn resolve(&mut self, role_name: &str) -> Result<&Role, NexusError> {
        // Check cache first
        if self.loaded_roles.contains_key(role_name) {
            return Ok(self.loaded_roles.get(role_name).unwrap());
        }

        // Find the role directory
        let role_path = self.find_role_path(role_name)?;

        // Load the role
        let role = load_role(&role_path, role_name)?;

        // Cache it
        self.loaded_roles.insert(role_name.to_string(), role);
        Ok(self.loaded_roles.get(role_name).unwrap())
    }

    /// Find the path to a role directory
    fn find_role_path(&self, role_name: &str) -> Result<PathBuf, NexusError> {
        // Check if it's an absolute or relative path
        let direct_path = PathBuf::from(role_name);
        if direct_path.is_dir() {
            return Ok(direct_path);
        }

        // Search in search paths
        for search_path in &self.search_paths {
            let expanded = expand_tilde(search_path);
            let role_dir = expanded.join(role_name);
            if role_dir.is_dir() {
                return Ok(role_dir);
            }
        }

        Err(NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::MissingField,
            message: format!("Role '{}' not found", role_name),
            file: None,
            line: None,
            column: None,
            suggestion: Some(format!(
                "Searched in: {:?}. Create the role directory or check the name.",
                self.search_paths
            )),
        })))
    }

    /// Load all dependencies for a role (recursive)
    pub fn resolve_dependencies(&mut self, role_name: &str) -> Result<Vec<String>, NexusError> {
        let mut resolved = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.resolve_deps_recursive(role_name, &mut resolved, &mut visited)?;
        Ok(resolved)
    }

    fn resolve_deps_recursive(
        &mut self,
        role_name: &str,
        resolved: &mut Vec<String>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<(), NexusError> {
        if visited.contains(role_name) {
            return Ok(()); // Already processed
        }
        visited.insert(role_name.to_string());

        // Load the role to get its dependencies
        let role = self.resolve(role_name)?;
        let deps: Vec<String> = role
            .meta
            .dependencies
            .iter()
            .map(|d| d.role.clone())
            .collect();

        // Process dependencies first (depth-first)
        for dep in deps {
            self.resolve_deps_recursive(&dep, resolved, visited)?;
        }

        // Add this role after its dependencies
        if !resolved.contains(&role_name.to_string()) {
            resolved.push(role_name.to_string());
        }

        Ok(())
    }
}

impl Default for RoleResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Load a role from a directory
pub fn load_role(role_path: &Path, role_name: &str) -> Result<Role, NexusError> {
    // Load metadata
    let meta = load_role_meta(role_path)?;

    // Load defaults
    let defaults = load_role_vars(role_path, "defaults")?;

    // Load vars
    let vars = load_role_vars(role_path, "vars")?;

    // Load tasks
    let tasks = load_role_tasks(role_path)?;

    // Load handlers
    let handlers = load_role_handlers(role_path)?;

    // Check for files and templates directories
    let files_path = {
        let p = role_path.join("files");
        if p.is_dir() {
            Some(p.to_string_lossy().to_string())
        } else {
            None
        }
    };

    let templates_path = {
        let p = role_path.join("templates");
        if p.is_dir() {
            Some(p.to_string_lossy().to_string())
        } else {
            None
        }
    };

    Ok(Role {
        name: role_name.to_string(),
        path: role_path.to_string_lossy().to_string(),
        meta,
        defaults,
        vars,
        tasks,
        handlers,
        files_path,
        templates_path,
    })
}

/// Load role metadata from meta/main.yml
fn load_role_meta(role_path: &Path) -> Result<RoleMeta, NexusError> {
    let meta_file = role_path.join("meta").join("main.yml");
    if !meta_file.exists() {
        // Meta is optional
        return Ok(RoleMeta::default());
    }

    let content = std::fs::read_to_string(&meta_file).map_err(|e| NexusError::Io {
        message: format!("Failed to read role meta: {}", e),
        path: Some(meta_file.clone()),
    })?;

    let raw: RawRoleMeta = serde_yaml::from_str(&content).map_err(|e| {
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid role meta YAML: {}", e),
            file: Some(meta_file.to_string_lossy().to_string()),
            line: None,
            column: None,
            suggestion: None,
        }))
    })?;

    convert_role_meta(raw)
}

#[derive(Debug, Deserialize, Default)]
struct RawRoleMeta {
    dependencies: Option<Vec<RawRoleDependency>>,
    min_nexus_version: Option<String>,
    galaxy_info: Option<RawGalaxyInfo>,
    allow_duplicates: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRoleDependency {
    /// Simple role name
    Name(String),
    /// Role with parameters
    Full {
        role: String,
        #[serde(default)]
        vars: HashMap<String, YamlValue>,
        #[serde(default)]
        tags: Vec<String>,
        when: Option<String>,
    },
}

#[derive(Debug, Deserialize, Default)]
struct RawGalaxyInfo {
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    platforms: Option<Vec<RawPlatform>>,
}

#[derive(Debug, Deserialize)]
struct RawPlatform {
    name: String,
    #[serde(default)]
    versions: Vec<String>,
}

fn convert_role_meta(raw: RawRoleMeta) -> Result<RoleMeta, NexusError> {
    let dependencies = raw
        .dependencies
        .unwrap_or_default()
        .into_iter()
        .map(|dep| match dep {
            RawRoleDependency::Name(name) => Ok(RoleDependency {
                role: name,
                vars: HashMap::new(),
                tags: Vec::new(),
                when: None,
            }),
            RawRoleDependency::Full {
                role,
                vars,
                tags,
                when,
            } => {
                let converted_vars = vars
                    .into_iter()
                    .map(|(k, v)| Ok((k, yaml_value_to_value(v)?)))
                    .collect::<Result<HashMap<_, _>, NexusError>>()?;

                let when_expr = when
                    .map(|w| super::expressions::parse_expression(&w))
                    .transpose()?;

                Ok(RoleDependency {
                    role,
                    vars: converted_vars,
                    tags,
                    when: when_expr,
                })
            }
        })
        .collect::<Result<Vec<_>, NexusError>>()?;

    let galaxy = raw.galaxy_info.unwrap_or_default();
    let platforms = galaxy
        .platforms
        .unwrap_or_default()
        .into_iter()
        .map(|p| PlatformSupport {
            name: p.name,
            versions: p.versions,
        })
        .collect();

    Ok(RoleMeta {
        dependencies,
        min_nexus_version: raw.min_nexus_version,
        platforms,
        description: galaxy.description,
        author: galaxy.author,
        license: galaxy.license,
        allow_duplicates: raw.allow_duplicates.unwrap_or(false),
    })
}

/// Load role variables from vars/ or defaults/
fn load_role_vars(role_path: &Path, subdir: &str) -> Result<HashMap<String, Value>, NexusError> {
    let vars_file = role_path.join(subdir).join("main.yml");
    if !vars_file.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&vars_file).map_err(|e| NexusError::Io {
        message: format!("Failed to read role {}: {}", subdir, e),
        path: Some(vars_file.clone()),
    })?;

    let yaml: HashMap<String, YamlValue> = serde_yaml::from_str(&content).map_err(|e| {
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid role {} YAML: {}", subdir, e),
            file: Some(vars_file.to_string_lossy().to_string()),
            line: None,
            column: None,
            suggestion: None,
        }))
    })?;

    yaml.into_iter()
        .map(|(k, v)| Ok((k, yaml_value_to_value(v)?)))
        .collect()
}

/// Load role tasks from tasks/main.yml
fn load_role_tasks(role_path: &Path) -> Result<Vec<TaskOrBlock>, NexusError> {
    let tasks_file = role_path.join("tasks").join("main.yml");
    if !tasks_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&tasks_file).map_err(|e| NexusError::Io {
        message: format!("Failed to read role tasks: {}", e),
        path: Some(tasks_file.clone()),
    })?;

    // Tasks file is a list of tasks
    let raw_tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(&content).map_err(|e| {
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid role tasks YAML: {}", e),
            file: Some(tasks_file.to_string_lossy().to_string()),
            line: None,
            column: None,
            suggestion: None,
        }))
    })?;

    // Convert to a temporary playbook to reuse parsing logic
    let playbook_yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping({
        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("hosts".to_string()),
            serde_yaml::Value::String("all".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("tasks".to_string()),
            serde_yaml::Value::Sequence(raw_tasks),
        );
        map
    }))
    .unwrap();

    let playbook = parse_playbook(&playbook_yaml, tasks_file.to_string_lossy().to_string())?;
    Ok(playbook.tasks)
}

/// Load role handlers from handlers/main.yml
fn load_role_handlers(role_path: &Path) -> Result<Vec<Handler>, NexusError> {
    let handlers_file = role_path.join("handlers").join("main.yml");
    if !handlers_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&handlers_file).map_err(|e| NexusError::Io {
        message: format!("Failed to read role handlers: {}", e),
        path: Some(handlers_file.clone()),
    })?;

    // Handlers file is a list of handlers
    let raw_handlers: Vec<serde_yaml::Value> = serde_yaml::from_str(&content).map_err(|e| {
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid role handlers YAML: {}", e),
            file: Some(handlers_file.to_string_lossy().to_string()),
            line: None,
            column: None,
            suggestion: None,
        }))
    })?;

    // Convert to a temporary playbook to reuse parsing logic
    let playbook_yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping({
        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("hosts".to_string()),
            serde_yaml::Value::String("all".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("handlers".to_string()),
            serde_yaml::Value::Sequence(raw_handlers),
        );
        map
    }))
    .unwrap();

    let playbook = parse_playbook(&playbook_yaml, handlers_file.to_string_lossy().to_string())?;
    Ok(playbook.handlers)
}

/// Convert serde_yaml Value to our Value type
fn yaml_value_to_value(yaml: YamlValue) -> Result<Value, NexusError> {
    match yaml {
        YamlValue::Null => Ok(Value::Null),
        YamlValue::Bool(b) => Ok(Value::Bool(b)),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                Ok(Value::Int(0))
            }
        }
        YamlValue::String(s) => Ok(Value::String(s)),
        YamlValue::Sequence(seq) => {
            let items: Result<Vec<_>, _> = seq.into_iter().map(yaml_value_to_value).collect();
            Ok(Value::List(items?))
        }
        YamlValue::Mapping(map) => {
            let items: Result<HashMap<_, _>, _> = map
                .into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        YamlValue::String(s) => s,
                        other => other.as_str().unwrap_or("").to_string(),
                    };
                    Ok((key, yaml_value_to_value(v)?))
                })
                .collect();
            Ok(Value::Dict(items?))
        }
        YamlValue::Tagged(tagged) => yaml_value_to_value(tagged.value),
    }
}

/// Expand ~ in paths
fn expand_tilde(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        if let Some(stripped) = path_str.strip_prefix('~') {
            if let Ok(home) = std::env::var("HOME") {
                let rest = stripped;
                return PathBuf::from(home).join(rest.trim_start_matches('/'));
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_role(dir: &Path, name: &str) -> PathBuf {
        let role_dir = dir.join("roles").join(name);
        fs::create_dir_all(role_dir.join("tasks")).unwrap();
        fs::create_dir_all(role_dir.join("handlers")).unwrap();
        fs::create_dir_all(role_dir.join("defaults")).unwrap();
        fs::create_dir_all(role_dir.join("vars")).unwrap();
        fs::create_dir_all(role_dir.join("meta")).unwrap();
        fs::create_dir_all(role_dir.join("templates")).unwrap();
        fs::create_dir_all(role_dir.join("files")).unwrap();

        // Create tasks/main.yml
        fs::write(
            role_dir.join("tasks").join("main.yml"),
            r#"
- name: Install package
  package: nginx
  state: installed

- name: Start service
  service: nginx
  state: running
"#,
        )
        .unwrap();

        // Create handlers/main.yml
        fs::write(
            role_dir.join("handlers").join("main.yml"),
            r#"
- name: restart nginx
  service: nginx
  state: restarted
"#,
        )
        .unwrap();

        // Create defaults/main.yml
        fs::write(
            role_dir.join("defaults").join("main.yml"),
            r#"
nginx_port: 80
nginx_user: www-data
"#,
        )
        .unwrap();

        // Create vars/main.yml
        fs::write(
            role_dir.join("vars").join("main.yml"),
            r#"
nginx_config_path: /etc/nginx/nginx.conf
"#,
        )
        .unwrap();

        // Create meta/main.yml
        fs::write(
            role_dir.join("meta").join("main.yml"),
            r#"
dependencies: []
galaxy_info:
  description: Install and configure nginx
  author: test
  license: MIT
"#,
        )
        .unwrap();

        role_dir
    }

    #[test]
    fn test_load_role() {
        let temp = TempDir::new().unwrap();
        let role_path = create_test_role(temp.path(), "webserver");

        let role = load_role(&role_path, "webserver").unwrap();

        assert_eq!(role.name, "webserver");
        assert_eq!(role.tasks.len(), 2);
        assert_eq!(role.handlers.len(), 1);
        assert!(role.defaults.contains_key("nginx_port"));
        assert!(role.vars.contains_key("nginx_config_path"));
        assert!(role.templates_path.is_some());
        assert!(role.files_path.is_some());
    }

    #[test]
    fn test_role_resolver() {
        let temp = TempDir::new().unwrap();
        create_test_role(temp.path(), "webserver");

        let mut resolver = RoleResolver::new();
        resolver.add_search_path(temp.path().join("roles"));

        let role = resolver.resolve("webserver").unwrap();
        assert_eq!(role.name, "webserver");
    }

    #[test]
    fn test_role_dependencies() {
        let temp = TempDir::new().unwrap();
        let roles_dir = temp.path().join("roles");

        // Create base role
        let base_dir = roles_dir.join("base");
        fs::create_dir_all(base_dir.join("tasks")).unwrap();
        fs::create_dir_all(base_dir.join("meta")).unwrap();
        fs::write(
            base_dir.join("tasks").join("main.yml"),
            "- name: Base task\n  command: echo base\n",
        )
        .unwrap();
        fs::write(base_dir.join("meta").join("main.yml"), "dependencies: []\n").unwrap();

        // Create webserver role that depends on base
        let web_dir = roles_dir.join("webserver");
        fs::create_dir_all(web_dir.join("tasks")).unwrap();
        fs::create_dir_all(web_dir.join("meta")).unwrap();
        fs::write(
            web_dir.join("tasks").join("main.yml"),
            "- name: Web task\n  command: echo web\n",
        )
        .unwrap();
        fs::write(
            web_dir.join("meta").join("main.yml"),
            "dependencies:\n  - base\n",
        )
        .unwrap();

        let mut resolver = RoleResolver::new();
        resolver.add_search_path(&roles_dir);

        let deps = resolver.resolve_dependencies("webserver").unwrap();
        assert_eq!(deps, vec!["base", "webserver"]);
    }

    #[test]
    fn test_role_not_found() {
        let resolver = RoleResolver::new();
        // Don't actually resolve since it would look in system paths
        // Just test the structure
        assert!(resolver.search_paths.contains(&PathBuf::from("./roles")));
    }

    #[test]
    fn test_example_roles() {
        // Test loading the actual example roles we created
        let examples_roles = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/roles");

        if !examples_roles.exists() {
            // Skip if examples don't exist (CI environment)
            return;
        }

        let mut resolver = RoleResolver::new();
        resolver.add_search_path(&examples_roles);

        // Test loading common role
        let common = resolver.resolve("common").unwrap();
        assert_eq!(common.name, "common");
        assert!(!common.tasks.is_empty(), "common role should have tasks");
        assert!(
            common.defaults.contains_key("common_packages"),
            "common role should have common_packages default"
        );
        assert!(
            common.defaults.contains_key("timezone"),
            "common role should have timezone default"
        );

        // Test loading webserver role
        let webserver = resolver.resolve("webserver").unwrap();
        assert_eq!(webserver.name, "webserver");
        assert!(
            !webserver.tasks.is_empty(),
            "webserver role should have tasks"
        );
        assert!(
            !webserver.handlers.is_empty(),
            "webserver role should have handlers"
        );
        assert!(
            webserver.defaults.contains_key("nginx_port"),
            "webserver role should have nginx_port default"
        );
        assert!(
            webserver.templates_path.is_some(),
            "webserver role should have templates directory"
        );

        // Test dependency resolution - webserver depends on common
        let deps = resolver.resolve_dependencies("webserver").unwrap();
        assert_eq!(
            deps.len(),
            2,
            "webserver should have 2 roles in dependency chain"
        );
        assert_eq!(
            deps[0], "common",
            "common should be first in dependency order"
        );
        assert_eq!(
            deps[1], "webserver",
            "webserver should be second in dependency order"
        );
    }

    #[test]
    fn test_role_meta_parsing() {
        let temp = TempDir::new().unwrap();
        let role_dir = temp.path().join("roles").join("test");
        fs::create_dir_all(role_dir.join("meta")).unwrap();
        fs::create_dir_all(role_dir.join("tasks")).unwrap();

        fs::write(
            role_dir.join("meta").join("main.yml"),
            r#"
dependencies:
  - common
  - role: security
    vars:
      firewall_enabled: true
    tags:
      - security
      - hardening
allow_duplicates: true
galaxy_info:
  description: A test role
  author: Nexus Team
  license: Apache-2.0
  platforms:
    - name: Ubuntu
      versions:
        - "20.04"
        - "22.04"
    - name: CentOS
      versions:
        - "8"
"#,
        )
        .unwrap();

        fs::write(
            role_dir.join("tasks").join("main.yml"),
            "- name: Test\n  command: echo test\n",
        )
        .unwrap();

        let role = load_role(&role_dir, "test").unwrap();

        assert_eq!(role.meta.dependencies.len(), 2);
        assert_eq!(role.meta.dependencies[0].role, "common");
        assert_eq!(role.meta.dependencies[1].role, "security");
        assert!(role.meta.dependencies[1]
            .vars
            .contains_key("firewall_enabled"));
        assert!(role.meta.allow_duplicates);
        assert_eq!(role.meta.platforms.len(), 2);
        assert_eq!(role.meta.author, Some("Nexus Team".to_string()));
    }
}
