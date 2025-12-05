use regex::Regex;
use std::collections::HashMap;

/// Converts Ansible Jinja2 expressions to Nexus expression syntax
pub struct ExpressionConverter {
    filter_map: HashMap<&'static str, FilterConversion>,
    variable_map: HashMap<&'static str, &'static str>,
}

#[derive(Clone)]
enum FilterConversion {
    /// Simple method call: `| filter` → `.method()`
    Method(&'static str),
    /// Method with args: `| filter(arg)` → `.method(arg)`
    MethodWithArgs(&'static str),
    /// Special handling required
    Custom(fn(&str, &str) -> String),
}

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub output: String,
    pub warnings: Vec<String>,
    pub unsupported_filters: Vec<String>,
}

impl ExpressionConverter {
    pub fn new() -> Self {
        let mut filter_map = HashMap::new();

        // Simple method conversions
        filter_map.insert("upper", FilterConversion::Method("upper"));
        filter_map.insert("lower", FilterConversion::Method("lower"));
        filter_map.insert("length", FilterConversion::Method("len"));
        filter_map.insert("first", FilterConversion::Method("first"));
        filter_map.insert("last", FilterConversion::Method("last"));
        filter_map.insert("trim", FilterConversion::Method("trim"));
        filter_map.insert("to_json", FilterConversion::Method("to_json"));
        filter_map.insert("to_yaml", FilterConversion::Method("to_yaml"));
        filter_map.insert("to_nice_json", FilterConversion::Method("to_json"));
        filter_map.insert("to_nice_yaml", FilterConversion::Method("to_yaml"));
        filter_map.insert("b64encode", FilterConversion::Method("base64_encode"));
        filter_map.insert("b64decode", FilterConversion::Method("base64_decode"));
        filter_map.insert("string", FilterConversion::Method("to_string"));
        filter_map.insert("int", FilterConversion::Method("to_int"));
        filter_map.insert("float", FilterConversion::Method("to_float"));
        filter_map.insert("bool", FilterConversion::Method("to_bool"));
        filter_map.insert("list", FilterConversion::Method("to_list"));
        filter_map.insert("unique", FilterConversion::Method("unique"));
        filter_map.insert("sort", FilterConversion::Method("sort"));
        filter_map.insert("reverse", FilterConversion::Method("reverse"));
        filter_map.insert("flatten", FilterConversion::Method("flatten"));
        filter_map.insert("keys", FilterConversion::Method("keys"));
        filter_map.insert("values", FilterConversion::Method("values"));
        filter_map.insert("items", FilterConversion::Method("items"));

        // Mathematical and utility filters
        filter_map.insert("abs", FilterConversion::Method("abs"));
        filter_map.insert("min", FilterConversion::Method("min"));
        filter_map.insert("max", FilterConversion::Method("max"));
        filter_map.insert("sum", FilterConversion::Method("sum"));
        filter_map.insert("round", FilterConversion::Method("round"));
        filter_map.insert("quote", FilterConversion::Method("shell_quote"));

        // Methods with arguments
        filter_map.insert("join", FilterConversion::MethodWithArgs("join"));
        filter_map.insert("split", FilterConversion::MethodWithArgs("split"));
        filter_map.insert("replace", FilterConversion::MethodWithArgs("replace"));
        filter_map.insert(
            "regex_replace",
            FilterConversion::MethodWithArgs("regex_replace"),
        );
        filter_map.insert("selectattr", FilterConversion::MethodWithArgs("select_attr"));
        filter_map.insert("rejectattr", FilterConversion::MethodWithArgs("reject_attr"));
        filter_map.insert("map", FilterConversion::MethodWithArgs("map"));
        filter_map.insert("select", FilterConversion::MethodWithArgs("select"));
        filter_map.insert("reject", FilterConversion::MethodWithArgs("reject"));

        // Custom conversions
        filter_map.insert("default", FilterConversion::Custom(convert_default));
        filter_map.insert("d", FilterConversion::Custom(convert_default));
        filter_map.insert("ternary", FilterConversion::Custom(convert_ternary));

        // Special variable mappings
        let mut variable_map = HashMap::new();
        variable_map.insert("ansible_hostname", "host.hostname");
        variable_map.insert("ansible_os_family", "host.os_family");
        variable_map.insert("ansible_distribution", "host.distribution");
        variable_map.insert("ansible_architecture", "host.arch");
        variable_map.insert("inventory_hostname", "host.name");
        variable_map.insert("inventory_hostname_short", "host.short_name");
        variable_map.insert("ansible_user", "host.user");
        variable_map.insert("ansible_host", "host.address");
        variable_map.insert("ansible_port", "host.port");
        variable_map.insert("ansible_become", "host.become");
        variable_map.insert("ansible_become_user", "host.become_user");
        variable_map.insert("playbook_dir", "playbook.dir");
        variable_map.insert("role_path", "role.path");

        Self {
            filter_map,
            variable_map,
        }
    }

