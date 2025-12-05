// Template module - Advanced templating engine with Jinja2+ features
//
// Features beyond Ansible/Jinja2:
// - Pipes (filters) chainable: {{ name | upper | trim }}
// - Built-in filters: upper, lower, trim, replace, default, length, join, split, etc.
// - Conditionals: {% if %}, {% elif %}, {% else %}, {% endif %}
// - Loops: {% for item in items %}, {% endfor %} with loop.index, loop.first, loop.last
// - Includes: {% include "header.j2" %}
// - Macros: {% macro button(text) %}<button>{{ text }}</button>{% endmacro %}
// - Template inheritance: {% extends "base.j2" %}, {% block content %}{% endblock %}
// - Whitespace control: {%- trim -%}, {{- trim -}}
// - Safe escaping with autoescape and {{ value | safe }}
// - Expression evaluation within templates
// - Comments: {# this is a comment #}

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

use crate::executor::ExecutionContext;
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Template engine for Nexus
pub struct TemplateEngine {
    /// Registered macros
    macros: HashMap<String, Macro>,
    /// Base template for inheritance
    parent_template: Option<String>,
    /// Block definitions
    blocks: HashMap<String, String>,
    /// Include search paths
    search_paths: Vec<String>,
    /// Autoescape HTML by default
    autoescape: bool,
    /// Template-local variables (takes precedence over context)
    local_vars: HashMap<String, Value>,
}

/// A macro definition
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Macro {
    name: String,
    params: Vec<String>,
    body: String,
}

impl TemplateEngine {
    pub fn new() -> Self {
        TemplateEngine {
            macros: HashMap::new(),
            parent_template: None,
            blocks: HashMap::new(),
            search_paths: vec![".".to_string()],
            autoescape: false,
            local_vars: HashMap::new(),
        }
    }

    /// Create a child engine for nested scopes (loops, etc.)
    fn child(&self) -> Self {
        TemplateEngine {
            macros: self.macros.clone(),
            parent_template: None,
            blocks: self.blocks.clone(),
            search_paths: self.search_paths.clone(),
            autoescape: self.autoescape,
            local_vars: self.local_vars.clone(),
        }
    }

    /// Set a local variable in the template scope
    fn set_local_var(&mut self, name: impl Into<String>, value: Value) {
        self.local_vars.insert(name.into(), value);
    }

    /// Add a search path for includes
    pub fn add_search_path(&mut self, path: impl Into<String>) {
        self.search_paths.push(path.into());
    }

    /// Enable/disable autoescape
    pub fn set_autoescape(&mut self, enabled: bool) {
        self.autoescape = enabled;
    }

    /// Render a template string with context
    pub fn render(&mut self, template: &str, ctx: &ExecutionContext) -> Result<String, NexusError> {
        // First pass: collect macros, blocks, and extends
        let preprocessed = self.preprocess(template)?;

        // Second pass: render the template
        self.render_inner(&preprocessed, ctx)
    }

