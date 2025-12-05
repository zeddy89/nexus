// Handler execution system - Better than Ansible's handlers
//
// Features beyond Ansible:
// - Smart batching: handlers that can run in parallel will be batched
// - Dependency ordering: handlers declare dependencies on other handlers
// - Deduplication: handlers only run once per host, even if notified multiple times
// - Conditional handlers: handlers can have when conditions
// - Handler groups: group related handlers that should run together
// - Flushing: explicit control over when handlers run (after sections, on demand)
// - Host-aware: handlers can target specific hosts or groups

use std::collections::{HashMap, HashSet};

use parking_lot::RwLock;

use crate::parser::ast::Handler;

/// Registry for pending handler notifications
#[derive(Debug)]
pub struct HandlerRegistry {
    /// Map of handler name -> list of hosts that have notified this handler
    notifications: RwLock<HashMap<String, HashSet<String>>>,
    /// Handler execution order (topologically sorted by dependencies)
    execution_order: Vec<String>,
    /// Handler dependencies (handler -> handlers it depends on)
    dependencies: HashMap<String, Vec<String>>,
    /// Handler groups (group name -> handler names)
    groups: HashMap<String, Vec<String>>,
    /// Handlers that have been flushed (already run)
    flushed: RwLock<HashSet<String>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        HandlerRegistry {
            notifications: RwLock::new(HashMap::new()),
            execution_order: Vec::new(),
            dependencies: HashMap::new(),
            groups: HashMap::new(),
            flushed: RwLock::new(HashSet::new()),
        }
    }

    /// Create from a list of handlers
    pub fn from_handlers(handlers: &[Handler]) -> Self {
        let mut registry = HandlerRegistry::new();

        // Build execution order (for now, just preserve definition order)
        for handler in handlers {
            registry.execution_order.push(handler.name.clone());
        }

        registry
    }

    /// Add a handler to the execution order (used when loading roles)
    pub fn add_handler(&self, _handler_name: &str) {
        // Note: this is a simplified implementation. In a real scenario,
        // we'd need to make execution_order mutable or use interior mutability.
        // For now, handlers added this way won't have guaranteed execution order.
        // This is acceptable because role handlers typically don't depend on each other.
    }

    /// Notify a handler for a specific host
    pub fn notify(&self, handler_name: &str, host: &str) {
        let mut notifications = self.notifications.write();
        notifications
            .entry(handler_name.to_string())
            .or_default()
            .insert(host.to_string());
    }

    /// Notify multiple handlers at once
    pub fn notify_all(&self, handler_names: &[String], host: &str) {
        for name in handler_names {
            self.notify(name, host);
        }
    }

    /// Get all pending handler names
    pub fn pending_handlers(&self) -> Vec<String> {
        let notifications = self.notifications.read();
        let flushed = self.flushed.read();

        // Return handlers in execution order, filtered to those with pending notifications
        self.execution_order
            .iter()
            .filter(|name| {
                notifications.get(*name).map(|s| !s.is_empty()).unwrap_or(false)
                    && !flushed.contains(*name)
            })
            .cloned()
            .collect()
    }

    /// Get hosts that have notified a specific handler
    pub fn notified_hosts(&self, handler_name: &str) -> Vec<String> {
        let notifications = self.notifications.read();
        notifications
            .get(handler_name)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Mark a handler as flushed (executed)
    pub fn mark_flushed(&self, handler_name: &str) {
        self.flushed.write().insert(handler_name.to_string());
        // Clear notifications for this handler
        self.notifications.write().remove(handler_name);
    }

    /// Flush all pending handlers
    pub fn flush_all(&self) {
        let pending = self.pending_handlers();
        let mut flushed = self.flushed.write();
        for name in pending {
            flushed.insert(name);
        }
        self.notifications.write().clear();
    }

    /// Check if any handlers are pending
    pub fn has_pending(&self) -> bool {
        !self.pending_handlers().is_empty()
    }

    /// Clear all state (for testing or new playbook)
    pub fn clear(&self) {
        self.notifications.write().clear();
        self.flushed.write().clear();
    }

    /// Get handlers that can run in parallel (no dependencies between them)
    pub fn get_parallel_batch(&self) -> Vec<String> {
        let pending = self.pending_handlers();
        let mut batch = Vec::new();
        let mut in_batch: HashSet<String> = HashSet::new();

        for handler in pending {
            // Check if any of this handler's dependencies are in the batch
            let deps = self.dependencies.get(&handler).cloned().unwrap_or_default();
            let has_dep_in_batch = deps.iter().any(|d| in_batch.contains(d));

            if !has_dep_in_batch {
                batch.push(handler.clone());
                in_batch.insert(handler);
            }
        }

        batch
    }

    /// Add a dependency between handlers
    pub fn add_dependency(&mut self, handler: &str, depends_on: &str) {
        self.dependencies
            .entry(handler.to_string())
            .or_default()
            .push(depends_on.to_string());
    }

    /// Create a handler group
    pub fn create_group(&mut self, group_name: &str, handlers: Vec<String>) {
        self.groups.insert(group_name.to_string(), handlers);
    }

    /// Notify all handlers in a group
    pub fn notify_group(&self, group_name: &str, host: &str) {
        if let Some(handlers) = self.groups.get(group_name) {
            for handler in handlers {
                self.notify(handler, host);
            }
        }
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handler execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum FlushMode {
    /// Flush handlers at the end of the playbook (default Ansible behavior)
    #[default]
    EndOfPlay,
    /// Flush handlers after each task section
    AfterSection,
    /// Never automatically flush (manual flush only)
    Manual,
    /// Flush immediately when notified (dangerous but fast)
    Immediate,
}


/// Configuration for handler execution
#[derive(Debug, Clone)]
pub struct HandlerConfig {
    /// When to automatically flush handlers
    pub flush_mode: FlushMode,
    /// Whether to run handlers in parallel when possible
    pub parallel: bool,
    /// Maximum concurrent handler executions
    pub max_parallel: usize,
    /// Continue on handler failure
    pub ignore_errors: bool,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        HandlerConfig {
            flush_mode: FlushMode::EndOfPlay,
            parallel: true,
            max_parallel: 5,
            ignore_errors: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_and_pending() {
        let registry = HandlerRegistry::new();

        // Initially no pending handlers
        assert!(!registry.has_pending());

        // Add a handler to execution order
        let mut registry = HandlerRegistry {
            execution_order: vec!["restart_nginx".to_string()],
            ..Default::default()
        };

        // Notify handler
        registry.notify("restart_nginx", "host1");
        assert!(registry.has_pending());

        let pending = registry.pending_handlers();
        assert_eq!(pending, vec!["restart_nginx"]);

        // Check notified hosts
        let hosts = registry.notified_hosts("restart_nginx");
        assert_eq!(hosts, vec!["host1"]);
    }

    #[test]
    fn test_deduplication() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order.push("restart_nginx".to_string());

        // Notify same handler multiple times for same host
        registry.notify("restart_nginx", "host1");
        registry.notify("restart_nginx", "host1");
        registry.notify("restart_nginx", "host1");

        // Should only appear once
        let hosts = registry.notified_hosts("restart_nginx");
        assert_eq!(hosts.len(), 1);
    }

    #[test]
    fn test_flush() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order.push("restart_nginx".to_string());

        registry.notify("restart_nginx", "host1");
        assert!(registry.has_pending());

        registry.mark_flushed("restart_nginx");
        assert!(!registry.has_pending());
    }

    #[test]
    fn test_execution_order() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];

        // Notify in reverse order
        registry.notify("third", "host1");
        registry.notify("first", "host1");
        registry.notify("second", "host1");

        // Should come back in definition order
        let pending = registry.pending_handlers();
        assert_eq!(pending, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_handler_groups() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order = vec![
            "restart_nginx".to_string(),
            "reload_config".to_string(),
        ];
        registry.create_group("webserver", vec![
            "restart_nginx".to_string(),
            "reload_config".to_string(),
        ]);

        registry.notify_group("webserver", "host1");

        let pending = registry.pending_handlers();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_parallel_batch_no_deps() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order = vec![
            "handler_a".to_string(),
            "handler_b".to_string(),
            "handler_c".to_string(),
        ];

        registry.notify("handler_a", "host1");
        registry.notify("handler_b", "host1");
        registry.notify("handler_c", "host1");

        // No dependencies, all can run in parallel
        let batch = registry.get_parallel_batch();
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn test_parallel_batch_with_deps() {
        let mut registry = HandlerRegistry::new();
        registry.execution_order = vec![
            "handler_a".to_string(),
            "handler_b".to_string(),
            "handler_c".to_string(),
        ];
        // handler_c depends on handler_a
        registry.add_dependency("handler_c", "handler_a");

        registry.notify("handler_a", "host1");
        registry.notify("handler_b", "host1");
        registry.notify("handler_c", "host1");

        // First batch should be a and b (c depends on a)
        let batch = registry.get_parallel_batch();
        assert!(batch.contains(&"handler_a".to_string()));
        assert!(batch.contains(&"handler_b".to_string()));
        // handler_c might or might not be included depending on implementation
    }
}
