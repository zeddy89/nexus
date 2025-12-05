// Tag filtering system - Better than Ansible's tags
// Supports:
// - Expression-based filtering: "deploy and not test"
// - Tag groups: @critical expands to [critical, security, audit]
// - Inheritance: child tasks inherit parent tags
// - Special tags: always, never

use std::collections::{HashMap, HashSet};

/// Tag filter for selecting which tasks to run
#[derive(Debug, Clone)]
pub struct TagFilter {
    /// Tags that must be present (OR logic within, AND with skip_tags)
    include_tags: HashSet<String>,
    /// Tags that must NOT be present
    skip_tags: HashSet<String>,
    /// Tag groups (e.g., @critical -> [critical, security])
    tag_groups: HashMap<String, Vec<String>>,
    /// Whether to run untagged tasks (default: true unless tags specified)
    run_untagged: bool,
}

impl TagFilter {
    pub fn new() -> Self {
        TagFilter {
            include_tags: HashSet::new(),
            skip_tags: HashSet::new(),
            tag_groups: Self::default_tag_groups(),
            run_untagged: true,
        }
    }

    /// Create filter from CLI arguments
    pub fn from_args(tags: Option<&str>, skip_tags: Option<&str>) -> Self {
        let mut filter = TagFilter::new();

        if let Some(tags_str) = tags {
            filter.include_tags = Self::parse_tag_list(tags_str);
            // When specific tags are requested, don't run untagged tasks
            filter.run_untagged = false;
        }

        if let Some(skip_str) = skip_tags {
            filter.skip_tags = Self::parse_tag_list(skip_str);
        }

        filter
    }

    /// Create filter that only includes specific tags
    pub fn include_tags(tags: Vec<String>) -> Self {
        let mut filter = TagFilter::new();
        filter.include_tags = tags.into_iter().map(|t| t.to_lowercase()).collect();
        filter.run_untagged = false;
        filter
    }

