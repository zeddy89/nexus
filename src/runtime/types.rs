// Type system utilities for the Nexus runtime

use crate::parser::ast::Value;

/// Check if a value can be converted to a specific type
pub fn can_convert(value: &Value, target_type: &str) -> bool {
    match target_type {
        "string" => true, // Everything can be stringified
        "int" => {
            matches!(value, Value::Int(_) | Value::Float(_) | Value::Bool(_))
                || matches!(value, Value::String(s) if s.parse::<i64>().is_ok())
        }
        "float" => {
            matches!(value, Value::Int(_) | Value::Float(_))
                || matches!(value, Value::String(s) if s.parse::<f64>().is_ok())
        }
        "bool" => true, // Everything has truthiness
        "list" => matches!(value, Value::List(_) | Value::String(_)),
        "dict" => matches!(value, Value::Dict(_) | Value::List(_)),
        _ => false,
    }
}

/// Get the type name of a value
pub fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::Dict(_) => "dict",
    }
}

/// Check if two values have compatible types for an operation
pub fn types_compatible(a: &Value, b: &Value, op: &str) -> bool {
    match op {
        "+" => {
            matches!(
                (a, b),
                (Value::Int(_), Value::Int(_))
                    | (Value::Float(_), Value::Float(_))
                    | (Value::Int(_), Value::Float(_))
                    | (Value::Float(_), Value::Int(_))
                    | (Value::String(_), Value::String(_))
                    | (Value::List(_), Value::List(_))
            )
        }
        "-" | "*" | "/" | "%" => {
            matches!(
                (a, b),
                (Value::Int(_), Value::Int(_))
                    | (Value::Float(_), Value::Float(_))
                    | (Value::Int(_), Value::Float(_))
                    | (Value::Float(_), Value::Int(_))
            )
        }
        "==" | "!=" => true, // Can compare anything
        "<" | "<=" | ">" | ">=" => {
            matches!(
                (a, b),
                (Value::Int(_), Value::Int(_))
                    | (Value::Float(_), Value::Float(_))
                    | (Value::Int(_), Value::Float(_))
                    | (Value::Float(_), Value::Int(_))
                    | (Value::String(_), Value::String(_))
            )
        }
        "and" | "or" => true, // Logical ops work on truthiness
        "in" => {
            matches!(b, Value::List(_) | Value::String(_) | Value::Dict(_))
        }
        _ => false,
    }
}

/// Coerce a value to int if possible
pub fn coerce_to_int(value: &Value) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::Float(f) => Some(*f as i64),
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Coerce a value to float if possible
pub fn coerce_to_float(value: &Value) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Coerce a value to string
pub fn coerce_to_string(value: &Value) -> String {
    value.to_string()
}

/// Coerce a value to bool (truthiness)
pub fn coerce_to_bool(value: &Value) -> bool {
    value.is_truthy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_of() {
        assert_eq!(type_of(&Value::Null), "null");
        assert_eq!(type_of(&Value::Bool(true)), "bool");
        assert_eq!(type_of(&Value::Int(42)), "int");
        assert_eq!(type_of(&Value::Float(3.14)), "float");
        assert_eq!(type_of(&Value::String("hello".to_string())), "string");
        assert_eq!(type_of(&Value::List(vec![])), "list");
        assert_eq!(
            type_of(&Value::Dict(std::collections::HashMap::new())),
            "dict"
        );
    }

    #[test]
    fn test_coerce_to_int() {
        assert_eq!(coerce_to_int(&Value::Int(42)), Some(42));
        assert_eq!(coerce_to_int(&Value::Float(3.7)), Some(3));
        assert_eq!(coerce_to_int(&Value::Bool(true)), Some(1));
        assert_eq!(coerce_to_int(&Value::String("123".to_string())), Some(123));
        assert_eq!(coerce_to_int(&Value::String("abc".to_string())), None);
    }

    #[test]
    fn test_types_compatible() {
        assert!(types_compatible(&Value::Int(1), &Value::Int(2), "+"));
        assert!(types_compatible(
            &Value::String("a".to_string()),
            &Value::String("b".to_string()),
            "+"
        ));
        assert!(!types_compatible(
            &Value::Int(1),
            &Value::String("a".to_string()),
            "+"
        ));
        assert!(types_compatible(
            &Value::Int(1),
            &Value::String("a".to_string()),
            "=="
        ));
    }
}
