// YAML playbook parser

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use super::ast::*;
use super::expressions::{has_interpolation, parse_expression, parse_interpolated_string};
use super::functions::parse_functions_block;
use crate::output::errors::{NexusError, ParseError, ParseErrorKind};

/// Raw YAML playbook structure (before AST conversion)
#[derive(Debug, Deserialize)]
struct RawPlaybook {
    hosts: Option<String>,
    vars: Option<HashMap<String, YamlValue>>,
    tasks: Option<Vec<RawTask>>,
    handlers: Option<Vec<RawHandler>>,
    functions: Option<String>,
    /// Enable sudo for all tasks by default
    sudo: Option<bool>,
    /// Default user to run as with sudo
    sudo_user: Option<String>,
    /// Roles to include
    roles: Option<Vec<RawRoleRef>>,
    /// Pre-tasks run before roles
    pre_tasks: Option<Vec<RawTask>>,
    /// Post-tasks run after roles
    post_tasks: Option<Vec<RawTask>>,
    /// Auto-gather facts at play start
    gather_facts: Option<bool>,
    /// Connection type (local, ssh, etc.)
    connection: Option<String>,
    /// Serial execution configuration
    serial: Option<RawSerial>,
    /// Max concurrent tasks
    throttle: Option<usize>,
    /// Execution strategy
    strategy: Option<String>,
}

/// Serial value can be a number, string (percentage), or list
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawSerial {
    /// Fixed count (e.g., serial: 2)
    Count(usize),
    /// Percentage string (e.g., serial: "25%")
    Percentage(String),
    /// Progressive batches (e.g., serial: [1, 5, 10])
    List(Vec<usize>),
}

/// Role reference - can be a simple string or object with vars
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRoleRef {
    /// Simple role name
    Name(String),
    /// Role with parameters
    Full {
        role: String,
        #[serde(default)]
        vars: HashMap<String, YamlValue>,
        #[serde(default)]
        tags: Vec<String>,
        when: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct RawTask {
    name: Option<String>,
    #[serde(rename = "when")]
    when_condition: Option<String>,
    register: Option<String>,
    fail_when: Option<String>,
    changed_when: Option<String>,
    notify: Option<NotifyValue>,
    #[serde(rename = "loop")]
    loop_expr: Option<String>,
    loop_var: Option<String>,
    /// Override sudo for this task
    sudo: Option<bool>,
    /// Run as specific user (e.g., "postgres", "root")
    #[serde(rename = "as")]
    run_as: Option<String>,
    /// Tags for filtering (string or list)
    tags: Option<TagsValue>,
    /// Retry configuration
    retry: Option<RawRetryConfig>,
    /// Simple task-level retry fields (alternative to full retry config)
    until: Option<String>,
    retries: Option<u32>,
    delay: Option<u64>,
    /// Async timeout (in seconds)
    #[serde(rename = "async")]
    async_timeout: Option<u64>,
    /// Poll interval (in seconds, 0 = no polling)
    poll: Option<u64>,
    /// Task timeout in seconds
    timeout: Option<u64>,
    /// Throttle - max concurrent executions of this task
    throttle: Option<usize>,
    /// Host to delegate execution to
    delegate_to: Option<String>,
    /// Store facts from delegate (default: false)
    delegate_facts: Option<bool>,
    /// Block tasks (main execution) - if present, this is a block
    block: Option<Vec<RawTask>>,
    /// Rescue tasks (error handling)
    rescue: Option<Vec<RawTask>>,
    /// Always tasks (cleanup)
    always: Option<Vec<RawTask>>,
    /// Static import - resolved at parse time
    import_tasks: Option<String>,
    /// Dynamic include - resolved at runtime
    include_tasks: Option<String>,
    /// Variables for import/include
    vars: Option<HashMap<String, YamlValue>>,
    #[serde(flatten)]
    module: HashMap<String, YamlValue>,
}

/// Tags can be a single string or a list
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TagsValue {
    Single(String),
    Multiple(Vec<String>),
}

/// Raw retry configuration from YAML
#[derive(Debug, Deserialize)]
struct RawRetryConfig {
    /// Number of attempts
    attempts: Option<u32>,
    /// Delay in seconds (simple) or strategy object
    delay: Option<RawDelayValue>,
    /// Retry condition
    retry_when: Option<String>,
    /// Success condition to stop retrying
    until: Option<String>,
    /// Circuit breaker name or config
    circuit_breaker: Option<RawCircuitBreakerValue>,
}

/// Delay can be a simple number (seconds) or a strategy object
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDelayValue {
    /// Simple fixed delay in seconds
    Seconds(u64),
    /// Strategy object
    Strategy(RawDelayStrategy),
}

/// Delay strategy configuration
#[derive(Debug, Deserialize)]
struct RawDelayStrategy {
    /// Strategy type: "fixed", "exponential", "linear"
    strategy: String,
    /// Base delay in seconds
    base: Option<u64>,
    /// Maximum delay in seconds
    max: Option<u64>,
    /// Increment for linear strategy
    increment: Option<u64>,
    /// Add jitter for exponential
    jitter: Option<bool>,
}

/// Circuit breaker can be a name string or full config
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawCircuitBreakerValue {
    /// Just a circuit name
    Name(String),
    /// Full configuration
    Config(RawCircuitBreakerConfig),
}

#[derive(Debug, Deserialize)]
struct RawCircuitBreakerConfig {
    name: String,
    failure_threshold: Option<u32>,
    reset_timeout: Option<u64>,
    success_threshold: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NotifyValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct RawHandler {
    name: Option<String>,
    #[serde(flatten)]
    module: HashMap<String, YamlValue>,
}

/// Parse a playbook from a file
pub fn parse_playbook_file(path: &Path) -> Result<Playbook, NexusError> {
    parse_playbook_file_with_vault(path, None)
}

/// Parse a playbook from a file with optional vault password
pub fn parse_playbook_file_with_vault(
    path: &Path,
    vault_password: Option<&str>,
) -> Result<Playbook, NexusError> {
    let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
        message: format!("Failed to read playbook file: {}", e),
        path: Some(path.to_path_buf()),
    })?;

    // Check if the file is vault-encrypted
    let content = if crate::vault::is_vault_string(&content) {
        let password = vault_password.ok_or_else(|| NexusError::Runtime {
            function: None,
            message: format!("Playbook file {} is encrypted but no vault password provided", path.display()),
            suggestion: Some("Use --vault-password, --vault-password-file, or --ask-vault-pass".to_string()),
        })?;

        crate::vault::format::VaultFile::parse(&content)
            .and_then(|vault| vault.decrypt(password))
            .map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to decrypt playbook: {}", e),
                suggestion: Some("Check that the vault password is correct".to_string()),
            })?
    } else {
        content
    };

    parse_playbook(&content, path.to_string_lossy().to_string())
}

