// Callback plugin system for Nexus

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;

use crate::executor::TaskOutput;
use crate::output::PlayRecap;

/// Trait for callback plugins that can hook into playbook execution lifecycle
#[async_trait]
pub trait CallbackPlugin: Send + Sync {
    /// Get the name of this plugin
    fn name(&self) -> &str;

    // Playbook lifecycle
    async fn on_playbook_start(&self, _playbook: &str, _hosts: &[String]) {}
    async fn on_playbook_complete(&self, _recap: &PlayRecap) {}

    // Play lifecycle
    async fn on_play_start(&self, _play: &str, _hosts: &[String]) {}

    // Task lifecycle
    async fn on_task_start(&self, _host: &str, _task: &str) {}
    async fn on_task_complete(
        &self,
        _host: &str,
        _task: &str,
        _result: &TaskOutput,
        _duration: Duration,
    ) {
    }
    async fn on_task_skipped(&self, _host: &str, _task: &str, _reason: &str) {}
    async fn on_task_failed(&self, _host: &str, _task: &str, _error: &str) {}

    // Handler lifecycle
    async fn on_handler_start(&self, _host: &str, _handler: &str) {}
    async fn on_handler_complete(&self, _host: &str, _handler: &str, _result: &TaskOutput) {}
}

/// Manager for callback plugins
pub struct CallbackManager {
    plugins: Vec<Box<dyn CallbackPlugin>>,
}

impl CallbackManager {
    /// Create a new callback manager
    pub fn new() -> Self {
        CallbackManager {
            plugins: Vec::new(),
        }
    }

    /// Add a callback plugin
    pub fn add(&mut self, plugin: Box<dyn CallbackPlugin>) {
        self.plugins.push(plugin);
    }

    /// Call on_playbook_start on all plugins
    pub async fn on_playbook_start(&self, playbook: &str, hosts: &[String]) {
        for plugin in &self.plugins {
            plugin.on_playbook_start(playbook, hosts).await;
        }
    }

    /// Call on_playbook_complete on all plugins
    pub async fn on_playbook_complete(&self, recap: &PlayRecap) {
        for plugin in &self.plugins {
            plugin.on_playbook_complete(recap).await;
        }
    }

    /// Call on_play_start on all plugins
    pub async fn on_play_start(&self, play: &str, hosts: &[String]) {
        for plugin in &self.plugins {
            plugin.on_play_start(play, hosts).await;
        }
    }

    /// Call on_task_start on all plugins
    pub async fn on_task_start(&self, host: &str, task: &str) {
        for plugin in &self.plugins {
            plugin.on_task_start(host, task).await;
        }
    }

    /// Call on_task_complete on all plugins
    pub async fn on_task_complete(
        &self,
        host: &str,
        task: &str,
        result: &TaskOutput,
        duration: Duration,
    ) {
        for plugin in &self.plugins {
            plugin.on_task_complete(host, task, result, duration).await;
        }
    }

    /// Call on_task_skipped on all plugins
    pub async fn on_task_skipped(&self, host: &str, task: &str, reason: &str) {
        for plugin in &self.plugins {
            plugin.on_task_skipped(host, task, reason).await;
        }
    }

    /// Call on_task_failed on all plugins
    pub async fn on_task_failed(&self, host: &str, task: &str, error: &str) {
        for plugin in &self.plugins {
            plugin.on_task_failed(host, task, error).await;
        }
    }

    /// Call on_handler_start on all plugins
    pub async fn on_handler_start(&self, host: &str, handler: &str) {
        for plugin in &self.plugins {
            plugin.on_handler_start(host, handler).await;
        }
    }

    /// Call on_handler_complete on all plugins
    pub async fn on_handler_complete(&self, host: &str, handler: &str, result: &TaskOutput) {
        for plugin in &self.plugins {
            plugin.on_handler_complete(host, handler, result).await;
        }
    }
}

impl Default for CallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

// ========== Built-in Plugins ==========

