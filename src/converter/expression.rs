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

        // Methods with arguments
        filter_map.insert("join", FilterConversion::MethodWithArgs("join"));
        filter_map.insert("split", FilterConversion::MethodWithArgs("split"));
        filter_map.insert("replace", FilterConversion::MethodWithArgs("replace"));
        filter_map.insert(
            "regex_replace",
            FilterConversion::MethodWithArgs("regex_replace"),
        );
        filter_map.insert("default", FilterConversion::Custom(convert_default));
        filter_map.insert("d", FilterConversion::Custom(convert_default));

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
            .replace_all(&output, "$${$1 != null}")
            .to_string();

        let not_defined_re = Regex::new(r"([\w.]+)\s+is\s+not\s+defined").unwrap();
        output = not_defined_re
            .replace_all(&output, "$${$1 == null}")
            .to_string();

        // Handle "result is changed/failed/success"
        let changed_re = Regex::new(r"(\w+)\s+is\s+changed").unwrap();
        output = changed_re
            .replace_all(&output, "$${$1.changed}")
            .to_string();

        let failed_re = Regex::new(r"(\w+)\s+is\s+failed").unwrap();
        output = failed_re.replace_all(&output, "$${$1.failed}").to_string();

        let success_re = Regex::new(r"(\w+)\s+is\s+success").unwrap();
        output = success_re.replace_all(&output, "$${$1.ok}").to_string();

        let skipped_re = Regex::new(r"(\w+)\s+is\s+skipped").unwrap();
        output = skipped_re
            .replace_all(&output, "$${$1.skipped}")
            .to_string();

        // Handle "is search" and "is match"
        let search_re = Regex::new(r"(\w+)\s+is\s+search\('(.+?)'\)").unwrap();
        output = search_re
            .replace_all(&output, "$${$1.contains('$2')}")
            .to_string();

        let match_re = Regex::new(r"(\w+)\s+is\s+match\('(.+?)'\)").unwrap();
        output = match_re
            .replace_all(&output, "$${$1.matches('$2')}")
            .to_string();

        // Convert any remaining Jinja2 expressions
        let result = self.convert_string(&output);

        ConversionResult {
            output: result.output,
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
}
