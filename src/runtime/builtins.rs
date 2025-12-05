// Built-in functions and filters

use std::collections::HashMap;

use crate::executor::ExecutionContext;
use crate::output::errors::NexusError;
use crate::parser::ast::{Expression, Value};

/// Call a built-in function
pub fn call_builtin(
    name: &str,
    args: Vec<Value>,
    kwargs: HashMap<String, Value>,
) -> Result<Value, NexusError> {
    match name {
        "len" => builtin_len(args),
        "str" => builtin_str(args),
        "int" => builtin_int(args),
        "float" => builtin_float(args),
        "bool" => builtin_bool(args),
        "list" => builtin_list(args),
        "dict" => builtin_dict(args),
        "range" => builtin_range(args),
        "min" => builtin_min(args),
        "max" => builtin_max(args),
        "sum" => builtin_sum(args),
        "abs" => builtin_abs(args),
        "round" => builtin_round(args),
        "sorted" => builtin_sorted(args, kwargs),
        "reversed" => builtin_reversed(args),
        "enumerate" => builtin_enumerate(args),
        "zip" => builtin_zip(args),
        "any" => builtin_any(args),
        "all" => builtin_all(args),
        "print" => builtin_print(args),
        _ => Err(NexusError::Runtime {
            function: Some(name.to_string()),
            message: format!("Unknown function: {}", name),
            suggestion: Some("Check function name and available builtins".to_string()),
        }),
    }
}

/// Call a built-in function with context (for lookup and other context-aware functions)
pub fn call_builtin_with_context(
    name: &str,
    args: Vec<Value>,
    kwargs: HashMap<String, Value>,
    ctx: &ExecutionContext,
) -> Result<Value, NexusError> {
    match name {
        "lookup" => builtin_lookup(args, ctx),
        _ => call_builtin(name, args, kwargs),
    }
}

fn builtin_lookup(args: Vec<Value>, ctx: &ExecutionContext) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup".to_string()),
            message: "lookup requires at least one argument (lookup type)".to_string(),
            suggestion: Some("Example: lookup('env', 'HOME')".to_string()),
        });
    }

    let lookup_type = match &args[0] {
        Value::String(s) => s.as_str(),
        _ => {
            return Err(NexusError::Runtime {
                function: Some("lookup".to_string()),
                message: "First argument to lookup must be a string (lookup type)".to_string(),
                suggestion: None,
            })
        }
    };

    crate::plugins::lookup(lookup_type, &args[1..], ctx)
}

