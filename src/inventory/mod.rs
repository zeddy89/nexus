// Inventory module for host management

mod discovery;
mod discovery_daemon;
mod discovery_profile;
mod dynamic;
mod groups;
mod static_inv;

pub use discovery::*;
pub use discovery_daemon::*;
pub use discovery_profile::*;
pub use dynamic::*;
pub use groups::*;
pub use static_inv::*;

use std::collections::HashMap;
use std::path::Path;

use crate::output::errors::NexusError;
use crate::parser::ast::{HostPattern, InlineHost, Value};

/// A single host in the inventory
#[derive(Debug, Clone)]
pub struct Host {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: String,
    pub vars: HashMap<String, Value>,
    pub groups: Vec<String>,
}

impl Host {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Host {
            address: name.clone(),
            name,
            port: 22,
            user: String::new(),
            vars: HashMap::new(),
            groups: Vec::new(),
        }
    }

    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = address.into();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    pub fn with_var(mut self, key: impl Into<String>, value: Value) -> Self {
        self.vars.insert(key.into(), value);
        self
    }

    pub fn get_var(&self, key: &str) -> Option<&Value> {
        self.vars.get(key)
    }

    /// Get the SSH connection string (user@host:port)
    pub fn ssh_target(&self) -> String {
        if self.user.is_empty() {
            format!("{}:{}", self.address, self.port)
        } else {
            format!("{}@{}:{}", self.user, self.address, self.port)
        }
    }

    /// Check if this host should use local connection
    pub fn is_local(&self) -> bool {
        // Check for explicit ansible_connection: local var
        if let Some(Value::String(conn)) = self.vars.get("ansible_connection") {
            if conn == "local" {
                return true;
            }
        }

        // Check if hostname is localhost or 127.0.0.1
        self.name == "localhost"
            || self.name == "127.0.0.1"
            || self.name == "::1"
            || self.address == "localhost"
            || self.address == "127.0.0.1"
            || self.address == "::1"
    }

    /// Create a localhost host for delegation
    pub fn localhost() -> Self {
        Host::new("localhost")
            .with_address("127.0.0.1")
            .with_var("ansible_connection", Value::String("local".to_string()))
    }
}

/// A group of hosts
#[derive(Debug, Clone, Default)]
pub struct HostGroup {
    pub name: String,
    pub hosts: Vec<String>,
    pub children: Vec<String>,
    pub vars: HashMap<String, Value>,
}

impl HostGroup {
    pub fn new(name: impl Into<String>) -> Self {
        HostGroup {
            name: name.into(),
            hosts: Vec::new(),
            children: Vec::new(),
            vars: HashMap::new(),
        }
    }
}

/// The complete inventory
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub hosts: HashMap<String, Host>,
    pub groups: HashMap<String, HostGroup>,
    pub default_user: Option<String>,
}

impl Inventory {
    pub fn new() -> Self {
        let mut inv = Inventory::default();
        // Always have an "all" group
        inv.groups.insert("all".to_string(), HostGroup::new("all"));
        inv
    }

    /// Load inventory from a file (YAML or executable script)
    ///
    /// Note: This is a synchronous function that handles both static and dynamic inventories.
    /// For dynamic inventories (executable scripts), it will spawn a blocking task if called
    /// from an async context.
    pub fn from_file(path: &Path) -> Result<Self, NexusError> {
        // Check if the file is executable - if so, treat as dynamic inventory
        if DynamicInventory::is_executable(path) {
            Self::from_file_dynamic(path)
        } else {
            // Static YAML inventory
            parse_inventory_file(path)
        }
    }

