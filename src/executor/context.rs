// Execution context for tasks

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::inventory::Host;
use crate::parser::ast::Value;

/// Context for task execution on a specific host
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// The target host
    pub host: Arc<Host>,
    /// All variables (host vars + playbook vars + registered vars)
    vars: Arc<RwLock<HashMap<String, Value>>>,
    /// Registered results from previous tasks
    registered: Arc<RwLock<HashMap<String, TaskOutput>>>,
    /// Whether we're in check mode (dry run)
    pub check_mode: bool,
    /// Whether to show diffs for file changes
    pub diff_mode: bool,
    /// Current loop item (if in a loop)
    pub loop_item: Option<Value>,
    /// Current loop index (if in a loop)
    pub loop_index: Option<usize>,
    /// Whether to use sudo for commands
    pub sudo: bool,
    /// User to run commands as (via sudo -u)
    pub sudo_user: Option<String>,
}

impl ExecutionContext {
    pub fn new(host: Arc<Host>, playbook_vars: HashMap<String, Value>) -> Self {
        let mut vars = playbook_vars;

        // Add host vars
        for (k, v) in &host.vars {
            vars.insert(k.clone(), v.clone());
        }

        // Add host facts
        vars.insert("host".to_string(), host_to_value(&host));
        vars.insert(
            "inventory_hostname".to_string(),
            Value::String(host.name.clone()),
        );

        ExecutionContext {
            host,
            vars: Arc::new(RwLock::new(vars)),
            registered: Arc::new(RwLock::new(HashMap::new())),
            check_mode: false,
            diff_mode: false,
            loop_item: None,
            loop_index: None,
            sudo: false,
            sudo_user: None,
        }
    }

    pub fn with_sudo(mut self, sudo: bool, sudo_user: Option<String>) -> Self {
        self.sudo = sudo;
        self.sudo_user = sudo_user;
        self
    }

    pub fn with_check_mode(mut self, check: bool) -> Self {
        self.check_mode = check;
        self
    }

    pub fn with_diff_mode(mut self, diff: bool) -> Self {
        self.diff_mode = diff;
        self
    }

    pub fn with_loop_item(mut self, item: Value, index: usize) -> Self {
        self.loop_item = Some(item.clone());
        self.loop_index = Some(index);

        // Add item to vars
        self.vars.write().insert("item".to_string(), item);

        self
    }

    /// Get a variable value
    pub fn get_var(&self, name: &str) -> Option<Value> {
        // Check for special variables
        if name == "item" {
            return self.loop_item.clone();
        }

        // Check registered results
        if let Some(output) = self.registered.read().get(name) {
            return Some(output.to_value());
        }

        // Check regular vars
        self.vars.read().get(name).cloned()
    }

    /// Get a nested variable value (e.g., "vars.foo.bar")
    pub fn get_nested_var(&self, path: &[String]) -> Option<Value> {
        if path.is_empty() {
            return None;
        }

        let first = &path[0];

        // Special handling for "vars" prefix
        if first == "vars" && path.len() > 1 {
            return self.get_nested_var(&path[1..]);
        }

        // Special handling for "host"
        if first == "host" {
            let mut current = host_to_value(&self.host);
            for part in &path[1..] {
                current = match current {
                    Value::Dict(ref map) => map.get(part).cloned()?,
                    _ => return None,
                };
            }
            return Some(current);
        }

        // Get base variable
        let mut current = self.get_var(first)?;

        // Navigate nested path
        for part in &path[1..] {
            current = match current {
                Value::Dict(ref map) => map.get(part).cloned()?,
                Value::List(ref list) => {
                    let idx: usize = part.parse().ok()?;
                    list.get(idx).cloned()?
                }
                _ => return None,
            };
        }

        Some(current)
    }

    /// Set a variable
    pub fn set_var(&self, name: impl Into<String>, value: Value) {
        self.vars.write().insert(name.into(), value);
    }

    /// Register task output
    pub fn register(&self, name: impl Into<String>, output: TaskOutput) {
        self.registered.write().insert(name.into(), output);
    }

    /// Get registered output
    pub fn get_registered(&self, name: &str) -> Option<TaskOutput> {
        self.registered.read().get(name).cloned()
    }

    /// Get all variables
    pub fn all_vars(&self) -> HashMap<String, Value> {
        self.vars.read().clone()
    }

    /// Clone context for parallel execution
    pub fn clone_for_task(&self) -> Self {
        ExecutionContext {
            host: self.host.clone(),
            vars: Arc::new(RwLock::new(self.vars.read().clone())),
            registered: self.registered.clone(),
            check_mode: self.check_mode,
            diff_mode: self.diff_mode,
            loop_item: self.loop_item.clone(),
            loop_index: self.loop_index,
            sudo: self.sudo,
            sudo_user: self.sudo_user.clone(),
        }
    }