/// Call a method on a value
pub fn call_method(
    obj: &Value,
    method: &str,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> Result<Value, NexusError> {
    match obj {
        Value::String(s) => call_string_method(s, method, args),
        Value::List(l) => call_list_method(l, method, args),
        Value::Dict(d) => call_dict_method(d, method, args),
        _ => Err(NexusError::Runtime {
            function: Some(method.to_string()),
            message: format!("Cannot call method '{}' on {:?}", method, obj),
            suggestion: None,
        }),
    }
}

/// Apply a filter to a value
pub fn apply_filter(
    input: &Value,
    filter_name: &str,
    predicate: Option<&Expression>,
    _ctx: &ExecutionContext,
) -> Result<Value, NexusError> {
    match filter_name {
        "filter" => {
            let list = match input {
                Value::List(l) => l,
                _ => return Err(filter_type_error(filter_name, "list", input)),
            };

            // Without a predicate, filter out falsy values
            if predicate.is_none() {
                let filtered: Vec<Value> = list.iter().filter(|v| v.is_truthy()).cloned().collect();
                return Ok(Value::List(filtered));
            }

            // With predicate, we'd need lambda evaluation
            // For now, just return the list
            Ok(input.clone())
        }

        "map" => {
            // Similar to filter, needs lambda support
            Ok(input.clone())
        }

        "first" => match input {
            Value::List(l) => l.first().cloned().ok_or_else(|| NexusError::Runtime {
                function: None,
                message: "Cannot get first element of empty list".to_string(),
                suggestion: None,
            }),
            _ => Err(filter_type_error(filter_name, "list", input)),
        },

        "last" => match input {
            Value::List(l) => l.last().cloned().ok_or_else(|| NexusError::Runtime {
                function: None,
                message: "Cannot get last element of empty list".to_string(),
                suggestion: None,
            }),
            _ => Err(filter_type_error(filter_name, "list", input)),
        },

        "unique" => match input {
            Value::List(l) => {
                let mut seen = std::collections::HashSet::new();
                let mut result = Vec::new();
                for item in l {
                    let key = item.to_string();
                    if seen.insert(key) {
                        result.push(item.clone());
                    }
                }
                Ok(Value::List(result))
            }
            _ => Err(filter_type_error(filter_name, "list", input)),
        },

        "join" => match input {
            Value::List(l) => {
                let sep = predicate
                    .and_then(|p| {
                        if let Expression::String(s) = p {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| ",".to_string());

                let joined: String = l.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep);
                Ok(Value::String(joined))
            }
            _ => Err(filter_type_error(filter_name, "list", input)),
        },

        "split" => match input {
            Value::String(s) => {
                let sep = predicate
                    .and_then(|p| {
                        if let Expression::String(sp) = p {
                            Some(sp.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| " ".to_string());

                let parts: Vec<Value> = s.split(&sep).map(|p| Value::String(p.to_string())).collect();
                Ok(Value::List(parts))
            }
            _ => Err(filter_type_error(filter_name, "string", input)),
        },

        "upper" => match input {
            Value::String(s) => Ok(Value::String(s.to_uppercase())),
            _ => Err(filter_type_error(filter_name, "string", input)),
        },

        "lower" => match input {
            Value::String(s) => Ok(Value::String(s.to_lowercase())),
            _ => Err(filter_type_error(filter_name, "string", input)),
        },

        "trim" => match input {
            Value::String(s) => Ok(Value::String(s.trim().to_string())),
            _ => Err(filter_type_error(filter_name, "string", input)),
        },

        "replace" => {
            // Would need two args from predicate
            Ok(input.clone())
        }

        "default" => {
            if input.is_truthy() {
                Ok(input.clone())
            } else if let Some(Expression::String(default)) = predicate {
                Ok(Value::String(default.clone()))
            } else {
                Ok(Value::String(String::new()))
            }
        }

        "int" => match input {
            Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| NexusError::Runtime {
                    function: None,
                    message: format!("Cannot convert '{}' to int", s),
                    suggestion: None,
                }),
            Value::Float(f) => Ok(Value::Int(*f as i64)),
            Value::Int(i) => Ok(Value::Int(*i)),
            _ => Err(filter_type_error(filter_name, "string/number", input)),
        },

        "float" => match input {
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| NexusError::Runtime {
                    function: None,
                    message: format!("Cannot convert '{}' to float", s),
                    suggestion: None,
                }),
            Value::Int(i) => Ok(Value::Float(*i as f64)),
            Value::Float(f) => Ok(Value::Float(*f)),
            _ => Err(filter_type_error(filter_name, "string/number", input)),
        },

        "length" | "count" => match input {
            Value::String(s) => Ok(Value::Int(s.len() as i64)),
            Value::List(l) => Ok(Value::Int(l.len() as i64)),
            Value::Dict(d) => Ok(Value::Int(d.len() as i64)),
            _ => Err(filter_type_error(filter_name, "string/list/dict", input)),
        },

        "keys" => match input {
            Value::Dict(d) => {
                let keys: Vec<Value> = d.keys().map(|k| Value::String(k.clone())).collect();
                Ok(Value::List(keys))
            }
            _ => Err(filter_type_error(filter_name, "dict", input)),
        },

        "values" => match input {
            Value::Dict(d) => {
                let values: Vec<Value> = d.values().cloned().collect();
                Ok(Value::List(values))
            }
            _ => Err(filter_type_error(filter_name, "dict", input)),
        },

        "items" => match input {
            Value::Dict(d) => {
                let items: Vec<Value> = d
                    .iter()
                    .map(|(k, v)| Value::List(vec![Value::String(k.clone()), v.clone()]))
                    .collect();
                Ok(Value::List(items))
            }
            _ => Err(filter_type_error(filter_name, "dict", input)),
        },

        _ => Err(NexusError::Runtime {
            function: None,
            message: format!("Unknown filter: {}", filter_name),
            suggestion: Some("Available filters: filter, map, first, last, unique, join, split, upper, lower, trim, default, int, float, length, keys, values, items".to_string()),
        }),
    }
}

// Built-in functions

fn builtin_len(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("len", &args, 1)?;
    match &args[0] {
        Value::String(s) => Ok(Value::Int(s.len() as i64)),
        Value::List(l) => Ok(Value::Int(l.len() as i64)),
        Value::Dict(d) => Ok(Value::Int(d.len() as i64)),
        _ => Err(arg_type_error("len", 0, "string/list/dict", &args[0])),
    }
}

fn builtin_str(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("str", &args, 1)?;
    Ok(Value::String(args[0].to_string()))
}

fn builtin_int(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("int", &args, 1)?;
    match &args[0] {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Float(f) => Ok(Value::Int(*f as i64)),
        Value::String(s) => {
            s.trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| NexusError::Runtime {
                    function: Some("int".to_string()),
                    message: format!("Cannot convert '{}' to int", s),
                    suggestion: None,
                })
        }
        Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
        _ => Err(arg_type_error("int", 0, "number/string", &args[0])),
    }
}

fn builtin_float(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("float", &args, 1)?;
    match &args[0] {
        Value::Int(i) => Ok(Value::Float(*i as f64)),
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::String(s) => {
            s.trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| NexusError::Runtime {
                    function: Some("float".to_string()),
                    message: format!("Cannot convert '{}' to float", s),
                    suggestion: None,
                })
        }
        _ => Err(arg_type_error("float", 0, "number/string", &args[0])),
    }
}

fn builtin_bool(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("bool", &args, 1)?;
    Ok(Value::Bool(args[0].is_truthy()))
}

fn builtin_list(args: Vec<Value>) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Ok(Value::List(Vec::new()));
    }
    match &args[0] {
        Value::List(l) => Ok(Value::List(l.clone())),
        Value::String(s) => {
            let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            Ok(Value::List(chars))
        }
        _ => Ok(Value::List(args)),
    }
}

