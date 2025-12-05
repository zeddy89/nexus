// async_status module - check status of async jobs

use async_trait::async_trait;

use super::Module;
use crate::executor::{AsyncJobTracker, ExecutionContext, SshConnection, TaskOutput};
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

pub struct AsyncStatusModule {
    tracker: AsyncJobTracker,
}

impl AsyncStatusModule {
    pub fn new() -> Self {
        AsyncStatusModule {
            tracker: AsyncJobTracker::new(),
        }
    }

    pub async fn execute_with_job_id(
        &self,
        _ctx: &ExecutionContext,
        conn: &SshConnection,
        job_id: &str,
    ) -> Result<TaskOutput, NexusError> {
        let status = self.tracker.check_status(conn, job_id).await?;

        match status {
            crate::executor::JobStatus::Running { pid, started_at } => {
                let mut output = TaskOutput::new();
                output.changed = false;
                output.skipped = false;
                output.stdout = format!("Job {} is still running (PID: {})", job_id, pid);
                output
                    .data
                    .insert("job_id".to_string(), Value::String(job_id.to_string()));
                output
                    .data
                    .insert("status".to_string(), Value::String("running".to_string()));
                output
                    .data
                    .insert("pid".to_string(), Value::Int(pid as i64));
                output
                    .data
                    .insert("started_at".to_string(), Value::String(started_at));
                output
                    .data
                    .insert("finished".to_string(), Value::Bool(false));
                Ok(output)
            }
            crate::executor::JobStatus::Finished {
                exit_code,
                stdout,
                stderr,
            } => {
                let mut output = if exit_code == 0 {
                    TaskOutput::changed()
                } else {
                    TaskOutput::failed(format!("Job failed with exit code {}", exit_code))
                };
                output.stdout = stdout;
                output.stderr = stderr;
                output.exit_code = exit_code;
                output
                    .data
                    .insert("job_id".to_string(), Value::String(job_id.to_string()));
                output
                    .data
                    .insert("status".to_string(), Value::String("finished".to_string()));
                output
                    .data
                    .insert("finished".to_string(), Value::Bool(true));
                output
                    .data
                    .insert("rc".to_string(), Value::Int(exit_code as i64));
                Ok(output)
            }
            crate::executor::JobStatus::Failed { error } => Ok(TaskOutput::failed(error)
                .with_data("job_id", Value::String(job_id.to_string()))
                .with_data("status", Value::String("failed".to_string()))
                .with_data("finished", Value::Bool(true))),
            crate::executor::JobStatus::TimedOut => Ok(TaskOutput::failed("Job timed out")
                .with_data("job_id", Value::String(job_id.to_string()))
                .with_data("status", Value::String("timeout".to_string()))
                .with_data("finished", Value::Bool(true))),
            crate::executor::JobStatus::NotFound => Ok(TaskOutput::failed(
                "Job not found - may have been cleaned up",
            )
            .with_data("job_id", Value::String(job_id.to_string()))
            .with_data("status", Value::String("not_found".to_string()))
            .with_data("finished", Value::Bool(true))),
        }
    }
}

#[async_trait]
impl Module for AsyncStatusModule {
    fn name(&self) -> &'static str {
        "async_status"
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        _conn: &SshConnection,
    ) -> Result<TaskOutput, NexusError> {
        // This module requires a job_id parameter
        Err(NexusError::Runtime {
            function: None,
            message: "async_status module requires 'job_id' parameter".to_string(),
            suggestion: Some("Use: async_status: { job_id: <job_id> }".to_string()),
        })
    }
}

impl Default for AsyncStatusModule {
    fn default() -> Self {
        Self::new()
    }
}