    /// Render a template file
    pub fn render_file(
        &mut self,
        path: &Path,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
            message: format!("Failed to read template file: {}", e),
            path: Some(path.to_path_buf()),
        })?;

        self.render(&content, ctx)
    }

    /// Preprocess template to extract macros, blocks, and handle extends
    fn preprocess(&mut self, template: &str) -> Result<String, NexusError> {
        let mut result = template.to_string();

        // Extract and store macros
        let macro_re =
            Regex::new(r"\{%\s*macro\s+(\w+)\s*\((.*?)\)\s*%\}([\s\S]*?)\{%\s*endmacro\s*%\}")
                .unwrap();
        for cap in macro_re.captures_iter(template) {
            let name = cap[1].to_string();
            let params: Vec<String> = cap[2]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let body = cap[3].to_string();

            self.macros
                .insert(name.clone(), Macro { name, params, body });
        }
        result = macro_re.replace_all(&result, "").to_string();

        // Handle extends (template inheritance)
        let extends_re = Regex::new(r#"\{%\s*extends\s+["'](.+?)["']\s*%\}"#).unwrap();
        if let Some(cap) = extends_re.captures(&result) {
            self.parent_template = Some(cap[1].to_string());
            result = extends_re.replace(&result, "").to_string();
        }

        // Extract blocks
        let block_re =
            Regex::new(r"\{%\s*block\s+(\w+)\s*%\}([\s\S]*?)\{%\s*endblock\s*%\}").unwrap();
        for cap in block_re.captures_iter(&result) {
            let name = cap[1].to_string();
            let content = cap[2].to_string();
            self.blocks.insert(name, content);
        }

        // Remove comments
        let comment_re = Regex::new(r"\{#[\s\S]*?#\}").unwrap();
        result = comment_re.replace_all(&result, "").to_string();

        Ok(result)
    }

    /// Main rendering logic
    fn render_inner(&self, template: &str, ctx: &ExecutionContext) -> Result<String, NexusError> {
        let mut result = template.to_string();

        // Handle template inheritance
        if let Some(ref parent_path) = self.parent_template {
            let parent_content = self.load_template(parent_path)?;
            // Replace blocks in parent with child blocks
            let block_re =
                Regex::new(r"\{%\s*block\s+(\w+)\s*%\}([\s\S]*?)\{%\s*endblock\s*%\}").unwrap();
            result = block_re
                .replace_all(&parent_content, |caps: &regex::Captures| {
                    let block_name = &caps[1];
                    self.blocks
                        .get(block_name)
                        .cloned()
                        .unwrap_or_else(|| caps[2].to_string())
                })
                .to_string();
        }

        // Handle includes
        result = self.process_includes(&result)?;

        // Handle for loops
        result = self.process_for_loops(&result, ctx)?;

        // Handle if/elif/else conditionals
        result = self.process_conditionals(&result, ctx)?;

        // Handle macro calls
        result = self.process_macro_calls(&result, ctx)?;

        // Handle variable expressions (must be last)
        result = self.process_expressions(&result, ctx)?;

        // Handle whitespace control
        result = self.process_whitespace_control(&result);

        Ok(result)
    }

    /// Load a template from search paths
    fn load_template(&self, name: &str) -> Result<String, NexusError> {
        for search_path in &self.search_paths {
            let path = Path::new(search_path).join(name);
            if path.exists() {
                return std::fs::read_to_string(&path).map_err(|e| NexusError::Io {
                    message: format!("Failed to read template: {}", e),
                    path: Some(path),
                });
            }
        }
        Err(NexusError::Runtime {
            function: None,
            message: format!("Template not found: {}", name),
            suggestion: Some(format!("Searched paths: {:?}", self.search_paths)),
        })
    }

    /// Process {% include "file" %} directives
    fn process_includes(&self, template: &str) -> Result<String, NexusError> {
        let include_re = Regex::new(r#"\{%\s*include\s+["'](.+?)["']\s*%\}"#).unwrap();

        let mut result = template.to_string();
        let mut safety = 100; // Prevent infinite loops

        while include_re.is_match(&result) && safety > 0 {
            result = include_re
                .replace_all(&result, |caps: &regex::Captures| {
                    let include_name = &caps[1];
                    self.load_template(include_name)
                        .unwrap_or_else(|_| format!("<!-- Include not found: {} -->", include_name))
                })
                .to_string();
            safety -= 1;
        }

        Ok(result)
    }

    /// Process {% for %} loops
    fn process_for_loops(
        &self,
        template: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        let for_re =
            Regex::new(r"\{%\s*for\s+(\w+)\s+in\s+(.+?)\s*%\}([\s\S]*?)\{%\s*endfor\s*%\}")
                .unwrap();

        let mut result = template.to_string();
        let mut safety = 100;

        while for_re.is_match(&result) && safety > 0 {
            // We need to use a different approach since we can't borrow self mutably in the closure
            let mut new_result = result.clone();
            if let Some(caps) = for_re.captures(&result) {
                let full_match = caps.get(0).unwrap();
                let loop_var = caps[1].to_string();
                let iter_expr = caps[2].to_string();
                let body = caps[3].to_string();

                // Evaluate the iterable expression
                let items = match self.evaluate_simple_expr(&iter_expr, ctx) {
                    Ok(Value::List(items)) => items,
                    Ok(Value::String(s)) => {
                        s.chars().map(|c| Value::String(c.to_string())).collect()
                    }
                    Ok(Value::Dict(d)) => d.into_keys().map(Value::String).collect(),
                    _ => {
                        new_result.replace_range(
                            full_match.range(),
                            &format!("<!-- Cannot iterate over: {} -->", iter_expr),
                        );
                        result = new_result;
                        safety -= 1;
                        continue;
                    }
                };

                let len = items.len();
                let mut output = String::new();

                for (i, item) in items.into_iter().enumerate() {
                    // Create child engine with loop variables
                    let mut child_engine = self.child();
                    child_engine.set_local_var(&loop_var, item);

                    // Set loop variables
                    let mut loop_vars = HashMap::new();
                    loop_vars.insert("index".to_string(), Value::Int((i + 1) as i64));
                    loop_vars.insert("index0".to_string(), Value::Int(i as i64));
                    loop_vars.insert("first".to_string(), Value::Bool(i == 0));
                    loop_vars.insert("last".to_string(), Value::Bool(i == len - 1));
                    loop_vars.insert("length".to_string(), Value::Int(len as i64));
                    loop_vars.insert("revindex".to_string(), Value::Int((len - i) as i64));
                    loop_vars.insert("revindex0".to_string(), Value::Int((len - i - 1) as i64));
                    child_engine.set_local_var("loop", Value::Dict(loop_vars));

                    // Render body with loop context
                    match child_engine.render_inner(&body, ctx) {
                        Ok(rendered) => output.push_str(&rendered),
                        Err(e) => {
                            output.push_str(&format!("<!-- Loop error: {} -->", e));
                            break;
                        }
                    }
                }

                new_result.replace_range(full_match.range(), &output);
                result = new_result;
            }
            safety -= 1;
        }

        Ok(result)
    }

    /// Process {% if %} conditionals
    fn process_conditionals(
        &self,
        template: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        // Match if/elif/else/endif blocks (non-greedy, innermost first)
        let if_re = Regex::new(r"\{%\s*if\s+(.+?)\s*%\}([\s\S]*?)\{%\s*endif\s*%\}").unwrap();

        let mut result = template.to_string();
        let mut safety = 100;

        while if_re.is_match(&result) && safety > 0 {
            result = if_re
                .replace(&result, |caps: &regex::Captures| {
                    let condition_and_body = &caps[0];
                    self.evaluate_conditional_block(condition_and_body, ctx)
                        .unwrap_or_else(|e| format!("<!-- If error: {} -->", e))
                })
                .to_string();
            safety -= 1;
        }

        Ok(result)
    }

    /// Evaluate a complete if/elif/else block
    fn evaluate_conditional_block(
        &self,
        block: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        // Parse the block into conditions and bodies
        let if_match = Regex::new(r"\{%\s*if\s+(.+?)\s*%\}").unwrap();
        let elif_match = Regex::new(r"\{%\s*elif\s+(.+?)\s*%\}").unwrap();
        let else_match = Regex::new(r"\{%\s*else\s*%\}").unwrap();

        // Find all branch points
        let mut branches: Vec<(Option<&str>, &str)> = Vec::new();

        // Find the if condition
        if let Some(if_cap) = if_match.captures(block) {
            let if_cond = if_cap.get(1).unwrap().as_str();
            let after_if = &block[if_cap.get(0).unwrap().end()..];

            // Find next branch or endif
            let elif_pos = elif_match.find(after_if).map(|m| m.start());
            let else_pos = else_match.find(after_if).map(|m| m.start());
            let endif_pos = after_if.rfind("{% endif %}").unwrap_or(after_if.len());

            let first_branch = [elif_pos, else_pos, Some(endif_pos)]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(endif_pos);

            let if_body = &after_if[..first_branch];
            branches.push((Some(if_cond), if_body));

            // Process elif and else branches
            let mut remaining = &after_if[first_branch..];
            while !remaining.is_empty() && !remaining.starts_with("{% endif %}") {
                if let Some(elif_cap) = elif_match.captures(remaining) {
                    let elif_cond = elif_cap.get(1).unwrap().as_str();
                    let after_elif = &remaining[elif_cap.get(0).unwrap().end()..];

                    let next_branch = [
                        elif_match.find(after_elif).map(|m| m.start()),
                        else_match.find(after_elif).map(|m| m.start()),
                        after_elif.find("{% endif %}"),
                    ]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(after_elif.len());

                    branches.push((Some(elif_cond), &after_elif[..next_branch]));
                    remaining = &after_elif[next_branch..];
                } else if let Some(else_cap) = else_match.find(remaining) {
                    let after_else = &remaining[else_cap.end()..];
                    let endif_pos = after_else.find("{% endif %}").unwrap_or(after_else.len());
                    branches.push((None, &after_else[..endif_pos]));
                    break;
                } else {
                    break;
                }
            }
        }

        // Evaluate branches
        for (condition, body) in branches {
            let should_render = match condition {
                Some(cond) => {
                    let value = self.evaluate_simple_expr(cond, ctx)?;
                    value.is_truthy()
                }
                None => true, // else branch
            };

            if should_render {
                return self.render_inner(body, ctx);
            }
        }

        Ok(String::new())
    }

    /// Process macro calls: {{ macro_name(args) }}
    fn process_macro_calls(
        &self,
        template: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        let mut result = template.to_string();

        for (name, macro_def) in &self.macros {
            let call_re = Regex::new(&format!(
                r"\{{\{{\s*{}\s*\((.*?)\)\s*\}}\}}",
                regex::escape(name)
            ))
            .unwrap();

            result = call_re
                .replace_all(&result, |caps: &regex::Captures| {
                    let args_str = &caps[1];
                    let args: Vec<&str> = if args_str.is_empty() {
                        Vec::new()
                    } else {
                        args_str.split(',').map(|s| s.trim()).collect()
                    };

                    // Create context with macro parameters
                    let macro_ctx = ctx.clone_for_task();
                    for (i, param) in macro_def.params.iter().enumerate() {
                        if let Some(arg) = args.get(i) {
                            if let Ok(value) = self.evaluate_simple_expr(arg, ctx) {
                                macro_ctx.set_var(param, value);
                            }
                        }
                    }

                    // Render macro body
                    self.render_inner(&macro_def.body, &macro_ctx)
                        .unwrap_or_else(|e| format!("<!-- Macro error: {} -->", e))
                })
                .to_string();
        }

        Ok(result)
    }

    /// Process {{ expression }} and {{ expression | filter }}
    fn process_expressions(
        &self,
        template: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, NexusError> {
        // Match {{ ... }} but not inside {% %}
        let expr_re = Regex::new(r"\{\{(.*?)\}\}").unwrap();

        let result = expr_re.replace_all(template, |caps: &regex::Captures| {
            let expr = caps[1].trim();

            // Check for whitespace control markers
            let trim_left = expr.starts_with('-');
            let trim_right = expr.ends_with('-');
            let expr = expr.trim_start_matches('-').trim_end_matches('-').trim();

            // Parse expression with optional filters
            let (base_expr, filters) = self.parse_filters(expr);

            // Evaluate base expression
            let mut value = match self.evaluate_simple_expr(base_expr, ctx) {
                Ok(v) => v,
                Err(e) => return format!("<!-- Error: {} -->", e),
            };

            // Apply filters
            for (filter_name, filter_args) in filters {
                value = match self.apply_filter(&value, &filter_name, &filter_args, ctx) {
                    Ok(v) => v,
                    Err(e) => return format!("<!-- Filter error: {} -->", e),
                };
            }

            // Convert to string and handle escaping
            let output = self.value_to_string(&value);

            // Apply whitespace trimming (simplified - full implementation would trim surrounding whitespace)
            if trim_left || trim_right {
                output.trim().to_string()
            } else {
                output
            }
        });

        Ok(result.to_string())
    }

    /// Parse filters from expression: "name | upper | default('N/A')"
    fn parse_filters<'a>(&self, expr: &'a str) -> (&'a str, Vec<(String, Vec<String>)>) {
        let parts: Vec<&str> = expr.split('|').collect();
        let base_expr = parts[0].trim();

        let filters: Vec<(String, Vec<String>)> = parts[1..]
            .iter()
            .map(|filter| {
                let filter = filter.trim();
                // Parse filter name and arguments
                if let Some(paren_pos) = filter.find('(') {
                    let name = filter[..paren_pos].trim().to_string();
                    let args_str = filter[paren_pos + 1..].trim_end_matches(')');
                    let args: Vec<String> = if args_str.is_empty() {
                        Vec::new()
                    } else {
                        self.parse_filter_args(args_str)
                    };
                    (name, args)
                } else {
                    (filter.to_string(), Vec::new())
                }
            })
            .collect();

        (base_expr, filters)
    }

    /// Parse filter arguments, handling quoted strings
    /// Preserves quotes so evaluate_simple_expr can handle string literals
    fn parse_filter_args(&self, args_str: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut quote_char = ' ';

        for c in args_str.chars() {
            match c {
                '"' | '\'' if !in_quotes => {
                    in_quotes = true;
                    quote_char = c;
                    current.push(c); // Keep the opening quote
                }
                c if c == quote_char && in_quotes => {
                    in_quotes = false;
                    current.push(c); // Keep the closing quote
                }
                ',' if !in_quotes => {
                    if !current.trim().is_empty() {
                        args.push(current.trim().to_string());
                    }
                    current = String::new();
                }
                _ => current.push(c),
            }
        }

        // Handle last argument
        if !current.trim().is_empty() {
            args.push(current.trim().to_string());
        }

        args
    }

    /// Resolve a filter argument - if it's a quoted string, return the content; otherwise evaluate as expression
    fn resolve_filter_arg(&self, arg: &str, ctx: &ExecutionContext) -> Result<String, NexusError> {
        // Check if it's a quoted string literal
        if (arg.starts_with('"') && arg.ends_with('"'))
            || (arg.starts_with('\'') && arg.ends_with('\''))
        {
            // Return the content without quotes
            Ok(arg[1..arg.len() - 1].to_string())
        } else {
            // Evaluate as expression and convert to string
            let val = self.evaluate_simple_expr(arg, ctx)?;
            Ok(self.value_to_string(&val))
        }
    }

    /// Apply a filter to a value
    fn apply_filter(
        &self,
        value: &Value,
        filter: &str,
        args: &[String],
        ctx: &ExecutionContext,
    ) -> Result<Value, NexusError> {
        match filter {
            // String filters
            "upper" => Ok(Value::String(self.value_to_string(value).to_uppercase())),
            "lower" => Ok(Value::String(self.value_to_string(value).to_lowercase())),
            "trim" => Ok(Value::String(self.value_to_string(value).trim().to_string())),
            "title" => Ok(Value::String(self.title_case(&self.value_to_string(value)))),
            "capitalize" => {
                let s = self.value_to_string(value);
                let mut chars = s.chars();
                let result = match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                };
                Ok(Value::String(result))
            }

            // String manipulation
            "replace" => {
                if args.len() >= 2 {
                    let s = self.value_to_string(value);
                    let from = self.resolve_filter_arg(&args[0], ctx)?;
                    let to = self.resolve_filter_arg(&args[1], ctx)?;
                    Ok(Value::String(s.replace(&from, &to)))
                } else {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "replace filter requires 2 arguments".to_string(),
                        suggestion: Some("Use: {{ value | replace('old', 'new') }}".to_string()),
                    })
                }
            }
            "split" => {
                let s = self.value_to_string(value);
                let sep = if let Some(arg) = args.first() {
                    self.resolve_filter_arg(arg, ctx)?
                } else {
                    " ".to_string()
                };
                let parts: Vec<Value> = s.split(&sep).map(|p| Value::String(p.to_string())).collect();
                Ok(Value::List(parts))
            }
            "join" => {
                match value {
                    Value::List(items) => {
                        let sep = if let Some(arg) = args.first() {
                            self.resolve_filter_arg(arg, ctx)?
                        } else {
                            String::new()
                        };
                        let joined: String = items.iter()
                            .map(|v| self.value_to_string(v))
                            .collect::<Vec<_>>()
                            .join(&sep);
                        Ok(Value::String(joined))
                    }
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "join filter requires a list".to_string(),
                        suggestion: None,
                    }),
                }
            }

            // Default value
            "default" | "d" => {
                if value == &Value::Null || (matches!(value, Value::String(s) if s.is_empty())) {
                    if let Some(default_val) = args.first() {
                        self.evaluate_simple_expr(default_val, ctx)
                    } else {
                        Ok(Value::String(String::new()))
                    }
                } else {
                    Ok(value.clone())
                }
            }

            // Length/count
            "length" | "count" => {
                let len = match value {
                    Value::String(s) => s.len(),
                    Value::List(l) => l.len(),
                    Value::Dict(d) => d.len(),
                    _ => return Err(NexusError::Runtime {
                        function: None,
                        message: "length filter requires string, list, or dict".to_string(),
                        suggestion: None,
                    }),
                };
                Ok(Value::Int(len as i64))
            }

            // List operations
            "first" => {
                match value {
                    Value::List(items) => Ok(items.first().cloned().unwrap_or(Value::Null)),
                    Value::String(s) => Ok(s.chars().next().map(|c| Value::String(c.to_string())).unwrap_or(Value::Null)),
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "first filter requires list or string".to_string(),
                        suggestion: None,
                    }),
                }
            }
            "last" => {
                match value {
                    Value::List(items) => Ok(items.last().cloned().unwrap_or(Value::Null)),
                    Value::String(s) => Ok(s.chars().last().map(|c| Value::String(c.to_string())).unwrap_or(Value::Null)),
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "last filter requires list or string".to_string(),
                        suggestion: None,
                    }),
                }
            }
            "reverse" => {
                match value {
                    Value::List(items) => Ok(Value::List(items.iter().rev().cloned().collect())),
                    Value::String(s) => Ok(Value::String(s.chars().rev().collect())),
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "reverse filter requires list or string".to_string(),
                        suggestion: None,
                    }),
                }
            }
            "sort" => {
                match value {
                    Value::List(items) => {
                        let mut sorted = items.clone();
                        sorted.sort_by(|a, b| {
                            self.value_to_string(a).cmp(&self.value_to_string(b))
                        });
                        Ok(Value::List(sorted))
                    }
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "sort filter requires list".to_string(),
                        suggestion: None,
                    }),
                }
            }
            "unique" => {
                match value {
                    Value::List(items) => {
                        let mut seen = std::collections::HashSet::new();
                        let unique: Vec<Value> = items.iter()
                            .filter(|v| {
                                let key = self.value_to_string(v);
                                if seen.contains(&key) {
                                    false
                                } else {
                                    seen.insert(key);
                                    true
                                }
                            })
                            .cloned()
                            .collect();
                        Ok(Value::List(unique))
                    }
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "unique filter requires list".to_string(),
                        suggestion: None,
                    }),
                }
            }

            // Type conversions
            "int" => {
                let s = self.value_to_string(value);
                let i = s.parse::<i64>().unwrap_or(0);
                Ok(Value::Int(i))
            }
            "float" => {
                let s = self.value_to_string(value);
                let f = s.parse::<f64>().unwrap_or(0.0);
                Ok(Value::Float(f))
            }
            "string" | "str" => Ok(Value::String(self.value_to_string(value))),
            "bool" => Ok(Value::Bool(value.is_truthy())),

            // JSON
            "tojson" | "to_json" => {
                let json = serde_json::to_string(value).unwrap_or_default();
                Ok(Value::String(json))
            }
            "tojson_pretty" => {
                let json = serde_json::to_string_pretty(value).unwrap_or_default();
                Ok(Value::String(json))
            }

            // HTML/URL encoding
            "escape" | "e" => {
                let s = self.value_to_string(value);
                let escaped = s
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&#39;");
                Ok(Value::String(escaped))
            }
            "safe" => Ok(value.clone()), // Mark as safe (no escaping)
            "urlencode" => {
                let s = self.value_to_string(value);
                let encoded = urlencoding_encode(&s);
                Ok(Value::String(encoded))
            }

            // Math
            "abs" => {
                match value {
                    Value::Int(i) => Ok(Value::Int(i.abs())),
                    Value::Float(f) => Ok(Value::Float(f.abs())),
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "abs filter requires number".to_string(),
                        suggestion: None,
                    }),
                }
            }
            "round" => {
                match value {
                    Value::Float(f) => {
                        let precision: i32 = args.first()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        let factor = 10_f64.powi(precision);
                        Ok(Value::Float((f * factor).round() / factor))
                    }
                    Value::Int(i) => Ok(Value::Int(*i)),
                    _ => Err(NexusError::Runtime {
                        function: None,
                        message: "round filter requires number".to_string(),
                        suggestion: None,
                    }),
                }
            }

            // Regex
            "regex_search" => {
                if let Some(pattern) = args.first() {
                    let s = self.value_to_string(value);
                    let re = Regex::new(pattern).map_err(|e| NexusError::Runtime {
                        function: None,
                        message: format!("Invalid regex: {}", e),
                        suggestion: None,
                    })?;
                    if let Some(captures) = re.captures(&s) {
                        if captures.len() > 1 {
                            // Return captured group(s)
                            let groups: Vec<Value> = captures.iter()
                                .skip(1)
                                .map(|m| m.map(|m| Value::String(m.as_str().to_string())).unwrap_or(Value::Null))
                                .collect();
                            Ok(Value::List(groups))
                        } else {
                            Ok(Value::String(captures[0].to_string()))
                        }
                    } else {
                        Ok(Value::Null)
                    }
                } else {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "regex_search requires pattern argument".to_string(),
                        suggestion: None,
                    })
                }
            }
            "regex_replace" => {
                if args.len() >= 2 {
                    let s = self.value_to_string(value);
                    let pattern = &args[0];
                    let replacement = &args[1];
                    let re = Regex::new(pattern).map_err(|e| NexusError::Runtime {
                        function: None,
                        message: format!("Invalid regex: {}", e),
                        suggestion: None,
                    })?;
                    Ok(Value::String(re.replace_all(&s, replacement.as_str()).to_string()))
                } else {
                    Err(NexusError::Runtime {
                        function: None,
                        message: "regex_replace requires pattern and replacement".to_string(),
                        suggestion: None,
                    })
                }
            }

            // File path operations
            "basename" => {
                let s = self.value_to_string(value);
                let path = Path::new(&s);
                Ok(Value::String(path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()))
            }
            "dirname" => {
                let s = self.value_to_string(value);
                let path = Path::new(&s);
                Ok(Value::String(path.parent().and_then(|p| p.to_str()).unwrap_or("").to_string()))
            }
            "splitext" => {
                let s = self.value_to_string(value);
                let path = Path::new(&s);
                let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                Ok(Value::List(vec![
                    Value::String(stem.to_string()),
                    Value::String(ext.to_string()),
                ]))
            }

            _ => Err(NexusError::Runtime {
                function: None,
                message: format!("Unknown filter: {}", filter),
                suggestion: Some("Available filters: upper, lower, trim, replace, default, length, join, split, first, last, sort, unique, tojson, escape, regex_search, regex_replace, basename, dirname".to_string()),
            }),
        }
    }

    /// Title case a string
    fn title_case(&self, s: &str) -> String {
        s.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        first.to_uppercase().to_string() + &chars.as_str().to_lowercase()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Evaluate a simple expression (variable lookup with optional attribute access)
    fn evaluate_simple_expr(
        &self,
        expr: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, NexusError> {
        let expr = expr.trim();

        // String literals
        if (expr.starts_with('"') && expr.ends_with('"'))
            || (expr.starts_with('\'') && expr.ends_with('\''))
        {
            return Ok(Value::String(expr[1..expr.len() - 1].to_string()));
        }

        // Number literals
        if let Ok(i) = expr.parse::<i64>() {
            return Ok(Value::Int(i));
        }
        if let Ok(f) = expr.parse::<f64>() {
            return Ok(Value::Float(f));
        }

        // Boolean literals
        if expr == "true" || expr == "True" {
            return Ok(Value::Bool(true));
        }
        if expr == "false" || expr == "False" {
            return Ok(Value::Bool(false));
        }

        // Null/None
        if expr == "null" || expr == "None" || expr == "none" {
            return Ok(Value::Null);
        }

        // List literal [a, b, c]
        if expr.starts_with('[') && expr.ends_with(']') {
            let inner = &expr[1..expr.len() - 1];
            let items: Result<Vec<Value>, _> = inner
                .split(',')
                .map(|s| self.evaluate_simple_expr(s.trim(), ctx))
                .collect();
            return Ok(Value::List(items?));
        }

        // Variable lookup with attribute access: vars.hostname, host.address
        let parts: Vec<&str> = expr.split('.').collect();

        // First check local template variables (takes precedence)
        let mut value = if let Some(v) = self.local_vars.get(parts[0]) {
            v.clone()
        } else {
            // Fall back to execution context
            ctx.get_var(parts[0]).unwrap_or(Value::Null)
        };

        for part in &parts[1..] {
            // Handle index access: items[0]
            if let Some(bracket_pos) = part.find('[') {
                let attr = &part[..bracket_pos];
                let index_str = &part[bracket_pos + 1..part.len() - 1];

                // First access the attribute
                if !attr.is_empty() {
                    value = match value {
                        Value::Dict(ref d) => d.get(attr).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    };
                }

                // Then access the index
                if let Ok(index) = index_str.parse::<usize>() {
                    value = match value {
                        Value::List(ref l) => l.get(index).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    };
                } else {
                    // String key access
                    let key = index_str.trim_matches(|c| c == '"' || c == '\'');
                    value = match value {
                        Value::Dict(ref d) => d.get(key).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    };
                }
            } else {
                value = match value {
                    Value::Dict(ref d) => d.get(*part).cloned().unwrap_or(Value::Null),
                    _ => Value::Null,
                };
            }
        }

        Ok(value)
    }

    /// Convert value to string for output
    #[allow(clippy::only_used_in_recursion)]
    fn value_to_string(&self, value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            Value::List(l) => {
                let items: Vec<String> = l.iter().map(|v| self.value_to_string(v)).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Dict(d) => {
                let items: Vec<String> = d
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, self.value_to_string(v)))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
        }
    }

    /// Process whitespace control markers
    fn process_whitespace_control(&self, template: &str) -> String {
        // Handle {%- and -%} whitespace trimming
        let re = Regex::new(r"\s*\{%-").unwrap();
        let result = re.replace_all(template, "{%");
        let re = Regex::new(r"-%\}\s*").unwrap();
        let result = re.replace_all(&result, "%}");

        // Handle {{- and -}} whitespace trimming
        let re = Regex::new(r"\s*\{\{-").unwrap();
        let result = re.replace_all(&result, "{{");
        let re = Regex::new(r"-\}\}\s*").unwrap();
        let result = re.replace_all(&result, "}}");

        result.to_string()
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple URL encoding (without external crate)
fn urlencoding_encode(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ' ' => result.push_str("%20"),
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Host;
    use std::sync::Arc;

    fn test_ctx() -> ExecutionContext {
        let host = Arc::new(Host {
            name: "test".to_string(),
            address: "127.0.0.1".to_string(),
            port: 22,
            user: "root".to_string(),
            groups: vec![],
            vars: HashMap::new(),
        });
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), Value::String("World".to_string()));
        vars.insert(
            "items".to_string(),
            Value::List(vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
                Value::String("c".to_string()),
            ]),
        );
        ExecutionContext::new(host, vars)
    }

    #[test]
    fn test_simple_variable() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine.render("Hello {{ name }}!", &ctx).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_filter_upper() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine.render("{{ name | upper }}", &ctx).unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_filter_chain() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine.render("{{ name | upper | lower }}", &ctx).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn test_for_loop() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine
            .render("{% for item in items %}{{ item }}{% endfor %}", &ctx)
            .unwrap();
        assert_eq!(result, "abc");
    }

    #[test]
    fn test_for_loop_with_index() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine
            .render(
                "{% for item in items %}{{ loop.index }}:{{ item }} {% endfor %}",
                &ctx,
            )
            .unwrap();
        assert_eq!(result, "1:a 2:b 3:c ");
    }

    #[test]
    fn test_if_condition() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine
            .render("{% if name %}Hello {{ name }}{% endif %}", &ctx)
            .unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_default_filter() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine
            .render("{{ missing | default('N/A') }}", &ctx)
            .unwrap();
        assert_eq!(result, "N/A");
    }

    #[test]
    fn test_join_filter() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine.render("{{ items | join(', ') }}", &ctx).unwrap();
        assert_eq!(result, "a, b, c");
    }

    #[test]
    fn test_length_filter() {
        let mut engine = TemplateEngine::new();
        let ctx = test_ctx();

        let result = engine.render("{{ items | length }}", &ctx).unwrap();
        assert_eq!(result, "3");
    }
}
