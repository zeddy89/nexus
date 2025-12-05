// Static YAML inventory parser

use std::collections::HashMap;
use std::path::Path;

use serde_yaml::Value as YamlValue;

use super::{Host, HostGroup, Inventory};
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Parse inventory from a YAML file
pub fn parse_inventory_file(path: &Path) -> Result<Inventory, NexusError> {
    let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
        message: format!("Failed to read inventory file: {}", e),
        path: Some(path.to_path_buf()),
    })?;

    parse_inventory(&content)
}

/// Parse inventory from a YAML string
pub fn parse_inventory(content: &str) -> Result<Inventory, NexusError> {
    let yaml: YamlValue = serde_yaml::from_str(content).map_err(|e| NexusError::Inventory {
        message: format!("Invalid inventory YAML: {}", e),
        suggestion: Some("Check inventory file syntax".to_string()),
    })?;

    let mut inventory = Inventory::new();

    match &yaml {
        YamlValue::Mapping(map) => {
            // Check for Ansible-style inventory with groups at top level
            // or simple format with hosts/groups keys

            if let Some(all) = map.get("all") {
                // Ansible-style nested format
                parse_ansible_group(all, "all", &mut inventory)?;
            } else if map.contains_key("hosts") || map.contains_key("groups") {
                // Simple format
                parse_simple_inventory(&yaml, &mut inventory)?;
            } else {
                // Assume top-level keys are group names
                for (key, value) in map {
                    if let Some(group_name) = key.as_str() {
                        parse_ansible_group(value, group_name, &mut inventory)?;
                    }
                }
            }
        }
        _ => {
            return Err(NexusError::Inventory {
                message: "Inventory must be a YAML mapping".to_string(),
                suggestion: Some("Start with 'all:' or 'hosts:'".to_string()),
            });
        }
    }

    Ok(inventory)
}

fn parse_simple_inventory(yaml: &YamlValue, inventory: &mut Inventory) -> Result<(), NexusError> {
    if let Some(hosts_val) = yaml.get("hosts") {
        parse_hosts_section(hosts_val, None, inventory)?;
    }

    if let Some(groups_val) = yaml.get("groups") {
        if let Some(groups_map) = groups_val.as_mapping() {
            for (group_name, group_val) in groups_map {
                if let Some(name) = group_name.as_str() {
                    parse_group_section(name, group_val, inventory)?;
                }
            }
        }
    }

    // Default user
    if let Some(defaults) = yaml.get("defaults") {
        if let Some(user) = defaults.get("user").and_then(|u| u.as_str()) {
            inventory.default_user = Some(user.to_string());
        }
    }

    Ok(())
}

fn parse_ansible_group(
    value: &YamlValue,
    group_name: &str,
    inventory: &mut Inventory,
) -> Result<(), NexusError> {
    // Ensure the group exists
    inventory
        .groups
        .entry(group_name.to_string())
        .or_insert_with(|| HostGroup::new(group_name));

    // Collect data to avoid borrow conflicts
    let mut hosts_to_add: Vec<(String, Host)> = Vec::new();
    let mut host_names: Vec<String> = Vec::new();
    let mut children: Vec<(String, YamlValue)> = Vec::new();
    let mut group_vars: Vec<(String, Value)> = Vec::new();

    if let Some(map) = value.as_mapping() {
        // Parse hosts in this group
        if let Some(hosts) = map.get("hosts") {
            if let Some(hosts_map) = hosts.as_mapping() {
                for (host_name, host_val) in hosts_map {
                    if let Some(name) = host_name.as_str() {
                        let mut host = Host::new(name);
                        host.groups.push(group_name.to_string());

                        if let Some(host_map) = host_val.as_mapping() {
                            parse_host_vars(&mut host, host_map)?;
                        }

                        host_names.push(name.to_string());
                        hosts_to_add.push((name.to_string(), host));
                    }
                }
            }
        }

        // Parse children (child groups)
        if let Some(children_val) = map.get("children") {
            if let Some(children_map) = children_val.as_mapping() {
                for (child_name, child_val) in children_map {
                    if let Some(name) = child_name.as_str() {
                        children.push((name.to_string(), child_val.clone()));
                    }
                }
            }
        }

        // Parse group vars
        if let Some(vars) = map.get("vars") {
            if let Some(vars_map) = vars.as_mapping() {
                for (k, v) in vars_map {
                    if let Some(key) = k.as_str() {
                        group_vars.push((key.to_string(), yaml_to_value(v)));
                    }
                }
            }
        }
    }

    // Now apply the collected data
    for (name, host) in hosts_to_add {
        inventory.hosts.entry(name).or_insert(host);
    }

    // Update the group
    if let Some(group) = inventory.groups.get_mut(group_name) {
        for name in host_names {
            if !group.hosts.contains(&name) {
                group.hosts.push(name);
            }
        }

        for (child_name, _) in &children {
            group.children.push(child_name.clone());
        }

        for (key, val) in group_vars {
            group.vars.insert(key, val);
        }
    }

    // Recursively parse children
    for (child_name, child_val) in children {
        parse_ansible_group(&child_val, &child_name, inventory)?;
    }

    Ok(())
}

