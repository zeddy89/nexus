// Parallel task scheduler

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::future::join_all;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use super::async_jobs::AsyncJobTracker;
use super::checkpoint::{Checkpoint, CheckpointManager};
use super::context::{ExecutionContext, TaskOutput};
use super::dag::TaskDag;
use super::handlers::HandlerRegistry;
use super::retry::{calculate_delay, CircuitBreakerRegistry};
use super::ssh::ConnectionPool;
use super::tags::TagFilter;
use crate::inventory::{Host, Inventory};
use crate::modules::ModuleExecutor;
use crate::output::errors::NexusError;
use crate::output::events::{EventEmitter, TaskStatus};
use crate::output::terminal::{PlayRecap, TaskResult};
use crate::output::OutputWriter;
use crate::parser::ast::{Block, Handler, Playbook, Serial, Task, TaskOrBlock, Value};
use crate::parser::roles::RoleResolver;
use crate::plugins::CallbackManager;
use crate::runtime::evaluate_expression;

/// Configuration for the scheduler
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum concurrent hosts
    pub max_parallel_hosts: usize,
    /// Maximum concurrent tasks per host
    pub max_parallel_tasks: usize,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Command timeout
    pub command_timeout: Duration,
    /// Check mode (dry run)
    pub check_mode: bool,
    /// Diff mode (show file differences)
    pub diff_mode: bool,
    /// Verbose output
    pub verbose: bool,
    /// SSH password for authentication
    pub ssh_password: Option<String>,
    /// SSH private key path
    pub ssh_private_key: Option<String>,
    /// SSH user override
    pub ssh_user: Option<String>,
    /// Enable sudo for all tasks (CLI override)
    pub sudo: bool,
    /// Sudo password for privilege escalation
    pub sudo_password: Option<String>,
    /// Tag filter for selecting tasks
    pub tag_filter: Option<TagFilter>,
    /// Enable checkpoint/resume support
    pub enable_checkpoints: bool,
    /// Resume from checkpoint
    pub resume: bool,
    /// Resume from specific checkpoint file
    pub resume_from: Option<std::path::PathBuf>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            max_parallel_hosts: 10,
            max_parallel_tasks: 1,
            connect_timeout: Duration::from_secs(30),
            command_timeout: Duration::from_secs(300),
            check_mode: false,
            diff_mode: false,
            verbose: false,
            ssh_password: None,
            ssh_private_key: None,
            ssh_user: None,
            sudo: false,
            sudo_password: None,
            tag_filter: None,
            enable_checkpoints: false,
            resume: false,
            resume_from: None,
        }
    }
}

/// The task scheduler
#[allow(dead_code)]
pub struct Scheduler {
    pub(super) config: SchedulerConfig,
    pool: Arc<ConnectionPool>,
    modules: Arc<ModuleExecutor>,
    pub(super) output: Arc<Mutex<OutputWriter>>,
    /// Circuit breaker registry for retry logic
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    /// Role resolver for loading roles
    role_resolver: Mutex<RoleResolver>,
    /// Async job tracker for background tasks
    async_tracker: Arc<AsyncJobTracker>,
    /// Callback manager for plugins
    callbacks: Arc<CallbackManager>,
    /// Checkpoint manager for resume support
    checkpoint_manager: Option<Arc<CheckpointManager>>,
    /// Active checkpoint for resume
    active_checkpoint: Arc<Mutex<Option<Checkpoint>>>,
    /// Optional event emitter for TUI mode
    event_emitter: Option<EventEmitter>,
    /// Playbook directory for resolving relative paths in includes/imports
    pub(super) playbook_dir: Arc<Mutex<Option<std::path::PathBuf>>>,
    /// Per-host execution contexts that persist registered variables across tasks
    host_contexts: Arc<DashMap<String, ExecutionContext>>,
}

impl Scheduler {
    pub fn new(config: SchedulerConfig, output: Arc<Mutex<OutputWriter>>) -> Self {
        Self::with_callbacks(config, output, Arc::new(CallbackManager::new()))
    }

    pub fn with_callbacks(
        config: SchedulerConfig,
        output: Arc<Mutex<OutputWriter>>,
        callbacks: Arc<CallbackManager>,
    ) -> Self {
        let mut pool = ConnectionPool::new()
            .with_connect_timeout(config.connect_timeout)
            .with_command_timeout(config.command_timeout);

        if let Some(ref password) = config.ssh_password {
            pool = pool.with_password(password.clone());
        }
        if let Some(ref key) = config.ssh_private_key {
            pool = pool.with_private_key(key.clone());
        }
        if let Some(ref user) = config.ssh_user {
            pool = pool.with_default_user(user.clone());
        }

        Scheduler {
            config,
            pool: Arc::new(pool),
            modules: Arc::new(ModuleExecutor::new()),
            output,
            circuit_breakers: Arc::new(CircuitBreakerRegistry::new()),
            role_resolver: Mutex::new(RoleResolver::new()),
            async_tracker: Arc::new(AsyncJobTracker::new()),
            callbacks,
            checkpoint_manager: None,
            active_checkpoint: Arc::new(Mutex::new(None)),
            event_emitter: None,
            playbook_dir: Arc::new(Mutex::new(None)),
            host_contexts: Arc::new(DashMap::new()),
        }
    }

    /// Add a role search path
    pub fn add_role_search_path(&self, path: impl Into<std::path::PathBuf>) {
        self.role_resolver.lock().add_search_path(path);
    }

    /// Add a role search path relative to the playbook
    pub fn add_playbook_role_path(&self, playbook_path: &std::path::Path) {
        self.role_resolver
            .lock()
            .add_playbook_relative_path(playbook_path);
    }