    /// Wrap a command with sudo if needed
    pub fn wrap_command(&self, cmd: &str) -> String {
        if self.sudo {
            if let Some(ref user) = self.sudo_user {
                format!("sudo -n -u {} -- sh -c {}", user, shell_escape(cmd))
            } else {
                format!("sudo -n -- sh -c {}", shell_escape(cmd))
            }
        } else {
            cmd.to_string()
        }
    }
}

/// Escape a command for use in sh -c
fn shell_escape(cmd: &str) -> String {
    // Wrap in single quotes and escape any existing single quotes
    format!("'{}'", cmd.replace('\'', "'\"'\"'"))
}

/// Output from a task execution
#[derive(Debug, Clone, Default)]
pub struct TaskOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub changed: bool,
    pub failed: bool,
    pub skipped: bool,
    pub message: Option<String>,
    pub data: HashMap<String, Value>,
    /// Diff output for file changes (when diff_mode is enabled)
    pub diff: Option<String>,
}

impl TaskOutput {
    pub fn new() -> Self {
        TaskOutput::default()
    }

    pub fn success() -> Self {
        TaskOutput {
            exit_code: 0,
            ..Default::default()
        }
    }

    pub fn changed() -> Self {
        TaskOutput {
            exit_code: 0,
            changed: true,
            ..Default::default()
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        TaskOutput {
            exit_code: 1,
            failed: true,
            message: Some(message.into()),
            ..Default::default()
        }
    }

    pub fn skipped() -> Self {
        TaskOutput {
            skipped: true,
            ..Default::default()
        }
    }

    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = stdout.into();
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = stderr.into();
        self
    }

    pub fn with_data(mut self, key: impl Into<String>, value: Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }

    pub fn with_diff(mut self, diff: impl Into<String>) -> Self {
        self.diff = Some(diff.into());
        self
    }

    /// Convert to a Value for use in expressions
    pub fn to_value(&self) -> Value {
        let mut map = HashMap::new();

        map.insert("stdout".to_string(), Value::String(self.stdout.clone()));
        map.insert("stderr".to_string(), Value::String(self.stderr.clone()));
        map.insert("rc".to_string(), Value::Int(self.exit_code as i64));
        map.insert("changed".to_string(), Value::Bool(self.changed));
        map.insert("failed".to_string(), Value::Bool(self.failed));
        map.insert("skipped".to_string(), Value::Bool(self.skipped));

        if let Some(ref msg) = self.message {
            map.insert("msg".to_string(), Value::String(msg.clone()));
        }

        // Add stdout lines
        let lines: Vec<Value> = self
            .stdout
            .lines()
            .map(|l| Value::String(l.to_string()))
            .collect();
        map.insert("stdout_lines".to_string(), Value::List(lines));

        // Add custom data
        for (k, v) in &self.data {
            map.insert(k.clone(), v.clone());
        }

        Value::Dict(map)
    }
}

/// Convert a Host to a Value for use in expressions
fn host_to_value(host: &Host) -> Value {
    let mut map = HashMap::new();

    map.insert("name".to_string(), Value::String(host.name.clone()));
    map.insert("address".to_string(), Value::String(host.address.clone()));
    map.insert("port".to_string(), Value::Int(host.port as i64));
    map.insert("user".to_string(), Value::String(host.user.clone()));

    // Add host groups
    let groups: Vec<Value> = host
        .groups
        .iter()
        .map(|g| Value::String(g.clone()))
        .collect();
    map.insert("groups".to_string(), Value::List(groups));

    // Add host vars
    for (k, v) in &host.vars {
        map.insert(k.clone(), v.clone());
    }

    Value::Dict(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> ExecutionContext {
        let host = Host::new("test-host")
            .with_address("192.168.1.10")
            .with_var("env", Value::String("prod".to_string()));

        let mut vars = HashMap::new();
        vars.insert(
            "webserver".to_string(),
            Value::String("nginx".to_string()),
        );

        ExecutionContext::new(Arc::new(host), vars)
    }

    #[test]
    fn test_get_var() {
        let ctx = create_test_context();

        assert_eq!(
            ctx.get_var("webserver"),
            Some(Value::String("nginx".to_string()))
        );
        assert!(ctx.get_var("nonexistent").is_none());
    }

    #[test]
    fn test_nested_var() {
        let ctx = create_test_context();

        let host_name = ctx.get_nested_var(&["host".to_string(), "name".to_string()]);
        assert_eq!(host_name, Some(Value::String("test-host".to_string())));
    }

    #[test]
    fn test_register() {
        let ctx = create_test_context();

        let output = TaskOutput::success().with_stdout("hello world");
        ctx.register("result", output);

        let val = ctx.get_var("result").unwrap();
        if let Value::Dict(map) = val {
            assert_eq!(map.get("stdout"), Some(&Value::String("hello world".to_string())));
        } else {
            panic!("Expected Dict");
        }
    }
}
