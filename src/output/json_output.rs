// JSON output for structured logging

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::terminal::{PlayRecap, TaskResult};

/// JSON output manager for machine-readable logging
pub struct JsonOutput {
    verbose: bool,
    quiet: bool,
}

impl JsonOutput {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        JsonOutput { verbose, quiet }
    }

    /// Print a header for a playbook run
    pub fn print_playbook_header(&self, playbook: &str, hosts_count: usize) {
        if self.quiet {
            return;
        }

        let event = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "playbook_start",
            "playbook": playbook,
            "hosts_count": hosts_count,
        });

        self.emit_json(&event);
    }

    /// Print a task header
    pub fn print_task_header(&self, task_name: &str) {
        if self.quiet {
            return;
        }

        let event = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "play_start",
            "name": task_name,
        });

        self.emit_json(&event);
    }

    /// Create a progress bar for a host (no-op for JSON, but required for interface compatibility)
    pub fn create_host_progress(&self, _host: &str) -> JsonProgressBar {
        JsonProgressBar
    }

    /// Print a task result for a host
    pub fn print_task_result(&self, result: &TaskResult) {
        if self.quiet && !result.failed {
            return;
        }

        let status = if result.failed {
            "failed"
        } else if result.changed {
            "changed"
        } else if result.skipped {
            "skipped"
        } else {
            "ok"
        };

        let mut event = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "task_complete",
            "host": result.host,
            "task": result.task_name,
            "status": status,
            "duration_ms": result.duration.as_millis(),
        });

        // Add optional fields
        let obj = event.as_object_mut().unwrap();

        if self.verbose || result.failed {
            let mut result_data = serde_json::Map::new();
            result_data.insert("changed".to_string(), json!(result.changed));

            if let Some(ref stdout) = result.stdout {
                if !stdout.is_empty() {
                    result_data.insert("stdout".to_string(), json!(stdout));
                }
            }

            if let Some(ref stderr) = result.stderr {
                if !stderr.is_empty() {
                    result_data.insert("stderr".to_string(), json!(stderr));
                }
            }

            if let Some(ref message) = result.message {
                if !message.is_empty() {
                    result_data.insert("message".to_string(), json!(message));
                }
            }

            if let Some(ref diff) = result.diff {
                if !diff.is_empty() {
                    result_data.insert("diff".to_string(), json!(diff));
                }
            }

            obj.insert("result".to_string(), json!(result_data));
        }

        self.emit_json(&event);
    }

    /// Print a colorized diff (no-op for JSON, handled in print_task_result)
    pub fn print_diff(&self, _diff: &str) {
        // Diff is included in task_complete event
    }

    /// Print the play recap summary
    pub fn print_recap(&self, recap: &PlayRecap) {
        if self.quiet {
            return;
        }

        let mut hosts_stats = HashMap::new();
        for (host, stats) in &recap.hosts {
            hosts_stats.insert(
                host.clone(),
                json!({
                    "ok": stats.ok,
                    "changed": stats.changed,
                    "failed": stats.failed,
                    "skipped": stats.skipped,
                }),
            );
        }

        let event = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "playbook_complete",
            "hosts": hosts_stats,
            "total_duration_ms": recap.total_duration.as_millis(),
            "total_failed": recap.total_failed(),
            "total_changed": recap.total_changed(),
            "has_failures": recap.has_failures(),
        });

        self.emit_json(&event);
    }

    /// Print streaming output from a command
    pub fn print_streaming_output(&self, host: &str, line: &str, is_stderr: bool) {
        if self.quiet {
            return;
        }

        let event = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "streaming_output",
            "host": host,
            "line": line,
            "stream": if is_stderr { "stderr" } else { "stdout" },
        });

        self.emit_json(&event);
    }

    /// Emit a JSON object as a single line (NDJSON format)
    fn emit_json(&self, value: &serde_json::Value) {
        if let Ok(json_str) = serde_json::to_string(value) {
            println!("{}", json_str);
        }
    }

    /// Get a dummy multi-progress reference (not used in JSON mode)
    pub fn multi_progress(&self) -> &JsonMultiProgress {
        &JSON_MULTI_PROGRESS
    }
}

/// Dummy progress bar for JSON output (no-op)
pub struct JsonProgressBar;

impl JsonProgressBar {
    pub fn set_prefix(&self, _prefix: String) {}
    pub fn set_message(&self, _msg: String) {}
    pub fn finish_with_message(&self, _msg: String) {}
    pub fn finish_and_clear(&self) {}
    pub fn enable_steady_tick(&self, _interval: Duration) {}
}

/// Dummy multi-progress for JSON output (no-op)
pub struct JsonMultiProgress;

impl JsonMultiProgress {
    pub fn add(&self, _pb: JsonProgressBar) -> JsonProgressBar {
        JsonProgressBar
    }
}

static JSON_MULTI_PROGRESS: JsonMultiProgress = JsonMultiProgress;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_output_task_result() {
        let output = JsonOutput::new(false, false);
        let result = TaskResult::ok("host1", "Install nginx")
            .with_stdout("Package installed")
            .with_duration(Duration::from_millis(1234));

        // Should not panic
        output.print_task_result(&result);
    }

    #[test]
    fn test_json_output_playbook_header() {
        let output = JsonOutput::new(false, false);
        output.print_playbook_header("webservers.yml", 5);
    }

    #[test]
    fn test_json_output_recap() {
        let output = JsonOutput::new(false, false);
        let mut recap = PlayRecap::new();
        recap.record(&TaskResult::ok("host1", "task1"));
        recap.record(&TaskResult::changed("host1", "task2"));
        recap.total_duration = Duration::from_secs(10);

        output.print_recap(&recap);
    }
}
