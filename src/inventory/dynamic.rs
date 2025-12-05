// Dynamic inventory loader - executes scripts to generate inventory
//
// Implements Ansible-compatible dynamic inventory interface:
// - Script must be executable
// - `--list` returns full inventory as JSON
// - `--host <hostname>` returns host-specific vars (optional)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{Host, HostGroup, Inventory};
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Dynamic inventory script executor
#[derive(Debug, Clone)]
pub struct DynamicInventory {
    script_path: PathBuf,
}

/// Format for Ansible-compatible dynamic inventory JSON output
#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
struct DynamicInventoryOutput {
    #[serde(flatten)]
    groups: HashMap<String, GroupData>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GroupData {
    #[serde(default)]
    hosts: Vec<String>,
    #[serde(default)]
    vars: HashMap<String, JsonValue>,
    #[serde(default)]
    children: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
struct MetaData {
    hostvars: HashMap<String, HashMap<String, JsonValue>>,
}

impl DynamicInventory {
    /// Create a new dynamic inventory from a script path
    pub fn new(path: PathBuf) -> Self {
        DynamicInventory { script_path: path }
    }

    /// Check if a file is executable
    #[cfg(unix)]
    pub fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let permissions = metadata.permissions();
            // Check if any execute bit is set (owner, group, or other)
            permissions.mode() & 0o111 != 0
        } else {
            false
        }
    }

    #[cfg(not(unix))]
    pub fn is_executable(path: &Path) -> bool {
        // On non-Unix systems, check for common script extensions
        if let Some(ext) = path.extension() {
            matches!(ext.to_str(), Some("sh") | Some("py") | Some("rb") | Some("pl"))
        } else {
            false
        }
    }

    /// Load inventory by executing the script with --list
    pub async fn load(&self) -> Result<Inventory, NexusError> {
        // Execute script with --list
        let list_output = self.run_script(&["--list"]).await?;

        // Parse the JSON output
        self.parse_list_output(&list_output)
    }

    /// Execute the inventory script with the given arguments
    async fn run_script(&self, args: &[&str]) -> Result<String, NexusError> {
        let output = Command::new(&self.script_path)
            .args(args)
            .output()
            .map_err(|e| NexusError::Inventory {
                message: format!(
                    "Failed to execute inventory script '{}': {}",
                    self.script_path.display(),
                    e
                ),
                suggestion: Some("Ensure the script is executable and has correct permissions".to_string()),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NexusError::Inventory {
                message: format!(
                    "Inventory script '{}' failed with exit code {}",
                    self.script_path.display(),
                    output.status.code().unwrap_or(-1)
                ),
                suggestion: if !stderr.is_empty() {
                    Some(format!("Script error: {}", stderr))
                } else {
                    Some("Check script output and logs".to_string())
                },
            });
        }

        let stdout = String::from_utf8(output.stdout).map_err(|e| NexusError::Inventory {
            message: format!("Script output is not valid UTF-8: {}", e),
            suggestion: Some("Ensure script outputs valid UTF-8 JSON".to_string()),
        })?;

        Ok(stdout)
    }