/// Parse a playbook from a string
pub fn parse_playbook(content: &str, source_file: String) -> Result<Playbook, NexusError> {
    let raw: RawPlaybook = serde_yaml::from_str(content).map_err(|e| {
        let (line, column) = extract_yaml_error_location(&e);
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid YAML: {}", e),
            file: Some(source_file.clone()),
            line,
            column,
            suggestion: Some("Check YAML syntax - ensure proper indentation and valid YAML".to_string()),
        }))
    })?;

    convert_playbook(raw, source_file)
}

pub(crate) fn extract_yaml_error_location(e: &serde_yaml::Error) -> (Option<usize>, Option<usize>) {
    if let Some(loc) = e.location() {
        (Some(loc.line()), Some(loc.column()))
    } else {
        (None, None)
    }
}

fn convert_playbook(raw: RawPlaybook, source_file: String) -> Result<Playbook, NexusError> {
    let hosts = match raw.hosts {
        Some(h) if h == "all" => HostPattern::All,
        Some(h) => {
            if h.contains(':') || h.contains('&') || h.contains('!') {
                HostPattern::Pattern(h)
            } else {
                HostPattern::Group(h)
            }
        }
        None => HostPattern::All,
    };

    let vars = raw
        .vars
        .map(convert_vars)
        .transpose()?
        .unwrap_or_default();

    let tasks = raw
        .tasks
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task_or_block(t, &source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    let handlers = raw
        .handlers
        .map(|handlers| {
            handlers
                .into_iter()
                .enumerate()
                .map(|(i, h)| convert_handler(h, &source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    let functions = raw
        .functions
        .map(|f| parse_functions_block(&f, &source_file))
        .transpose()?;

    // Parse roles
    let roles = raw
        .roles
        .map(|roles| {
            roles
                .into_iter()
                .map(|r| convert_role_ref(r, &source_file))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Parse pre_tasks
    let pre_tasks = raw
        .pre_tasks
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task_or_block(t, &source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Parse post_tasks
    let post_tasks = raw
        .post_tasks
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task_or_block(t, &source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Parse serial configuration
    let serial = raw.serial.map(convert_serial).transpose()?;

    // Parse strategy
    let strategy = raw.strategy
        .map(|s| match s.to_lowercase().as_str() {
            "free" => ExecutionStrategy::Free,
            _ => ExecutionStrategy::Linear,
        })
        .unwrap_or_default();

    Ok(Playbook {
        source_file,
        hosts,
        vars,
        tasks,
        handlers,
        functions,
        sudo: raw.sudo.unwrap_or(false),
        sudo_user: raw.sudo_user,
        roles,
        pre_tasks,
        post_tasks,
        gather_facts: raw.gather_facts.unwrap_or(false),
        connection: raw.connection,
        serial,
        throttle: raw.throttle,
        strategy,
    })
}

fn convert_serial(raw: RawSerial) -> Result<Serial, NexusError> {
    match raw {
        RawSerial::Count(n) => Ok(Serial::Count(n)),
        RawSerial::Percentage(s) => {
            // Parse percentage string like "25%"
            if let Some(stripped) = s.strip_suffix('%') {
                let percentage = stripped.parse::<u8>().map_err(|_| NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: format!("Invalid percentage value: {}", s),
                    file: None,
                    line: None,
                    column: None,
                    suggestion: Some("Use a number between 0-100 followed by % (e.g., '25%')".to_string()),
                })))?;
                if percentage > 100 {
                    return Err(NexusError::Parse(Box::new(ParseError {
                        kind: ParseErrorKind::InvalidValue,
                        message: format!("Percentage must be between 0-100, got {}", percentage),
                        file: None,
                        line: None,
                        column: None,
                        suggestion: None,
                    })));
                }
                Ok(Serial::Percentage(percentage))
            } else {
                Err(NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: format!("Expected percentage string with % suffix, got: {}", s),
                    file: None,
                    line: None,
                    column: None,
                    suggestion: Some("Use format like '25%'".to_string()),
                })))
            }
        }
        RawSerial::List(list) => Ok(Serial::List(list)),
    }
}

pub(crate) fn convert_vars(vars: HashMap<String, YamlValue>) -> Result<HashMap<String, Value>, NexusError> {
    vars.into_iter()
        .map(|(k, v)| Ok((k, yaml_to_value(v)?)))
        .collect()
}

fn yaml_to_value(yaml: YamlValue) -> Result<Value, NexusError> {
    match yaml {
        YamlValue::Null => Ok(Value::Null),
        YamlValue::Bool(b) => Ok(Value::Bool(b)),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                Ok(Value::Int(0))
            }
        }
        YamlValue::String(s) => Ok(Value::String(s)),
        YamlValue::Sequence(seq) => {
            let items: Result<Vec<_>, _> = seq.into_iter().map(yaml_to_value).collect();
            Ok(Value::List(items?))
        }
        YamlValue::Mapping(map) => {
            let items: Result<HashMap<_, _>, _> = map
                .into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        YamlValue::String(s) => s,
                        other => other.as_str().unwrap_or("").to_string(),
                    };
                    Ok((key, yaml_to_value(v)?))
                })
                .collect();
            Ok(Value::Dict(items?))
        }
        YamlValue::Tagged(tagged) => yaml_to_value(tagged.value),
    }
}

