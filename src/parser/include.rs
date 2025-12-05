// Task file inclusion support (import_tasks and include_tasks)

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as YamlValue;

use super::ast::*;
use super::expressions::{has_interpolation, parse_interpolated_string};
use super::yaml::{convert_vars, extract_yaml_error_location, parse_condition, yaml_to_expression};
use crate::output::errors::{NexusError, ParseError, ParseErrorKind};

/// Raw task structure for parsing (subset of full RawTask)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawTaskFile {
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
    sudo: Option<bool>,
    #[serde(rename = "as")]
    run_as: Option<String>,
    tags: Option<TagsValue>,
    retry: Option<RawRetryConfig>,
    until: Option<String>,
    retries: Option<u32>,
    delay: Option<u64>,
    #[serde(rename = "async")]
    async_timeout: Option<u64>,
    poll: Option<u64>,
    timeout: Option<u64>,
    throttle: Option<usize>,
    delegate_to: Option<String>,
    delegate_facts: Option<bool>,
    block: Option<Vec<RawTaskFile>>,
    rescue: Option<Vec<RawTaskFile>>,
    always: Option<Vec<RawTaskFile>>,
    import_tasks: Option<String>,
    include_tasks: Option<String>,
    vars: Option<HashMap<String, YamlValue>>,
    #[serde(flatten)]
    module: HashMap<String, YamlValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TagsValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NotifyValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawRetryConfig {
    attempts: Option<u32>,
    delay: Option<RawDelayValue>,
    retry_when: Option<String>,
    until: Option<String>,
    circuit_breaker: Option<RawCircuitBreakerValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum RawDelayValue {
    Seconds(u64),
    Strategy(RawDelayStrategy),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawDelayStrategy {
    strategy: String,
    base: Option<u64>,
    max: Option<u64>,
    increment: Option<u64>,
    jitter: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum RawCircuitBreakerValue {
    Name(String),
    Config(RawCircuitBreakerConfig),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawCircuitBreakerConfig {
    name: String,
    failure_threshold: Option<u32>,
    reset_timeout: Option<u64>,
    success_threshold: Option<u32>,
}

/// Convert import_tasks - static import resolved at parse time
pub fn convert_import_tasks(
    import_file: String,
    tags: Vec<String>,
    vars: Option<HashMap<String, YamlValue>>,
    source_file: &str,
) -> Result<TaskOrBlock, NexusError> {
    // Convert vars
    let converted_vars = vars
        .map(convert_vars)
        .transpose()?
        .unwrap_or_default();

    // Resolve the file path relative to the playbook directory
    let playbook_dir = Path::new(source_file)
        .parent()
        .unwrap_or(Path::new("."));
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
        vars: converted_vars,
        tags,
        location: None,
    }))
}

/// Convert include_tasks - dynamic include resolved at runtime
pub fn convert_include_tasks(
    include_file: String,
    when: Option<String>,
    loop_expr: Option<String>,
    loop_var: Option<String>,
    tags: Vec<String>,
    vars: Option<HashMap<String, YamlValue>>,
) -> Result<TaskOrBlock, NexusError> {
    // Parse the file expression (can contain variables)
    let file_expr = if has_interpolation(&include_file) {
        parse_interpolated_string(&include_file)?
    } else {
        Expression::String(include_file)
    };

    // Parse when condition
    let when_expr = when.map(|w| parse_condition(&w)).transpose()?;

    // Parse loop expression
    let loop_expr_parsed = loop_expr.map(|l| parse_condition(&l)).transpose()?;
    let loop_var_name = loop_var.unwrap_or_else(|| "item".to_string());

    // Convert vars to expressions
    let converted_vars = vars
        .map(|v| {
            v.into_iter()
                .map(|(k, val)| Ok((k, yaml_to_expression(&val)?)))
                .collect::<Result<HashMap<_, _>, NexusError>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(TaskOrBlock::Include(IncludeTasks {
        file: file_expr,
        vars: converted_vars,
        when: when_expr,
        loop_expr: loop_expr_parsed,
        loop_var: loop_var_name,
        tags,
        location: None,
    }))
}

/// Parse a task file (YAML file containing just a list of tasks)
/// This is called at runtime for dynamic includes or at parse time for static imports
pub fn parse_task_file(path: &Path) -> Result<Vec<TaskOrBlock>, NexusError> {
    let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
        message: format!("Failed to read task file: {}", e),
        path: Some(path.to_path_buf()),
    })?;

    // Parse as a YAML list of tasks
    let raw_tasks: Vec<RawTaskFile> = serde_yaml::from_str(&content).map_err(|e| {
        let (line, column) = extract_yaml_error_location(&e);
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidYaml,
            message: format!("Invalid YAML in task file: {}", e),
            file: Some(path.to_string_lossy().to_string()),
            line,
            column,
            suggestion: Some("Task files should contain a YAML list of tasks".to_string()),
        }))
    })?;

    // Convert each raw task to TaskOrBlock
    let tasks: Result<Vec<TaskOrBlock>, NexusError> = raw_tasks
        .into_iter()
        .enumerate()
        .map(|(index, raw)| convert_task_file(raw, path.to_string_lossy().as_ref(), index))
        .collect();

    tasks
}

