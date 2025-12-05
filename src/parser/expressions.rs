// Expression parser for ${...} substitutions and conditions

use pest::Parser;
use pest_derive::Parser;
use std::collections::HashMap;

use super::ast::{BinaryOperator, Expression, StringPart, UnaryOperator};
use crate::output::errors::{NexusError, ParseError, ParseErrorKind};

#[derive(Parser)]
#[grammar = "parser/expressions.pest"]
pub struct ExpressionParser;

/// Parse an expression string (without ${} delimiters)
pub fn parse_expression(input: &str) -> Result<Expression, NexusError> {
    let pairs = ExpressionParser::parse(Rule::expression, input).map_err(|e| {
        NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::InvalidExpression,
            message: format!("Failed to parse expression: {}", e),
            file: None,
            line: None,
            column: None,
            suggestion: Some("Check expression syntax".to_string()),
        }))
    })?;

    let pair = pairs.into_iter().next().unwrap();
    parse_or_expr(pair.into_inner().next().unwrap())
}

/// Parse an interpolated string containing ${...} expressions
pub fn parse_interpolated_string(input: &str) -> Result<Expression, NexusError> {
    let mut parts = Vec::new();
    let mut current_literal = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            // Save any accumulated literal
            if !current_literal.is_empty() {
                parts.push(StringPart::Literal(std::mem::take(&mut current_literal)));
            }

            // Skip the '{'
            chars.next();

            // Find matching '}'
            let mut depth = 1;
            let mut expr_str = String::new();
            for c in chars.by_ref() {
                match c {
                    '{' => {
                        depth += 1;
                        expr_str.push(c);
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        expr_str.push(c);
                    }
                    _ => expr_str.push(c),
                }
            }

            // Parse the expression
            let expr = parse_expression(&expr_str)?;
            parts.push(StringPart::Expression(expr));
        } else if c == '\\' && chars.peek() == Some(&'$') {
            // Escaped $
            chars.next();
            current_literal.push('$');
        } else {
            current_literal.push(c);
        }
    }

    // Save any remaining literal
    if !current_literal.is_empty() {
        parts.push(StringPart::Literal(current_literal));
    }

    // Optimize: if only one literal part, return a plain string
    if parts.len() == 1 {
        if let StringPart::Literal(s) = &parts[0] {
            return Ok(Expression::String(s.clone()));
        }
    }

    // If no interpolation found, return plain string
    if parts.is_empty() {
        return Ok(Expression::String(String::new()));
    }

    Ok(Expression::InterpolatedString(parts))
}

/// Check if a string contains any ${...} interpolation
pub fn has_interpolation(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            return true;
        }
        if c == '\\' {
            chars.next(); // Skip escaped char
        }
    }
    false
}

fn parse_or_expr(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut left = parse_and_expr(inner.next().unwrap())?;

    for pair in inner {
        let right = parse_and_expr(pair)?;
        left = Expression::BinaryOp {
            left: Box::new(left),
            op: BinaryOperator::Or,
            right: Box::new(right),
        };
    }

    Ok(left)
}

fn parse_and_expr(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut left = parse_not_expr(inner.next().unwrap())?;

    for pair in inner {
        let right = parse_not_expr(pair)?;
        left = Expression::BinaryOp {
            left: Box::new(left),
            op: BinaryOperator::And,
            right: Box::new(right),
        };
    }

    Ok(left)
}

fn parse_not_expr(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let first = inner.next().unwrap();

    match first.as_rule() {
        Rule::not_expr => {
            let operand = parse_not_expr(first)?;
            Ok(Expression::UnaryOp {
                op: UnaryOperator::Not,
                operand: Box::new(operand),
            })
        }
        Rule::comparison => parse_comparison(first),
        _ => unreachable!("Unexpected rule in not_expr: {:?}", first.as_rule()),
    }
}

fn parse_comparison(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut left = parse_additive(inner.next().unwrap())?;

    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "==" => BinaryOperator::Eq,
            "!=" => BinaryOperator::Ne,
            "<" => BinaryOperator::Lt,
            "<=" => BinaryOperator::Le,
            ">" => BinaryOperator::Gt,
            ">=" => BinaryOperator::Ge,
            "in" => BinaryOperator::In,
            s if s.contains("not") && s.contains("in") => BinaryOperator::NotIn,
            _ => unreachable!("Unknown comparison operator: {}", op_pair.as_str()),
        };

        let right = parse_additive(inner.next().unwrap())?;
        left = Expression::BinaryOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }

    Ok(left)
}