    /// Convert a full string that may contain multiple Jinja2 expressions
    pub fn convert_string(&self, input: &str) -> ConversionResult {
        let mut output = input.to_string();
        let mut warnings = Vec::new();
        let mut unsupported = Vec::new();

        // Find all {{ ... }} expressions
        let re = Regex::new(r"\{\{\s*(.+?)\s*\}\}").unwrap();

        // Process each match (in reverse to preserve positions)
        let matches: Vec<_> = re.find_iter(input).collect();
        for mat in matches.into_iter().rev() {
            let full_match = mat.as_str();
            let inner = &full_match[2..full_match.len() - 2].trim();

            let (converted, warns, unsup) = self.convert_expression(inner);
            warnings.extend(warns);
            unsupported.extend(unsup);

            output = output.replace(full_match, &format!("${{{}}}", converted));
        }

        ConversionResult {
            output,
            warnings,
            unsupported_filters: unsupported,
        }
    }

    /// Convert a single Jinja2 expression (without the {{ }})
    pub fn convert_expression(&self, expr: &str) -> (String, Vec<String>, Vec<String>) {
        let warnings = Vec::new();
        let mut unsupported = Vec::new();

        // Handle Jinja2 inline-if syntax: 'value' if condition else 'other'
        // This needs to be done before filter chain processing
        let inline_if_re = Regex::new(r"^(.+?)\s+if\s+(.+?)\s+else\s+(.+)$").unwrap();
        if let Some(caps) = inline_if_re.captures(expr.trim()) {
            let true_val = caps[1].trim();
            let condition = caps[2].trim();
            let false_val = caps[3].trim();
            // Recursively convert each part in case they have filters
            let (cond_conv, _, _) = self.convert_expression(condition);
            let (true_conv, _, _) = self.convert_expression(true_val);
            let (false_conv, _, _) = self.convert_expression(false_val);
            return (
                format!("iif({}, {}, {})", cond_conv, true_conv, false_conv),
                warnings,
                unsupported,
            );
        }

        // Handle filter chains: variable | filter1 | filter2(arg)
        let parts: Vec<&str> = expr.split('|').map(|s| s.trim()).collect();

        if parts.is_empty() {
            return (expr.to_string(), warnings, unsupported);
        }

        // Convert the base variable
        let mut result = self.convert_variable(parts[0]);

        // Apply each filter
        for filter_part in &parts[1..] {
            let (filter_name, args) = parse_filter(filter_part);

            if let Some(conversion) = self.filter_map.get(filter_name) {
                match conversion {
                    FilterConversion::Method(method) => {
                        result = format!("{}.{}()", result, method);
                    }
                    FilterConversion::MethodWithArgs(method) => {
                        if let Some(args) = args {
                            result = format!("{}.{}({})", result, method, args);
                        } else {
                            result = format!("{}.{}()", result, method);
                        }
                    }
                    FilterConversion::Custom(func) => {
                        result = func(&result, args.unwrap_or(""));
                    }
                }
            } else {
                // Unknown filter - keep as method call but warn
                unsupported.push(filter_name.to_string());
                if let Some(args) = args {
                    result = format!("{}.{}({})", result, filter_name, args);
                } else {
                    result = format!("{}.{}()", result, filter_name);
                }
            }
        }

        (result, warnings, unsupported)
    }

