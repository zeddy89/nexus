// Async job tracking for background tasks

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::context::TaskOutput;
use super::ssh::SshConnection;
use crate::output::errors::NexusError;

/// Unique identifier for async jobs
pub type JobId = String;

/// Status of an async job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is still running
    Running {
        pid: i32,
        started_at: String,
    },
    /// Job completed successfully
    Finished {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    /// Job failed
    Failed {
        error: String,
    },
    /// Job timed out
    TimedOut,
    /// Job not found (may have been cleaned up)
    NotFound,
}

/// Async job tracker
pub struct AsyncJobTracker {
    /// Map of host -> job_id -> job info
    jobs: Arc<Mutex<HashMap<String, HashMap<JobId, AsyncJobInfo>>>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AsyncJobInfo {
    job_id: JobId,
    host: String,
    pid: i32,
    started_at: Instant,
    timeout: Duration,
}

impl AsyncJobTracker {
    pub fn new() -> Self {
        AsyncJobTracker {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start an async job on a remote host
    pub async fn start_job(
        &self,
        conn: &SshConnection,
        command: &str,
        timeout: u64,
    ) -> Result<JobId, NexusError> {
        let job_id = generate_job_id();
        let host = conn.host_name().to_string();

        // Create the background job command using nohup
        // The job file will contain the PID and be used to track status
        let job_file = format!("/tmp/.nexus_async_{}", job_id);
        let out_file = format!("/tmp/.nexus_async_{}.out", job_id);
        let err_file = format!("/tmp/.nexus_async_{}.err", job_id);

        // Start the command in background:
        // 1. Redirect stdout/stderr to files
        // 2. Run in background with nohup
        // 3. Save PID to job file
        // 4. Return immediately
        let bg_command = format!(
            r#"nohup sh -c '({}) > {} 2> {} & echo $! > {} && echo "started:$!" > {}.status'"#,
            command, out_file, err_file, job_file, job_file
        );

        let result = conn.exec(&bg_command)?;

        if !result.success() {
            return Err(NexusError::Runtime {
                function: None,
                message: format!("Failed to start async job: {}", result.stderr),
                suggestion: Some("Check command syntax and permissions".to_string()),
            });
        }

        // Read the PID from the job file
        let pid_result = conn.exec(&format!("cat {}", job_file))?;
        let pid: i32 = pid_result
            .stdout
            .trim()
            .parse()
            .map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to parse PID: {}", e),
                suggestion: None,
            })?;

        // Track the job
        let job_info = AsyncJobInfo {
            job_id: job_id.clone(),
            host: host.clone(),
            pid,
            started_at: Instant::now(),
            timeout: Duration::from_secs(timeout),
        };

        self.jobs
            .lock()
            .entry(host)
            .or_default()
            .insert(job_id.clone(), job_info);

        Ok(job_id)
    }

    /// Check the status of an async job
    pub async fn check_status(
        &self,
        conn: &SshConnection,
        job_id: &str,
    ) -> Result<JobStatus, NexusError> {
        let _host = conn.host_name();
        let job_file = format!("/tmp/.nexus_async_{}", job_id);
        let out_file = format!("/tmp/.nexus_async_{}.out", job_id);
        let err_file = format!("/tmp/.nexus_async_{}.err", job_id);

        // Check if job file exists
        let exists = conn.exec(&format!("test -f {}", job_file))?.success();
        if !exists {
            return Ok(JobStatus::NotFound);
        }

        // Read the PID
        let pid_result = conn.exec(&format!("cat {}", job_file))?;
        let pid: i32 = pid_result
            .stdout
            .trim()
            .parse()
            .unwrap_or(0);

        // Check if process is still running
        let is_running = conn.exec(&format!("kill -0 {} 2>/dev/null", pid))?.success();

        if is_running {
            // Process still running
            return Ok(JobStatus::Running {
                pid,
                started_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // Process finished - collect output
        let stdout = conn
            .exec(&format!("cat {} 2>/dev/null || echo ''", out_file))?
            .stdout;
        let stderr = conn
            .exec(&format!("cat {} 2>/dev/null || echo ''", err_file))?
            .stderr;

        // Get exit code from status file if it exists
        let exit_code_result = conn.exec(&format!("cat {}.exit 2>/dev/null || echo 0", job_file))?;
        let exit_code: i32 = exit_code_result
            .stdout
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(JobStatus::Finished {
            exit_code,
            stdout,
            stderr,
        })
    }

    /// Poll for job completion with retries
    pub async fn poll_until_complete(
        &self,
        conn: &SshConnection,
        job_id: &str,
        poll_interval: u64,
        max_retries: u32,
    ) -> Result<TaskOutput, NexusError> {
        let poll_duration = Duration::from_secs(poll_interval);
        let mut attempts = 0;

        loop {
            let status = self.check_status(conn, job_id).await?;

            match status {
                JobStatus::Running { .. } => {
                    attempts += 1;
                    if attempts >= max_retries {
                        // Timeout - kill the job
                        self.kill_job(conn, job_id).await?;
                        return Ok(TaskOutput::failed(format!(
                            "Async job timed out after {} polls",
                            max_retries
                        )));
                    }

                    // Wait before next poll
                    tokio::time::sleep(poll_duration).await;
                }
                JobStatus::Finished { exit_code, stdout, stderr } => {
                    // Cleanup job files
                    self.cleanup_job(conn, job_id).await.ok();

                    if exit_code == 0 {
                        return Ok(TaskOutput::changed()
                            .with_stdout(stdout)
                            .with_stderr(stderr));
                    } else {
                        let mut output = TaskOutput::failed(format!(
                            "Async job failed with exit code {}",
                            exit_code
                        ));
                        output.stdout = stdout;
                        output.stderr = stderr;
                        output.exit_code = exit_code;
                        return Ok(output);
                    }
                }
                JobStatus::Failed { error } => {
                    return Ok(TaskOutput::failed(error));
                }
                JobStatus::TimedOut => {
                    return Ok(TaskOutput::failed("Async job timed out"));
                }
                JobStatus::NotFound => {
                    return Ok(TaskOutput::failed("Async job not found"));
                }
            }
        }
    }

    /// Kill a running async job
    pub async fn kill_job(
        &self,
        conn: &SshConnection,
        job_id: &str,
    ) -> Result<(), NexusError> {
        let job_file = format!("/tmp/.nexus_async_{}", job_id);

        // Read PID
        let pid_result = conn.exec(&format!("cat {} 2>/dev/null || echo 0", job_file))?;
        let pid: i32 = pid_result.stdout.trim().parse().unwrap_or(0);

        if pid > 0 {
            // Kill the process and all children
            conn.exec(&format!("kill -TERM -{} 2>/dev/null || kill -TERM {} 2>/dev/null", pid, pid))?;
        }

        Ok(())
    }

    /// Cleanup job files from remote host
    pub async fn cleanup_job(
        &self,
        conn: &SshConnection,
        job_id: &str,
    ) -> Result<(), NexusError> {
        let pattern = format!("/tmp/.nexus_async_{}*", job_id);
        conn.exec(&format!("rm -f {}", pattern))?;

        // Remove from tracker
        let host = conn.host_name().to_string();
        if let Some(host_jobs) = self.jobs.lock().get_mut(&host) {
            host_jobs.remove(job_id);
        }

        Ok(())
    }
}

impl Default for AsyncJobTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique job ID
fn generate_job_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // Use timestamp + random suffix for uniqueness
    let random: u32 = rand::random();
    format!("{:x}_{:x}", now, random)
}

// Add rand dependency if not already present
extern crate rand;