/// Convert RawTask to either Task or Block
fn convert_task_or_block(raw: RawTask, source_file: &str, index: usize) -> Result<TaskOrBlock, NexusError> {
    // Check if this is an import_tasks (static import)
    if let Some(ref import_file) = raw.import_tasks {
        return convert_import_tasks(import_file.clone(), raw, source_file);
    }

    // Check if this is an include_tasks (dynamic include)
    if let Some(ref include_file) = raw.include_tasks {
        return convert_include_tasks(include_file.clone(), raw, source_file);
    }

    // Check if this is a block (has block: field)
    if raw.block.is_some() {
        return convert_block(raw, source_file, index);
    }

    // Otherwise, it's a regular task
    let task = convert_task(raw, source_file, index)?;
    Ok(TaskOrBlock::Task(Box::new(task)))
}

/// Convert RawTask to Block
fn convert_block(raw: RawTask, source_file: &str, _index: usize) -> Result<TaskOrBlock, NexusError> {
    let name = raw.name.clone();

    // Parse when condition
    let when = raw.when_condition
        .map(|w| parse_condition(&w))
        .transpose()?;

    // Parse tags
    let tags = match raw.tags {
        Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(TagsValue::Multiple(v)) => v,
        None => vec![],
    };

    // Convert block tasks
    let block = raw.block
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task(t, source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Convert rescue tasks
    let rescue = raw.rescue
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task(t, source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Convert always tasks
    let always = raw.always
        .map(|tasks| {
            tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| convert_task(t, source_file, i))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(TaskOrBlock::Block(Block {
        name,
        block,
        rescue,
        always,
        when,
        tags,
        location: None,
    }))
}

fn convert_task(raw: RawTask, source_file: &str, index: usize) -> Result<Task, NexusError> {
    let name = raw.name.clone().unwrap_or_else(|| format!("Task {}", index + 1));

    let when = raw
        .when_condition
        .map(|w| parse_condition(&w))
        .transpose()?;

    let fail_when = raw.fail_when.map(|w| parse_condition(&w)).transpose()?;

    let changed_when = raw
        .changed_when
        .map(|w| parse_condition(&w))
        .transpose()?;

    let notify = match raw.notify {
        Some(NotifyValue::Single(s)) => vec![s],
        Some(NotifyValue::Multiple(v)) => v,
        None => vec![],
    };

    let loop_expr = raw.loop_expr.map(|l| parse_condition(&l)).transpose()?;

    let loop_var = raw.loop_var.unwrap_or_else(|| "item".to_string());

    let module = parse_module_call(&raw.module, source_file)?;

    // Parse tags
    let tags = match raw.tags {
        Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(TagsValue::Multiple(v)) => v,
        None => vec![],
    };

    // Parse retry configuration
    // Support both full retry config and simple task-level fields (until, retries, delay)
    let retry = if let Some(retry_config) = raw.retry {
        Some(convert_retry_config(retry_config, &name)?)
    } else if raw.until.is_some() || raw.retries.is_some() || raw.delay.is_some() {
        // Build RetryConfig from simple task-level fields
        let until_expr = raw.until.map(|u| parse_condition(&u)).transpose()?;
        let attempts = raw.retries.unwrap_or(3);
        let delay_secs = raw.delay.unwrap_or(5);

        Some(RetryConfig {
            attempts,
            delay: DelayStrategy::Fixed(Duration::from_secs(delay_secs)),
            retry_when: None,
            until: until_expr,
            circuit_breaker: None,
        })
    } else {
        None
    };

    // Parse async configuration
    let async_config = if raw.async_timeout.is_some() || raw.poll.is_some() {
        Some(convert_async_config(raw.async_timeout, raw.poll))
    } else {
        None
    };

    // Parse timeout
    let timeout = raw.timeout.map(Duration::from_secs);

    // Parse delegate_to
    let delegate_to = raw.delegate_to
        .map(|d| parse_condition(&d))
        .transpose()?;

    Ok(Task {
        name,
        module,
        when,
        register: raw.register,
        fail_when,
        changed_when,
        notify,
        loop_expr,
        loop_var,
        location: None, // TODO: track source locations
        sudo: raw.sudo,
        run_as: raw.run_as,
        tags,
        retry,
        async_config,
        timeout,
        throttle: raw.throttle,
        delegate_to,
        delegate_facts: raw.delegate_facts.unwrap_or(false),
    })
}

/// Convert raw retry config to AST
fn convert_retry_config(raw: RawRetryConfig, _task_name: &str) -> Result<RetryConfig, NexusError> {
    let delay = match raw.delay {
        Some(RawDelayValue::Seconds(s)) => DelayStrategy::Fixed(Duration::from_secs(s)),
        Some(RawDelayValue::Strategy(strat)) => {
            let base = Duration::from_secs(strat.base.unwrap_or(5));
            let max = Duration::from_secs(strat.max.unwrap_or(300));

            match strat.strategy.as_str() {
                "exponential" => DelayStrategy::Exponential {
                    base,
                    max,
                    jitter: strat.jitter.unwrap_or(true),
                },
                "linear" => DelayStrategy::Linear {
                    base,
                    increment: Duration::from_secs(strat.increment.unwrap_or(5)),
                    max,
                },
                _ => DelayStrategy::Fixed(base),
            }
        }
        None => DelayStrategy::Fixed(Duration::from_secs(5)),
    };

    let retry_when = raw.retry_when.map(|w| parse_condition(&w)).transpose()?;
    let until = raw.until.map(|w| parse_condition(&w)).transpose()?;

    let circuit_breaker = match raw.circuit_breaker {
        Some(RawCircuitBreakerValue::Name(name)) => Some(CircuitBreakerConfig {
            name,
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(60),
            success_threshold: 2,
        }),
        Some(RawCircuitBreakerValue::Config(cfg)) => Some(CircuitBreakerConfig {
            name: cfg.name,
            failure_threshold: cfg.failure_threshold.unwrap_or(5),
            reset_timeout: Duration::from_secs(cfg.reset_timeout.unwrap_or(60)),
            success_threshold: cfg.success_threshold.unwrap_or(2),
        }),
        None => None,
    };

    Ok(RetryConfig {
        attempts: raw.attempts.unwrap_or(3),
        delay,
        retry_when,
        until,
        circuit_breaker,
    })
}

/// Convert raw async config to AST
fn convert_async_config(async_timeout: Option<u64>, poll: Option<u64>) -> AsyncConfig {
    let timeout = async_timeout.unwrap_or(300);
    let poll_interval = poll.unwrap_or(10);

    // Calculate retries based on timeout and poll interval
    let retries = if poll_interval > 0 {
        ((timeout / poll_interval) + 5).max(10) as u32
    } else {
        0
    };

    AsyncConfig {
        async_timeout: timeout,
        poll: poll_interval,
        retries,
    }
}

fn convert_handler(raw: RawHandler, source_file: &str, index: usize) -> Result<Handler, NexusError> {
    let name = raw
        .name
        .ok_or_else(|| NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::MissingField,
            message: format!("Handler {} is missing a 'name' field", index + 1),
            file: Some(source_file.to_string()),
            line: None,
            column: None,
            suggestion: Some("Add a 'name:' field to identify this handler".to_string()),
        })))?;

    let module = parse_module_call(&raw.module, source_file)?;

    Ok(Handler {
        name,
        module,
        location: None,
    })
}