    /// Load dynamic inventory - handles async execution properly
    fn from_file_dynamic(path: &Path) -> Result<Self, NexusError> {
        let dynamic = DynamicInventory::new(path.to_path_buf());

        // Check if we're already in a tokio runtime
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're in an async context - we need to spawn a blocking task
                // and wait for it synchronously. This is a bit tricky but necessary.
                // The best approach is to use a channel to communicate between threads.
                let (tx, rx) = std::sync::mpsc::channel();
                let path = path.to_path_buf();

                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let dynamic = DynamicInventory::new(path);
                    let result = rt.block_on(dynamic.load());
                    tx.send(result).ok();
                });

                rx.recv().map_err(|_| NexusError::Runtime {
                    function: None,
                    message: "Failed to receive result from dynamic inventory thread".to_string(),
                    suggestion: None,
                })?
            }
            Err(_) => {
                // Not in async context, create a new runtime
                let rt = tokio::runtime::Runtime::new().map_err(|e| NexusError::Runtime {
                    function: None,
                    message: format!("Failed to create async runtime: {}", e),
                    suggestion: None,
                })?;
                rt.block_on(dynamic.load())
            }
        }
    }

    /// Load inventory from a YAML string
    pub fn parse_str(content: &str) -> Result<Self, NexusError> {
        parse_inventory(content)
    }

    /// Create inventory from CLI hosts string (comma-separated)
    ///
    /// Example: "server1.example.com,server2.example.com,192.168.1.10"
    pub fn from_cli_hosts(hosts_str: &str, default_user: Option<&str>) -> Self {
        let mut inv = Inventory::new();
        inv.default_user = default_user.map(|s| s.to_string());

        for host_str in hosts_str.split(',') {
            let host_str = host_str.trim();
            if host_str.is_empty() {
                continue;
            }

            let mut host = Host::new(host_str);

            // If it looks like an IP or hostname, use it as the address
            host = host.with_address(host_str);

            // Apply default user if provided
            if let Some(user) = default_user {
                host = host.with_user(user);
            }

            inv.add_host(host);
        }

        inv
    }

    /// Create a localhost-only inventory
    ///
    /// Used for playbooks that only target localhost
    pub fn localhost_only() -> Self {
        let mut inv = Inventory::new();
        inv.add_host(Host::localhost());
        inv
    }

    /// Create inventory from inline host definitions (playbook-embedded)
    ///
    /// Used when playbooks define hosts directly in the `hosts:` section
    pub fn from_inline_hosts(inline_hosts: &[InlineHost], default_user: Option<&str>) -> Self {
        let mut inv = Inventory::new();
        inv.default_user = default_user.map(|s| s.to_string());

        for inline in inline_hosts {
            let mut host = Host::new(&inline.name);

            // Set address (falls back to name if not specified)
            if let Some(addr) = &inline.address {
                host = host.with_address(addr);
            }

            // Set port
            if let Some(port) = inline.port {
                host = host.with_port(port);
            }

            // Set user (inline user takes precedence, then default)
            if let Some(user) = &inline.user {
                host = host.with_user(user);
            } else if let Some(user) = default_user {
                host = host.with_user(user);
            }

            // Copy inline variables
            for (key, value) in &inline.vars {
                host = host.with_var(key, value.clone());
            }

            inv.add_host(host);
        }

        inv
    }
}

impl std::str::FromStr for Inventory {
    type Err = NexusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_inventory(s)
    }
}

impl Inventory {
    /// Add a host to the inventory
    pub fn add_host(&mut self, host: Host) {
        let name = host.name.clone();

        // Add to "all" group
        if let Some(all) = self.groups.get_mut("all") {
            if !all.hosts.contains(&name) {
                all.hosts.push(name.clone());
            }
        }

        // Add to specified groups
        for group_name in &host.groups {
            self.groups
                .entry(group_name.clone())
                .or_insert_with(|| HostGroup::new(group_name))
                .hosts
                .push(name.clone());
        }

        self.hosts.insert(name, host);
    }

    /// Add a group to the inventory
    pub fn add_group(&mut self, group: HostGroup) {
        self.groups.insert(group.name.clone(), group);
    }

    /// Get hosts matching a pattern
    pub fn get_hosts(&self, pattern: &HostPattern) -> Vec<&Host> {
        match pattern {
            HostPattern::All => self.hosts.values().collect(),
            HostPattern::Group(name) => {
                // First try to find a group with this name
                if let Some(group) = self.groups.get(name) {
                    self.expand_group(group)
                } else if let Some(host) = self.hosts.get(name) {
                    // If no group found, check if it's a direct hostname
                    vec![host]
                } else {
                    // Neither group nor host found
                    vec![]
                }
            }
            HostPattern::Pattern(pat) => self.match_pattern(pat),
            HostPattern::Localhost => {
                // Return localhost from inventory, or create one if not present
                if let Some(host) = self.hosts.get("localhost") {
                    vec![host]
                } else {
                    // Check for 127.0.0.1 as well
                    self.hosts
                        .get("127.0.0.1")
                        .map(|h| vec![h])
                        .unwrap_or_default()
                }
            }
            HostPattern::Inline(_) => {
                // For inline hosts, the inventory was pre-built from the inline list
                // Return all hosts in the inventory
                self.hosts.values().collect()
            }
        }
    }

