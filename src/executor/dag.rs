// Task dependency graph builder

use std::collections::{HashMap, HashSet};

use crate::parser::ast::Task;

/// A node in the task dependency graph
#[derive(Debug, Clone)]
pub struct TaskNode {
    pub id: usize,
    pub task: Task,
    pub dependencies: HashSet<usize>,
}

/// Directed acyclic graph of task dependencies
#[derive(Debug)]
pub struct TaskDag {
    pub nodes: Vec<TaskNode>,
    pub name_to_id: HashMap<String, usize>,
}

impl TaskDag {
    /// Build a DAG from a list of tasks
    pub fn build(tasks: Vec<Task>) -> Self {
        let mut nodes = Vec::new();
        let mut name_to_id = HashMap::new();

        for (id, task) in tasks.into_iter().enumerate() {
            name_to_id.insert(task.name.clone(), id);
            nodes.push(TaskNode {
                id,
                task,
                dependencies: HashSet::new(),
            });
        }

        // By default, tasks are sequential (each depends on the previous)
        // This can be overridden with explicit dependencies or parallel hints
        for (i, node) in nodes.iter_mut().enumerate().skip(1) {
            node.dependencies.insert(i - 1);
        }

        TaskDag { nodes, name_to_id }
    }

    /// Build a DAG with parallel tasks (no implicit dependencies)
    pub fn build_parallel(tasks: Vec<Task>) -> Self {
        let mut nodes = Vec::new();
        let mut name_to_id = HashMap::new();

        for (id, task) in tasks.into_iter().enumerate() {
            name_to_id.insert(task.name.clone(), id);
            nodes.push(TaskNode {
                id,
                task,
                dependencies: HashSet::new(),
            });
        }

        TaskDag { nodes, name_to_id }
    }

    /// Get tasks that are ready to execute (all dependencies satisfied)
    pub fn ready_tasks(&self, completed: &HashSet<usize>) -> Vec<&TaskNode> {
        self.nodes
            .iter()
            .filter(|n| !completed.contains(&n.id))
            .filter(|n| n.dependencies.iter().all(|d| completed.contains(d)))
            .collect()
    }

    /// Add explicit dependency between tasks
    pub fn add_dependency(&mut self, task_name: &str, depends_on: &str) {
        if let (Some(&task_id), Some(&dep_id)) = (
            self.name_to_id.get(task_name),
            self.name_to_id.get(depends_on),
        ) {
            self.nodes[task_id].dependencies.insert(dep_id);
        }
    }

    /// Remove implicit sequential dependency (make task parallel)
    pub fn make_parallel(&mut self, task_name: &str) {
        if let Some(&task_id) = self.name_to_id.get(task_name) {
            // Only remove the implicit dependency (previous task)
            if task_id > 0 {
                self.nodes[task_id].dependencies.remove(&(task_id - 1));
            }
        }
    }

    /// Check for cycles (should never happen with our simple dependency model)
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node in &self.nodes {
            if self.has_cycle_util(node.id, &mut visited, &mut rec_stack) {
                return true;
            }
        }

        false
    }

    fn has_cycle_util(
        &self,
        node_id: usize,
        visited: &mut HashSet<usize>,
        rec_stack: &mut HashSet<usize>,
    ) -> bool {
        if rec_stack.contains(&node_id) {
            return true;
        }

        if visited.contains(&node_id) {
            return false;
        }

        visited.insert(node_id);
        rec_stack.insert(node_id);

        for &dep in &self.nodes[node_id].dependencies {
            if self.has_cycle_util(dep, visited, rec_stack) {
                return true;
            }
        }

        rec_stack.remove(&node_id);
        false
    }

    /// Topological sort of tasks
    pub fn topological_order(&self) -> Vec<usize> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();

        fn visit(
            dag: &TaskDag,
            node_id: usize,
            visited: &mut HashSet<usize>,
            result: &mut Vec<usize>,
        ) {
            if visited.contains(&node_id) {
                return;
            }
            visited.insert(node_id);

            for &dep in &dag.nodes[node_id].dependencies {
                visit(dag, dep, visited, result);
            }

            result.push(node_id);
        }

        for node in &self.nodes {
            visit(self, node.id, &mut visited, &mut result);
        }

        result
    }

    /// Get the number of tasks
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{ModuleCall, Task};

    fn make_task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            module: ModuleCall::Command {
                cmd: crate::parser::ast::Expression::String("echo test".to_string()),
                creates: None,
                removes: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_sequential_dag() {
        let tasks = vec![make_task("task1"), make_task("task2"), make_task("task3")];

        let dag = TaskDag::build(tasks);

        assert_eq!(dag.nodes.len(), 3);
        assert!(dag.nodes[0].dependencies.is_empty());
        assert!(dag.nodes[1].dependencies.contains(&0));
        assert!(dag.nodes[2].dependencies.contains(&1));
    }

    #[test]
    fn test_parallel_dag() {
        let tasks = vec![make_task("task1"), make_task("task2"), make_task("task3")];

        let dag = TaskDag::build_parallel(tasks);

        let completed = HashSet::new();
        let ready = dag.ready_tasks(&completed);

        assert_eq!(ready.len(), 3); // All tasks ready
    }

    #[test]
    fn test_ready_tasks() {
        let tasks = vec![make_task("task1"), make_task("task2"), make_task("task3")];

        let dag = TaskDag::build(tasks);

        let completed = HashSet::new();
        let ready = dag.ready_tasks(&completed);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 0);

        let mut completed = HashSet::new();
        completed.insert(0);
        let ready = dag.ready_tasks(&completed);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
    }

    #[test]
    fn test_no_cycle() {
        let tasks = vec![make_task("task1"), make_task("task2")];
        let dag = TaskDag::build(tasks);
        assert!(!dag.has_cycle());
    }

    #[test]
    fn test_topological_order() {
        let tasks = vec![make_task("task1"), make_task("task2"), make_task("task3")];
        let dag = TaskDag::build(tasks);

        let order = dag.topological_order();
        assert_eq!(order, vec![0, 1, 2]);
    }
}