fn parse_hosts_section(
    value: &YamlValue,
    group_name: Option<&str>,
    inventory: &mut Inventory,
) -> Result<(), NexusError> {
    match value {
        YamlValue::Sequence(hosts) => {
            for host_val in hosts {
                match host_val {
                    YamlValue::String(name) => {
                        let mut host = Host::new(name);
                        if let Some(group) = group_name {
                            host.groups.push(group.to_string());
                        }
                        inventory.add_host(host);
                    }
                    YamlValue::Mapping(map) => {
                        // host with inline vars: { name: host1, address: 192.168.1.1 }
                        if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                            let mut host = Host::new(name);
                            if let Some(group) = group_name {
                                host.groups.push(group.to_string());
                            }
                            parse_host_vars(&mut host, map)?;
                            inventory.add_host(host);
                        }
                    }
                    _ => {}
                }
            }
        }
        YamlValue::Mapping(hosts_map) => {
            // Ansible-style: host_name: { vars... }
            for (host_name, host_vars) in hosts_map {
                if let Some(name) = host_name.as_str() {
                    let mut host = Host::new(name);
                    if let Some(group) = group_name {
                        host.groups.push(group.to_string());
                    }

                    if let Some(vars_map) = host_vars.as_mapping() {
                        parse_host_vars(&mut host, vars_map)?;
                    }

                    inventory.add_host(host);
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn parse_group_section(
    name: &str,
    value: &YamlValue,
    inventory: &mut Inventory,
) -> Result<(), NexusError> {
    let mut group = HostGroup::new(name);

    if let Some(map) = value.as_mapping() {
        // Hosts in this group
        if let Some(hosts) = map.get("hosts") {
            parse_hosts_section(hosts, Some(name), inventory)?;

            // Collect host names for the group
            if let Some(hosts_seq) = hosts.as_sequence() {
                for h in hosts_seq {
                    match h {
                        YamlValue::String(n) => group.hosts.push(n.clone()),
                        YamlValue::Mapping(m) => {
                            if let Some(n) = m.get("name").and_then(|v| v.as_str()) {
                                group.hosts.push(n.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            } else if let Some(hosts_map) = hosts.as_mapping() {
                for (k, _) in hosts_map {
                    if let Some(n) = k.as_str() {
                        group.hosts.push(n.to_string());
                    }
                }
            }
        }

        // Children (child groups)
        if let Some(children) = map.get("children") {
            if let Some(children_seq) = children.as_sequence() {
                for child in children_seq {
                    if let Some(child_name) = child.as_str() {
                        group.children.push(child_name.to_string());
                    }
                }
            }
        }

        // Group vars
        if let Some(vars) = map.get("vars") {
            if let Some(vars_map) = vars.as_mapping() {
                for (k, v) in vars_map {
                    if let Some(key) = k.as_str() {
                        group.vars.insert(key.to_string(), yaml_to_value(v));
                    }
                }
            }
        }
    }

    inventory.add_group(group);
    Ok(())
}

fn parse_host_vars(host: &mut Host, map: &serde_yaml::Mapping) -> Result<(), NexusError> {
    for (k, v) in map {
        if let Some(key) = k.as_str() {
            match key {
                "name" => {} // Already handled
                "address" | "ansible_host" | "host" => {
                    if let Some(addr) = v.as_str() {
                        host.address = addr.to_string();
                    }
                }
                "port" | "ansible_port" => {
                    if let Some(p) = v.as_u64() {
                        host.port = p as u16;
                    }
                }
                "user" | "ansible_user" => {
                    if let Some(u) = v.as_str() {
                        host.user = u.to_string();
                    }
                }
                "groups" => {
                    if let Some(groups) = v.as_sequence() {
                        for g in groups {
                            if let Some(group_name) = g.as_str() {
                                host.groups.push(group_name.to_string());
                            }
                        }
                    }
                }
                _ => {
                    // Store as host variable
                    host.vars.insert(key.to_string(), yaml_to_value(v));
                }
            }
        }
    }

    Ok(())
}

fn yaml_to_value(yaml: &YamlValue) -> Value {
    match yaml {
        YamlValue::Null => Value::Null,
        YamlValue::Bool(b) => Value::Bool(*b),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Int(0)
            }
        }
        YamlValue::String(s) => Value::String(s.clone()),
        YamlValue::Sequence(seq) => Value::List(seq.iter().map(yaml_to_value).collect()),
        YamlValue::Mapping(map) => {
            let items: HashMap<String, Value> = map
                .iter()
                .filter_map(|(k, v)| k.as_str().map(|key| (key.to_string(), yaml_to_value(v))))
                .collect();
            Value::Dict(items)
        }
        YamlValue::Tagged(tagged) => yaml_to_value(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_inventory() {
        let yaml = r#"
hosts:
  - name: web1
    address: 192.168.1.10
    user: admin
  - name: web2
    address: 192.168.1.11
    user: admin

groups:
  webservers:
    hosts:
      - web1
      - web2
    vars:
      http_port: 80
"#;

        let inv = parse_inventory(yaml).unwrap();
        assert_eq!(inv.hosts.len(), 2);
        assert!(inv.groups.contains_key("webservers"));
    }

    #[test]
    fn test_parse_ansible_style_inventory() {
        let yaml = r#"
all:
  children:
    webservers:
      hosts:
        web1:
          ansible_host: 192.168.1.10
        web2:
          ansible_host: 192.168.1.11
      vars:
        http_port: 80
    databases:
      hosts:
        db1:
          ansible_host: 192.168.1.20
"#;

        let inv = parse_inventory(yaml).unwrap();
        assert_eq!(inv.hosts.len(), 3);
        assert!(inv.groups.contains_key("webservers"));
        assert!(inv.groups.contains_key("databases"));
    }
}