/// JSON log callback - writes events to a JSON file
pub struct JsonLogCallback {
    file: Arc<Mutex<File>>,
}

impl JsonLogCallback {
    /// Create a new JSON log callback that writes to the specified file
    pub fn new(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let file = OpenOptions::new().create(true).append(true).open(path)?;

        Ok(JsonLogCallback {
            file: Arc::new(Mutex::new(file)),
        })
    }

    fn write_event(&self, event: serde_json::Value) {
        let mut file = self.file.lock();
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = writeln!(file, "{}", json);
            let _ = file.flush();
        }
    }
}

#[async_trait]
impl CallbackPlugin for JsonLogCallback {
    fn name(&self) -> &str {
        "json_log"
    }

    async fn on_playbook_start(&self, playbook: &str, hosts: &[String]) {
        self.write_event(json!({
            "event": "playbook_start",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "playbook": playbook,
            "hosts": hosts,
        }));
    }

    async fn on_playbook_complete(&self, recap: &PlayRecap) {
        let hosts: HashMap<String, serde_json::Value> = recap
            .hosts
            .iter()
            .map(|(host, stats)| {
                (
                    host.clone(),
                    json!({
                        "ok": stats.ok,
                        "changed": stats.changed,
                        "failed": stats.failed,
                        "skipped": stats.skipped,
                    }),
                )
            })
            .collect();

        self.write_event(json!({
            "event": "playbook_complete",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "duration_secs": recap.total_duration.as_secs_f64(),
            "hosts": hosts,
        }));
    }

    async fn on_task_start(&self, host: &str, task: &str) {
        self.write_event(json!({
            "event": "task_start",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "host": host,
            "task": task,
        }));
    }

    async fn on_task_complete(
        &self,
        host: &str,
        task: &str,
        result: &TaskOutput,
        duration: Duration,
    ) {
        self.write_event(json!({
            "event": "task_complete",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "host": host,
            "task": task,
            "changed": result.changed,
            "failed": result.failed,
            "duration_secs": duration.as_secs_f64(),
            "stdout": result.stdout,
            "stderr": result.stderr,
        }));
    }

    async fn on_task_failed(&self, host: &str, task: &str, error: &str) {
        self.write_event(json!({
            "event": "task_failed",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "host": host,
            "task": task,
            "error": error,
        }));
    }
}

/// Timer callback - tracks task execution times and shows statistics
pub struct TimerCallback {
    task_times: Mutex<HashMap<String, Vec<Duration>>>,
    task_host_times: Mutex<HashMap<(String, String), Duration>>,
}

impl TimerCallback {
    /// Create a new timer callback
    pub fn new() -> Self {
        TimerCallback {
            task_times: Mutex::new(HashMap::new()),
            task_host_times: Mutex::new(HashMap::new()),
        }
    }

    /// Get timing statistics
    pub fn get_stats(&self) -> TimingStats {
        let task_times = self.task_times.lock();
        let mut stats = TimingStats {
            task_stats: Vec::new(),
            total_time: Duration::ZERO,
        };

        for (task, times) in task_times.iter() {
            if times.is_empty() {
                continue;
            }

            let total: Duration = times.iter().sum();
            let avg = total / times.len() as u32;
            let max = *times.iter().max().unwrap();
            let min = *times.iter().min().unwrap();

            stats.task_stats.push(TaskStats {
                task: task.clone(),
                count: times.len(),
                total,
                avg,
                min,
                max,
            });

            stats.total_time += total;
        }

        // Sort by total time (descending)
        stats.task_stats.sort_by(|a, b| b.total.cmp(&a.total));

        stats
    }
}

impl Default for TimerCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CallbackPlugin for TimerCallback {
    fn name(&self) -> &str {
        "timer"
    }

