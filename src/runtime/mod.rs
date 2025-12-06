// Runtime module - expression evaluation and function execution

mod builtins;
mod interpreter;
mod types;

pub use builtins::*;
pub use interpreter::*;
pub use types::*;

use std::collections::HashMap;

use crate::executor::ExecutionContext;
use crate::output::errors::NexusError;
use crate::parser::ast::{BinaryOperator, Expression, StringPart, UnaryOperator, Value};

/// Evaluate an expression in a given context
pub fn evaluate_expression(expr: &Expression, ctx: &ExecutionContext) -> Result<Value, NexusError> {
    match expr {
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Integer(i) => Ok(Value::Int(*i)),
        Expression::Float(f) => Ok(Value::Float(*f)),
        Expression::Boolean(b) => Ok(Value::Bool(*b)),
        Expression::Null => Ok(Value::Null),

        Expression::Variable(path) => ctx.get_nested_var(path).ok_or_else(|| NexusError::Runtime {
            function: None,
            message: format!("Variable not found: {}", path.join(".")),
            suggestion: Some("Check variable name and ensure it's defined".to_string()),
        }),

        Expression::InterpolatedString(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    StringPart::Literal(s) => result.push_str(s),
                    StringPart::Expression(e) => {
                        let val = evaluate_expression(e, ctx)?;
                        result.push_str(&val.to_string());
                    }
                }
            }
            Ok(Value::String(result))
        }

        Expression::BinaryOp { left, op, right } => {
            let left_val = evaluate_expression(left, ctx)?;
            let right_val = evaluate_expression(right, ctx)?;
            evaluate_binary_op(&left_val, op, &right_val)
        }

        Expression::UnaryOp { op, operand } => {
            let val = evaluate_expression(operand, ctx)?;
            evaluate_unary_op(op, &val)
        }

        Expression::FunctionCall { name, args, kwargs } => {
            let evaluated_args: Result<Vec<_>, _> =
                args.iter().map(|a| evaluate_expression(a, ctx)).collect();
            let evaluated_kwargs: Result<HashMap<_, _>, _> = kwargs
                .iter()
                .map(|(k, v)| evaluate_expression(v, ctx).map(|val| (k.clone(), val)))
                .collect();

            call_builtin_with_context(name, evaluated_args?, evaluated_kwargs?, ctx)
        }

        Expression::MethodCall {
            object,
            method,
            args,
            kwargs,
        } => {
            let obj_val = evaluate_expression(object, ctx)?;
            let evaluated_args: Result<Vec<_>, _> =
                args.iter().map(|a| evaluate_expression(a, ctx)).collect();
            let evaluated_kwargs: Result<HashMap<_, _>, _> = kwargs
                .iter()
                .map(|(k, v)| evaluate_expression(v, ctx).map(|val| (k.clone(), val)))
                .collect();

            call_method(&obj_val, method, evaluated_args?, evaluated_kwargs?)
        }

        Expression::Index { object, index } => {
            let obj_val = evaluate_expression(object, ctx)?;
            let idx_val = evaluate_expression(index, ctx)?;

            match (&obj_val, &idx_val) {
                (Value::List(list), Value::Int(i)) => {
                    // Handle negative indices Python-style
                    let idx = if *i < 0 {
                        let adjusted = list.len() as i64 + i;
                        if adjusted < 0 {
                            return Err(NexusError::Runtime {
                                function: None,
                                message: format!(
                                    "Index {} out of bounds for list of length {}",
                                    i,
                                    list.len()
                                ),
                                suggestion: None,
                            });
                        }
                        adjusted as usize
                    } else {
                        *i as usize
                    };
                    list.get(idx).cloned().ok_or_else(|| NexusError::Runtime {
                        function: None,
                        message: format!(
                            "Index {} out of bounds for list of length {}",
                            i,
                            list.len()
                        ),
                        suggestion: None,
                    })
                }
                (Value::Dict(map), Value::String(key)) => {
                    map.get(key).cloned().ok_or_else(|| NexusError::Runtime {
                        function: None,
                        message: format!("Key '{}' not found in dict", key),
                        suggestion: None,
                    })
                }
                (Value::String(s), Value::Int(i)) => {
                    let char_count = s.chars().count();
                    // Handle negative indices Python-style
                    let idx = if *i < 0 {
                        let adjusted = char_count as i64 + i;
                        if adjusted < 0 {
                            return Err(NexusError::Runtime {
                                function: None,
                                message: format!(
                                    "Index {} out of bounds for string of length {}",
                                    i, char_count
                                ),
                                suggestion: None,
                            });
                        }
                        adjusted as usize
                    } else {
                        *i as usize
                    };
                    s.chars()
                        .nth(idx)
                        .map(|c| Value::String(c.to_string()))
                        .ok_or_else(|| NexusError::Runtime {
                            function: None,
                            message: format!(
                                "Index {} out of bounds for string of length {}",
                                i, char_count
                            ),
                            suggestion: None,
                        })
                }
                _ => Err(NexusError::Runtime {
                    function: None,
                    message: format!("Cannot index {:?} with {:?}", obj_val, idx_val),
                    suggestion: None,
                }),
            }
        }

        Expression::Attribute { object, attr } => {
            let obj_val = evaluate_expression(object, ctx)?;

            match &obj_val {
                Value::Dict(map) => map.get(attr).cloned().ok_or_else(|| NexusError::Runtime {
                    function: None,
                    message: format!("Attribute '{}' not found", attr),
                    suggestion: None,
                }),
                _ => Err(NexusError::Runtime {
                    function: None,
                    message: format!("Cannot access attribute '{}' on {:?}", attr, obj_val),
                    suggestion: None,
                }),
            }
        }

        Expression::List(items) => {
            let values: Result<Vec<_>, _> =
                items.iter().map(|i| evaluate_expression(i, ctx)).collect();
            Ok(Value::List(values?))
        }

        Expression::Dict(items) => {
            let mut map = HashMap::new();
            for (k, v) in items {
                let key = evaluate_expression(k, ctx)?;
                let value = evaluate_expression(v, ctx)?;
                let key_str = match key {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                map.insert(key_str, value);
            }
            Ok(Value::Dict(map))
        }

        Expression::Filter {
            input,
            filter_name,
            predicate,
        } => {
            let input_val = evaluate_expression(input, ctx)?;
            apply_filter(&input_val, filter_name, predicate.as_deref(), ctx)
        }

        Expression::Lambda { params: _, body: _ } => {
            // Lambdas are evaluated when called, not when defined
            // For now, we'll return null as we don't have closures
            Err(NexusError::Runtime {
                function: None,
                message: "Lambda expressions not fully supported yet".to_string(),
                suggestion: None,
            })
        }

        Expression::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            let cond_val = evaluate_expression(condition, ctx)?;
            if cond_val.is_truthy() {
                evaluate_expression(then_expr, ctx)
            } else {
                evaluate_expression(else_expr, ctx)
            }
        }
    }
}

