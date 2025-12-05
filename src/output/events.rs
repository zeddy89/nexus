// Event system for TUI updates

use std::time::Duration;
use tokio::sync::mpsc;

use super::terminal::{PlayRecap, TaskResult};

/// Events emitted during playbook execution
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Playbook execution started
    PlaybookStart {
        name: String,
        hosts: Vec<String>,
        total_tasks: usize,
    },

    /// Task started on a host
    TaskStart {
        host: String,
        task: String,
    },

    /// Task completed on a host
    TaskComplete {
        host: String,
        task: String,
        status: TaskStatus,
        duration: Duration,
    },

    /// Task was skipped on a host
    TaskSkipped {
        host: String,
        task: String,
    },

    /// Task failed on a host
    TaskFailed {
        host: String,
        task: String,
        error: String,
    },

    /// Log output from a task
    Log {
        host: String,
        message: String,
    },

    /// Playbook execution completed
    PlaybookComplete {
        recap: PlayRecap,
    },
}

/// Status of a completed task
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task completed without changes
    Ok,
    /// Task completed with changes
    Changed,
    /// Task failed
    Failed,
    /// Task was skipped
    Skipped,
}

impl From<&TaskResult> for TaskStatus {
    fn from(result: &TaskResult) -> Self {
        if result.failed {
            TaskStatus::Failed
        } else if result.skipped {
            TaskStatus::Skipped
        } else if result.changed {
            TaskStatus::Changed
        } else {
            TaskStatus::Ok
        }
    }
}

/// Event emitter for sending execution events
#[derive(Clone)]
pub struct EventEmitter {
    tx: mpsc::UnboundedSender<ExecutionEvent>,
}

impl EventEmitter {
    /// Create a new event emitter with the given channel
    pub fn new(tx: mpsc::UnboundedSender<ExecutionEvent>) -> Self {
        EventEmitter { tx }
    }

    /// Emit a playbook start event
    pub fn playbook_start(&self, name: String, hosts: Vec<String>, total_tasks: usize) {
        let _ = self.tx.send(ExecutionEvent::PlaybookStart {
            name,
            hosts,
            total_tasks,
        });
    }

    /// Emit a task start event
    pub fn task_start(&self, host: String, task: String) {
        let _ = self.tx.send(ExecutionEvent::TaskStart { host, task });
    }

    /// Emit a task complete event
    pub fn task_complete(&self, host: String, task: String, status: TaskStatus, duration: Duration) {
        let _ = self.tx.send(ExecutionEvent::TaskComplete {
            host,
            task,
            status,
            duration,
        });
    }

    /// Emit a task skipped event
    pub fn task_skipped(&self, host: String, task: String) {
        let _ = self.tx.send(ExecutionEvent::TaskSkipped { host, task });
    }

    /// Emit a task failed event
    pub fn task_failed(&self, host: String, task: String, error: String) {
        let _ = self.tx.send(ExecutionEvent::TaskFailed { host, task, error });
    }

    /// Emit a log event
    pub fn log(&self, host: String, message: String) {
        let _ = self.tx.send(ExecutionEvent::Log { host, message });
    }

    /// Emit a playbook complete event
    pub fn playbook_complete(&self, recap: PlayRecap) {
        let _ = self.tx.send(ExecutionEvent::PlaybookComplete { recap });
    }
}

/// Create a new event channel
pub fn create_event_channel() -> (EventEmitter, mpsc::UnboundedReceiver<ExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (EventEmitter::new(tx), rx)
}