fn convert_role_ref(raw: RawRoleRef, _source_file: &str) -> Result<RoleRef, NexusError> {
    match raw {
        RawRoleRef::Name(name) => Ok(RoleRef {
            role: name,
            vars: HashMap::new(),
            tags: Vec::new(),
            when: None,
        }),
        RawRoleRef::Full { role, vars, tags, when } => {
            let converted_vars = vars
                .into_iter()
                .map(|(k, v)| Ok((k, yaml_to_value(v)?)))
                .collect::<Result<HashMap<_, _>, NexusError>>()?;

            let when_expr = when.map(|w| parse_condition(&w)).transpose()?;

            Ok(RoleRef {
                role,
                vars: converted_vars,
                tags,
                when: when_expr,
            })
        }
    }
}

pub(crate) fn parse_condition(cond: &str) -> Result<Expression, NexusError> {
    // Strip ${} if present
    let expr_str = if cond.starts_with("${") && cond.ends_with('}') {
        &cond[2..cond.len() - 1]
    } else {
        cond
    };

    parse_expression(expr_str)
}

fn parse_module_call(
    module: &HashMap<String, YamlValue>,
    source_file: &str,
) -> Result<ModuleCall, NexusError> {
    // Known module keywords that should be ignored
    let skip_keys = [
        "name",
        "when",
        "register",
        "fail_when",
        "changed_when",
        "notify",
        "loop",
        "loop_var",
        "sudo",
        "as",
        "tags",
        "retry",
        "until",
        "retries",
        "delay",
        "async",
        "poll",
        "timeout",
        "throttle",
        "delegate_to",
        "delegate_facts",
    ];

    // Find the module type
    let module_keys: Vec<_> = module
        .keys()
        .filter(|k| !skip_keys.contains(&k.as_str()))
        .collect();

    if module_keys.is_empty() {
        return Err(NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::UnknownModule,
            message: "No module specified in task".to_string(),
            file: Some(source_file.to_string()),
            line: None,
            column: None,
            suggestion: Some(
                "Add a module like 'package:', 'service:', 'file:', 'command:', or 'run:'"
                    .to_string(),
            ),
        })));
    }

    // Check for 'run:' (function call)
    if let Some(run_value) = module.get("run") {
        return parse_run_module(run_value, source_file);
    }

    // Check for known modules
    if let Some(pkg_value) = module.get("package") {
        return parse_package_module(pkg_value, module, source_file);
    }

    if let Some(svc_value) = module.get("service") {
        return parse_service_module(svc_value, module, source_file);
    }

    if let Some(file_value) = module.get("file") {
        return parse_file_module(file_value, module, source_file);
    }

    if let Some(cmd_value) = module.get("command") {
        return parse_command_module(cmd_value, module, source_file);
    }

    if let Some(user_value) = module.get("user") {
        return parse_user_module(user_value, module, source_file);
    }

    if let Some(template_value) = module.get("template") {
        return parse_template_module(template_value, module, source_file);
    }

    if let Some(facts_value) = module.get("facts") {
        return parse_facts_module(facts_value, module, source_file);
    }

    // Unknown module - provide helpful error
    let unknown_key = module_keys[0];
    let _suggestion = suggest_module(unknown_key);

    Err(NexusError::Parse(Box::new(ParseError {
        kind: ParseErrorKind::UnknownModule,
        message: format!("Unknown or unsupported module. Available keys: {:?}", module.keys().collect::<Vec<_>>()),
        file: None,
        line: None,
        column: None,
        suggestion: Some("Check module name and arguments".to_string()),
    })))
}