fn evaluate_binary_op(
    left: &Value,
    op: &BinaryOperator,
    right: &Value,
) -> Result<Value, NexusError> {
    match op {
        // Arithmetic operations
        BinaryOperator::Add => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            (Value::List(a), Value::List(b)) => {
                let mut result = a.clone();
                result.extend(b.clone());
                Ok(Value::List(result))
            }
            _ => Err(type_error("add", left, right)),
        },
        BinaryOperator::Sub => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            _ => Err(type_error("subtract", left, right)),
        },
        BinaryOperator::Mul => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
            (Value::String(s), Value::Int(n)) | (Value::Int(n), Value::String(s)) => {
                if *n < 0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Cannot multiply string by negative number".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::String(s.repeat(*n as usize)))
                }
            }
            _ => Err(type_error("multiply", left, right)),
        },
        BinaryOperator::Div => match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Division by zero".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::Int(a / b))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Division by zero".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Division by zero".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::Float(*a as f64 / b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Division by zero".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::Float(a / *b as f64))
                }
            }
            _ => Err(type_error("divide", left, right)),
        },
        BinaryOperator::Mod => match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "Modulo by zero".to_string(),
                        suggestion: None,
                    })
                } else {
                    Ok(Value::Int(a % b))
                }
            }
            _ => Err(type_error("modulo", left, right)),
        },

        // Comparison operations
        BinaryOperator::Eq => Ok(Value::Bool(values_equal(left, right))),
        BinaryOperator::Ne => Ok(Value::Bool(!values_equal(left, right))),
        BinaryOperator::Lt => compare_values(left, right, |ord| ord == std::cmp::Ordering::Less),
        BinaryOperator::Le => compare_values(left, right, |ord| ord != std::cmp::Ordering::Greater),
        BinaryOperator::Gt => compare_values(left, right, |ord| ord == std::cmp::Ordering::Greater),
        BinaryOperator::Ge => compare_values(left, right, |ord| ord != std::cmp::Ordering::Less),

        // Logical operations
        BinaryOperator::And => Ok(Value::Bool(left.is_truthy() && right.is_truthy())),
        BinaryOperator::Or => Ok(Value::Bool(left.is_truthy() || right.is_truthy())),

        // Membership
        BinaryOperator::In => {
            let contained = match right {
                Value::List(list) => list.iter().any(|v| values_equal(left, v)),
                Value::String(s) => match left {
                    Value::String(sub) => s.contains(sub.as_str()),
                    _ => false,
                },
                Value::Dict(map) => match left {
                    Value::String(key) => map.contains_key(key),
                    _ => false,
                },
                _ => false,
            };
            Ok(Value::Bool(contained))
        }
        BinaryOperator::NotIn => {
            let result = evaluate_binary_op(left, &BinaryOperator::In, right)?;
            match result {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                _ => unreachable!(),
            }
        }
    }
}

fn evaluate_unary_op(op: &UnaryOperator, val: &Value) -> Result<Value, NexusError> {
    match op {
        UnaryOperator::Not => Ok(Value::Bool(!val.is_truthy())),
        UnaryOperator::Neg => match val {
            Value::Int(i) => Ok(Value::Int(-i)),
            Value::Float(f) => Ok(Value::Float(-f)),
            _ => Err(NexusError::Runtime {
                function: None,
                message: format!("Cannot negate {:?}", val),
                suggestion: None,
            }),
        },
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => (a - b).abs() < f64::EPSILON,
        (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
            (*a as f64 - b).abs() < f64::EPSILON
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (Value::Dict(a), Value::Dict(b)) => {
            a.len() == b.len()
                && a.keys().all(|k| match (a.get(k), b.get(k)) {
                    (Some(av), Some(bv)) => values_equal(av, bv),
                    _ => false,
                })
        }
        _ => false,
    }
}

fn compare_values<F>(left: &Value, right: &Value, f: F) -> Result<Value, NexusError>
where
    F: Fn(std::cmp::Ordering) -> bool,
{
    let ord = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Int(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Float(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        _ => {
            return Err(NexusError::Runtime {
                function: None,
                message: format!("Cannot compare {:?} and {:?}", left, right),
                suggestion: None,
            })
        }
    };

    Ok(Value::Bool(f(ord)))
}

fn type_error(op: &str, left: &Value, right: &Value) -> NexusError {
    NexusError::Runtime {
        function: None,
        message: format!(
            "Cannot {} {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ),
        suggestion: None,
    }
}

fn type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::Dict(_) => "dict",
    }
}
