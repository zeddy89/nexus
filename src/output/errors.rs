// Human-readable error messages for Nexus

use std::fmt;
use std::io::IsTerminal;
use std::path::PathBuf;

use colored::*;

/// Initialize color output based on TTY detection and NO_COLOR environment variable
fn should_use_colors() -> bool {
    // Check NO_COLOR environment variable first (https://no-color.org/)
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Check if stderr is a TTY (errors are typically written to stderr)
    std::io::stderr().is_terminal()
}

/// All error types in Nexus
#[derive(Debug)]
pub enum NexusError {
    /// Parse errors (YAML, expressions, functions)
    Parse(Box<ParseError>),

    /// I/O errors
    Io {
        message: String,
        path: Option<PathBuf>,
    },

    /// SSH connection errors
    Ssh {
        host: String,
        message: String,
        suggestion: Option<String>,
    },

    /// Module execution errors
    Module(Box<ModuleError>),

    /// Condition evaluation errors
    Condition {
        expression: String,
        message: String,
        suggestion: Option<String>,
    },

    /// Inventory errors
    Inventory {
        message: String,
        suggestion: Option<String>,
    },

    /// Runtime errors (function execution)
    Runtime {
        function: Option<String>,
        message: String,
        suggestion: Option<String>,
    },

    /// Task failure (fail_when triggered)
    TaskFailed {
        task_name: String,
        host: String,
        condition: String,
    },

    /// Timeout errors
    Timeout {
        operation: String,
        duration_secs: u64,
    },
}

#[derive(Debug)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub suggestion: Option<String>,
}

#[derive(Debug)]
pub struct ModuleError {
    pub module: String,
    pub task_name: String,
    pub host: String,
    pub message: String,
    pub stderr: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    InvalidYaml,
    InvalidExpression,
    InvalidFunction,
    UnknownModule,
    MissingField,
    InvalidValue,
}

impl std::error::Error for NexusError {}

