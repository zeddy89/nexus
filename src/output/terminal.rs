// Rich terminal output for Nexus

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use colored::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parking_lot::Mutex;

/// Terminal output manager
pub struct TerminalOutput {
    multi_progress: MultiProgress,
    verbose: bool,
    quiet: bool,
    is_tty: bool,
}

impl TerminalOutput {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        let is_tty = atty::is(atty::Stream::Stdout);

        // Respect NO_COLOR environment variable (https://no-color.org/)
        // Also disable colors if not a TTY
        if std::env::var("NO_COLOR").is_ok() || !is_tty {
            colored::control::set_override(false);
        }

        TerminalOutput {
            multi_progress: MultiProgress::new(),
            verbose,
            quiet,
            is_tty,
        }
    }

    /// Print a header for a playbook run
    pub fn print_playbook_header(&self, playbook: &str, hosts_count: usize) {
        if self.quiet {
            return;
        }

        println!();
        println!(
            "{} {} ({} hosts)",
            "PLAY".green().bold(),
            playbook.cyan(),
            hosts_count
        );
        println!("{}", "─".repeat(60).dimmed());
    }

    /// Print a task header
    pub fn print_task_header(&self, task_name: &str) {
        if self.quiet {
            return;
        }

        println!();
        println!("{} {}", "TASK".yellow().bold(), task_name);
    }

    /// Create a progress bar for a host
    pub fn create_host_progress(&self, host: &str) -> ProgressBar {
        let pb = self.multi_progress.add(ProgressBar::new_spinner());

        let style = if self.is_tty {
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {prefix:.bold} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        } else {
            ProgressStyle::default_spinner()
                .template("{prefix} {msg}")
                .unwrap()
        };

        pb.set_style(style);
        pb.set_prefix(host.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    /// Print a task result for a host
    pub fn print_task_result(&self, result: &TaskResult) {
        if self.quiet && !result.failed {
            return;
        }

        let status = if result.failed {
            "FAILED".red().bold()
        } else if result.changed {
            "CHANGED".yellow()
        } else if result.skipped {
            "SKIPPED".cyan()
        } else {
            "OK".green()
        };

        println!(
            "  {} {} {}",
            status,
            "=>".dimmed(),
            result.host.white().bold()
        );

        if self.verbose || result.failed {
            if let Some(stdout) = &result.stdout {
                if !stdout.is_empty() {
                    for line in stdout.lines() {
                        println!("      {}", line.dimmed());
                    }
                }
            }

            if let Some(stderr) = &result.stderr {
                if !stderr.is_empty() {
                    for line in stderr.lines() {
                        println!("      {}", line.red());
                    }
                }
            }

            if let Some(msg) = &result.message {
                if !msg.is_empty() {
                    println!("      {}", msg);
                }
            }
        }

        // Display diff if present
        if let Some(diff) = &result.diff {
            if !diff.is_empty() {
                self.print_diff(diff);
            }
        }
    }

    /// Print a colorized diff
    pub fn print_diff(&self, diff: &str) {
        println!();
        for line in diff.lines() {
            if line.starts_with("---") || line.starts_with("+++") {
                println!("      {}", line.bold());
            } else if line.starts_with("@@") {
                println!("      {}", line.cyan());
            } else if line.starts_with('+') && !line.starts_with("+++") {
                println!("      {}", line.green());
            } else if line.starts_with('-') && !line.starts_with("---") {
                println!("      {}", line.red());
            } else {
                println!("      {}", line);
            }
        }
        println!();
    }

    /// Print the play recap summary
    pub fn print_recap(&self, recap: &PlayRecap) {
        if self.quiet {
            return;
        }

        println!();
        println!("{}", "PLAY RECAP".green().bold());
        println!("{}", "─".repeat(60).dimmed());

        for (host, stats) in &recap.hosts {
            let ok = format!("ok={}", stats.ok).green();
            let changed = if stats.changed > 0 {
                format!("changed={}", stats.changed).yellow()
            } else {
                format!("changed={}", stats.changed).normal()
            };
            let failed = if stats.failed > 0 {
                format!("failed={}", stats.failed).red().bold()
            } else {
                format!("failed={}", stats.failed).normal()
            };
            let skipped = format!("skipped={}", stats.skipped).cyan();

            println!(
                "{:<30} : {}    {}    {}    {}",
                host.white().bold(),
                ok,
                changed,
                failed,
                skipped
            );
        }

        // Print overall timing
        println!();
        println!("Total time: {:.2}s", recap.total_duration.as_secs_f64());
    }

    /// Print streaming output from a command
    pub fn print_streaming_output(&self, host: &str, line: &str, is_stderr: bool) {
        if self.quiet {
            return;
        }

        let prefix = format!("[{}]", host).dimmed();
        if is_stderr {
            println!("{} {}", prefix, line.red());
        } else {
            println!("{} {}", prefix, line);
        }
    }

    /// Get the multi-progress bar for concurrent operations
    pub fn multi_progress(&self) -> &MultiProgress {
        &self.multi_progress
    }
}

/// Result of a single task execution on a host
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub host: String,
    pub task_name: String,
    pub changed: bool,
    pub failed: bool,
    pub skipped: bool,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub message: Option<String>,
    pub duration: Duration,
    /// Diff output for file changes
    pub diff: Option<String>,
}

