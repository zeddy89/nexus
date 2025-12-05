// Handler for import_tasks and include_tasks

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::executor::context::ExecutionContext;
use crate::inventory::Host;
use crate::output::errors::NexusError;
use crate::output::terminal::PlayRecap;
use crate::parser::ast::{IncludeTasks, ImportTasks, Value};
use crate::parser::parse_task_file;
use crate::runtime::evaluate_expression;

use super::handlers::HandlerRegistry;
use super::scheduler::Scheduler;
use super::tags::TagFilter;

impl Scheduler {
    /// Execute a single include (optionally with loop item)
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_single_include(
        &self,
        include: &IncludeTasks,
        hosts: &[&Host],
        vars: &HashMap<String, Value>,
        use_sudo: bool,
        sudo_user: &Option<String>,
        tag_filter: &TagFilter,
        handler_registry: &HandlerRegistry,
        recap: &mut PlayRecap,
        loop_item: Option<(Value, usize)>,
    ) -> Result<bool, NexusError> {
        // Create context for variable evaluation
        let mut include_vars = vars.clone();

        // Add loop item if present
        if let Some((item, idx)) = loop_item {
            include_vars.insert(include.loop_var.clone(), item);
            include_vars.insert("ansible_loop_var".to_string(), Value::String(include.loop_var.clone()));
            include_vars.insert("ansible_loop_index".to_string(), Value::Int(idx as i64));
            include_vars.insert("ansible_loop_index0".to_string(), Value::Int(idx as i64));
            include_vars.insert("ansible_loop_index1".to_string(), Value::Int((idx + 1) as i64));
        }

        let ctx = ExecutionContext::new(
            Arc::new(hosts[0].clone()),
            include_vars.clone(),
        );

        // Evaluate file path expression
        let file_path_value = evaluate_expression(&include.file, &ctx)?;
        let file_path = match file_path_value {
            Value::String(s) => s,
            _ => {
                return Err(NexusError::Runtime {
                    function: None,
                    message: format!("include_tasks file must be a string, got: {:?}", file_path_value),
                    suggestion: None,
                });
            }
        };

        // Resolve relative path if needed
        let task_path = if Path::new(&file_path).is_absolute() {
            Path::new(&file_path).to_path_buf()
        } else {
            // Relative to playbook directory if available, otherwise current directory
            let playbook_dir = self.playbook_dir.lock();
            if let Some(ref dir) = *playbook_dir {
                dir.join(file_path.as_str())
            } else {
                Path::new(&file_path).to_path_buf()
            }
        };

        // Load tasks from file
        let included_tasks = parse_task_file(&task_path)?;

        // Merge include vars
        for (k, v_expr) in &include.vars {
            let value = evaluate_expression(v_expr, &ctx)?;
            include_vars.insert(k.clone(), value);
        }

        if self.config.verbose {
            self.output.lock().print_task_header(&format!("INCLUDE: {}", file_path));
        }

        // Execute the included tasks
        Box::pin(self.execute_task_list(
            &included_tasks,
            hosts,
            &include_vars,
            use_sudo,
            sudo_user,
            tag_filter,
            handler_registry,
            recap,
        )).await
    }

    /// Handle static import (called during task list execution)
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_import(
        &self,
        import: &ImportTasks,
        hosts: &[&Host],
        vars: &HashMap<String, Value>,
        use_sudo: bool,
        sudo_user: &Option<String>,
        tag_filter: &TagFilter,
        handler_registry: &HandlerRegistry,
        recap: &mut PlayRecap,
    ) -> Result<bool, NexusError> {
        // Resolve relative path if needed
        let task_path = if Path::new(&import.file).is_absolute() {
            Path::new(&import.file).to_path_buf()
        } else {
            // Relative to playbook directory if available, otherwise current directory
            let playbook_dir = self.playbook_dir.lock();
            if let Some(ref dir) = *playbook_dir {
                dir.join(import.file.as_str())
            } else {
                Path::new(&import.file).to_path_buf()
            }
        };

        // Load tasks from file
        let included_tasks = parse_task_file(&task_path)?;

        // Merge vars for the imported tasks
        let mut import_vars = vars.clone();
        for (k, v) in &import.vars {
            import_vars.insert(k.clone(), v.clone());
        }

        if self.config.verbose {
            self.output.lock().print_task_header(&format!("IMPORT: {}", import.file));
        }

        // Execute the imported tasks
        Box::pin(self.execute_task_list(
            &included_tasks,
            hosts,
            &import_vars,
            use_sudo,
            sudo_user,
            tag_filter,
            handler_registry,
            recap,
        )).await
    }
}
