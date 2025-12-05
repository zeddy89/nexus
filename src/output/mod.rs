// Output module for Nexus

use indicatif::{MultiProgress, ProgressBar};
use once_cell::sync::Lazy;

pub mod diff;
pub mod errors;
pub mod events;
pub mod json_output;
pub mod plan;
pub mod terminal;
pub mod tui;

pub use diff::*;
pub use errors::*;
pub use events::*;
pub use json_output::*;
pub use plan::*;
pub use terminal::*;
pub use tui::*;

/// Output format for Nexus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text output with colors
    #[default]
    Text,
    /// Machine-readable JSON output (NDJSON format)
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            _ => Err(()),
        }
    }
}

/// Unified output writer supporting both text and JSON formats
pub enum OutputWriter {
    Text(TerminalOutput),
    Json(JsonOutput),
    /// Silent mode for TUI - suppresses all output
    Silent,
}

impl OutputWriter {
    pub fn new(format: OutputFormat, verbose: bool, quiet: bool) -> Self {
        match format {
            OutputFormat::Text => OutputWriter::Text(TerminalOutput::new(verbose, quiet)),
            OutputFormat::Json => OutputWriter::Json(JsonOutput::new(verbose, quiet)),
        }
    }

    /// Create a silent output writer (for TUI mode)
    pub fn silent() -> Self {
        OutputWriter::Silent
    }

    pub fn print_playbook_header(&self, playbook: &str, hosts_count: usize) {
        match self {
            OutputWriter::Text(output) => output.print_playbook_header(playbook, hosts_count),
            OutputWriter::Json(output) => output.print_playbook_header(playbook, hosts_count),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn print_task_header(&self, task_name: &str) {
        match self {
            OutputWriter::Text(output) => output.print_task_header(task_name),
            OutputWriter::Json(output) => output.print_task_header(task_name),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn create_host_progress(&self, host: &str) -> ProgressBar {
        match self {
            OutputWriter::Text(output) => output.create_host_progress(host),
            OutputWriter::Json(_output) => ProgressBar::hidden(),
            OutputWriter::Silent => ProgressBar::hidden(),
        }
    }

    pub fn print_task_result(&self, result: &TaskResult) {
        match self {
            OutputWriter::Text(output) => output.print_task_result(result),
            OutputWriter::Json(output) => output.print_task_result(result),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn print_diff(&self, diff: &str) {
        match self {
            OutputWriter::Text(output) => output.print_diff(diff),
            OutputWriter::Json(output) => output.print_diff(diff),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn print_recap(&self, recap: &PlayRecap) {
        match self {
            OutputWriter::Text(output) => output.print_recap(recap),
            OutputWriter::Json(output) => output.print_recap(recap),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn print_streaming_output(&self, host: &str, line: &str, is_stderr: bool) {
        match self {
            OutputWriter::Text(output) => output.print_streaming_output(host, line, is_stderr),
            OutputWriter::Json(output) => output.print_streaming_output(host, line, is_stderr),
            OutputWriter::Silent => {} // No output in TUI mode
        }
    }

    pub fn multi_progress(&self) -> &MultiProgress {
        match self {
            OutputWriter::Text(output) => output.multi_progress(),
            OutputWriter::Json(_output) => &JSON_NO_OP_MULTI_PROGRESS,
            OutputWriter::Silent => &JSON_NO_OP_MULTI_PROGRESS,
        }
    }
}

// Static no-op multi-progress for JSON mode
static JSON_NO_OP_MULTI_PROGRESS: Lazy<MultiProgress> = Lazy::new(MultiProgress::new);