fn suggest_module(name: &str) -> String {
    let modules = ["package", "service", "file", "command", "user", "template", "facts", "run"];

    // Simple edit distance for suggestions
    for m in &modules {
        if name.to_lowercase().contains(&m[..m.len().min(4)]) {
            return format!("Did you mean '{}'?", m);
        }
    }

    // Check for plurals
    if let Some(singular) = name.strip_suffix('s') {
        if modules.contains(&singular) {
            return format!(
                "Did you mean '{}'? Module names should be singular.",
                singular
            );
        }
    }

    format!(
        "Available modules: {}",
        modules.join(", ")
    )
}

fn parse_run_module(value: &YamlValue, _source_file: &str) -> Result<ModuleCall, NexusError> {
    let call_str = value
        .as_str()
        .ok_or_else(|| NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidValue,
            message: "command must be a string".to_string(),
            file: None,
            line: None,
            column: None,
            suggestion: None,
        })))?;

    // Parse function call: function_name() or function_name(arg1, arg2)
    let (name, args) = if let Some(paren_pos) = call_str.find('(') {
        let name = call_str[..paren_pos].trim().to_string();
        let args_str = &call_str[paren_pos + 1..call_str.len() - 1];

        let args = if args_str.trim().is_empty() {
            vec![]
        } else {
            args_str
                .split(',')
                .map(|s| parse_expression(s.trim()))
                .collect::<Result<Vec<_>, _>>()?
        };

        (name, args)
    } else {
        (call_str.to_string(), vec![])
    };

    Ok(ModuleCall::RunFunction { name, args })
}