    /// Convert Ansible variable names to Nexus equivalents
    fn convert_variable(&self, var: &str) -> String {
        // Check for special variables first
        if let Some(nexus_var) = self.variable_map.get(var) {
            return nexus_var.to_string();
        }

        // Handle groups['name'] → groups.name
        let groups_re = Regex::new(r"groups\['(\w+)'\]").unwrap();
        if let Some(caps) = groups_re.captures(var) {
            return format!("groups.{}", &caps[1]);
        }

        // Handle hostvars[host]['var'] → hosts[host].var
        let hostvars_re = Regex::new(r"hostvars\[(.+?)\]\['(\w+)'\]").unwrap();
        if let Some(caps) = hostvars_re.captures(var) {
            return format!("hosts[{}].{}", &caps[1], &caps[2]);
        }

        var.to_string()
    }

    /// Convert a when condition from Ansible to Nexus
    pub fn convert_condition(&self, condition: &str) -> ConversionResult {
        let mut output = condition.to_string();
        let warnings = Vec::new();
        let unsupported = Vec::new();

        // Handle "is defined" / "is not defined"
        // Match variable paths like: my_var, result.stdout, foo.bar.baz
        let defined_re = Regex::new(r"([\w.]+)\s+is\s+defined").unwrap();
        output = defined_re
            .replace_all(&output, "$1 != null")
            .to_string();

        let not_defined_re = Regex::new(r"([\w.]+)\s+is\s+not\s+defined").unwrap();
        output = not_defined_re
            .replace_all(&output, "$1 == null")
            .to_string();

        // Handle "result is changed/failed/success"
        let changed_re = Regex::new(r"([\w.]+)\s+is\s+changed").unwrap();
        output = changed_re.replace_all(&output, "$1.changed").to_string();

        let failed_re = Regex::new(r"([\w.]+)\s+is\s+failed").unwrap();
        output = failed_re.replace_all(&output, "$1.failed").to_string();

        let success_re = Regex::new(r"([\w.]+)\s+is\s+success").unwrap();
        output = success_re.replace_all(&output, "$1.ok").to_string();

        let skipped_re = Regex::new(r"([\w.]+)\s+is\s+skipped").unwrap();
        output = skipped_re.replace_all(&output, "$1.skipped").to_string();

        // Handle type checking tests
        let number_re = Regex::new(r"([\w.]+)\s+is\s+number").unwrap();
        output = number_re.replace_all(&output, "$1.is_number()").to_string();

        let string_re = Regex::new(r"([\w.]+)\s+is\s+string").unwrap();
        output = string_re.replace_all(&output, "$1.is_string()").to_string();

        // Handle "is mapping" and "is dict" (synonyms in Jinja2)
        let mapping_re = Regex::new(r"([\w.]+)\s+is\s+mapping").unwrap();
        output = mapping_re.replace_all(&output, "$1.is_dict()").to_string();

        let dict_re = Regex::new(r"([\w.]+)\s+is\s+dict").unwrap();
        output = dict_re.replace_all(&output, "$1.is_dict()").to_string();

        // Handle "is sequence" and "is list" (synonyms in Jinja2)
        let sequence_re = Regex::new(r"([\w.]+)\s+is\s+sequence").unwrap();
        output = sequence_re.replace_all(&output, "$1.is_list()").to_string();

        let list_re = Regex::new(r"([\w.]+)\s+is\s+list").unwrap();
        output = list_re.replace_all(&output, "$1.is_list()").to_string();

        // Handle "is divisibleby(n)"
        let divisibleby_re = Regex::new(r"([\w.]+)\s+is\s+divisibleby\((.+?)\)").unwrap();
        output = divisibleby_re
            .replace_all(&output, "$1 % $2 == 0")
            .to_string();

        // Handle "is sameas(value)"
        let sameas_re = Regex::new(r"([\w.]+)\s+is\s+sameas\((.+?)\)").unwrap();
        output = sameas_re
            .replace_all(&output, "$1 === $2")
            .to_string();

        // Handle "is search" and "is match"
        // Note: Ansible's "is search" does regex matching, not substring matching
        let search_re = Regex::new(r"([\w.]+)\s+is\s+search\('(.+?)'\)").unwrap();
        output = search_re
            .replace_all(&output, "$1.matches('$2')")
            .to_string();

        let match_re = Regex::new(r"([\w.]+)\s+is\s+match\('(.+?)'\)").unwrap();
        output = match_re
            .replace_all(&output, "$1.matches('$2')")
            .to_string();

        // Convert Jinja2 filters in conditions
        // Handle "var | length" -> "len(var)"
        let length_re = Regex::new(r"([\w.]+)\s*\|\s*length").unwrap();
        output = length_re.replace_all(&output, "len($1)").to_string();

        // Handle "var | int" -> "var.to_int()"
        let int_re = Regex::new(r"([\w.]+)\s*\|\s*int").unwrap();
        output = int_re.replace_all(&output, "$1.to_int()").to_string();

        // Handle "var | string" -> "var.to_string()"
        let string_re = Regex::new(r"([\w.]+)\s*\|\s*string").unwrap();
        output = string_re.replace_all(&output, "$1.to_string()").to_string();

        // Handle "var | bool" -> "var.to_bool()"
        let bool_re = Regex::new(r"([\w.]+)\s*\|\s*bool").unwrap();
        output = bool_re.replace_all(&output, "$1.to_bool()").to_string();

        // Handle "var | default(val)" -> "(var ?? val)"
        let default_re = Regex::new(r"([\w.]+)\s*\|\s*default\(([^)]+)\)").unwrap();
        output = default_re.replace_all(&output, "($1 ?? $2)").to_string();

        // Keep boolean operators as-is (Nexus uses 'and', 'or', 'not' like Python)
        // No conversion needed - Ansible and Nexus use the same keywords

        // Convert any remaining Jinja2 {{ }} expressions
        let result = self.convert_string(&output);
        output = result.output;

        // Wrap the entire condition in ${...} if it's not already
        // and doesn't start with a literal string/number
        // NOTE: convert_string() may have already added ${} wrapping, so check for that first
        let already_wrapped = output.starts_with("${");
        let needs_wrap = !already_wrapped
            && !output.starts_with('"')
            && !output.starts_with('\'')
            && !output.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && !output.eq_ignore_ascii_case("true")
            && !output.eq_ignore_ascii_case("false");

        if needs_wrap {
            output = format!("${{{}}}", output);
        }

        ConversionResult {
            output,
            warnings: [warnings, result.warnings].concat(),
            unsupported_filters: [unsupported, result.unsupported_filters].concat(),
        }
    }
}

impl Default for ExpressionConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a filter like "join(',')" into ("join", Some("','"))
fn parse_filter(filter: &str) -> (&str, Option<&str>) {
    if let Some(paren_pos) = filter.find('(') {
        let name = &filter[..paren_pos];
        let args = &filter[paren_pos + 1..filter.len() - 1];
        (name, Some(args))
    } else {
        (filter, None)
    }
}

/// Convert Ansible's default filter to Nexus's null coalescing
fn convert_default(base: &str, args: &str) -> String {
    if args.is_empty() {
        format!("{} ?? ''", base)
    } else {
        format!("{} ?? {}", base, args)
    }
}

/// Convert Ansible's ternary filter to Nexus's iif() function
/// Example: condition | ternary('yes', 'no') -> iif(condition, 'yes', 'no')
fn convert_ternary(base: &str, args: &str) -> String {
    if args.is_empty() {
        format!("iif({}, true, false)", base)
    } else {
        format!("iif({}, {})", base, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ my_var }}");
        assert_eq!(result.output, "${my_var}");
    }