/// Convert a RawTaskFile to a TaskOrBlock
fn convert_task_file(raw: RawTaskFile, source_file: &str, index: usize) -> Result<TaskOrBlock, NexusError> {
    // Check if this is an import_tasks (static import)
    if let Some(ref import_file) = raw.import_tasks {
        let tags = match raw.tags {
            Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
            Some(TagsValue::Multiple(v)) => v,
            None => vec![],
        };
        return convert_import_tasks(import_file.clone(), tags, raw.vars, source_file);
    }

    // Check if this is an include_tasks (dynamic include)
    if let Some(ref include_file) = raw.include_tasks {
        let tags = match raw.tags {
            Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
            Some(TagsValue::Multiple(v)) => v,
            None => vec![],
        };
        return convert_include_tasks(
            include_file.clone(),
            raw.when_condition,
            raw.loop_expr,
            raw.loop_var,
            tags,
            raw.vars,
        );
    }

    // Check if this is a block (has block: field)
    if raw.block.is_some() {
        return convert_block_file(raw, source_file, index);
    }

    // Otherwise, it's a regular task - convert module call
    let module_call = parse_module_call_from_raw(&raw.module, source_file)?;

    // Parse name (default to module name if not specified)
    let name = raw.name.unwrap_or_else(|| format!("Task {}", index + 1));

    // Parse when condition
    let when = raw.when_condition
        .map(|w| parse_condition(&w))
        .transpose()?;

    // Parse loop
    let loop_expr = raw.loop_expr.map(|l| parse_condition(&l)).transpose()?;
    let loop_var = raw.loop_var.unwrap_or_else(|| "item".to_string());

    // Parse register
    let register = raw.register;

    // Parse fail_when
    let fail_when = raw.fail_when
        .map(|f| parse_condition(&f))
        .transpose()?;

    // Parse changed_when
    let changed_when = raw.changed_when
        .map(|c| parse_condition(&c))
        .transpose()?;

    // Parse notify
    let notify = match raw.notify {
        Some(NotifyValue::Single(s)) => vec![s],
        Some(NotifyValue::Multiple(v)) => v,
        None => vec![],
    };

    // Parse tags
    let tags = match raw.tags {
        Some(TagsValue::Single(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(TagsValue::Multiple(v)) => v,
        None => vec![],
    };

    // Parse delegation
    let delegate_to = raw.delegate_to
        .map(|d| {
            if has_interpolation(&d) {
                parse_interpolated_string(&d)
            } else {
                Ok(Expression::String(d))
            }
        })
        .transpose()?;

    // Get line/column for location
    let (line, column) = (0, 0);

    // Build the task
    Ok(TaskOrBlock::Task(Box::new(Task {
        name,
        module: module_call,
        when,
        register,
        loop_expr,
        loop_var,
        fail_when,
        changed_when,
        notify,
        sudo: raw.sudo,
        run_as: raw.run_as,
        tags,
        retry: None, // TODO: Parse retry config if needed
        async_config: if let Some(async_timeout) = raw.async_timeout {
            Some(AsyncConfig {
                async_timeout,
                poll: raw.poll.unwrap_or(10),
                retries: raw.retries.unwrap_or(30),
            })
        } else {
            None
        },
        timeout: raw.timeout.map(std::time::Duration::from_secs),
        throttle: raw.throttle,
        delegate_to,
        delegate_facts: raw.delegate_facts.unwrap_or(false),
        location: Some(SourceLocation {
            file: source_file.to_string(),
            line,
            column,
        }),
    })))
}

/// Convert a block from a RawTaskFile
fn convert_block_file(raw: RawTaskFile, source_file: &str, _index: usize) -> Result<TaskOrBlock, NexusError> {
    let name = raw.name;

    // Convert block tasks - only keep Task variants
    let block_tasks: Result<Vec<Task>, NexusError> = raw.block
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            match convert_task_file(t, source_file, i)? {
                TaskOrBlock::Task(task) => Ok(*task),
                _ => Err(NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: "Blocks can only contain tasks, not nested blocks/imports/includes".to_string(),
                    file: Some(source_file.to_string()),
                    line: None,
                    column: None,
                    suggestion: None,
                }))),
            }
        })
        .collect();

    // Convert rescue tasks - only keep Task variants
    let rescue_tasks: Result<Vec<Task>, NexusError> = raw.rescue
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            match convert_task_file(t, source_file, i)? {
                TaskOrBlock::Task(task) => Ok(*task),
                _ => Err(NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: "Rescue blocks can only contain tasks".to_string(),
                    file: Some(source_file.to_string()),
                    line: None,
                    column: None,
                    suggestion: None,
                }))),
            }
        })
        .collect();

    // Convert always tasks - only keep Task variants
    let always_tasks: Result<Vec<Task>, NexusError> = raw.always
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            match convert_task_file(t, source_file, i)? {
                TaskOrBlock::Task(task) => Ok(*task),
                _ => Err(NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: "Always blocks can only contain tasks".to_string(),
                    file: Some(source_file.to_string()),
                    line: None,
                    column: None,
                    suggestion: None,
                }))),
            }
        })
        .collect();

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

    Ok(TaskOrBlock::Block(Block {
        name,
        block: block_tasks?,
        rescue: rescue_tasks?,
        always: always_tasks?,
        when,
        tags,
        location: None,
    }))
}