fn parse_package_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    let name = yaml_to_expression(value)?;

    let state = module
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "installed" | "present" => PackageState::Installed,
            "latest" => PackageState::Latest,
            "absent" | "removed" => PackageState::Absent,
            _ => PackageState::Installed,
        })
        .unwrap_or(PackageState::Installed);

    Ok(ModuleCall::Package { name, state })
}

fn parse_service_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    let name = yaml_to_expression(value)?;

    let state = module
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "running" | "started" => ServiceState::Running,
            "stopped" => ServiceState::Stopped,
            "restarted" => ServiceState::Restarted,
            "reloaded" => ServiceState::Reloaded,
            _ => ServiceState::Running,
        })
        .unwrap_or(ServiceState::Running);

    let enabled = module.get("enabled").and_then(|v| v.as_bool());

    Ok(ModuleCall::Service {
        name,
        state,
        enabled,
    })
}

fn parse_file_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    // Helper function to get from either Mapping or HashMap
    let get_param = |key: &str| -> Option<&YamlValue> {
        if let YamlValue::Mapping(map) = value {
            map.get(YamlValue::String(key.to_string()))
        } else {
            None
        }.or_else(|| module.get(key))
    };

    // Extract path - either from value mapping or value itself
    let path = if let YamlValue::Mapping(map) = value {
        // value is the params mapping, get path from it
        let val = map.get("path")
            .ok_or_else(|| NexusError::Parse(Box::new(ParseError {
                kind: ParseErrorKind::MissingField,
                message: "file module requires 'path' field".to_string(),
                file: None,
                line: None,
                column: None,
                suggestion: Some("Add path: /path/to/file".to_string()),
            })))?;
        yaml_to_expression(val)?
    } else {
        // value itself is the path
        yaml_to_expression(value)?
    };

    let state = get_param("state")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "file" => FileState::File,
            "directory" => FileState::Directory,
            "link" => FileState::Link,
            "absent" => FileState::Absent,
            "touch" => FileState::Touch,
            _ => FileState::File,
        })
        .unwrap_or(FileState::File);

    let source = get_param("source")
        .or_else(|| get_param("src"))
        .map(yaml_to_expression)
        .transpose()?;

    let content = get_param("content")
        .map(yaml_to_expression)
        .transpose()?;

    let owner = get_param("owner").map(yaml_to_expression).transpose()?;

    let group = get_param("group").map(yaml_to_expression).transpose()?;

    let mode = get_param("mode").map(yaml_to_expression).transpose()?;

    Ok(ModuleCall::File {
        path,
        state,
        source,
        content,
        owner,
        group,
        mode,
    })
}

fn parse_command_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    let cmd = yaml_to_expression(value)?;

    let creates = module
        .get("creates")
        .map(yaml_to_expression)
        .transpose()?;

    let removes = module
        .get("removes")
        .map(yaml_to_expression)
        .transpose()?;

    Ok(ModuleCall::Command {
        cmd,
        creates,
        removes,
    })
}

fn parse_user_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    let name = yaml_to_expression(value)?;

    let state = module
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "present" => UserState::Present,
            "absent" => UserState::Absent,
            _ => UserState::Present,
        })
        .unwrap_or(UserState::Present);

    let uid = module.get("uid").map(yaml_to_expression).transpose()?;
    let gid = module.get("gid").map(yaml_to_expression).transpose()?;
    let shell = module.get("shell").map(yaml_to_expression).transpose()?;
    let home = module.get("home").map(yaml_to_expression).transpose()?;
    let create_home = module.get("create_home").and_then(|v| v.as_bool());

    let groups = module
        .get("groups")
        .map(|v| match v {
            YamlValue::Sequence(seq) => seq
                .iter()
                .map(yaml_to_expression)
                .collect::<Result<Vec<_>, _>>(),
            YamlValue::String(s) => Ok(s
                .split(',')
                .map(|g| Expression::String(g.trim().to_string()))
                .collect()),
            _ => Ok(vec![]),
        })
        .transpose()?
        .unwrap_or_default();

    Ok(ModuleCall::User {
        name,
        state,
        uid,
        gid,
        groups,
        shell,
        home,
        create_home,
    })
}

fn parse_template_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    let src = yaml_to_expression(value)?;

    let dest = module
        .get("dest")
        .map(yaml_to_expression)
        .transpose()?
        .ok_or_else(|| NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::MissingField,
            message: "template module requires 'dest' field".to_string(),
            file: None,
            line: None,
            column: None,
            suggestion: Some("Add dest: /path/to/destination".to_string()),
        })))?;

    let owner = module.get("owner").map(yaml_to_expression).transpose()?;
    let group = module.get("group").map(yaml_to_expression).transpose()?;
    let mode = module.get("mode").map(yaml_to_expression).transpose()?;

    Ok(ModuleCall::Template {
        src,
        dest,
        owner,
        group,
        mode,
    })
}