    #[test]
    fn test_filter_chain() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ items | join(',') }}");
        assert_eq!(result.output, "${items.join(',')}");
    }

    #[test]
    fn test_default_filter() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ my_var | default('fallback') }}");
        assert_eq!(result.output, "${my_var ?? 'fallback'}");
    }

    #[test]
    fn test_ansible_variables() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ ansible_hostname }}");
        assert_eq!(result.output, "${host.hostname}");
    }

    #[test]
    fn test_condition_is_defined() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_condition("my_var is defined");
        assert_eq!(result.output, "${my_var != null}");
    }

    #[test]
    fn test_condition_complex() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_condition(
            "available_updates.results is defined and available_updates.results | length > 0",
        );
        assert_eq!(
            result.output,
            "${available_updates.results != null and len(available_updates.results) > 0}"
        );
    }

    #[test]
    fn test_inline_if() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ 'yes' if condition else 'no' }}");
        assert_eq!(result.output, "${iif(condition, 'yes', 'no')}");
    }

    #[test]
    fn test_ternary_filter() {
        let converter = ExpressionConverter::new();
        let result = converter.convert_string("{{ my_var | ternary('enabled', 'disabled') }}");
        assert_eq!(result.output, "${iif(my_var, 'enabled', 'disabled')}");
    }
}