fn builtin_dict(args: Vec<Value>) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Ok(Value::Dict(HashMap::new()));
    }
    match &args[0] {
        Value::Dict(d) => Ok(Value::Dict(d.clone())),
        Value::List(pairs) => {
            let mut result = HashMap::new();
            for pair in pairs {
                if let Value::List(kv) = pair {
                    if kv.len() == 2 {
                        let key = kv[0].to_string();
                        result.insert(key, kv[1].clone());
                    }
                }
            }
            Ok(Value::Dict(result))
        }
        _ => Err(arg_type_error("dict", 0, "dict/list", &args[0])),
    }
}

fn builtin_range(args: Vec<Value>) -> Result<Value, NexusError> {
    let (start, end, step) = match args.len() {
        1 => (0, get_int(&args[0])?, 1),
        2 => (get_int(&args[0])?, get_int(&args[1])?, 1),
        3 => (get_int(&args[0])?, get_int(&args[1])?, get_int(&args[2])?),
        _ => {
            return Err(NexusError::Runtime {
                function: Some("range".to_string()),
                message: "range takes 1-3 arguments".to_string(),
                suggestion: None,
            })
        }
    };

    if step == 0 {
        return Err(NexusError::Runtime {
            function: Some("range".to_string()),
            message: "range step cannot be zero".to_string(),
            suggestion: None,
        });
    }

    let mut result = Vec::new();
    let mut i = start;
    while (step > 0 && i < end) || (step < 0 && i > end) {
        result.push(Value::Int(i));
        i += step;
    }

    Ok(Value::List(result))
}

fn builtin_min(args: Vec<Value>) -> Result<Value, NexusError> {
    let items = if args.len() == 1 {
        match &args[0] {
            Value::List(l) => l.clone(),
            _ => args,
        }
    } else {
        args
    };

    if items.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("min".to_string()),
            message: "min requires at least one argument".to_string(),
            suggestion: None,
        });
    }

    let mut min = &items[0];
    for item in &items[1..] {
        if compare_for_sort(item, min) == std::cmp::Ordering::Less {
            min = item;
        }
    }
    Ok(min.clone())
}