fn parse_additive(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut left = parse_multiplicative(inner.next().unwrap())?;

    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "+" => BinaryOperator::Add,
            "-" => BinaryOperator::Sub,
            _ => unreachable!(),
        };

        let right = parse_multiplicative(inner.next().unwrap())?;
        left = Expression::BinaryOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }

    Ok(left)
}

fn parse_multiplicative(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut left = parse_unary(inner.next().unwrap())?;

    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "*" => BinaryOperator::Mul,
            "/" => BinaryOperator::Div,
            "%" => BinaryOperator::Mod,
            _ => unreachable!(),
        };

        let right = parse_unary(inner.next().unwrap())?;
        left = Expression::BinaryOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }

    Ok(left)
}

fn parse_unary(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let first = inner.next().unwrap();

    match first.as_rule() {
        Rule::unary_op => {
            let op = match first.as_str() {
                "-" => UnaryOperator::Neg,
                "!" => UnaryOperator::Not,
                _ => unreachable!(),
            };
            let operand = parse_unary(inner.next().unwrap())?;
            Ok(Expression::UnaryOp {
                op,
                operand: Box::new(operand),
            })
        }
        Rule::postfix => parse_postfix(first),
        _ => unreachable!("Unexpected rule in unary: {:?}", first.as_rule()),
    }
}

fn parse_postfix(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let mut expr = parse_primary(inner.next().unwrap())?;

    for op in inner {
        // Unwrap postfix_op if present
        let actual_op = if op.as_rule() == Rule::postfix_op {
            op.into_inner().next().unwrap()
        } else {
            op
        };

        expr = match actual_op.as_rule() {
            Rule::call => {
                let args = parse_args(actual_op)?;
                // Convert to function call
                match expr {
                    Expression::Variable(path) if path.len() == 1 => Expression::FunctionCall {
                        name: path.into_iter().next().unwrap(),
                        args: args.0,
                        kwargs: args.1,
                    },
                    Expression::Attribute { object, attr } => Expression::MethodCall {
                        object,
                        method: attr,
                        args: args.0,
                        kwargs: args.1,
                    },
                    _ => {
                        return Err(NexusError::Parse(Box::new(ParseError {
                            kind: ParseErrorKind::InvalidExpression,
                            message: "Cannot call non-function expression".to_string(),
                            file: None,
                            line: None,
                            column: None,
                            suggestion: None,
                        })))
                    }
                }
            }
            Rule::index => {
                let index_expr = parse_or_expr(actual_op.into_inner().next().unwrap())?;
                Expression::Index {
                    object: Box::new(expr),
                    index: Box::new(index_expr),
                }
            }
            Rule::attribute => {
                let attr = actual_op.into_inner().next().unwrap().as_str().to_string();
                Expression::Attribute {
                    object: Box::new(expr),
                    attr,
                }
            }
            rule => unreachable!("Unexpected postfix rule: {:?}", rule),
        };
    }

    Ok(expr)
}

fn parse_args(
    pair: pest::iterators::Pair<Rule>,
) -> Result<(Vec<Expression>, HashMap<String, Expression>), NexusError> {
    let mut positional = Vec::new();
    let mut kwargs = HashMap::new();

    for arg in pair.into_inner() {
        if arg.as_rule() == Rule::args {
            for inner_arg in arg.into_inner() {
                match inner_arg.as_rule() {
                    Rule::kwarg => {
                        let mut kw_inner = inner_arg.into_inner();
                        let name = kw_inner.next().unwrap().as_str().to_string();
                        let value = parse_or_expr(kw_inner.next().unwrap())?;
                        kwargs.insert(name, value);
                    }
                    Rule::arg => {
                        let expr = parse_or_expr(inner_arg.into_inner().next().unwrap())?;
                        positional.push(expr);
                    }
                    _ => {
                        let expr = parse_or_expr(inner_arg)?;
                        positional.push(expr);
                    }
                }
            }
        }
    }

    Ok((positional, kwargs))
}