fn parse_facts_module(
    value: &YamlValue,
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    // Categories can be in the value itself or in a 'categories' field
    let categories = if let Some(cats_value) = module.get("categories") {
        // Categories specified as separate field
        match cats_value {
            YamlValue::String(s) => {
                // Single category as string
                vec![s.clone()]
            }
            YamlValue::Sequence(seq) => {
                // List of categories
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            }
            _ => vec!["all".to_string()],
        }
    } else if let YamlValue::String(s) = value {
        // Single category as the value
        vec![s.clone()]
    } else if let YamlValue::Sequence(seq) = value {
        // List of categories as the value
        seq.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    } else {
        // No categories specified, gather all
        vec!["all".to_string()]
    };

    Ok(ModuleCall::Facts { categories })
}

pub(crate) fn yaml_to_expression(value: &YamlValue) -> Result<Expression, NexusError> {
    match value {
        YamlValue::String(s) => {
            if has_interpolation(s) {
                parse_interpolated_string(s)
            } else {
                Ok(Expression::String(s.clone()))
            }
        }
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Expression::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Expression::Float(f))
            } else {
                Ok(Expression::Integer(0))
            }
        }
        YamlValue::Bool(b) => Ok(Expression::Boolean(*b)),
        YamlValue::Null => Ok(Expression::Null),
        YamlValue::Sequence(seq) => {
            let items: Result<Vec<_>, _> = seq.iter().map(yaml_to_expression).collect();
            Ok(Expression::List(items?))
        }
        YamlValue::Mapping(map) => {
            let items: Result<Vec<_>, _> = map
                .iter()
                .map(|(k, v)| {
                    let key = yaml_to_expression(k)?;
                    let value = yaml_to_expression(v)?;
                    Ok((key, value))
                })
                .collect();
            Ok(Expression::Dict(items?))
        }
        YamlValue::Tagged(tagged) => yaml_to_expression(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_playbook() {
        let yaml = r#"
hosts: webservers

tasks:
  - name: Install nginx
    package: nginx
    state: installed
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();
        assert_eq!(playbook.hosts, HostPattern::Group("webservers".to_string()));
        assert_eq!(playbook.tasks.len(), 1);
        if let TaskOrBlock::Task(ref task) = playbook.tasks[0] {
            assert_eq!(task.name, "Install nginx");
        } else {
            panic!("Expected Task, got Block");
        }
    }

    #[test]
    fn test_parse_with_variables() {
        let yaml = r#"
hosts: all

vars:
  webserver: nginx
  port: 80

tasks:
  - name: Install webserver
    package: ${vars.webserver}
    state: installed
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();
        assert!(playbook.vars.contains_key("webserver"));
        assert!(playbook.vars.contains_key("port"));
    }

    #[test]
    fn test_unknown_module_error() {
        let yaml = r#"
hosts: all

tasks:
  - name: Bad task
    packages: nginx
"#;

        let result = parse_playbook(yaml, "test.nx.yaml".to_string());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown module"));
    }

    #[test]
    fn test_parse_playbook_with_roles() {
        let yaml = r#"
hosts: webservers

vars:
  app_name: myapp

pre_tasks:
  - name: Check connectivity
    command: echo "Starting"

roles:
  - common
  - role: webserver
    vars:
      nginx_port: 8080
    tags:
      - nginx
      - web

tasks:
  - name: Deploy app
    command: echo "Deploying"

post_tasks:
  - name: Verify
    command: echo "Done"

handlers:
  - name: restart app
    service: myapp
    state: restarted
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();

        // Check hosts
        assert_eq!(playbook.hosts, HostPattern::Group("webservers".to_string()));

        // Check pre_tasks
        assert_eq!(playbook.pre_tasks.len(), 1);
        if let TaskOrBlock::Task(ref task) = playbook.pre_tasks[0] {
            assert_eq!(task.name, "Check connectivity");
        } else {
            panic!("Expected Task, got Block");
        }

        // Check roles
        assert_eq!(playbook.roles.len(), 2);
        assert_eq!(playbook.roles[0].role, "common");
        assert!(playbook.roles[0].vars.is_empty());

        assert_eq!(playbook.roles[1].role, "webserver");
        assert!(playbook.roles[1].vars.contains_key("nginx_port"));
        assert_eq!(playbook.roles[1].tags, vec!["nginx", "web"]);

        // Check regular tasks
        assert_eq!(playbook.tasks.len(), 1);
        if let TaskOrBlock::Task(ref task) = playbook.tasks[0] {
            assert_eq!(task.name, "Deploy app");
        } else {
            panic!("Expected Task, got Block");
        }

        // Check post_tasks
        assert_eq!(playbook.post_tasks.len(), 1);
        if let TaskOrBlock::Task(ref task) = playbook.post_tasks[0] {
            assert_eq!(task.name, "Verify");
        } else {
            panic!("Expected Task, got Block");
        }

        // Check handlers
        assert_eq!(playbook.handlers.len(), 1);
        assert_eq!(playbook.handlers[0].name, "restart app");
    }

    #[test]
    fn test_parse_role_with_when_condition() {
        let yaml = r#"
hosts: all

roles:
  - role: webserver
    when: vars.install_web == true
    vars:
      port: 80
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();

        assert_eq!(playbook.roles.len(), 1);
        assert!(playbook.roles[0].when.is_some(), "role should have when condition");
    }

    #[test]
    fn test_parse_block() {
        let yaml = r#"
hosts: localhost

tasks:
  - name: Deploy with rollback
    block:
      - name: Deploy new version
        command: deploy.sh
      - name: Run migrations
        command: migrate.sh
    rescue:
      - name: Rollback deployment
        command: rollback.sh
    always:
      - name: Cleanup temp files
        command: rm -rf /tmp/deploy
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();

        assert_eq!(playbook.tasks.len(), 1);

        if let TaskOrBlock::Block(ref block) = playbook.tasks[0] {
            assert_eq!(block.name, Some("Deploy with rollback".to_string()));
            assert_eq!(block.block.len(), 2);
            assert_eq!(block.rescue.len(), 1);
            assert_eq!(block.always.len(), 1);

            // Check block tasks
            assert_eq!(block.block[0].name, "Deploy new version");
            assert_eq!(block.block[1].name, "Run migrations");

            // Check rescue tasks
            assert_eq!(block.rescue[0].name, "Rollback deployment");

            // Check always tasks
            assert_eq!(block.always[0].name, "Cleanup temp files");
        } else {
            panic!("Expected Block, got Task");
        }
    }

    #[test]
    fn test_parse_block_with_when() {
        let yaml = r#"
hosts: localhost

tasks:
  - name: Conditional block
    when: vars.env == "production"
    block:
      - name: Production task
        command: echo "prod"
    rescue:
      - name: Handle error
        command: echo "error"
    always:
      - name: Cleanup
        command: echo "cleanup"
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();

        assert_eq!(playbook.tasks.len(), 1);

        if let TaskOrBlock::Block(ref block) = playbook.tasks[0] {
            assert!(block.when.is_some(), "block should have when condition");
        } else {
            panic!("Expected Block, got Task");
        }
    }

    #[test]
    fn test_parse_block_with_tags() {
        let yaml = r#"
hosts: localhost

tasks:
  - name: Tagged block
    tags:
      - deploy
      - critical
    block:
      - name: Deploy task
        command: deploy.sh
    rescue:
      - name: Rollback task
        command: rollback.sh
"#;

        let playbook = parse_playbook(yaml, "test.nx.yaml".to_string()).unwrap();

        assert_eq!(playbook.tasks.len(), 1);

        if let TaskOrBlock::Block(ref block) = playbook.tasks[0] {
            assert_eq!(block.tags, vec!["deploy", "critical"]);
        } else {
            panic!("Expected Block, got Task");
        }
    }
}