    async fn on_task_complete(
        &self,
        host: &str,
        task: &str,
        _result: &TaskOutput,
        duration: Duration,
    ) {
        // Record task time
        self.task_times
            .lock()
            .entry(task.to_string())
            .or_default()
            .push(duration);

        // Record host-specific time
        self.task_host_times
            .lock()
            .insert((host.to_string(), task.to_string()), duration);
    }

    async fn on_playbook_complete(&self, _recap: &PlayRecap) {
        let stats = self.get_stats();

        println!("\n{}", "=".repeat(60));
        println!("Task Timing Statistics");
        println!("{}", "=".repeat(60));

        if stats.task_stats.is_empty() {
            println!("No timing data collected");
            return;
        }

        println!("\nTop 10 Slowest Tasks:");
        println!("{:<40} {:>8} {:>10}", "Task", "Count", "Total Time");
        println!("{}", "-".repeat(60));

        for (i, task_stat) in stats.task_stats.iter().take(10).enumerate() {
            println!(
                "{:>2}. {:<37} {:>6}x {:>9.2}s",
                i + 1,
                truncate(&task_stat.task, 37),
                task_stat.count,
                task_stat.total.as_secs_f64()
            );
        }

        println!("\nTask Details:");
        println!("{:<40} {:>8} {:>8} {:>8}", "Task", "Avg", "Min", "Max");
        println!("{}", "-".repeat(60));

        for task_stat in stats.task_stats.iter().take(10) {
            println!(
                "{:<40} {:>7.2}s {:>7.2}s {:>7.2}s",
                truncate(&task_stat.task, 40),
                task_stat.avg.as_secs_f64(),
                task_stat.min.as_secs_f64(),
                task_stat.max.as_secs_f64()
            );
        }

        println!(
            "\nTotal task execution time: {:.2}s",
            stats.total_time.as_secs_f64()
        );
        println!("{}", "=".repeat(60));
    }
}

/// Webhook callback - POSTs events to a URL
pub struct WebhookCallback {
    url: String,
    client: reqwest::Client,
}

