// Nexus - Next-Generation Infrastructure Automation
//
// A modern infrastructure automation tool that fixes Ansible's core problems
// while keeping its simplicity for basic tasks.

pub mod executor;
pub mod inventory;
pub mod modules;
pub mod output;
pub mod parser;
pub mod plugins;
pub mod runtime;
pub mod vault;

pub use executor::{ExecutionContext, Scheduler, SchedulerConfig, TaskOutput};
pub use inventory::{Host, HostGroup, Inventory};
pub use output::{NexusError, PlayRecap, TaskResult, TerminalOutput};
pub use parser::{parse_playbook, parse_playbook_file, Playbook};
pub use runtime::evaluate_expression;

/// Version of the Nexus tool
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Re-export commonly used types
pub mod prelude {
    pub use crate::executor::{ExecutionContext, Scheduler, SchedulerConfig};
    pub use crate::inventory::{Host, Inventory};
    pub use crate::output::{NexusError, PlayRecap, TaskResult, TerminalOutput};
    pub use crate::parser::{parse_playbook, parse_playbook_file, Playbook};
}
