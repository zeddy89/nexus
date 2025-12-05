// Executor module - task execution engine

use crate::output::errors::NexusError;
use async_trait::async_trait;

pub mod async_jobs;
pub mod checkpoint;
pub mod context;
pub mod dag;
pub mod facts;
pub mod handlers;
pub mod include_handler;
pub mod local;
pub mod plan;
pub mod retry;
pub mod scheduler;
pub mod ssh;
pub mod tags;

pub use async_jobs::{AsyncJobTracker, JobId, JobStatus};
pub use checkpoint::{Checkpoint, CheckpointInfo, CheckpointManager, TaskKey};
pub use context::{ExecutionContext, TaskOutput};
pub use dag::TaskDag;
pub use facts::{FactCache, FactCategory, FactGatherer, HostFacts};
pub use handlers::{FlushMode, HandlerConfig, HandlerRegistry};
pub use local::LocalConnection;
pub use plan::{ChangeType, ExecutionPlan, HostPlan, PlanGenerator, PlannedChange, SshConfig};
pub use retry::{
    calculate_delay, CircuitBreaker, CircuitBreakerRegistry, CircuitState, RetryResult,
};
pub use scheduler::{Scheduler, SchedulerConfig};
pub use ssh::{CommandResult, ConnectionPool, ConnectionType, SshConnection};
pub use tags::TagFilter;

/// Common trait for all connection types (SSH, local, etc.)
#[async_trait]
pub trait Connection: Send + Sync {
    /// Execute a command and return the result
    async fn exec(&self, cmd: &str) -> Result<CommandResult, NexusError>;

    /// Execute a command with streaming output callbacks
    /// Note: For simplicity, callbacks receive owned Strings
    async fn exec_streaming(
        &self,
        cmd: &str,
        on_stdout: Box<dyn Fn(String) + Send + Sync>,
        on_stderr: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<CommandResult, NexusError>;

    /// Read a file from the target
    async fn read_file(&self, path: &str) -> Result<String, NexusError>;

    /// Write content to a file on the target
    async fn write_file(&self, path: &str, content: &str) -> Result<(), NexusError>;

    /// Get the host name for this connection
    fn host_name(&self) -> &str;
}