impl Default for TaskResult {
    fn default() -> Self {
        TaskResult {
            host: String::new(),
            task_name: String::new(),
            changed: false,
            failed: false,
            skipped: false,
            stdout: None,
            stderr: None,
            message: None,
            duration: Duration::ZERO,
            diff: None,
        }
    }
}

impl TaskResult {
    pub fn ok(host: impl Into<String>, task_name: impl Into<String>) -> Self {
        TaskResult {
            host: host.into(),
            task_name: task_name.into(),
            ..Default::default()
        }
    }

    pub fn changed(host: impl Into<String>, task_name: impl Into<String>) -> Self {
        TaskResult {
            host: host.into(),
            task_name: task_name.into(),
            changed: true,
            ..Default::default()
        }
    }

    pub fn failed(
        host: impl Into<String>,
        task_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        TaskResult {
            host: host.into(),
            task_name: task_name.into(),
            failed: true,
            message: Some(message.into()),
            ..Default::default()
        }
    }

    pub fn skipped(host: impl Into<String>, task_name: impl Into<String>) -> Self {
        TaskResult {
            host: host.into(),
            task_name: task_name.into(),
            skipped: true,
            ..Default::default()
        }
    }

    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = Some(stdout.into());
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = Some(stderr.into());
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

/// Statistics for a single host
#[derive(Debug, Default, Clone)]
pub struct HostStats {
    pub ok: usize,
    pub changed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl HostStats {
    pub fn record(&mut self, result: &TaskResult) {
        if result.failed {
            self.failed += 1;
        } else if result.skipped {
            self.skipped += 1;
        } else if result.changed {
            self.changed += 1;
        } else {
            self.ok += 1;
        }
    }
}

/// Summary of the entire play
#[derive(Debug, Default, Clone)]
pub struct PlayRecap {
    pub hosts: std::collections::HashMap<String, HostStats>,
    pub total_duration: Duration,
}

impl PlayRecap {
    pub fn new() -> Self {
        PlayRecap::default()
    }

    pub fn record(&mut self, result: &TaskResult) {
        self.hosts
            .entry(result.host.clone())
            .or_default()
            .record(result);
    }

    pub fn has_failures(&self) -> bool {
        self.hosts.values().any(|s| s.failed > 0)
    }

    pub fn total_failed(&self) -> usize {
        self.hosts.values().map(|s| s.failed).sum()
    }

    pub fn total_changed(&self) -> usize {
        self.hosts.values().map(|s| s.changed).sum()
    }
}

/// Streaming output handler for real-time command output
#[allow(dead_code)]
pub struct StreamingOutput {
    host: String,
    task: String,
    output: Arc<Mutex<super::OutputWriter>>,
    start_time: Instant,
    cancelled: Arc<AtomicBool>,
}

impl StreamingOutput {
    pub fn new(host: String, task: String, output: Arc<Mutex<super::OutputWriter>>) -> Self {
        StreamingOutput {
            host,
            task,
            output,
            start_time: Instant::now(),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn write_stdout(&self, data: &[u8]) {
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(text) = std::str::from_utf8(data) {
            let out = self.output.lock();
            for line in text.lines() {
                out.print_streaming_output(&self.host, line, false);
            }
        }
    }

    pub fn write_stderr(&self, data: &[u8]) {
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(text) = std::str::from_utf8(data) {
            let out = self.output.lock();
            for line in text.lines() {
                out.print_streaming_output(&self.host, line, true);
            }
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Helper for checking if stdout is a TTY
mod atty {
    use std::io::IsTerminal;

    pub enum Stream {
        Stdout,
    }

    pub fn is(stream: Stream) -> bool {
        match stream {
            Stream::Stdout => std::io::stdout().is_terminal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_result_builders() {
        let ok = TaskResult::ok("host1", "Install nginx");
        assert!(!ok.failed);
        assert!(!ok.changed);

        let changed = TaskResult::changed("host1", "Install nginx");
        assert!(changed.changed);

        let failed = TaskResult::failed("host1", "Install nginx", "Package not found");
        assert!(failed.failed);
        assert_eq!(failed.message.as_deref(), Some("Package not found"));
    }

    #[test]
    fn test_play_recap() {
        let mut recap = PlayRecap::new();

        recap.record(&TaskResult::ok("host1", "task1"));
        recap.record(&TaskResult::changed("host1", "task2"));
        recap.record(&TaskResult::failed("host2", "task1", "error"));

        assert!(recap.has_failures());
        assert_eq!(recap.total_failed(), 1);
        assert_eq!(recap.total_changed(), 1);
    }
}
