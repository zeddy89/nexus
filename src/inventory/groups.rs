// Group filtering and pattern matching for inventory

use super::{Host, Inventory};
use crate::parser::ast::HostPattern;

/// Filter options for host selection
#[derive(Debug, Clone, Default)]
pub struct HostFilter {
    /// Limit to specific hosts
    pub limit: Option<Vec<String>>,
    /// Exclude specific hosts
    pub exclude: Option<Vec<String>>,
    /// Filter by variable values
    pub var_filters: Vec<(String, String)>,
}

impl HostFilter {
    pub fn new() -> Self {
        HostFilter::default()
    }

    pub fn with_limit(mut self, hosts: Vec<String>) -> Self {
        self.limit = Some(hosts);
        self
    }

    pub fn with_exclude(mut self, hosts: Vec<String>) -> Self {
        self.exclude = Some(hosts);
        self
    }

    pub fn with_var_filter(mut self, key: String, value: String) -> Self {
        self.var_filters.push((key, value));
        self
    }

    /// Apply filter to hosts
    pub fn apply<'a>(&self, hosts: Vec<&'a Host>) -> Vec<&'a Host> {
        let mut result = hosts;

        // Apply limit
        if let Some(ref limit) = self.limit {
            result.retain(|h| limit.contains(&h.name));
        }

        // Apply exclude
        if let Some(ref exclude) = self.exclude {
            result.retain(|h| !exclude.contains(&h.name));
        }

        // Apply var filters
        for (key, expected) in &self.var_filters {
            result.retain(|h| {
                h.vars
                    .get(key)
                    .map(|v| v.to_string() == *expected)
                    .unwrap_or(false)
            });
        }

        result
    }
}

/// Parse a host pattern string into a HostPattern
pub fn parse_host_pattern(pattern: &str) -> HostPattern {
    let pattern = pattern.trim();

    if pattern.is_empty() || pattern == "all" {
        return HostPattern::All;
    }

    // Check if it's a complex pattern
    if pattern.contains(':') || pattern.contains('&') || pattern.contains('!') {
        return HostPattern::Pattern(pattern.to_string());
    }

    // Check if it looks like a group name (alphanumeric + underscore)
    if pattern
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return HostPattern::Group(pattern.to_string());
    }

    // Treat as pattern
    HostPattern::Pattern(pattern.to_string())
}

/// Expand a pattern to get host names
pub fn expand_pattern(inventory: &Inventory, pattern: &HostPattern) -> Vec<String> {
    inventory
        .get_hosts(pattern)
        .into_iter()
        .map(|h| h.name.clone())
        .collect()
}

/// Check if a host matches a group pattern
pub fn host_matches_group(inventory: &Inventory, host: &Host, group_name: &str) -> bool {
    if let Some(group) = inventory.groups.get(group_name) {
        if group.hosts.contains(&host.name) {
            return true;
        }

        // Check children recursively
        for child in &group.children {
            if host_matches_group(inventory, host, child) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::HostGroup;

    fn create_test_inventory() -> Inventory {
        let mut inv = Inventory::new();

        // Add hosts
        inv.add_host(Host::new("web1").with_var("env", "prod".into()));
        inv.add_host(Host::new("web2").with_var("env", "prod".into()));
        inv.add_host(Host::new("db1").with_var("env", "prod".into()));
        inv.add_host(Host::new("staging1").with_var("env", "staging".into()));

        // Add groups
        let mut web = HostGroup::new("webservers");
        web.hosts = vec!["web1".into(), "web2".into()];
        inv.add_group(web);

        let mut db = HostGroup::new("databases");
        db.hosts = vec!["db1".into()];
        inv.add_group(db);

        let mut staging = HostGroup::new("staging");
        staging.hosts = vec!["staging1".into()];
        inv.add_group(staging);

        inv
    }

    #[test]
    fn test_host_filter_limit() {
        let inv = create_test_inventory();
        let all_hosts = inv.get_hosts(&HostPattern::All);

        let filter = HostFilter::new().with_limit(vec!["web1".into(), "db1".into()]);
        let filtered = filter.apply(all_hosts);

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_host_filter_exclude() {
        let inv = create_test_inventory();
        let all_hosts = inv.get_hosts(&HostPattern::All);

        let filter = HostFilter::new().with_exclude(vec!["staging1".into()]);
        let filtered = filter.apply(all_hosts);

        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_host_filter_var() {
        let inv = create_test_inventory();
        let all_hosts = inv.get_hosts(&HostPattern::All);

        let filter = HostFilter::new().with_var_filter("env".into(), "prod".into());
        let filtered = filter.apply(all_hosts);

        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_parse_host_pattern() {
        assert_eq!(parse_host_pattern("all"), HostPattern::All);
        assert_eq!(parse_host_pattern(""), HostPattern::All);
        assert_eq!(
            parse_host_pattern("webservers"),
            HostPattern::Group("webservers".to_string())
        );
        assert!(matches!(
            parse_host_pattern("web:&prod"),
            HostPattern::Pattern(_)
        ));
    }
}
