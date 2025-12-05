use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use crate::output::errors::NexusError;

/// Represents a parsed Ansible playbook
#[derive(Debug, Clone, Deserialize)]
pub struct AnsiblePlaybook {
    pub plays: Vec<AnsiblePlay>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnsiblePlay {
    pub name: Option<String>,
    pub hosts: String,
    #[serde(default)]
    pub vars: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub tasks: Vec<AnsibleTask>,
    #[serde(default)]
    pub handlers: Vec<AnsibleTask>,
    #[serde(default)]
    #[allow(dead_code)]
    pub roles: Vec<serde_yaml::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub pre_tasks: Vec<AnsibleTask>,
    #[serde(default)]
    #[allow(dead_code)]
    pub post_tasks: Vec<AnsibleTask>,
    #[allow(dead_code)]
    pub gather_facts: Option<bool>,
    #[serde(rename = "become")]
    #[allow(dead_code)]
    pub r#become: Option<bool>,
    #[allow(dead_code)]
    pub become_user: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnsibleTask {
    pub name: Option<String>,
    #[serde(rename = "when")]
    pub when_condition: Option<serde_yaml::Value>,
    pub register: Option<String>,
    pub notify: Option<serde_yaml::Value>,
    #[serde(rename = "loop")]
    pub loop_expr: Option<serde_yaml::Value>,
    pub with_items: Option<serde_yaml::Value>,
    pub with_dict: Option<serde_yaml::Value>,
    pub tags: Option<serde_yaml::Value>,
    #[serde(rename = "become")]
    pub r#become: Option<bool>,
    pub become_user: Option<String>,
    pub ignore_errors: Option<bool>,
    pub changed_when: Option<serde_yaml::Value>,
    pub failed_when: Option<serde_yaml::Value>,
    pub delegate_to: Option<String>,
    pub run_once: Option<bool>,
    pub block: Option<Vec<AnsibleTask>>,
    pub rescue: Option<Vec<AnsibleTask>>,
    pub always: Option<Vec<AnsibleTask>>,
    // Module and args stored as remaining fields
    #[serde(flatten)]
    pub module_args: HashMap<String, serde_yaml::Value>,
}

/// Parse an Ansible playbook file
pub fn parse_playbook(path: &Path) -> Result<AnsiblePlaybook, NexusError> {
    let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
        message: format!("Failed to read {}: {}", path.display(), e),
        path: Some(path.to_path_buf()),
    })?;

    // Ansible playbooks are a list of plays
    let plays: Vec<AnsiblePlay> = serde_yaml::from_str(&content).map_err(|e| NexusError::Parse(
        Box::new(crate::output::errors::ParseError {
            kind: crate::output::errors::ParseErrorKind::InvalidYaml,
            message: format!("Failed to parse Ansible playbook: {}", e),
            file: Some(path.display().to_string()),
            line: None,
            column: None,
            suggestion: Some("Ensure the playbook is valid YAML".to_string()),
        })
    ))?;

    Ok(AnsiblePlaybook { plays })
}