    /// Set the event emitter for TUI mode
    pub fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }

    /// Get or create an execution context for a host
    /// This ensures registered variables persist across tasks for the same host
    fn get_or_create_context(
        &self,
        host: &Host,
        playbook_vars: &HashMap<String, Value>,
    ) -> ExecutionContext {
        self.host_contexts
            .entry(host.name.clone())
            .or_insert_with(|| ExecutionContext::new(Arc::new(host.clone()), playbook_vars.clone()))
            .clone()
    }

    /// Clear host contexts (should be called at start of playbook execution)
    fn clear_host_contexts(&self) {
        self.host_contexts.clear();
    }

    /// Execute a playbook
    pub async fn execute_playbook(
        &self,
        playbook: &Playbook,
        inventory: &Inventory,
    ) -> Result<PlayRecap, NexusError> {
        // Clear any previous host contexts to start fresh
        self.clear_host_contexts();

        // Set playbook directory for resolving relative includes/imports
        {
            let path = std::path::Path::new(&playbook.source_file);
            if let Some(dir) = path.parent() {
                *self.playbook_dir.lock() = Some(dir.to_path_buf());
            }
        }

        let hosts = inventory.get_hosts(&playbook.hosts);

        if hosts.is_empty() {
            return Err(NexusError::Inventory {
                message: format!("No hosts matched pattern: {:?}", playbook.hosts),
                suggestion: Some("Check your inventory and host pattern".to_string()),
            });
        }

        // Print header
        {
            let out = self.output.lock();
            out.print_playbook_header(&playbook.source_file, hosts.len());
        }

        // Callback: playbook start
        let host_names: Vec<String> = hosts.iter().map(|h| h.name.clone()).collect();
        self.callbacks
            .on_playbook_start(&playbook.source_file, &host_names)
            .await;

        // Emit playbook start event for TUI
        if let Some(ref emitter) = self.event_emitter {
            // Count total tasks including role tasks
            let mut total_tasks =
                playbook.tasks.len() + playbook.pre_tasks.len() + playbook.post_tasks.len();

            // Add tasks from roles
            for role_ref in &playbook.roles {
                // Resolve role dependencies to get all roles that will be executed
                let role_execution_order = {
                    let mut resolver = self.role_resolver.lock();
                    match resolver.resolve_dependencies(&role_ref.role) {
                        Ok(order) => order,
                        Err(_) => continue, // Skip counting if resolution fails
                    }
                };

                // Count tasks in each role
                for role_name in role_execution_order {
                    let role = {
                        let mut resolver = self.role_resolver.lock();
                        match resolver.resolve(&role_name) {
                            Ok(r) => r.clone(),
                            Err(_) => continue,
                        }
                    };
                    total_tasks += role.tasks.len();
                }
            }

            emitter.playbook_start(
                playbook.source_file.clone(),
                host_names.clone(),
                total_tasks,
            );
        }

        // If serial execution is configured, use batched execution
        if let Some(ref serial) = playbook.serial {
            return self
                .execute_playbook_serial(playbook, inventory, &hosts, serial)
                .await;
        }

        let mut recap = PlayRecap::new();
        let start_time = Instant::now();

        // Collect all handlers (from playbook + roles)
        let mut all_handlers = playbook.handlers.clone();

        // Create handler registry - will be populated with role handlers too
        let handler_registry = Arc::new(HandlerRegistry::from_handlers(&playbook.handlers));

        // CLI --sudo flag overrides playbook sudo setting
        let use_sudo = self.config.sudo || playbook.sudo;

        // Get tag filter (default allows all tasks)
        let tag_filter = self.config.tag_filter.clone().unwrap_or_default();

        // Merge playbook vars with role defaults/vars
        let mut effective_vars = playbook.vars.clone();

        // 0. Auto-gather facts if enabled
        if playbook.gather_facts {
            use crate::executor::facts::{FactCategory, FactGatherer};
            use std::collections::HashMap;

            if self.config.verbose {
                self.output.lock().print_task_header("GATHERING FACTS");
            }

            // Gather facts on all hosts
            for host in &hosts {
                // Skip fact gathering for local connections (not yet implemented)
                if host.is_local() {
                    if self.config.verbose {
                        let out = self.output.lock();
                        out.print_task_result(&TaskResult {
                            host: host.name.clone(),
                            task_name: "Gathering Facts".to_string(),
                            changed: false,
                            failed: false,
                            skipped: true,
                            stdout: Some("Skipped for local connection".to_string()),
                            stderr: None,
                            message: None,
                            duration: Duration::from_millis(0),
                            diff: None,
                        });
                    }
                    continue;
                }

                let conn = self.pool.get(host)?;

                // Gather all fact categories
                match FactGatherer::gather(&conn, &[FactCategory::All]) {
                    Ok(facts) => {
                        // Convert facts to Ansible-compatible names
                        let mut ansible_facts = HashMap::new();
                        for (key, value) in &facts {
                            let ansible_key = match key.as_str() {
                                "hostname" => "ansible_hostname",
                                "hostname_short" => "ansible_hostname_short",
                                "os_family" => "ansible_os_family",
                                "os_name" => "ansible_distribution",
                                "os_version" => "ansible_distribution_version",
                                "kernel_version" => "ansible_kernel",
                                "architecture" => "ansible_architecture",
                                "cpu_count" => "ansible_processor_count",
                                "memory_total_mb" => "ansible_memtotal_mb",
                                "default_ipv4" => "ansible_default_ipv4_address",
                                "interfaces" => "ansible_interfaces",
                                _ => key.as_str(),
                            };
                            ansible_facts.insert(ansible_key.to_string(), value.clone());
                        }

                        // Store facts in effective_vars for this playbook run
                        for (key, value) in ansible_facts {
                            effective_vars.insert(key, value);
                        }

                        if self.config.verbose {
                            let out = self.output.lock();
                            out.print_task_result(&TaskResult {
                                host: host.name.clone(),
                                task_name: "Gathering Facts".to_string(),
                                changed: false,
                                failed: false,
                                skipped: false,
                                stdout: Some(format!("Gathered {} facts", facts.len())),
                                stderr: None,
                                message: None,
                                duration: Duration::from_millis(0),
                                diff: None,
                            });
                        }
                    }
                    Err(e) => {
                        if self.config.verbose {
                            let out = self.output.lock();
                            out.print_task_result(&TaskResult {
                                host: host.name.clone(),
                                task_name: "Gathering Facts".to_string(),
                                changed: false,
                                failed: true,
                                skipped: false,
                                stdout: None,
                                stderr: Some(e.to_string()),
                                message: Some(format!("Failed to gather facts: {}", e)),
                                duration: Duration::from_millis(0),
                                diff: None,
                            });
                        }
                        // Don't fail the playbook if fact gathering fails
                    }
                }
            }
        }

        // 1. Execute pre_tasks
        if !playbook.pre_tasks.is_empty() {
            self.output.lock().print_task_header("PRE-TASKS");

            let failed = self
                .execute_task_list(
                    &playbook.pre_tasks,
                    &hosts,
                    &effective_vars,
                    use_sudo,
                    &playbook.sudo_user,
                    &tag_filter,
                    &handler_registry,
                    &mut recap,
                )
                .await?;

            if failed {
                recap.total_duration = start_time.elapsed();
                self.output.lock().print_recap(&recap);
                return Ok(recap);
            }
        }

        // 2. Execute roles
        if !playbook.roles.is_empty() {
            for role_ref in &playbook.roles {
                // Check when condition for role
                if let Some(ref when) = role_ref.when {
                    let ctx =
                        ExecutionContext::new(Arc::new(hosts[0].clone()), effective_vars.clone());
                    let result = evaluate_expression(when, &ctx)?;
                    if !result.is_truthy() {
                        if self.config.verbose {
                            self.output.lock().print_task_header(&format!(
                                "ROLE: {} (skipped by condition)",
                                role_ref.role
                            ));
                        }
                        continue;
                    }
                }

                // Load role with dependencies
                let role_execution_order = {
                    let mut resolver = self.role_resolver.lock();
                    resolver.resolve_dependencies(&role_ref.role)?
                };

                for role_name in role_execution_order {
                    let role = {
                        let mut resolver = self.role_resolver.lock();
                        resolver.resolve(&role_name)?.clone()
                    };

                    // Create role-specific vars (defaults < role vars < role_ref vars < playbook vars)
                    let mut role_vars = role.defaults.clone();
                    for (k, v) in &role.vars {
                        role_vars.insert(k.clone(), v.clone());
                    }
                    for (k, v) in &role_ref.vars {
                        role_vars.insert(k.clone(), v.clone());
                    }
                    for (k, v) in &effective_vars {
                        role_vars.insert(k.clone(), v.clone());
                    }

                    // Add role paths to vars for template/file lookups
                    if let Some(ref templates_path) = role.templates_path {
                        role_vars.insert(
                            "role_templates_path".to_string(),
                            Value::String(templates_path.clone()),
                        );
                    }
                    if let Some(ref files_path) = role.files_path {
                        role_vars.insert(
                            "role_files_path".to_string(),
                            Value::String(files_path.clone()),
                        );
                    }
                    role_vars.insert("role_path".to_string(), Value::String(role.path.clone()));
                    role_vars.insert("role_name".to_string(), Value::String(role.name.clone()));

                    // Print role header
                    self.output
                        .lock()
                        .print_task_header(&format!("ROLE: {}", role.name));

                    // Add role handlers to registry
                    for handler in &role.handlers {
                        handler_registry.add_handler(&handler.name);
                        all_handlers.push(handler.clone());
                    }

                    // Apply role-level tags
                    let role_tag_filter = if !role_ref.tags.is_empty() {
                        TagFilter::include_tags(role_ref.tags.clone())
                    } else {
                        tag_filter.clone()
                    };

                    // Execute role tasks
                    let failed = self
                        .execute_task_list(
                            &role.tasks,
                            &hosts,
                            &role_vars,
                            use_sudo,
                            &playbook.sudo_user,
                            &role_tag_filter,
                            &handler_registry,
                            &mut recap,
                        )
                        .await?;

                    if failed {
                        recap.total_duration = start_time.elapsed();
                        self.output.lock().print_recap(&recap);
                        return Ok(recap);
                    }
                }
            }
        }

        // 3. Execute regular tasks
        if !playbook.tasks.is_empty() {
            let failed = self
                .execute_task_list(
                    &playbook.tasks,
                    &hosts,
                    &effective_vars,
                    use_sudo,
                    &playbook.sudo_user,
                    &tag_filter,
                    &handler_registry,
                    &mut recap,
                )
                .await?;

            if failed {
                recap.total_duration = start_time.elapsed();
                self.output.lock().print_recap(&recap);
                return Ok(recap);
            }
        }

        // 4. Execute post_tasks
        if !playbook.post_tasks.is_empty() {
            self.output.lock().print_task_header("POST-TASKS");

            let failed = self
                .execute_task_list(
                    &playbook.post_tasks,
                    &hosts,
                    &effective_vars,
                    use_sudo,
                    &playbook.sudo_user,
                    &tag_filter,
                    &handler_registry,
                    &mut recap,
                )
                .await?;

            if failed {
                recap.total_duration = start_time.elapsed();
                self.output.lock().print_recap(&recap);
                return Ok(recap);
            }
        }

        // 5. Execute pending handlers at end of playbook
        if handler_registry.has_pending() {
            if self.config.verbose {
                self.output.lock().print_task_header("RUNNING HANDLERS");
            }

            let handler_results = self
                .execute_handlers(
                    &all_handlers,
                    &hosts,
                    &effective_vars,
                    use_sudo,
                    &playbook.sudo_user,
                    &handler_registry,
                )
                .await?;

            for result in handler_results {
                recap.record(&result);
                self.output.lock().print_task_result(&result);
            }
        }

        recap.total_duration = start_time.elapsed();

        // Callback: playbook complete
        self.callbacks.on_playbook_complete(&recap).await;

        // Emit playbook complete event for TUI
        if let Some(ref emitter) = self.event_emitter {
            emitter.playbook_complete(recap.clone());
        }

        // Print recap
        self.output.lock().print_recap(&recap);

        Ok(recap)
    }

    /// Execute a list of tasks, returns true if execution should stop (failure)
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_task_list(
        &self,
        tasks: &[TaskOrBlock],
        hosts: &[&Host],
        vars: &HashMap<String, Value>,
        use_sudo: bool,
        sudo_user: &Option<String>,
        tag_filter: &TagFilter,
        handler_registry: &HandlerRegistry,
        recap: &mut PlayRecap,
    ) -> Result<bool, NexusError> {
        for item in tasks {
            match item {
                TaskOrBlock::Import(import) => {
                    // Static import - load tasks from file and execute inline
                    let failed = self
                        .execute_import(
                            import,
                            hosts,
                            vars,
                            use_sudo,
                            sudo_user,
                            tag_filter,
                            handler_registry,
                            recap,
                        )
                        .await?;

                    if failed {
                        return Ok(true);
                    }
                }
                TaskOrBlock::Include(include) => {
                    // Dynamic include - resolve file path and vars at runtime
                    // Check when condition
                    if let Some(ref when) = include.when {
                        let ctx = ExecutionContext::new(Arc::new(hosts[0].clone()), vars.clone());
                        let result = evaluate_expression(when, &ctx)?;
                        if !result.is_truthy() {
                            if self.config.verbose {
                                self.output
                                    .lock()
                                    .print_task_header("INCLUDE (skipped by condition)");
                            }
                            continue;
                        }
                    }

                    // Handle loop
                    if let Some(ref loop_expr) = include.loop_expr {
                        let ctx = ExecutionContext::new(Arc::new(hosts[0].clone()), vars.clone());
                        let loop_value = evaluate_expression(loop_expr, &ctx)?;

                        let items = match loop_value {
                            Value::List(items) => items,
                            _ => {
                                return Err(NexusError::Runtime {
                                    function: None,
                                    message: "include_tasks loop must evaluate to a list"
                                        .to_string(),
                                    suggestion: None,
                                })
                            }
                        };

                        // Execute include for each item
                        for (i, item) in items.into_iter().enumerate() {
                            let failed = self
                                .execute_single_include(
                                    include,
                                    hosts,
                                    vars,
                                    use_sudo,
                                    sudo_user,
                                    tag_filter,
                                    handler_registry,
                                    recap,
                                    Some((item, i)),
                                )
                                .await?;

                            if failed {
                                return Ok(true);
                            }
                        }
                    } else {
                        // No loop - execute once
                        let failed = self
                            .execute_single_include(
                                include,
                                hosts,
                                vars,
                                use_sudo,
                                sudo_user,
                                tag_filter,
                                handler_registry,
                                recap,
                                None,
                            )
                            .await?;

                        if failed {
                            return Ok(true);
                        }
                    }
                }
                TaskOrBlock::Task(task) => {
                    let results = self
                        .execute_task_on_hosts_with_handlers(
                            task,
                            hosts,
                            vars,
                            use_sudo,
                            sudo_user,
                            handler_registry,
                        )
                        .await?;

                    for result in results {
                        recap.record(&result);
                        self.output.lock().print_task_result(&result);

                        // Stop on failure
                        if result.failed {
                            return Ok(true);
                        }
                    }
                }
                TaskOrBlock::Block(block) => {
                    // Check if block should run based on tags
                    if !tag_filter.should_run(&block.tags) {
                        if self.config.verbose {
                            let block_name = block.name.as_deref().unwrap_or("Block");
                            self.output
                                .lock()
                                .print_task_header(&format!("{} (skipped by tags)", block_name));
                        }
                        continue;
                    }

                    // Check when condition for block
                    if let Some(ref when) = block.when {
                        // Create a temporary context for the when evaluation
                        let ctx = ExecutionContext::new(Arc::new(hosts[0].clone()), vars.clone());
                        let result = evaluate_expression(when, &ctx)?;
                        if !result.is_truthy() {
                            if self.config.verbose {
                                let block_name = block.name.as_deref().unwrap_or("Block");
                                self.output.lock().print_task_header(&format!(
                                    "{} (skipped by condition)",
                                    block_name
                                ));
                            }
                            continue;
                        }
                    }

                    // Execute block
                    let block_failed = self
                        .execute_block(
                            block,
                            hosts,
                            vars,
                            use_sudo,
                            sudo_user,
                            tag_filter,
                            handler_registry,
                            recap,
                        )
                        .await?;

                    if block_failed {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Execute a block with rescue/always logic
    #[allow(clippy::too_many_arguments)]
    async fn execute_block(
        &self,
        block: &Block,
        hosts: &[&Host],
        vars: &HashMap<String, Value>,
        use_sudo: bool,
        sudo_user: &Option<String>,
        tag_filter: &TagFilter,
        handler_registry: &HandlerRegistry,
        recap: &mut PlayRecap,
    ) -> Result<bool, NexusError> {
        let block_name = block.name.as_deref().unwrap_or("Block");

        // Print block header
        if self.config.verbose {
            self.output
                .lock()
                .print_task_header(&format!("BLOCK: {}", block_name));
        }

        let mut block_failed = false;
        let mut failed_task_info: Option<(String, String)> = None;

        // Execute main block tasks
        for task in &block.block {
            // Check if task should run based on tags
            if !tag_filter.should_run(&task.tags) {
                if self.config.verbose {
                    self.output
                        .lock()
                        .print_task_header(&format!("{} (skipped by tags)", task.name));
                }
                continue;
            }

            let results = self
                .execute_task_on_hosts_with_handlers(
                    task,
                    hosts,
                    vars,
                    use_sudo,
                    sudo_user,
                    handler_registry,
                )
                .await?;

            for result in results {
                recap.record(&result);
                self.output.lock().print_task_result(&result);

                // If task failed, run rescue
                if result.failed {
                    block_failed = true;
                    failed_task_info = Some((
                        task.name.clone(),
                        result.message.unwrap_or_else(|| "Task failed".to_string()),
                    ));
                    break;
                }
            }

            if block_failed {
                break;
            }
        }

        // Execute rescue tasks if block failed
        if block_failed && !block.rescue.is_empty() {
            if self.config.verbose {
                self.output
                    .lock()
                    .print_task_header(&format!("RESCUE: {}", block_name));
            }

            // Add failed task info to vars for rescue tasks
            let mut rescue_vars = vars.clone();
            if let Some((task_name, error_msg)) = failed_task_info {
                let mut nexus_failed_task = HashMap::new();
                nexus_failed_task.insert("task".to_string(), Value::String(task_name));
                nexus_failed_task.insert("message".to_string(), Value::String(error_msg));
                rescue_vars.insert(
                    "nexus_failed_task".to_string(),
                    Value::Dict(nexus_failed_task),
                );
            }

            // Execute rescue tasks
            for task in &block.rescue {
                if !tag_filter.should_run(&task.tags) {
                    if self.config.verbose {
                        self.output
                            .lock()
                            .print_task_header(&format!("{} (skipped by tags)", task.name));
                    }
                    continue;
                }

                let results = self
                    .execute_task_on_hosts_with_handlers(
                        task,
                        hosts,
                        &rescue_vars,
                        use_sudo,
                        sudo_user,
                        handler_registry,
                    )
                    .await?;

                for result in results {
                    recap.record(&result);
                    self.output.lock().print_task_result(&result);

                    // If rescue task fails, the whole block fails
                    if result.failed {
                        // Execute always tasks and return failure
                        self.execute_always_tasks(
                            &block.always,
                            hosts,
                            vars,
                            use_sudo,
                            sudo_user,
                            tag_filter,
                            handler_registry,
                            recap,
                        )
                        .await?;
                        return Ok(true);
                    }
                }
            }

            // Rescue succeeded, reset block_failed
            block_failed = false;
        }

        // Execute always tasks (cleanup)
        if !block.always.is_empty() {
            self.execute_always_tasks(
                &block.always,
                hosts,
                vars,
                use_sudo,
                sudo_user,
                tag_filter,
                handler_registry,
                recap,
            )
            .await?;
        }

        Ok(block_failed)
    }

    /// Execute always tasks (cleanup section)
    #[allow(clippy::too_many_arguments)]
    async fn execute_always_tasks(
        &self,
        always_tasks: &[Task],
        hosts: &[&Host],
        vars: &HashMap<String, Value>,
        use_sudo: bool,
        sudo_user: &Option<String>,
        tag_filter: &TagFilter,
        handler_registry: &HandlerRegistry,
        recap: &mut PlayRecap,
    ) -> Result<(), NexusError> {
        if always_tasks.is_empty() {
            return Ok(());
        }

        if self.config.verbose {
            self.output.lock().print_task_header("ALWAYS");
        }

        for task in always_tasks {
            if !tag_filter.should_run(&task.tags) {
                if self.config.verbose {
                    self.output
                        .lock()
                        .print_task_header(&format!("{} (skipped by tags)", task.name));
                }
                continue;
            }

            let results = self
                .execute_task_on_hosts_with_handlers(
                    task,
                    hosts,
                    vars,
                    use_sudo,
                    sudo_user,
                    handler_registry,
                )
                .await?;

            for result in results {
                recap.record(&result);
                self.output.lock().print_task_result(&result);
                // Note: We don't stop on failure in always section
            }
        }

        Ok(())
    }

    /// Execute pending handlers
    async fn execute_handlers(
        &self,
        handlers: &[Handler],
        hosts: &[&Host],
        playbook_vars: &HashMap<String, Value>,
        playbook_sudo: bool,
        playbook_sudo_user: &Option<String>,
        registry: &HandlerRegistry,
    ) -> Result<Vec<TaskResult>, NexusError> {
        let mut all_results = Vec::new();

        // Get pending handlers in execution order
        let pending = registry.pending_handlers();

        for handler_name in pending {
            // Find the handler definition
            let handler = match handlers.iter().find(|h| h.name == handler_name) {
                Some(h) => h,
                None => {
                    // Handler was notified but not defined - this is an error
                    return Err(NexusError::Runtime {
                        function: None,
                        message: format!(
                            "Handler '{}' was notified but is not defined",
                            handler_name
                        ),
                        suggestion: Some("Define the handler in the handlers section".to_string()),
                    });
                }
            };

            // Get hosts that notified this handler
            let notified_hosts: Vec<&Host> = registry
                .notified_hosts(&handler_name)
                .iter()
                .filter_map(|h| hosts.iter().find(|host| host.name == *h).copied())
                .collect();

            if notified_hosts.is_empty() {
                continue;
            }

            // Print handler header
            {
                let out = self.output.lock();
                out.print_task_header(&format!("HANDLER: {}", handler.name));
            }

            // Convert Handler to Task for execution
            let task = Task {
                name: handler.name.clone(),
                module: handler.module.clone(),
                when: None,
                register: None,
                fail_when: None,
                changed_when: None,
                notify: Vec::new(),
                loop_expr: None,
                loop_var: "item".to_string(),
                location: handler.location.clone(),
                sudo: None,
                run_as: None,
                tags: Vec::new(),
                retry: None,
                async_config: None,
                timeout: None,
                throttle: None,
                delegate_to: None,
                delegate_facts: false,
            };

            // Callback: handler start for each host
            for host in &notified_hosts {
                self.callbacks
                    .on_handler_start(&host.name, &handler.name)
                    .await;
            }

            // Execute handler on notified hosts
            let results = self
                .execute_task_on_hosts(
                    &task,
                    &notified_hosts,
                    playbook_vars,
                    playbook_sudo,
                    playbook_sudo_user,
                )
                .await?;

            // Callback: handler complete for each result
            for result in &results {
                if !result.failed {
                    // Convert TaskResult back to TaskOutput for callback
                    let output = TaskOutput {
                        stdout: result.stdout.clone().unwrap_or_default(),
                        stderr: result.stderr.clone().unwrap_or_default(),
                        exit_code: if result.failed { 1 } else { 0 },
                        changed: result.changed,
                        failed: result.failed,
                        skipped: result.skipped,
                        message: result.message.clone(),
                        data: HashMap::new(),
                        diff: result.diff.clone(),
                    };
                    self.callbacks
                        .on_handler_complete(&result.host, &handler.name, &output)
                        .await;
                }
            }

            all_results.extend(results);

            // Mark handler as flushed
            registry.mark_flushed(&handler_name);
        }

        Ok(all_results)
    }

    /// Execute a task on multiple hosts in parallel, with handler notification support
    async fn execute_task_on_hosts_with_handlers(
        &self,
        task: &Task,
        hosts: &[&Host],
        playbook_vars: &HashMap<String, Value>,
        playbook_sudo: bool,
        playbook_sudo_user: &Option<String>,
        handler_registry: &HandlerRegistry,
    ) -> Result<Vec<TaskResult>, NexusError> {
        let results = self
            .execute_task_on_hosts(
                task,
                hosts,
                playbook_vars,
                playbook_sudo,
                playbook_sudo_user,
            )
            .await?;

        // Track handler notifications for hosts where task changed
        if !task.notify.is_empty() {
            for result in &results {
                if result.changed && !result.failed {
                    // Notify all handlers for this host
                    handler_registry.notify_all(&task.notify, &result.host);
                }
            }
        }

        Ok(results)
    }

    /// Execute a task on multiple hosts in parallel
    async fn execute_task_on_hosts(
        &self,
        task: &Task,
        hosts: &[&Host],
        playbook_vars: &HashMap<String, Value>,
        playbook_sudo: bool,
        playbook_sudo_user: &Option<String>,
    ) -> Result<Vec<TaskResult>, NexusError> {
        // Print task header
        {
            let out = self.output.lock();
            out.print_task_header(&task.name);
        }

        // Semaphore to limit concurrent hosts
        // Task-level throttle overrides global max_parallel_hosts
        let max_concurrent = task.throttle.unwrap_or(self.config.max_parallel_hosts);
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        // Determine effective sudo settings for this task
        // Task-level overrides playbook-level, run_as overrides sudo_user
        let use_sudo = task.sudo.unwrap_or(playbook_sudo);
        let sudo_user = task.run_as.clone().or_else(|| playbook_sudo_user.clone());

        // Create futures for each host
        let event_emitter = self.event_emitter.clone();
        let futures: Vec<_> = hosts
            .iter()
            .map(|host| {
                let sem = semaphore.clone();
                let pool = self.pool.clone();
                let modules = self.modules.clone();
                let callbacks = self.callbacks.clone();
                let emitter = event_emitter.clone();
                let task = task.clone();
                let host = (*host).clone();
                let check_mode = self.config.check_mode;
                let diff_mode = self.config.diff_mode;
                let sudo = use_sudo;
                let sudo_user = sudo_user.clone();

                // Get or create context for this host (preserves registered vars across tasks)
                let ctx = self
                    .get_or_create_context(&host, playbook_vars)
                    .with_check_mode(check_mode)
                    .with_diff_mode(diff_mode)
                    .with_sudo(sudo, sudo_user.clone());

                async move {
                    let _permit = sem.acquire().await.unwrap();

                    // Emit task start event
                    if let Some(ref emitter) = emitter {
                        emitter.task_start(host.name.clone(), task.name.clone());
                    }

                    // Callback: task start
                    callbacks.on_task_start(&host.name, &task.name).await;

                    let start = Instant::now();
                    let result = execute_single_task(&task, &ctx, &pool, &modules, None).await;
                    let duration = start.elapsed();

                    let task_result = match result {
                        Ok(output) => {
                            let tr = TaskResult {
                                host: host.name.clone(),
                                task_name: task.name.clone(),
                                changed: output.changed,
                                failed: output.failed,
                                skipped: output.skipped,
                                stdout: Some(output.stdout.clone()),
                                stderr: Some(output.stderr.clone()),
                                message: output.message.clone(),
                                duration,
                                diff: output.diff.clone(),
                            };

                            // Emit task complete event
                            if let Some(ref emitter) = emitter {
                                if output.skipped {
                                    emitter.task_skipped(host.name.clone(), task.name.clone());
                                } else if output.failed {
                                    let error = output.message.as_deref().unwrap_or("task failed");
                                    emitter.task_failed(
                                        host.name.clone(),
                                        task.name.clone(),
                                        error.to_string(),
                                    );
                                } else {
                                    let status: TaskStatus = (&tr).into();
                                    emitter.task_complete(
                                        host.name.clone(),
                                        task.name.clone(),
                                        status,
                                        duration,
                                    );
                                }
                            }

                            // Callback: task complete
                            if output.skipped {
                                callbacks
                                    .on_task_skipped(&host.name, &task.name, "condition not met")
                                    .await;
                            } else if output.failed {
                                let error = output.message.as_deref().unwrap_or("task failed");
                                callbacks
                                    .on_task_failed(&host.name, &task.name, error)
                                    .await;
                            } else {
                                callbacks
                                    .on_task_complete(&host.name, &task.name, &output, duration)
                                    .await;
                            }

                            tr
                        }
                        Err(e) => {
                            let error_msg = e.to_string();

                            // Emit task failed event
                            if let Some(ref emitter) = emitter {
                                emitter.task_failed(
                                    host.name.clone(),
                                    task.name.clone(),
                                    error_msg.clone(),
                                );
                            }

                            callbacks
                                .on_task_failed(&host.name, &task.name, &error_msg)
                                .await;

                            TaskResult {
                                host: host.name.clone(),
                                task_name: task.name.clone(),
                                changed: false,
                                failed: true,
                                skipped: false,
                                stdout: None,
                                stderr: None,
                                message: Some(error_msg),
                                duration,
                                diff: None,
                            }
                        }
                    };

                    task_result
                }
            })
            .collect();

        // Execute all futures
        let results = join_all(futures).await;

        Ok(results)
    }

    /// Execute a DAG of tasks
    pub async fn execute_dag(
        &self,
        dag: &TaskDag,
        hosts: &[&Host],
        playbook_vars: &HashMap<String, Value>,
        playbook_sudo: bool,
        playbook_sudo_user: &Option<String>,
    ) -> Result<Vec<TaskResult>, NexusError> {
        let mut completed: HashSet<usize> = HashSet::new();
        let mut all_results = Vec::new();

        loop {
            let ready = dag.ready_tasks(&completed);

            if ready.is_empty() {
                break;
            }

            // Execute ready tasks in parallel across hosts
            for node in ready {
                let results = self
                    .execute_task_on_hosts(
                        &node.task,
                        hosts,
                        playbook_vars,
                        playbook_sudo,
                        playbook_sudo_user,
                    )
                    .await?;

                // Check for failures
                let has_failure = results.iter().any(|r| r.failed);

                all_results.extend(results);
                completed.insert(node.id);

                if has_failure {
                    // Stop execution on failure
                    return Ok(all_results);
                }
            }
        }

        Ok(all_results)
    }

    /// Execute playbook with serial batching
    async fn execute_playbook_serial(
        &self,
        playbook: &Playbook,
        _inventory: &Inventory,
        all_hosts: &[&Host],
        serial: &Serial,
    ) -> Result<PlayRecap, NexusError> {
        let mut recap = PlayRecap::new();
        let start_time = Instant::now();

        // Calculate batches based on serial configuration
        let batches = calculate_batches(all_hosts, serial);

        if self.config.verbose {
            self.output
                .lock()
                .print_task_header(&format!("SERIAL EXECUTION: {} batch(es)", batches.len()));
        }

        // Collect all handlers
        let all_handlers = playbook.handlers.clone();
        let handler_registry = Arc::new(HandlerRegistry::from_handlers(&playbook.handlers));

        let use_sudo = self.config.sudo || playbook.sudo;
        let tag_filter = self.config.tag_filter.clone().unwrap_or_default();
        let effective_vars = playbook.vars.clone();

        // Execute on each batch sequentially
        for (batch_num, batch) in batches.iter().enumerate() {
            if self.config.verbose {
                self.output.lock().print_task_header(&format!(
                    "BATCH {}/{}: {} host(s)",
                    batch_num + 1,
                    batches.len(),
                    batch.len()
                ));
            }

            // Execute all sections on this batch
            if !playbook.pre_tasks.is_empty() && batch_num == 0 {
                self.output.lock().print_task_header("PRE-TASKS");
            }

            if !playbook.pre_tasks.is_empty() {
                let failed = self
                    .execute_task_list(
                        &playbook.pre_tasks,
                        batch,
                        &effective_vars,
                        use_sudo,
                        &playbook.sudo_user,
                        &tag_filter,
                        &handler_registry,
                        &mut recap,
                    )
                    .await?;

                if failed {
                    recap.total_duration = start_time.elapsed();
                    self.output.lock().print_recap(&recap);
                    return Ok(recap);
                }
            }

            // Execute main tasks
            if !playbook.tasks.is_empty() {
                let failed = self
                    .execute_task_list(
                        &playbook.tasks,
                        batch,
                        &effective_vars,
                        use_sudo,
                        &playbook.sudo_user,
                        &tag_filter,
                        &handler_registry,
                        &mut recap,
                    )
                    .await?;

                if failed {
                    recap.total_duration = start_time.elapsed();
                    self.output.lock().print_recap(&recap);
                    return Ok(recap);
                }
            }

            // Execute post_tasks
            if !playbook.post_tasks.is_empty() && batch_num == 0 {
                self.output.lock().print_task_header("POST-TASKS");
            }

            if !playbook.post_tasks.is_empty() {
                let failed = self
                    .execute_task_list(
                        &playbook.post_tasks,
                        batch,
                        &effective_vars,
                        use_sudo,
                        &playbook.sudo_user,
                        &tag_filter,
                        &handler_registry,
                        &mut recap,
                    )
                    .await?;

                if failed {
                    recap.total_duration = start_time.elapsed();
                    self.output.lock().print_recap(&recap);
                    return Ok(recap);
                }
            }

            // Execute handlers for this batch
            if handler_registry.has_pending() {
                if self.config.verbose {
                    self.output.lock().print_task_header("RUNNING HANDLERS");
                }

                let handler_results = self
                    .execute_handlers(
                        &all_handlers,
                        batch,
                        &effective_vars,
                        use_sudo,
                        &playbook.sudo_user,
                        &handler_registry,
                    )
                    .await?;

                for result in handler_results {
                    recap.record(&result);
                    self.output.lock().print_task_result(&result);
                }
            }
        }

        recap.total_duration = start_time.elapsed();

        // Callback: playbook complete
        self.callbacks.on_playbook_complete(&recap).await;

        self.output.lock().print_recap(&recap);

        Ok(recap)
    }
}

/// Calculate host batches based on serial configuration
fn calculate_batches<'a>(hosts: &[&'a Host], serial: &Serial) -> Vec<Vec<&'a Host>> {
    let total_hosts = hosts.len();
    if total_hosts == 0 {
        return vec![];
    }

    match serial {
        Serial::Count(n) => {
            // Fixed batch size
            let batch_size = (*n).min(total_hosts);
            hosts
                .chunks(batch_size)
                .map(|chunk| chunk.to_vec())
                .collect()
        }
        Serial::Percentage(pct) => {
            // Percentage of hosts per batch
            let batch_size = ((total_hosts * (*pct as usize)) / 100).max(1);
            hosts
                .chunks(batch_size)
                .map(|chunk| chunk.to_vec())
                .collect()
        }
        Serial::List(sizes) => {
            // Progressive batches
            let mut batches = Vec::new();
            let mut remaining = hosts.to_vec();

            for size in sizes {
                if remaining.is_empty() {
                    break;
                }
                let batch_size = (*size).min(remaining.len());
                let batch = remaining.drain(..batch_size).collect();
                batches.push(batch);
            }

            // Add remaining hosts in final batch
            if !remaining.is_empty() {
                batches.push(remaining);
            }

            batches
        }
    }
}

/// Execute a single task on a single host
async fn execute_single_task(
    task: &Task,
    ctx: &ExecutionContext,
    pool: &ConnectionPool,
    modules: &ModuleExecutor,
    async_tracker: Option<&AsyncJobTracker>,
) -> Result<TaskOutput, NexusError> {
    execute_single_task_with_retry(task, ctx, pool, modules, None, async_tracker).await
}

/// Execute a single task with retry support
async fn execute_single_task_with_retry(
    task: &Task,
    ctx: &ExecutionContext,
    pool: &ConnectionPool,
    modules: &ModuleExecutor,
    circuit_breakers: Option<&CircuitBreakerRegistry>,
    async_tracker: Option<&AsyncJobTracker>,
) -> Result<TaskOutput, NexusError> {
    // Check when condition
    if let Some(ref when_expr) = task.when {
        let result = evaluate_expression(when_expr, ctx)?;
        if !result.is_truthy() {
            return Ok(TaskOutput::skipped());
        }
    }

    // Handle async execution
    if let Some(ref async_config) = task.async_config {
        return execute_async_task(task, ctx, pool, modules, async_config, async_tracker).await;
    }

    // Handle loop
    if let Some(ref loop_expr) = task.loop_expr {
        let loop_value = evaluate_expression(loop_expr, ctx)?;

        let items = match loop_value {
            Value::List(items) => items,
            _ => {
                return Err(NexusError::Runtime {
                    function: None,
                    message: "loop expression must evaluate to a list".to_string(),
                    suggestion: None,
                })
            }
        };

        let mut combined_output = TaskOutput::new();

        for (i, item) in items.into_iter().enumerate() {
            let loop_ctx = ctx.clone_for_task().with_loop_item(item, i);

            let output = execute_task_body_with_retry(
                task,
                &loop_ctx,
                pool,
                modules,
                circuit_breakers,
                async_tracker,
            )
            .await?;

            combined_output.changed = combined_output.changed || output.changed;
            combined_output.failed = combined_output.failed || output.failed;

            if !output.stdout.is_empty() {
                combined_output.stdout.push_str(&output.stdout);
                combined_output.stdout.push('\n');
            }

            if output.failed {
                break;
            }
        }

        return Ok(combined_output);
    }

    execute_task_body_with_retry(task, ctx, pool, modules, circuit_breakers, async_tracker).await
}

/// Execute the body of a task (module call)
async fn execute_task_body(
    task: &Task,
    ctx: &ExecutionContext,
    pool: &ConnectionPool,
    modules: &ModuleExecutor,
) -> Result<TaskOutput, NexusError> {
    use crate::executor::LocalConnection;
    use crate::modules::AnyConnection;

    // Get appropriate connection type (SSH or local)
    let conn = match pool.get_connection_type(&ctx.host) {
        crate::executor::ssh::ConnectionType::Local => {
            AnyConnection::Local(LocalConnection::new(&ctx.host.name))
        }
        crate::executor::ssh::ConnectionType::Ssh => AnyConnection::Ssh(pool.get(&ctx.host)?),
    };

    // Execute the module
    let mut output = modules.execute(&task.module, ctx, &conn).await?;

    // Register output if requested (before changed_when/fail_when evaluation)
    if let Some(ref var_name) = task.register {
        ctx.register(var_name, output.clone());
    }

    // Check fail_when condition
    if let Some(ref fail_when) = task.fail_when {
        let result = evaluate_expression(fail_when, ctx)?;
        if result.is_truthy() {
            return Ok(TaskOutput::failed(format!(
                "fail_when condition triggered: {:?}",
                fail_when
            )));
        }
    }

    // CRITICAL BUG FIX: Evaluate changed_when condition
    // This was completely missing, causing changed status to be incorrectly reported
    if let Some(ref changed_when) = task.changed_when {
        // Register output temporarily if not already registered for evaluation
        if task.register.is_none() {
            ctx.register("result", output.clone());
        }

        let result = evaluate_expression(changed_when, ctx)?;
        // Override the changed status based on the condition
        output.changed = result.is_truthy();
    }

    Ok(output)
}

/// Execute task body with retry and circuit breaker support
async fn execute_task_body_with_retry(
    task: &Task,
    ctx: &ExecutionContext,
    pool: &ConnectionPool,
    modules: &ModuleExecutor,
    circuit_breakers: Option<&CircuitBreakerRegistry>,
    _async_tracker: Option<&AsyncJobTracker>,
) -> Result<TaskOutput, NexusError> {
    // If no retry config, execute directly
    let retry_config = match &task.retry {
        Some(config) => config,
        None => return execute_task_body(task, ctx, pool, modules).await,
    };

    // Check circuit breaker if configured
    if let (Some(cb_config), Some(registry)) = (&retry_config.circuit_breaker, circuit_breakers) {
        let circuit = registry.get_or_create(cb_config);
        let mut circuit_guard = circuit.write();

        if !circuit_guard.should_allow() {
            let time_until_retry = circuit_guard.time_until_retry();
            return Ok(TaskOutput::failed(format!(
                "Circuit breaker '{}' is open. {}",
                cb_config.name,
                time_until_retry
                    .map(|d| format!("Retry in {}s", d.as_secs()))
                    .unwrap_or_default()
            )));
        }
    }

    let mut last_error = String::new();
    let mut attempt = 0;

    while attempt < retry_config.attempts {
        // Execute the task
        let result = execute_task_body(task, ctx, pool, modules).await;

        match result {
            Ok(output) => {
                // Check if we should retry based on retry_when condition
                let should_retry = if let Some(ref retry_when) = retry_config.retry_when {
                    // Register output temporarily for condition evaluation
                    if let Some(ref var_name) = task.register {
                        ctx.register(var_name, output.clone());
                    } else {
                        ctx.register("result", output.clone());
                    }
                    evaluate_expression(retry_when, ctx)
                        .map(|v| v.is_truthy())
                        .unwrap_or(false)
                } else {
                    // Default: retry on failure
                    output.failed
                };

                // Check until condition (success condition)
                let is_success = if let Some(ref until) = retry_config.until {
                    if let Some(ref var_name) = task.register {
                        ctx.register(var_name, output.clone());
                    } else {
                        ctx.register("result", output.clone());
                    }
                    evaluate_expression(until, ctx)
                        .map(|v| v.is_truthy())
                        .unwrap_or(false)
                } else {
                    !output.failed
                };

                if is_success {
                    // Success - record to circuit breaker
                    if let (Some(cb_config), Some(registry)) =
                        (&retry_config.circuit_breaker, circuit_breakers)
                    {
                        let circuit = registry.get_or_create(cb_config);
                        circuit.write().record_success();
                    }
                    return Ok(output);
                }

                if !should_retry || attempt >= retry_config.attempts - 1 {
                    // No more retries - record failure to circuit breaker
                    if let (Some(cb_config), Some(registry)) =
                        (&retry_config.circuit_breaker, circuit_breakers)
                    {
                        let circuit = registry.get_or_create(cb_config);
                        circuit.write().record_failure();
                    }
                    return Ok(output);
                }

                last_error = output.message.unwrap_or_else(|| "Task failed".to_string());
            }
            Err(e) => {
                last_error = e.to_string();

                // Record failure to circuit breaker
                if let (Some(cb_config), Some(registry)) =
                    (&retry_config.circuit_breaker, circuit_breakers)
                {
                    let circuit = registry.get_or_create(cb_config);
                    circuit.write().record_failure();
                }

                if attempt >= retry_config.attempts - 1 {
                    return Err(e);
                }
            }
        }

        attempt += 1;

        // Wait before retrying
        let delay = calculate_delay(&retry_config.delay, attempt);
        tokio::time::sleep(delay).await;
    }

    Ok(TaskOutput::failed(format!(
        "Task failed after {} attempts. Last error: {}",
        retry_config.attempts, last_error
    )))
}

/// Execute an async task in the background
async fn execute_async_task(
    task: &Task,
    ctx: &ExecutionContext,
    pool: &ConnectionPool,
    _modules: &ModuleExecutor,
    async_config: &crate::parser::ast::AsyncConfig,
    async_tracker: Option<&AsyncJobTracker>,
) -> Result<TaskOutput, NexusError> {
    // Get the command to execute
    let command = match &task.module {
        crate::parser::ast::ModuleCall::Command { cmd, .. } => {
            // Evaluate the command expression
            let cmd_value = evaluate_expression(cmd, ctx)?;
            match cmd_value {
                Value::String(s) => s,
                _ => {
                    return Err(NexusError::Runtime {
                        function: None,
                        message: "command must be a string".to_string(),
                        suggestion: None,
                    });
                }
            }
        }
        _ => {
            return Err(NexusError::Runtime {
                function: None,
                message: "async execution is only supported for command module".to_string(),
                suggestion: Some("Use 'command:' module for async tasks".to_string()),
            });
        }
    };

    // Get SSH connection (async tasks don't support local connections yet)
    if ctx.host.is_local() {
        return Err(NexusError::Runtime {
            function: None,
            message: "async execution is not supported for local connections".to_string(),
            suggestion: Some("Remove 'async:' parameter for localhost tasks".to_string()),
        });
    }
    let conn = pool.get(&ctx.host)?;

    // Wrap command with sudo if needed
    let final_command = ctx.wrap_command(&command);

    // Check mode - don't actually run
    if ctx.check_mode {
        return Ok(TaskOutput::changed().with_stdout(format!("Would run async: {}", final_command)));
    }

    // Start the async job
    let tracker = async_tracker.ok_or_else(|| NexusError::Runtime {
        function: None,
        message: "async tracker not available".to_string(),
        suggestion: None,
    })?;

    let job_id = tracker
        .start_job(&conn, &final_command, async_config.async_timeout)
        .await?;

    // Fire and forget mode (poll == 0)
    if async_config.poll == 0 {
        let mut output = TaskOutput::changed();
        output.stdout = format!("Async job started (fire and forget): {}", job_id);
        output
            .data
            .insert("job_id".to_string(), Value::String(job_id));
        output.data.insert("started".to_string(), Value::Bool(true));
        output
            .data
            .insert("finished".to_string(), Value::Bool(false));

        // Register output if requested
        if let Some(ref var_name) = task.register {
            ctx.register(var_name, output.clone());
        }

        return Ok(output);
    }

    // Poll for completion
    let result = tracker
        .poll_until_complete(&conn, &job_id, async_config.poll, async_config.retries)
        .await?;

    // Add job_id to output data
    let mut final_output = result;
    final_output
        .data
        .insert("job_id".to_string(), Value::String(job_id));
    final_output
        .data
        .insert("finished".to_string(), Value::Bool(true));

    // Register output if requested
    if let Some(ref var_name) = task.register {
        ctx.register(var_name, final_output.clone());
    }

    Ok(final_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_config_default() {
        let config = SchedulerConfig::default();
        assert_eq!(config.max_parallel_hosts, 10);
        assert!(!config.check_mode);
    }
}