impl fmt::Display for NexusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Set color mode based on TTY detection and NO_COLOR
        let use_colors = should_use_colors();
        if !use_colors {
            colored::control::set_override(false);
        }

        match self {
            NexusError::Parse(err) => {
                writeln!(f, "{}: {}", "ERROR".red().bold(), err.message)?;
                writeln!(f)?;

                if let Some(ref file) = err.file {
                    write!(f, "  {} ", "-->".blue())?;
                    write!(f, "{}", file.cyan())?;
                    if let Some(line) = err.line {
                        write!(f, ":{}", line)?;
                        if let Some(col) = err.column {
                            write!(f, ":{}", col)?;
                        }
                    }
                    writeln!(f)?;
                }

                if let Some(ref suggestion) = err.suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::Io { message, path } => {
                writeln!(f, "{}: {}", "I/O ERROR".red().bold(), message)?;
                if let Some(path) = path {
                    writeln!(f, "  {} {}", "Path:".dimmed(), path.display())?;
                }
                Ok(())
            }

            NexusError::Ssh {
                host,
                message,
                suggestion,
            } => {
                writeln!(f, "{}: {}", "SSH ERROR".red().bold(), message)?;
                writeln!(f, "  {} {}", "Host:".dimmed(), host)?;

                if let Some(suggestion) = suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::Module(err) => {
                writeln!(f, "{}: {}", "MODULE ERROR".red().bold(), err.message)?;
                writeln!(f, "  {} {}", "Module:".dimmed(), err.module)?;
                writeln!(f, "  {} {}", "Task:".dimmed(), err.task_name)?;
                writeln!(f, "  {} {}", "Host:".dimmed(), err.host)?;

                if let Some(ref stderr) = err.stderr {
                    if !stderr.is_empty() {
                        writeln!(f)?;
                        writeln!(f, "  {}:", "stderr".dimmed())?;
                        for line in stderr.lines().take(10) {
                            writeln!(f, "    {}", line)?;
                        }
                    }
                }

                if let Some(ref suggestion) = err.suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::Condition {
                expression,
                message,
                suggestion,
            } => {
                writeln!(f, "{}: {}", "CONDITION ERROR".red().bold(), message)?;
                writeln!(f, "  {} {}", "Expression:".dimmed(), expression)?;

                if let Some(suggestion) = suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::Inventory {
                message,
                suggestion,
            } => {
                writeln!(f, "{}: {}", "INVENTORY ERROR".red().bold(), message)?;

                if let Some(suggestion) = suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::Runtime {
                function,
                message,
                suggestion,
            } => {
                writeln!(f, "{}: {}", "RUNTIME ERROR".red().bold(), message)?;

                if let Some(func) = function {
                    writeln!(f, "  {} {}", "Function:".dimmed(), func)?;
                }

                if let Some(suggestion) = suggestion {
                    writeln!(f)?;
                    writeln!(f, "{}: {}", "Hint".yellow().bold(), suggestion)?;
                }

                Ok(())
            }

            NexusError::TaskFailed {
                task_name,
                host,
                condition,
            } => {
                writeln!(f, "{}: Task failed condition", "TASK FAILED".red().bold())?;
                writeln!(f, "  {} {}", "Task:".dimmed(), task_name)?;
                writeln!(f, "  {} {}", "Host:".dimmed(), host)?;
                writeln!(f, "  {} {}", "Condition:".dimmed(), condition)?;
                Ok(())
            }

            NexusError::Timeout {
                operation,
                duration_secs,
            } => {
                writeln!(
                    f,
                    "{}: {} timed out after {}s",
                    "TIMEOUT".red().bold(),
                    operation,
                    duration_secs
                )?;
                Ok(())
            }
        }
    }
}

/// Format a source code snippet with error highlighting
pub fn format_source_error(source: &str, line: usize, column: usize, message: &str) -> String {
    // Respect color settings
    let use_colors = should_use_colors();
    if !use_colors {
        colored::control::set_override(false);
    }

    let mut result = String::new();
    let lines: Vec<&str> = source.lines().collect();

    // Show context (2 lines before and after)
    let start = line.saturating_sub(2);
    let end = (line + 2).min(lines.len());

    for (i, src_line) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        let prefix = if line_num == line {
            format!("{:>4} {} ", line_num, ">".red())
        } else {
            format!("{:>4} {} ", line_num, "|".blue())
        };

        result.push_str(&prefix);
        result.push_str(src_line);
        result.push('\n');

        // Add error indicator
        if line_num == line {
            let spaces = " ".repeat(6 + column.saturating_sub(1));
            result.push_str(&spaces);
            result.push_str(&"^".red().to_string());

            if !message.is_empty() {
                result.push(' ');
                result.push_str(&message.red().to_string());
            }

            result.push('\n');
        }
    }

    result
}

/// Suggest common fixes for errors
pub fn suggest_fix(error: &NexusError) -> Option<String> {
    match error {
        NexusError::Ssh { message, .. } => {
            if message.contains("connection refused") {
                Some("Ensure SSH service is running on the target host".to_string())
            } else if message.contains("timeout") {
                Some("Check network connectivity and firewall rules".to_string())
            } else if message.contains("authentication") {
                Some("Verify SSH key or password is correct".to_string())
            } else {
                None
            }
        }

        NexusError::Module(err) => {
            if err.message.contains("not found") {
                Some("Check if the package/service name is correct".to_string())
            } else if err.message.contains("permission denied") {
                Some("Try running with elevated privileges (sudo)".to_string())
            } else {
                None
            }
        }

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_display() {
        let err = NexusError::Parse(Box::new(ParseError {
            kind: ParseErrorKind::UnknownModule,
            message: "Unknown module 'packages'".to_string(),
            file: Some("test.nx.yaml".to_string()),
            line: Some(12),
            column: Some(5),
            suggestion: Some("Did you mean 'package'?".to_string()),
        }));

        let output = format!("{}", err);
        // Strip ANSI codes for comparison
        let clean_output = console::strip_ansi_codes(&output);

        assert!(clean_output.contains("Unknown module"));
        assert!(clean_output.contains("test.nx.yaml:12:5"));
        assert!(clean_output.contains("package"));
    }
}