    /// Parse the JSON output from --list into an Inventory
    fn parse_list_output(&self, json: &str) -> Result<Inventory, NexusError> {
        // Parse JSON
        let raw_json: JsonValue = serde_json::from_str(json).map_err(|e| NexusError::Inventory {
            message: format!("Invalid JSON from inventory script: {}", e),
            suggestion: Some(format!("JSON parse error at line {}, column {}", e.line(), e.column())),
        })?;

        let mut inventory = Inventory::new();

        // Extract _meta.hostvars if present
        let hostvars = if let Some(meta) = raw_json.get("_meta") {
            if let Some(hostvars) = meta.get("hostvars") {
                serde_json::from_value::<HashMap<String, HashMap<String, JsonValue>>>(hostvars.clone())
                    .unwrap_or_default()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        // Parse all groups (everything except _meta)
        if let Some(obj) = raw_json.as_object() {
            for (group_name, group_value) in obj {
                // Skip _meta
                if group_name == "_meta" {
                    continue;
                }

                // Parse group data
                if let Ok(group_data) = serde_json::from_value::<GroupData>(group_value.clone()) {
                    // Create group
                    let mut group = HostGroup::new(group_name);
                    group.hosts = group_data.hosts.clone();
                    group.children = group_data.children;

                    // Convert group vars
                    for (key, value) in group_data.vars {
                        group.vars.insert(key, json_to_value(&value));
                    }

                    // Add hosts to inventory
                    for host_name in &group_data.hosts {
                        // Only create host if it doesn't exist yet
                        if !inventory.hosts.contains_key(host_name) {
                            let mut host = Host::new(host_name);
                            host.groups.push(group_name.clone());

                            // Apply hostvars from _meta
                            if let Some(vars) = hostvars.get(host_name) {
                                for (key, value) in vars {
                                    match key.as_str() {
                                        "ansible_host" | "host" | "address" => {
                                            if let Some(addr) = value.as_str() {
                                                host.address = addr.to_string();
                                            }
                                        }
                                        "ansible_port" | "port" => {
                                            if let Some(port) = value.as_u64() {
                                                host.port = port as u16;
                                            }
                                        }
                                        "ansible_user" | "user" => {
                                            if let Some(user) = value.as_str() {
                                                host.user = user.to_string();
                                            }
                                        }
                                        _ => {
                                            host.vars.insert(key.clone(), json_to_value(value));
                                        }
                                    }
                                }
                            }

                            inventory.add_host(host);
                        } else {
                            // Host exists, just add this group to its groups list
                            if let Some(host) = inventory.hosts.get_mut(host_name) {
                                if !host.groups.contains(group_name) {
                                    host.groups.push(group_name.clone());
                                }
                            }
                        }
                    }

                    inventory.add_group(group);
                } else {
                    return Err(NexusError::Inventory {
                        message: format!("Invalid group data for '{}'", group_name),
                        suggestion: Some("Group must have 'hosts' (array) and optional 'vars' (object)".to_string()),
                    });
                }
            }
        } else {
            return Err(NexusError::Inventory {
                message: "Dynamic inventory output must be a JSON object".to_string(),
                suggestion: Some("Script should return { \"groupname\": { \"hosts\": [...], \"vars\": {...} }, ... }".to_string()),
            });
        }

        Ok(inventory)
    }

    /// Get host-specific variables (optional, for backwards compatibility)
    #[allow(dead_code)]
    pub async fn get_host_vars(&self, hostname: &str) -> Result<HashMap<String, Value>, NexusError> {
        let output = self.run_script(&["--host", hostname]).await?;

        let json: JsonValue = serde_json::from_str(&output).map_err(|e| NexusError::Inventory {
            message: format!("Invalid JSON from --host query: {}", e),
            suggestion: Some("Check script output for --host argument".to_string()),
        })?;

        let mut vars = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (key, value) in obj {
                vars.insert(key.clone(), json_to_value(value));
            }
        }

        Ok(vars)
    }
}

/// Convert serde_json::Value to our internal Value type
fn json_to_value(json: &JsonValue) -> Value {
    match json {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Int(0)
            }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        JsonValue::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        JsonValue::Object(obj) => {
            let items: HashMap<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Dict(items)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ansible_dynamic_inventory() {
        let json = r#"{
            "webservers": {
                "hosts": ["web1", "web2"],
                "vars": {"http_port": 80}
            },
            "dbservers": {
                "hosts": ["db1"],
                "vars": {"db_port": 5432}
            },
            "_meta": {
                "hostvars": {
                    "web1": {"ansible_host": "192.168.1.10"},
                    "web2": {"ansible_host": "192.168.1.11"},
                    "db1": {"ansible_host": "192.168.1.20"}
                }
            }
        }"#;

        let inv_path = PathBuf::from("/tmp/inventory.py");
        let dynamic = DynamicInventory::new(inv_path);
        let inventory = dynamic.parse_list_output(json).unwrap();

        // Check hosts
        assert_eq!(inventory.hosts.len(), 3);
        assert!(inventory.hosts.contains_key("web1"));
        assert!(inventory.hosts.contains_key("web2"));
        assert!(inventory.hosts.contains_key("db1"));

        // Check addresses from hostvars
        assert_eq!(inventory.hosts.get("web1").unwrap().address, "192.168.1.10");
        assert_eq!(inventory.hosts.get("web2").unwrap().address, "192.168.1.11");
        assert_eq!(inventory.hosts.get("db1").unwrap().address, "192.168.1.20");

        // Check groups
        assert!(inventory.groups.contains_key("webservers"));
        assert!(inventory.groups.contains_key("dbservers"));

        let webservers = inventory.groups.get("webservers").unwrap();
        assert_eq!(webservers.hosts.len(), 2);
        assert!(webservers.vars.contains_key("http_port"));
    }

    #[test]
    fn test_parse_minimal_dynamic_inventory() {
        let json = r#"{
            "all": {
                "hosts": ["host1", "host2"]
            }
        }"#;

        let inv_path = PathBuf::from("/tmp/inventory.py");
        let dynamic = DynamicInventory::new(inv_path);
        let inventory = dynamic.parse_list_output(json).unwrap();

        assert_eq!(inventory.hosts.len(), 2);
        assert!(inventory.hosts.contains_key("host1"));
        assert!(inventory.hosts.contains_key("host2"));
    }

    #[test]
    fn test_parse_with_children() {
        let json = r#"{
            "webservers": {
                "hosts": ["web1"],
                "children": ["frontend", "backend"]
            },
            "frontend": {
                "hosts": ["web2"]
            },
            "backend": {
                "hosts": ["web3"]
            }
        }"#;

        let inv_path = PathBuf::from("/tmp/inventory.py");
        let dynamic = DynamicInventory::new(inv_path);
        let inventory = dynamic.parse_list_output(json).unwrap();

        let webservers = inventory.groups.get("webservers").unwrap();
        assert_eq!(webservers.children.len(), 2);
        assert!(webservers.children.contains(&"frontend".to_string()));
        assert!(webservers.children.contains(&"backend".to_string()));
    }
}