fn parse_primary(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let inner = pair.into_inner().next().unwrap();

    match inner.as_rule() {
        Rule::or_expr => parse_or_expr(inner),
        Rule::lambda => parse_lambda(inner),
        Rule::list_literal => parse_list(inner),
        Rule::dict_literal => parse_dict(inner),
        Rule::float_literal => {
            let f: f64 = inner.as_str().parse().unwrap();
            Ok(Expression::Float(f))
        }
        Rule::int_literal => {
            let i: i64 = inner.as_str().parse().unwrap();
            Ok(Expression::Integer(i))
        }
        Rule::bool_literal => {
            let b = inner.as_str() == "true";
            Ok(Expression::Boolean(b))
        }
        Rule::null_literal => Ok(Expression::Null),
        Rule::string_literal => {
            let s = parse_string_literal(inner);
            Ok(Expression::String(s))
        }
        Rule::variable => {
            let parts: Vec<String> = inner.into_inner().map(|p| p.as_str().to_string()).collect();
            Ok(Expression::Variable(parts))
        }
        _ => unreachable!("Unexpected primary rule: {:?}", inner.as_rule()),
    }
}

fn parse_string_literal(pair: pest::iterators::Pair<Rule>) -> String {
    let inner = pair.into_inner().next().unwrap();
    let raw = inner.as_str();

    // Process escape sequences
    let mut result = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some('$') => result.push('$'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn parse_lambda(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut inner = pair.into_inner();
    let params_pair = inner.next().unwrap();
    let body_pair = inner.next().unwrap();

    let params: Vec<String> = params_pair
        .into_inner()
        .map(|p| p.as_str().to_string())
        .collect();

    let body = parse_or_expr(body_pair)?;

    Ok(Expression::Lambda {
        params,
        body: Box::new(body),
    })
}

fn parse_list(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let items: Result<Vec<_>, _> = pair.into_inner().map(parse_or_expr).collect();
    Ok(Expression::List(items?))
}

fn parse_dict(pair: pest::iterators::Pair<Rule>) -> Result<Expression, NexusError> {
    let mut entries = Vec::new();

    for entry in pair.into_inner() {
        let mut inner = entry.into_inner();
        let key_pair = inner.next().unwrap();
        let value = parse_or_expr(inner.next().unwrap())?;

        let key = match key_pair.as_rule() {
            Rule::string_literal => Expression::String(parse_string_literal(key_pair)),
            Rule::ident => Expression::String(key_pair.as_str().to_string()),
            _ => unreachable!(),
        };

        entries.push((key, value));
    }

    Ok(Expression::Dict(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_expressions() {
        assert!(matches!(
            parse_expression("42").unwrap(),
            Expression::Integer(42)
        ));
        assert!(matches!(
            parse_expression("true").unwrap(),
            Expression::Boolean(true)
        ));
        assert!(matches!(
            parse_expression("null").unwrap(),
            Expression::Null
        ));
    }

    #[test]
    fn test_variables() {
        // The grammar parses host.name as a single variable with dotted path
        let expr = parse_expression("host").unwrap();
        if let Expression::Variable(path) = expr {
            assert_eq!(path, vec!["host"]);
        } else {
            panic!("Expected Variable, got {:?}", expr);
        }
    }

    #[test]
    fn test_comparison() {
        let expr = parse_expression("x > 5").unwrap();
        if let Expression::BinaryOp { op, .. } = expr {
            assert_eq!(op, BinaryOperator::Gt);
        } else {
            panic!("Expected BinaryOp");
        }
    }

    #[test]
    fn test_interpolation() {
        let expr = parse_interpolated_string("Hello ${name}!").unwrap();
        if let Expression::InterpolatedString(parts) = expr {
            assert_eq!(parts.len(), 3);
        } else {
            panic!("Expected InterpolatedString");
        }
    }

    #[test]
    fn test_has_interpolation() {
        assert!(has_interpolation("Hello ${name}"));
        assert!(!has_interpolation("Hello name"));
        assert!(!has_interpolation("Hello \\${name}"));
    }
}