fn builtin_max(args: Vec<Value>) -> Result<Value, NexusError> {
    let items = if args.len() == 1 {
        match &args[0] {
            Value::List(l) => l.clone(),
            _ => args,
        }
    } else {
        args
    };

    if items.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("max".to_string()),
            message: "max requires at least one argument".to_string(),
            suggestion: None,
        });
    }

    let mut max = &items[0];
    for item in &items[1..] {
        if compare_for_sort(item, max) == std::cmp::Ordering::Greater {
            max = item;
        }
    }
    Ok(max.clone())
}

fn builtin_sum(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("sum", &args, 1)?;
    match &args[0] {
        Value::List(l) => {
            let mut sum = 0i64;
            let mut is_float = false;
            let mut sum_f = 0.0f64;

            for item in l {
                match item {
                    Value::Int(i) => {
                        if is_float {
                            sum_f += *i as f64;
                        } else {
                            sum += i;
                        }
                    }
                    Value::Float(f) => {
                        if !is_float {
                            sum_f = sum as f64;
                            is_float = true;
                        }
                        sum_f += f;
                    }
                    _ => {
                        return Err(NexusError::Runtime {
                            function: Some("sum".to_string()),
                            message: "sum requires a list of numbers".to_string(),
                            suggestion: None,
                        })
                    }
                }
            }

            if is_float {
                Ok(Value::Float(sum_f))
            } else {
                Ok(Value::Int(sum))
            }
        }
        _ => Err(arg_type_error("sum", 0, "list", &args[0])),
    }
}

