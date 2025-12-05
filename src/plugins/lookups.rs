// Lookup plugins for fetching data from various sources during playbook execution

use std::fs;
use std::path::Path;
use std::process::Command;

use rand::distributions::{Alphanumeric, DistString};
use rand::Rng;

use crate::executor::ExecutionContext;
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Main lookup function that dispatches to specific lookup implementations
pub fn lookup(
    lookup_type: &str,
    args: &[Value],
    ctx: &ExecutionContext,
) -> Result<Value, NexusError> {
    match lookup_type {
        "file" => lookup_file(args),
        "env" => lookup_env(args),
        "pipe" => lookup_pipe(args),
        "password" => lookup_password(args),
        "template" => lookup_template(args, ctx),
        "first_found" => lookup_first_found(args),
        _ => Err(NexusError::Runtime {
            function: Some("lookup".to_string()),
            message: format!("Unknown lookup type: {}", lookup_type),
            suggestion: Some(
                "Available lookups: file, env, pipe, password, template, first_found".to_string(),
            ),
        }),
    }
}

/// Read file contents
/// Usage: lookup('file', '/path/to/file.txt')
fn lookup_file(args: &[Value]) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(file)".to_string()),
            message: "file lookup requires a file path argument".to_string(),
            suggestion: Some("Example: lookup('file', '/path/to/file.txt')".to_string()),
        });
    }

    let path = args[0].to_string();

    fs::read_to_string(&path)
        .map(|content| Value::String(content.trim_end().to_string()))
        .map_err(|e| NexusError::Runtime {
            function: Some("lookup(file)".to_string()),
            message: format!("Failed to read file '{}': {}", path, e),
            suggestion: Some("Check that the file exists and is readable".to_string()),
        })
}

/// Get environment variable value
/// Usage: lookup('env', 'HOME')
fn lookup_env(args: &[Value]) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(env)".to_string()),
            message: "env lookup requires an environment variable name".to_string(),
            suggestion: Some("Example: lookup('env', 'HOME')".to_string()),
        });
    }

    let var_name = args[0].to_string();

    std::env::var(&var_name)
        .map(Value::String)
        .map_err(|_| NexusError::Runtime {
            function: Some("lookup(env)".to_string()),
            message: format!("Environment variable '{}' not found", var_name),
            suggestion: Some("Check that the environment variable is set".to_string()),
        })
}

/// Execute command and return output
/// Usage: lookup('pipe', 'date +%Y-%m-%d')
fn lookup_pipe(args: &[Value]) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(pipe)".to_string()),
            message: "pipe lookup requires a command string".to_string(),
            suggestion: Some("Example: lookup('pipe', 'date +%Y-%m-%d')".to_string()),
        });
    }

    let command = args[0].to_string();

    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .map_err(|e| NexusError::Runtime {
            function: Some("lookup(pipe)".to_string()),
            message: format!("Failed to execute command '{}': {}", command, e),
            suggestion: Some("Check that the command is valid and executable".to_string()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NexusError::Runtime {
            function: Some("lookup(pipe)".to_string()),
            message: format!("Command '{}' failed: {}", command, stderr),
            suggestion: None,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(Value::String(stdout.trim_end().to_string()))
}

/// Generate or retrieve password
/// Usage: lookup('password', '/tmp/passwords/db_pass length=20')
///        lookup('password', '/tmp/pass chars=ascii_letters,digits,punctuation')
fn lookup_password(args: &[Value]) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(password)".to_string()),
            message: "password lookup requires a file path argument".to_string(),
            suggestion: Some(
                "Example: lookup('password', '/tmp/passwords/db_pass length=20')".to_string(),
            ),
        });
    }

    let arg_str = args[0].to_string();
    let parts: Vec<&str> = arg_str.split_whitespace().collect();

    if parts.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(password)".to_string()),
            message: "password lookup requires a file path".to_string(),
            suggestion: None,
        });
    }

    let file_path = parts[0];
    let mut length = 16; // default length
    let mut use_special = false;

    // Parse options
    for part in &parts[1..] {
        if let Some(len_str) = part.strip_prefix("length=") {
            length = len_str.parse().unwrap_or(16);
        } else if part.contains("punctuation") || part.contains("special") {
            use_special = true;
        }
    }

    // Check if password file exists
    if Path::new(file_path).exists() {
        return fs::read_to_string(file_path)
            .map(|content| Value::String(content.trim().to_string()))
            .map_err(|e| NexusError::Runtime {
                function: Some("lookup(password)".to_string()),
                message: format!("Failed to read password file '{}': {}", file_path, e),
                suggestion: None,
            });
    }

    // Generate new password
    let password = if use_special {
        generate_password_with_special(length)
    } else {
        Alphanumeric.sample_string(&mut rand::thread_rng(), length)
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = Path::new(file_path).parent() {
        fs::create_dir_all(parent).map_err(|e| NexusError::Runtime {
            function: Some("lookup(password)".to_string()),
            message: format!("Failed to create password directory: {}", e),
            suggestion: None,
        })?;
    }

    // Write password to file
    fs::write(file_path, &password).map_err(|e| NexusError::Runtime {
        function: Some("lookup(password)".to_string()),
        message: format!("Failed to write password file '{}': {}", file_path, e),
        suggestion: None,
    })?;

    // Set file permissions to 0600 (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(file_path)
            .map_err(|e| NexusError::Runtime {
                function: Some("lookup(password)".to_string()),
                message: format!("Failed to get file metadata: {}", e),
                suggestion: None,
            })?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(file_path, perms).map_err(|e| NexusError::Runtime {
            function: Some("lookup(password)".to_string()),
            message: format!("Failed to set file permissions: {}", e),
            suggestion: None,
        })?;
    }

    Ok(Value::String(password))
}