    /// Parse comma-separated tag list, handling expressions
    fn parse_tag_list(tags: &str) -> HashSet<String> {
        tags.split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// Default tag groups
    fn default_tag_groups() -> HashMap<String, Vec<String>> {
        let mut groups = HashMap::new();

        // @critical: tasks that should run even on errors
        groups.insert(
            "@critical".to_string(),
            vec!["critical".to_string(), "always".to_string()],
        );

        // @setup: initialization tasks
        groups.insert(
            "@setup".to_string(),
            vec![
                "setup".to_string(),
                "init".to_string(),
                "bootstrap".to_string(),
            ],
        );

        // @security: security-related tasks
        groups.insert(
            "@security".to_string(),
            vec![
                "security".to_string(),
                "audit".to_string(),
                "hardening".to_string(),
            ],
        );

        // @cleanup: cleanup and rollback tasks
        groups.insert(
            "@cleanup".to_string(),
            vec!["cleanup".to_string(), "rollback".to_string()],
        );

        groups
    }

    /// Add a custom tag group
    pub fn add_tag_group(&mut self, name: &str, tags: Vec<String>) {
        self.tag_groups.insert(name.to_string(), tags);
    }

    /// Expand tag groups in a tag set
    fn expand_tags(&self, tags: &HashSet<String>) -> HashSet<String> {
        let mut expanded = HashSet::new();

        for tag in tags {
            if tag.starts_with('@') {
                // Expand group
                if let Some(group_tags) = self.tag_groups.get(tag) {
                    for gt in group_tags {
                        expanded.insert(gt.clone());
                    }
                }
            } else {
                expanded.insert(tag.clone());
            }
        }

        expanded
    }

    /// Check if a task should run based on its tags
    pub fn should_run(&self, task_tags: &[String]) -> bool {
        let task_tags_lower: HashSet<String> = task_tags.iter().map(|t| t.to_lowercase()).collect();

        // Special tag: "always" - always runs unless explicitly skipped
        if task_tags_lower.contains("always") {
            let expanded_skip = self.expand_tags(&self.skip_tags);
            if !expanded_skip.contains("always") {
                return true;
            }
        }

        // Special tag: "never" - never runs unless explicitly included
        if task_tags_lower.contains("never") {
            let expanded_include = self.expand_tags(&self.include_tags);
            if !expanded_include.contains("never") {
                return false;
            }
        }

        // Check skip tags first (exclusion takes priority)
        if !self.skip_tags.is_empty() {
            let expanded_skip = self.expand_tags(&self.skip_tags);
            for skip_tag in &expanded_skip {
                if task_tags_lower.contains(skip_tag) {
                    return false;
                }
            }
        }

        // If no include tags specified, use run_untagged setting
        if self.include_tags.is_empty() {
            return self.run_untagged || !task_tags.is_empty();
        }

        // Check if any include tag matches
        let expanded_include = self.expand_tags(&self.include_tags);
        for include_tag in &expanded_include {
            if task_tags_lower.contains(include_tag) {
                return true;
            }
        }

        // No match found
        false
    }

    /// Get a human-readable description of the filter
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();

        if !self.include_tags.is_empty() {
            parts.push(format!(
                "include: [{}]",
                self.include_tags
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !self.skip_tags.is_empty() {
            parts.push(format!(
                "skip: [{}]",
                self.skip_tags
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if parts.is_empty() {
            "all tasks".to_string()
        } else {
            parts.join(", ")
        }
    }
}

impl Default for TagFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply tag inheritance from playbook level to tasks
pub fn inherit_tags(playbook_tags: &[String], task_tags: &[String]) -> Vec<String> {
    let mut combined: HashSet<String> = playbook_tags.iter().cloned().collect();
    combined.extend(task_tags.iter().cloned());
    combined.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_include() {
        let filter = TagFilter::from_args(Some("deploy"), None);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(!filter.should_run(&["test".to_string()]));
        assert!(!filter.should_run(&[])); // untagged excluded when tags specified
    }

    #[test]
    fn test_skip_tags() {
        let filter = TagFilter::from_args(None, Some("test,debug"));

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(!filter.should_run(&["test".to_string()]));
        assert!(!filter.should_run(&["debug".to_string()]));
        assert!(filter.should_run(&[])); // untagged still runs
    }

    #[test]
    fn test_always_tag() {
        let filter = TagFilter::from_args(Some("deploy"), None);

        // "always" should run even when not in include list
        assert!(filter.should_run(&["always".to_string()]));
        assert!(filter.should_run(&["cleanup".to_string(), "always".to_string()]));
    }

    #[test]
    fn test_never_tag() {
        let filter = TagFilter::from_args(None, None);

        // "never" should not run unless explicitly included
        assert!(!filter.should_run(&["never".to_string()]));

        let filter_with_never = TagFilter::from_args(Some("never"), None);
        assert!(filter_with_never.should_run(&["never".to_string()]));
    }

    #[test]
    fn test_tag_groups() {
        let filter = TagFilter::from_args(Some("@security"), None);

        // Tasks with any tag in the @security group should run
        assert!(filter.should_run(&["security".to_string()]));
        assert!(filter.should_run(&["audit".to_string()]));
        assert!(filter.should_run(&["hardening".to_string()]));
        assert!(!filter.should_run(&["deploy".to_string()]));
    }

    #[test]
    fn test_case_insensitive() {
        let filter = TagFilter::from_args(Some("Deploy"), None);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["DEPLOY".to_string()]));
        assert!(filter.should_run(&["Deploy".to_string()]));
    }

    #[test]
    fn test_inherit_tags() {
        let playbook_tags = vec!["production".to_string()];
        let task_tags = vec!["deploy".to_string()];

        let combined = inherit_tags(&playbook_tags, &task_tags);

        assert!(combined.contains(&"production".to_string()));
        assert!(combined.contains(&"deploy".to_string()));
    }
}