fn builtin_abs(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("abs", &args, 1)?;
    match &args[0] {
        Value::Int(i) => Ok(Value::Int(i.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        _ => Err(arg_type_error("abs", 0, "number", &args[0])),
    }
}

fn builtin_round(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("round", &args, 1)?;
    match &args[0] {
        Value::Float(f) => Ok(Value::Int(f.round() as i64)),
        Value::Int(i) => Ok(Value::Int(*i)),
        _ => Err(arg_type_error("round", 0, "number", &args[0])),
    }
}

fn builtin_sorted(args: Vec<Value>, kwargs: HashMap<String, Value>) -> Result<Value, NexusError> {
    require_args("sorted", &args, 1)?;
    match &args[0] {
        Value::List(l) => {
            let mut sorted = l.clone();
            sorted.sort_by(compare_for_sort);

            if kwargs
                .get("reverse")
                .map(|v| v.is_truthy())
                .unwrap_or(false)
            {
                sorted.reverse();
            }

            Ok(Value::List(sorted))
        }
        _ => Err(arg_type_error("sorted", 0, "list", &args[0])),
    }
}

fn builtin_reversed(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("reversed", &args, 1)?;
    match &args[0] {
        Value::List(l) => {
            let reversed: Vec<Value> = l.iter().rev().cloned().collect();
            Ok(Value::List(reversed))
        }
        Value::String(s) => Ok(Value::String(s.chars().rev().collect())),
        _ => Err(arg_type_error("reversed", 0, "list/string", &args[0])),
    }
}

fn builtin_enumerate(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("enumerate", &args, 1)?;
    match &args[0] {
        Value::List(l) => {
            let enumerated: Vec<Value> = l
                .iter()
                .enumerate()
                .map(|(i, v)| Value::List(vec![Value::Int(i as i64), v.clone()]))
                .collect();
            Ok(Value::List(enumerated))
        }
        _ => Err(arg_type_error("enumerate", 0, "list", &args[0])),
    }
}

fn builtin_zip(args: Vec<Value>) -> Result<Value, NexusError> {
    if args.len() < 2 {
        return Err(NexusError::Runtime {
            function: Some("zip".to_string()),
            message: "zip requires at least 2 arguments".to_string(),
            suggestion: None,
        });
    }

    let lists: Result<Vec<&Vec<Value>>, _> = args
        .iter()
        .map(|a| match a {
            Value::List(l) => Ok(l),
            _ => Err(NexusError::Runtime {
                function: Some("zip".to_string()),
                message: "zip requires list arguments".to_string(),
                suggestion: None,
            }),
        })
        .collect();

    let lists = lists?;
    let min_len = lists.iter().map(|l| l.len()).min().unwrap_or(0);

    let zipped: Vec<Value> = (0..min_len)
        .map(|i| Value::List(lists.iter().map(|l| l[i].clone()).collect()))
        .collect();

    Ok(Value::List(zipped))
}

fn builtin_any(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("any", &args, 1)?;
    match &args[0] {
        Value::List(l) => Ok(Value::Bool(l.iter().any(|v| v.is_truthy()))),
        _ => Err(arg_type_error("any", 0, "list", &args[0])),
    }
}

fn builtin_all(args: Vec<Value>) -> Result<Value, NexusError> {
    require_args("all", &args, 1)?;
    match &args[0] {
        Value::List(l) => Ok(Value::Bool(l.iter().all(|v| v.is_truthy()))),
        _ => Err(arg_type_error("all", 0, "list", &args[0])),
    }
}

fn builtin_print(args: Vec<Value>) -> Result<Value, NexusError> {
    let output: Vec<String> = args.iter().map(|v| v.to_string()).collect();
    println!("{}", output.join(" "));
    Ok(Value::Null)
}

// String methods

fn call_string_method(s: &str, method: &str, args: Vec<Value>) -> Result<Value, NexusError> {
    match method {
        "upper" => Ok(Value::String(s.to_uppercase())),
        "lower" => Ok(Value::String(s.to_lowercase())),
        "strip" | "trim" => Ok(Value::String(s.trim().to_string())),
        "lstrip" => Ok(Value::String(s.trim_start().to_string())),
        "rstrip" => Ok(Value::String(s.trim_end().to_string())),
        "split" => {
            let sep = args.first().and_then(|v| v.as_str()).unwrap_or(" ");
            let parts: Vec<Value> = s.split(sep).map(|p| Value::String(p.to_string())).collect();
            Ok(Value::List(parts))
        }
        "join" => match args.first() {
            Some(Value::List(items)) => {
                let joined: String = items
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(s);
                Ok(Value::String(joined))
            }
            _ => Err(NexusError::Runtime {
                function: Some("join".to_string()),
                message: "join requires a list argument".to_string(),
                suggestion: None,
            }),
        },
        "replace" => {
            if args.len() < 2 {
                return Err(NexusError::Runtime {
                    function: Some("replace".to_string()),
                    message: "replace requires 2 arguments".to_string(),
                    suggestion: None,
                });
            }
            let old = args[0].to_string();
            let new = args[1].to_string();
            Ok(Value::String(s.replace(&old, &new)))
        }
        "startswith" => {
            let prefix = args.first().map(|v| v.to_string()).unwrap_or_default();
            Ok(Value::Bool(s.starts_with(&prefix)))
        }
        "endswith" => {
            let suffix = args.first().map(|v| v.to_string()).unwrap_or_default();
            Ok(Value::Bool(s.ends_with(&suffix)))
        }
        "contains" => {
            let sub = args.first().map(|v| v.to_string()).unwrap_or_default();
            Ok(Value::Bool(s.contains(&sub)))
        }
        "find" => {
            let sub = args.first().map(|v| v.to_string()).unwrap_or_default();
            Ok(Value::Int(s.find(&sub).map(|i| i as i64).unwrap_or(-1)))
        }
        "isdigit" => Ok(Value::Bool(s.chars().all(|c| c.is_ascii_digit()))),
        "isalpha" => Ok(Value::Bool(s.chars().all(|c| c.is_alphabetic()))),
        "isalnum" => Ok(Value::Bool(s.chars().all(|c| c.is_alphanumeric()))),
        _ => Err(NexusError::Runtime {
            function: Some(method.to_string()),
            message: format!("Unknown string method: {}", method),
            suggestion: None,
        }),
    }
}

// List methods

fn call_list_method(l: &[Value], method: &str, args: Vec<Value>) -> Result<Value, NexusError> {
    match method {
        "append" => {
            let mut new_list = l.to_vec();
            if let Some(item) = args.into_iter().next() {
                new_list.push(item);
            }
            Ok(Value::List(new_list))
        }
        "extend" => {
            let mut new_list = l.to_vec();
            if let Some(Value::List(items)) = args.into_iter().next() {
                new_list.extend(items);
            }
            Ok(Value::List(new_list))
        }
        "index" => {
            let item = args.first().ok_or_else(|| NexusError::Runtime {
                function: Some("index".to_string()),
                message: "index requires an argument".to_string(),
                suggestion: None,
            })?;
            for (i, v) in l.iter().enumerate() {
                if v.to_string() == item.to_string() {
                    return Ok(Value::Int(i as i64));
                }
            }
            Ok(Value::Int(-1))
        }
        "count" => {
            let item = args.first().ok_or_else(|| NexusError::Runtime {
                function: Some("count".to_string()),
                message: "count requires an argument".to_string(),
                suggestion: None,
            })?;
            let count = l
                .iter()
                .filter(|v| v.to_string() == item.to_string())
                .count();
            Ok(Value::Int(count as i64))
        }
        "reverse" => {
            let reversed: Vec<Value> = l.iter().rev().cloned().collect();
            Ok(Value::List(reversed))
        }
        "sort" => {
            let mut sorted = l.to_vec();
            sorted.sort_by(compare_for_sort);
            Ok(Value::List(sorted))
        }
        _ => Err(NexusError::Runtime {
            function: Some(method.to_string()),
            message: format!("Unknown list method: {}", method),
            suggestion: None,
        }),
    }
}

// Dict methods

fn call_dict_method(
    d: &HashMap<String, Value>,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, NexusError> {
    match method {
        "keys" => {
            let keys: Vec<Value> = d.keys().map(|k| Value::String(k.clone())).collect();
            Ok(Value::List(keys))
        }
        "values" => {
            let values: Vec<Value> = d.values().cloned().collect();
            Ok(Value::List(values))
        }
        "items" => {
            let items: Vec<Value> = d
                .iter()
                .map(|(k, v)| Value::List(vec![Value::String(k.clone()), v.clone()]))
                .collect();
            Ok(Value::List(items))
        }
        "get" => {
            let key = args.first().map(|v| v.to_string()).unwrap_or_default();
            let default = args.get(1).cloned().unwrap_or(Value::Null);
            Ok(d.get(&key).cloned().unwrap_or(default))
        }
        "contains" => {
            let key = args.first().map(|v| v.to_string()).unwrap_or_default();
            Ok(Value::Bool(d.contains_key(&key)))
        }
        _ => Err(NexusError::Runtime {
            function: Some(method.to_string()),
            message: format!("Unknown dict method: {}", method),
            suggestion: None,
        }),
    }
}

// Helpers

fn require_args(func: &str, args: &[Value], expected: usize) -> Result<(), NexusError> {
    if args.len() < expected {
        Err(NexusError::Runtime {
            function: Some(func.to_string()),
            message: format!(
                "{} requires {} argument(s), got {}",
                func,
                expected,
                args.len()
            ),
            suggestion: None,
        })
    } else {
        Ok(())
    }
}

fn arg_type_error(func: &str, arg_idx: usize, expected: &str, got: &Value) -> NexusError {
    NexusError::Runtime {
        function: Some(func.to_string()),
        message: format!(
            "Argument {} to {} must be {}, got {:?}",
            arg_idx, func, expected, got
        ),
        suggestion: None,
    }
}

fn filter_type_error(filter: &str, expected: &str, got: &Value) -> NexusError {
    NexusError::Runtime {
        function: None,
        message: format!("Filter '{}' requires {}, got {:?}", filter, expected, got),
        suggestion: None,
    }
}

fn get_int(v: &Value) -> Result<i64, NexusError> {
    match v {
        Value::Int(i) => Ok(*i),
        _ => Err(NexusError::Runtime {
            function: None,
            message: format!("Expected int, got {:?}", v),
            suggestion: None,
        }),
    }
}

fn compare_for_sort(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}