/// Parse a module call from the flattened module HashMap
fn parse_module_call_from_raw(
    module: &HashMap<String, YamlValue>,
    _source_file: &str,
) -> Result<ModuleCall, NexusError> {
    // This is a simplified version - just handle command module for now
    if let Some(cmd_value) = module.get("command") {
        let cmd = match cmd_value {
            YamlValue::String(s) => {
                if has_interpolation(s) {
                    parse_interpolated_string(s)?
                } else {
                    Expression::String(s.clone())
                }
            }
            _ => {
                return Err(NexusError::Parse(Box::new(ParseError {
                    kind: ParseErrorKind::InvalidValue,
                    message: "command must be a string".to_string(),
                    file: None,
                    line: None,
                    column: None,
                    suggestion: None,
                })));
            }
        };
        return Ok(ModuleCall::Command {
            cmd,
            creates: None,
            removes: None,
        });
    }

    // Add more module types as needed
    Err(NexusError::Parse(Box::new(ParseError {
        kind: ParseErrorKind::UnknownModule,
        message: format!("Unknown or unsupported module. Available keys: {:?}", module.keys().collect::<Vec<_>>()),
        file: None,
        line: None,
        column: None,
        suggestion: Some("Currently only 'command' module is supported in included task files".to_string()),
    })))
}
