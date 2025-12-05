// Function interpreter for Nexus scripts

use std::collections::HashMap;

use crate::executor::ExecutionContext;
use crate::output::errors::NexusError;
use crate::parser::ast::{Expression, FunctionBlock, FunctionDef, Statement, Value};
use crate::runtime::evaluate_expression;

/// Result of executing a function
#[derive(Debug)]
pub enum FunctionResult {
    Value(Value),
    Return(Value),
    Break,
    Continue,
    Skip,
}

/// Interpreter for function blocks
pub struct Interpreter {
    functions: HashMap<String, FunctionDef>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            functions: HashMap::new(),
        }
    }

    /// Load functions from a function block
    pub fn load_functions(&mut self, block: &FunctionBlock) {
        for func in &block.functions {
            self.functions.insert(func.name.clone(), func.clone());
        }
    }

    /// Call a function by name
    pub fn call_function(
        &self,
        name: &str,
        args: Vec<Value>,
        ctx: &ExecutionContext,
    ) -> Result<Value, NexusError> {
        let func = self
            .functions
            .get(name)
            .ok_or_else(|| NexusError::Runtime {
                function: Some(name.to_string()),
                message: format!("Function '{}' not found", name),
                suggestion: Some(
                    "Check function name and ensure it's defined in the functions block"
                        .to_string(),
                ),
            })?;

        // Create local scope with parameters
        let mut local_vars = HashMap::new();

        // Bind arguments to parameters
        for (i, param) in func.params.iter().enumerate() {
            let value = if i < args.len() {
                args[i].clone()
            } else if let Some(ref default) = param.default {
                evaluate_expression(default, ctx)?
            } else {
                return Err(NexusError::Runtime {
                    function: Some(name.to_string()),
                    message: format!("Missing required argument: {}", param.name),
                    suggestion: None,
                });
            };
            local_vars.insert(param.name.clone(), value);
        }

        // Execute function body
        let result = self.execute_block(&func.body, ctx, &mut local_vars)?;

        match result {
            FunctionResult::Value(v) | FunctionResult::Return(v) => Ok(v),
            FunctionResult::Skip => Ok(Value::Null), // Special skip() result
            _ => Ok(Value::Null),
        }
    }

    /// Execute a block of statements
    fn execute_block(
        &self,
        statements: &[Statement],
        ctx: &ExecutionContext,
        local_vars: &mut HashMap<String, Value>,
    ) -> Result<FunctionResult, NexusError> {
        for stmt in statements {
            match self.execute_statement(stmt, ctx, local_vars)? {
                FunctionResult::Value(_) => continue,
                result => return Ok(result),
            }
        }
        Ok(FunctionResult::Value(Value::Null))
    }

    /// Execute a single statement
    fn execute_statement(
        &self,
        stmt: &Statement,
        ctx: &ExecutionContext,
        local_vars: &mut HashMap<String, Value>,
    ) -> Result<FunctionResult, NexusError> {
        match stmt {
            Statement::Assign { target, value } => {
                let val = self.eval_expr(value, ctx, local_vars)?;
                local_vars.insert(target.clone(), val);
                Ok(FunctionResult::Value(Value::Null))
            }

            Statement::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                let cond_val = self.eval_expr(condition, ctx, local_vars)?;

                if cond_val.is_truthy() {
                    return self.execute_block(then_body, ctx, local_vars);
                }

                for (elif_cond, elif_body) in elif_clauses {
                    let elif_val = self.eval_expr(elif_cond, ctx, local_vars)?;
                    if elif_val.is_truthy() {
                        return self.execute_block(elif_body, ctx, local_vars);
                    }
                }

                if let Some(else_body) = else_body {
                    return self.execute_block(else_body, ctx, local_vars);
                }

                Ok(FunctionResult::Value(Value::Null))
            }

            Statement::For { var, iter, body } => {
                let iter_val = self.eval_expr(iter, ctx, local_vars)?;

                let items = match iter_val {
                    Value::List(l) => l,
                    Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                    _ => {
                        return Err(NexusError::Runtime {
                            function: None,
                            message: "Cannot iterate over non-iterable value".to_string(),
                            suggestion: None,
                        })
                    }
                };

                for item in items {
                    local_vars.insert(var.clone(), item);

                    match self.execute_block(body, ctx, local_vars)? {
                        FunctionResult::Break => break,
                        FunctionResult::Continue => continue,
                        FunctionResult::Return(v) => return Ok(FunctionResult::Return(v)),
                        FunctionResult::Skip => return Ok(FunctionResult::Skip),
                        FunctionResult::Value(_) => {}
                    }
                }

                Ok(FunctionResult::Value(Value::Null))
            }

            Statement::While { condition, body } => {
                loop {
                    let cond_val = self.eval_expr(condition, ctx, local_vars)?;
                    if !cond_val.is_truthy() {
                        break;
                    }

                    match self.execute_block(body, ctx, local_vars)? {
                        FunctionResult::Break => break,
                        FunctionResult::Continue => continue,
                        FunctionResult::Return(v) => return Ok(FunctionResult::Return(v)),
                        FunctionResult::Skip => return Ok(FunctionResult::Skip),
                        FunctionResult::Value(_) => {}
                    }
                }

                Ok(FunctionResult::Value(Value::Null))
            }

            Statement::Try {
                try_body,
                except_clauses,
            } => {
                match self.execute_block(try_body, ctx, local_vars) {
                    Ok(result) => Ok(result),
                    Err(e) => {
                        // Find matching except clause
                        if let Some((_exc_type, exc_var, except_body)) =
                            except_clauses.iter().next()
                        {
                            // For now, catch all exceptions
                            if let Some(var) = exc_var {
                                local_vars.insert(var.clone(), Value::String(e.to_string()));
                            }
                            return self.execute_block(except_body, ctx, local_vars);
                        }
                        Err(e)
                    }
                }
            }

            Statement::Return(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expr(e, ctx, local_vars)?
                } else {
                    Value::Null
                };
                Ok(FunctionResult::Return(val))
            }

            Statement::Expression(expr) => {
                // Check for special function calls
                if let Expression::FunctionCall { name, .. } = expr {
                    if name == "skip" {
                        return Ok(FunctionResult::Skip);
                    }
                }

                let val = self.eval_expr(expr, ctx, local_vars)?;
                Ok(FunctionResult::Value(val))
            }

            Statement::Break => Ok(FunctionResult::Break),
            Statement::Continue => Ok(FunctionResult::Continue),
        }
    }

    /// Evaluate an expression with local variables
    fn eval_expr(
        &self,
        expr: &Expression,
        ctx: &ExecutionContext,
        local_vars: &HashMap<String, Value>,
    ) -> Result<Value, NexusError> {
        // Check local variables first for simple variable references
        if let Expression::Variable(path) = expr {
            if path.len() == 1 {
                if let Some(val) = local_vars.get(&path[0]) {
                    return Ok(val.clone());
                }
            }
        }

        // Fall back to context evaluation
        evaluate_expression(expr, ctx)
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Host;
    use std::sync::Arc;

    fn create_test_context() -> ExecutionContext {
        let host = Host::new("test-host");
        ExecutionContext::new(Arc::new(host), HashMap::new())
    }

    #[test]
    fn test_interpreter_creation() {
        let interp = Interpreter::new();
        assert!(interp.functions.is_empty());
    }
}