impl WebhookCallback {
    /// Create a new webhook callback
    pub fn new(url: impl Into<String>) -> Self {
        WebhookCallback {
            url: url.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn post_event(&self, event: serde_json::Value) {
        let _ = self.client.post(&self.url).json(&event).send().await;
    }
}

#[async_trait]
impl CallbackPlugin for WebhookCallback {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn on_playbook_start(&self, playbook: &str, hosts: &[String]) {
        self.post_event(json!({
            "event": "playbook_start",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "playbook": playbook,
            "hosts": hosts,
        }))
        .await;
    }

    async fn on_playbook_complete(&self, recap: &PlayRecap) {
        let hosts: HashMap<String, serde_json::Value> = recap
            .hosts
            .iter()
            .map(|(host, stats)| {
                (
                    host.clone(),
                    json!({
                        "ok": stats.ok,
                        "changed": stats.changed,
                        "failed": stats.failed,
                        "skipped": stats.skipped,
                    }),
                )
            })
            .collect();

        self.post_event(json!({
            "event": "playbook_complete",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "duration_secs": recap.total_duration.as_secs_f64(),
            "hosts": hosts,
            "has_failures": recap.has_failures(),
        }))
        .await;
    }

    async fn on_task_failed(&self, host: &str, task: &str, error: &str) {
        self.post_event(json!({
            "event": "task_failed",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "host": host,
            "task": task,
            "error": error,
        }))
        .await;
    }
}

/// Slack callback - sends notifications to Slack
pub struct SlackCallback {
    webhook_url: String,
    client: reqwest::Client,
}

impl SlackCallback {
    /// Create a new Slack callback
    pub fn new(webhook_url: impl Into<String>) -> Self {
        SlackCallback {
            webhook_url: webhook_url.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn send_message(&self, message: &str, color: &str) {
        let payload = json!({
            "attachments": [{
                "color": color,
                "text": message,
                "mrkdwn_in": ["text"]
            }]
        });

        let _ = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await;
    }
}

#[async_trait]
impl CallbackPlugin for SlackCallback {
    fn name(&self) -> &str {
        "slack"
    }

    async fn on_playbook_complete(&self, recap: &PlayRecap) {
        let total_ok: usize = recap.hosts.values().map(|s| s.ok).sum();
        let total_changed: usize = recap.hosts.values().map(|s| s.changed).sum();
        let total_failed: usize = recap.hosts.values().map(|s| s.failed).sum();
        let total_skipped: usize = recap.hosts.values().map(|s| s.skipped).sum();

        let (color, status) = if recap.has_failures() {
            ("danger", "FAILED")
        } else if total_changed > 0 {
            ("warning", "CHANGED")
        } else {
            ("good", "SUCCESS")
        };

        let message = format!(
            "*Playbook execution {}*\n\
            Duration: {:.2}s\n\
            Hosts: {}\n\
            OK: {} | Changed: {} | Failed: {} | Skipped: {}",
            status,
            recap.total_duration.as_secs_f64(),
            recap.hosts.len(),
            total_ok,
            total_changed,
            total_failed,
            total_skipped
        );

        self.send_message(&message, color).await;
    }

    async fn on_task_failed(&self, host: &str, task: &str, error: &str) {
        let message = format!(
            "*Task Failed*\n\
            Host: `{}`\n\
            Task: {}\n\
            Error: ```{}```",
            host, task, error
        );

        self.send_message(&message, "danger").await;
    }
}

// ========== Helper Types ==========

/// Statistics about task timing
#[derive(Debug, Clone)]
pub struct TimingStats {
    pub task_stats: Vec<TaskStats>,
    pub total_time: Duration,
}

/// Statistics for a single task
#[derive(Debug, Clone)]
pub struct TaskStats {
    pub task: String,
    pub count: usize,
    pub total: Duration,
    pub avg: Duration,
    pub min: Duration,
    pub max: Duration,
}

/// Parse callback plugin specification from CLI
/// Format: "plugin_name:args" or just "plugin_name"
pub fn parse_callback_spec(spec: &str) -> Result<(&str, Option<&str>), String> {
    if let Some((name, args)) = spec.split_once(':') {
        Ok((name, Some(args)))
    } else {
        Ok((spec, None))
    }
}

/// Create a callback plugin from a specification string
pub fn create_callback_plugin(spec: &str) -> Result<Box<dyn CallbackPlugin>, String> {
    let (name, args) = parse_callback_spec(spec)?;

    match name {
        "json_log" => {
            let path = args.ok_or_else(|| {
                "json_log callback requires a file path (e.g., json_log:/tmp/nexus.json)"
                    .to_string()
            })?;

            JsonLogCallback::new(path)
                .map(|p| Box::new(p) as Box<dyn CallbackPlugin>)
                .map_err(|e| format!("Failed to create json_log callback: {}", e))
        }

        "timer" => Ok(Box::new(TimerCallback::new())),

        "webhook" => {
            let url = args.ok_or_else(|| {
                "webhook callback requires a URL (e.g., webhook:https://example.com/events)"
                    .to_string()
            })?;

            Ok(Box::new(WebhookCallback::new(url)))
        }

        "slack" => {
            let webhook_url = args.ok_or_else(|| {
                "slack callback requires a webhook URL (e.g., slack:https://hooks.slack.com/...)".to_string()
            })?;

            Ok(Box::new(SlackCallback::new(webhook_url)))
        }

        _ => Err(format!("Unknown callback plugin: {}", name)),
    }
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_callback_spec() {
        assert_eq!(parse_callback_spec("timer").unwrap(), ("timer", None));

        assert_eq!(
            parse_callback_spec("json_log:/tmp/log.json").unwrap(),
            ("json_log", Some("/tmp/log.json"))
        );

        assert_eq!(
            parse_callback_spec("webhook:https://example.com/events").unwrap(),
            ("webhook", Some("https://example.com/events"))
        );
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("test", 4), "test");
    }
}