/// Convert import_tasks - static import resolved at parse time
fn convert_import_tasks(import_file: String, raw: RawTask, source_file: &str) -> Result<TaskOrBlock, NexusError> {
    // Parse tags
    let tags = match raw.tags {
        Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(TagsValue::Multiple(v)) => v,
        None => vec![],
    };

    // Convert vars
    let vars = raw.vars
        .map(convert_vars)
        .transpose()?
        .unwrap_or_default();

    // Resolve the file path relative to the playbook directory
    let playbook_dir = Path::new(source_file).parent().unwrap_or(Path::new("."));
    let import_path = playbook_dir.join(&import_file);

    // Validate that the file exists
    if !import_path.exists() {
        return Err(NexusError::Io {
            message: format!("Task file not found: {}", import_path.display()),
            path: Some(import_path.clone()),
        });
    }

    // Return the Import node - scheduler will handle loading the tasks
    Ok(TaskOrBlock::Import(ImportTasks {
        file: import_path.to_string_lossy().to_string(),
        vars,
        tags,
        location: None,
    }))
}

/// Convert include_tasks - dynamic include resolved at runtime
fn convert_include_tasks(include_file: String, raw: RawTask, _source_file: &str) -> Result<TaskOrBlock, NexusError> {
    // Parse the file expression (can contain variables)
    let file_expr = if has_interpolation(&include_file) {
        parse_interpolated_string(&include_file)?
    } else {
        Expression::String(include_file)
    };

    // Parse when condition
    let when = raw.when_condition
        .map(|w| parse_condition(&w))
        .transpose()?;

    // Parse loop expression
    let loop_expr = raw.loop_expr.map(|l| parse_condition(&l)).transpose()?;
    let loop_var = raw.loop_var.unwrap_or_else(|| "item".to_string());

    // Parse tags
    let tags = match raw.tags {
        Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(TagsValue::Multiple(v)) => v,
        None => vec![],
    };

    // Convert vars to expressions
    let vars = raw.vars
        .map(|v| {
            v.into_iter()
                .map(|(k, val)| Ok((k, yaml_to_expression(&val)?)))
                .collect::<Result<HashMap<_, _>, NexusError>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(TaskOrBlock::Include(IncludeTasks {
        file: file_expr,
        vars,
        when,
        loop_expr,
        loop_var,
        tags,
        location: None,
    }))
}