/// Generate password with special characters
fn generate_password_with_special(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:,.<>?";
    let mut rng = rand::thread_rng();

    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Render a template string
/// Usage: lookup('template', 'Hello {{ name }}!')
fn lookup_template(args: &[Value], ctx: &ExecutionContext) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(template)".to_string()),
            message: "template lookup requires a template string".to_string(),
            suggestion: Some("Example: lookup('template', 'Hello {{ name }}!')".to_string()),
        });
    }

    let template = args[0].to_string();

    // Simple template rendering - replace {{ var }} with variable values
    let mut result = template.clone();
    let re = regex::Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)\s*\}\}")
        .map_err(|e| NexusError::Runtime {
            function: Some("lookup(template)".to_string()),
            message: format!("Regex error: {}", e),
            suggestion: None,
        })?;

    for cap in re.captures_iter(&template.clone()) {
        let full_match = &cap[0];
        let var_name = &cap[1];

        // Parse nested variable path
        let path: Vec<String> = var_name.split('.').map(|s| s.to_string()).collect();
        let value = ctx
            .get_nested_var(&path)
            .unwrap_or_else(|| Value::String(String::new()));

        result = result.replace(full_match, &value.to_string());
    }

    Ok(Value::String(result))
}

/// Return the first file that exists from a list
/// Usage: lookup('first_found', ['config.local.yml', 'config.yml', 'defaults.yml'])
fn lookup_first_found(args: &[Value]) -> Result<Value, NexusError> {
    if args.is_empty() {
        return Err(NexusError::Runtime {
            function: Some("lookup(first_found)".to_string()),
            message: "first_found lookup requires a list of file paths".to_string(),
            suggestion: Some(
                "Example: lookup('first_found', ['config.local.yml', 'config.yml'])".to_string(),
            ),
        });
    }

    let paths = match &args[0] {
        Value::List(list) => list
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>(),
        Value::String(s) => vec![s.clone()],
        _ => {
            return Err(NexusError::Runtime {
                function: Some("lookup(first_found)".to_string()),
                message: "first_found lookup requires a list or string argument".to_string(),
                suggestion: None,
            })
        }
    };

    for path in paths {
        if Path::new(&path).exists() {
            return Ok(Value::String(path));
        }
    }

    Err(NexusError::Runtime {
        function: Some("lookup(first_found)".to_string()),
        message: "No files found from the provided list".to_string(),
        suggestion: Some("Check that at least one file in the list exists".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Host;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn create_test_context() -> ExecutionContext {
        let host = Host::new("test-host");
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), Value::String("World".to_string()));
        ExecutionContext::new(Arc::new(host), vars)
    }

    #[test]
    fn test_lookup_env() {
        std::env::set_var("TEST_VAR", "test_value");
        let args = vec![Value::String("TEST_VAR".to_string())];
        let result = lookup_env(&args).unwrap();
        assert_eq!(result, Value::String("test_value".to_string()));
    }

    #[test]
    fn test_lookup_template() {
        let ctx = create_test_context();
        let args = vec![Value::String("Hello {{ name }}!".to_string())];
        let result = lookup_template(&args, &ctx).unwrap();
        assert_eq!(result, Value::String("Hello World!".to_string()));
    }

    #[test]
    fn test_lookup_password_generation() {
        let temp_file = "/tmp/nexus_test_password.txt";
        // Clean up any existing file
        let _ = fs::remove_file(temp_file);

        let args = vec![Value::String(format!("{} length=12", temp_file))];
        let result = lookup_password(&args).unwrap();

        if let Value::String(password) = result {
            assert_eq!(password.len(), 12);
            // Verify file was created
            assert!(Path::new(temp_file).exists());
            // Clean up
            let _ = fs::remove_file(temp_file);
        } else {
            panic!("Expected String value");
        }
    }

    #[test]
    fn test_main_lookup_dispatch() {
        std::env::set_var("TEST_LOOKUP_VAR", "dispatch_test");
        let ctx = create_test_context();
        let args = vec![Value::String("TEST_LOOKUP_VAR".to_string())];
        let result = lookup("env", &args, &ctx).unwrap();
        assert_eq!(result, Value::String("dispatch_test".to_string()));
    }

    #[test]
    fn test_unknown_lookup_type() {
        let ctx = create_test_context();
        let args = vec![];
        let result = lookup("unknown_type", &args, &ctx);
        assert!(result.is_err());
    }
}