    /// Get a single host by name
    pub fn get_host(&self, name: &str) -> Option<&Host> {
        self.hosts.get(name)
    }

    /// Expand a group to its hosts (including children)
    fn expand_group(&self, group: &HostGroup) -> Vec<&Host> {
        let mut hosts = Vec::new();

        // Direct hosts
        for host_name in &group.hosts {
            if let Some(host) = self.hosts.get(host_name) {
                hosts.push(host);
            }
        }

        // Child groups (recursive)
        for child_name in &group.children {
            if let Some(child) = self.groups.get(child_name) {
                hosts.extend(self.expand_group(child));
            }
        }

        // Deduplicate
        let mut seen = std::collections::HashSet::new();
        hosts.retain(|h| seen.insert(h.name.clone()));

        hosts
    }

    /// Match hosts against a complex pattern
    fn match_pattern(&self, pattern: &str) -> Vec<&Host> {
        // Handle patterns like:
        // - "webservers:dbservers" (union)
        // - "webservers:&prod" (intersection)
        // - "webservers:!staging" (exclusion)

        let mut result: Vec<&Host> = Vec::new();
        let mut first = true;

        for part in pattern.split(':') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            if let Some(group_name) = part.strip_prefix('&') {
                // Intersection
                let group_hosts: std::collections::HashSet<_> = self
                    .groups
                    .get(group_name)
                    .map(|g| self.expand_group(g))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|h| &h.name)
                    .collect();

                result.retain(|h| group_hosts.contains(&h.name));
            } else if let Some(group_name) = part.strip_prefix('!') {
                // Exclusion
                let group_hosts: std::collections::HashSet<_> = self
                    .groups
                    .get(group_name)
                    .map(|g| self.expand_group(g))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|h| &h.name)
                    .collect();

                result.retain(|h| !group_hosts.contains(&h.name));
            } else {
                // Union (or first group)
                let group_hosts = self
                    .groups
                    .get(part)
                    .map(|g| self.expand_group(g))
                    .unwrap_or_default();

                if first {
                    result = group_hosts;
                    first = false;
                } else {
                    // Add hosts not already in result
                    let existing: std::collections::HashSet<_> =
                        result.iter().map(|h| &h.name).collect();

                    for host in group_hosts {
                        if !existing.contains(&host.name) {
                            result.push(host);
                        }
                    }
                }
            }
        }

        result
    }

    /// Get effective variables for a host (host vars + group vars)
    pub fn get_host_vars(&self, host: &Host) -> HashMap<String, Value> {
        let mut vars = HashMap::new();

        // Start with "all" group vars
        if let Some(all) = self.groups.get("all") {
            vars.extend(all.vars.clone());
        }

        // Add group vars (in order)
        for group_name in &host.groups {
            if let Some(group) = self.groups.get(group_name) {
                vars.extend(group.vars.clone());
            }
        }

        // Host vars override group vars
        vars.extend(host.vars.clone());

        vars
    }

    /// Get the total number of hosts
    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Get all group names
    pub fn group_names(&self) -> Vec<&str> {
        self.groups.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_creation() {
        let host = Host::new("web1")
            .with_address("192.168.1.10")
            .with_port(22)
            .with_user("admin")
            .with_var("env", Value::String("prod".to_string()));

        assert_eq!(host.name, "web1");
        assert_eq!(host.address, "192.168.1.10");
        assert_eq!(host.port, 22);
        assert_eq!(host.user, "admin");
        assert_eq!(host.ssh_target(), "admin@192.168.1.10:22");
    }

    #[test]
    fn test_inventory_groups() {
        let mut inv = Inventory::new();

        let host1 = Host::new("web1").with_address("192.168.1.10");
        let host2 = Host::new("web2").with_address("192.168.1.11");
        let host3 = Host::new("db1").with_address("192.168.1.20");

        inv.add_host(host1);
        inv.add_host(host2);
        inv.add_host(host3);

        let mut web_group = HostGroup::new("webservers");
        web_group.hosts = vec!["web1".to_string(), "web2".to_string()];
        inv.add_group(web_group);

        let mut db_group = HostGroup::new("databases");
        db_group.hosts = vec!["db1".to_string()];
        inv.add_group(db_group);

        // Test all hosts
        let all = inv.get_hosts(&HostPattern::All);
        assert_eq!(all.len(), 3);

        // Test group selection
        let webs = inv.get_hosts(&HostPattern::Group("webservers".to_string()));
        assert_eq!(webs.len(), 2);
    }
}
