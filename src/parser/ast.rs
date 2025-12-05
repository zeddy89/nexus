// Abstract Syntax Tree types for Nexus playbooks

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Source location for error reporting
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// Serial execution configuration - controls how many hosts to run on at a time
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Serial {
    /// Run on a fixed number of hosts at a time (e.g., serial: 2)
    Count(usize),
    /// Run on a percentage of hosts at a time (e.g., serial: "25%")
    Percentage(u8),
    /// Progressive batches - run on different batch sizes (e.g., serial: [1, 5, 10])
    List(Vec<usize>),
}

/// Execution strategy - controls how tasks are executed across hosts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionStrategy {
    /// Linear strategy - wait for all hosts to complete a task before moving to next task
    #[default]
    Linear,
    /// Free strategy - each host proceeds independently through tasks
    Free,
}

/// A complete Nexus playbook
#[derive(Debug, Clone)]
pub struct Playbook {
    pub source_file: String,
    pub hosts: HostPattern,
    pub vars: HashMap<String, Value>,
    pub tasks: Vec<TaskOrBlock>,
    pub handlers: Vec<Handler>,
    pub functions: Option<FunctionBlock>,
    /// Default privilege escalation for all tasks
    pub sudo: bool,
    /// Default user to run as (via sudo -u)
    pub sudo_user: Option<String>,
    /// Roles to execute (in order)
    pub roles: Vec<RoleRef>,
    /// Pre-tasks run before roles
    pub pre_tasks: Vec<TaskOrBlock>,
    /// Post-tasks run after roles
    pub post_tasks: Vec<TaskOrBlock>,
    /// Auto-gather facts at play start
    pub gather_facts: bool,
    /// Connection type (local, ssh, etc.)
    pub connection: Option<String>,
    /// Serial execution - run on N hosts at a time (rolling deployment)
    pub serial: Option<Serial>,
    /// Max concurrent tasks across all hosts
    pub throttle: Option<usize>,
    /// Execution strategy (linear vs free)
    pub strategy: ExecutionStrategy,
}

/// Either a Task or a Block - unified representation in playbooks
#[derive(Debug, Clone)]
pub enum TaskOrBlock {
    Task(Box<Task>),
    Block(Block),
    Import(ImportTasks),
    Include(IncludeTasks),
}

/// Host targeting pattern
#[derive(Debug, Clone, PartialEq, Default)]
pub enum HostPattern {
    /// All hosts
    #[default]
    All,
    /// A specific group name
    Group(String),
    /// Multiple groups with intersection/union
    Pattern(String),
    /// Inline host list defined in playbook (Nexus differentiator)
    Inline(Vec<InlineHost>),
    /// Special pattern for localhost-only execution
    Localhost,
}

/// Inline host definition for playbook-embedded hosts
#[derive(Debug, Clone, PartialEq)]
pub struct InlineHost {
    /// Host name/identifier
    pub name: String,
    /// IP address or hostname to connect to
    pub address: Option<String>,
    /// SSH port (default: 22)
    pub port: Option<u16>,
    /// SSH user
    pub user: Option<String>,
    /// Host-specific variables
    pub vars: HashMap<String, Value>,
}

/// A single task in a playbook
#[derive(Debug, Clone)]
pub struct Task {
    pub name: String,
    pub module: ModuleCall,
    pub when: Option<Expression>,
    pub register: Option<String>,
    pub fail_when: Option<Expression>,
    pub changed_when: Option<Expression>,
    pub notify: Vec<String>,
    pub loop_expr: Option<Expression>,
    pub loop_var: String,
    pub location: Option<SourceLocation>,
    /// Override sudo for this task (None = use playbook default)
    pub sudo: Option<bool>,
    /// Run as specific user (e.g., "postgres", "root")
    pub run_as: Option<String>,
    /// Tags for filtering task execution (supports expressions like "deploy and not test")
    pub tags: Vec<String>,
    /// Retry configuration with circuit breaker support
    pub retry: Option<RetryConfig>,
    /// Async execution configuration
    pub async_config: Option<AsyncConfig>,
    /// Timeout for this specific task (overrides global)
    pub timeout: Option<Duration>,
    /// Throttle - max concurrent executions of this task across all hosts
    pub throttle: Option<usize>,
    /// Host to run on (delegate execution to different host)
    pub delegate_to: Option<Expression>,
    /// Store facts from delegate (default: false)
    pub delegate_facts: bool,
}

// ============================================================================
// Import/Include Tasks - Task File Inclusion
// ============================================================================

/// Static import of tasks - resolved at parse time
#[derive(Debug, Clone)]
pub struct ImportTasks {
    /// File path to import (static, no variables)
    pub file: String,
    /// Variables to pass to imported tasks
    pub vars: HashMap<String, Value>,
    /// Tags to apply to imported tasks
    pub tags: Vec<String>,
    /// Source location
    pub location: Option<SourceLocation>,
}

/// Dynamic include of tasks - resolved at runtime
#[derive(Debug, Clone)]
pub struct IncludeTasks {
    /// File path expression (can contain variables)
    pub file: Expression,
    /// Variables to pass to included tasks (can contain expressions)
    pub vars: HashMap<String, Expression>,
    /// Conditional execution
    pub when: Option<Expression>,
    /// Loop over items
    pub loop_expr: Option<Expression>,
    /// Loop variable name
    pub loop_var: String,
    /// Tags to apply to included tasks
    pub tags: Vec<String>,
    /// Source location
    pub location: Option<SourceLocation>,
}

// ============================================================================
// Block (try/rescue/finally) - Error Handling Pattern
// ============================================================================

/// Block structure for error handling - similar to try/rescue/finally
/// Better than Ansible's limited block/rescue/always support
#[derive(Debug, Clone)]
pub struct Block {
    /// Optional name for the block
    pub name: Option<String>,
    /// Main tasks to execute (the "try" section)
    pub block: Vec<Task>,
    /// Tasks to run if block fails (the "rescue" section)
    pub rescue: Vec<Task>,
    /// Tasks that always run regardless of outcome (the "finally" section)
    pub always: Vec<Task>,
    /// Conditional execution for the entire block
    pub when: Option<Expression>,
    /// Tags for filtering block execution
    pub tags: Vec<String>,
    /// Location in source
    pub location: Option<SourceLocation>,
}

// ============================================================================
// Retry Configuration - Circuit Breaker Pattern
// ============================================================================

/// Retry configuration - Better than Ansible's simple retries
/// Supports circuit breaker pattern for intelligent failure handling
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts (including first try)
    pub attempts: u32,
    /// Delay strategy between retries
    pub delay: DelayStrategy,
    /// Condition that must be true for retry to happen (e.g., "result.rc != 0")
    pub retry_when: Option<Expression>,
    /// Condition that means success - stop retrying (e.g., "result.stdout contains 'ready'")
    pub until: Option<Expression>,
    /// Circuit breaker configuration for shared failure tracking
    pub circuit_breaker: Option<CircuitBreakerConfig>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            attempts: 3,
            delay: DelayStrategy::Fixed(Duration::from_secs(5)),
            retry_when: None,
            until: None,
            circuit_breaker: None,
        }
    }
}

/// Delay strategy for retries - supports multiple backoff algorithms
#[derive(Debug, Clone)]
pub enum DelayStrategy {
    /// Fixed delay between retries
    Fixed(Duration),
    /// Exponential backoff: base * 2^attempt, with optional jitter
    Exponential {
        base: Duration,
        max: Duration,
        /// Add random jitter to prevent thundering herd
        jitter: bool,
    },
    /// Linear increase: base + (increment * attempt)
    Linear {
        base: Duration,
        increment: Duration,
        max: Duration,
    },
}

/// Circuit breaker configuration - prevents cascading failures
/// Named circuits are shared across tasks for coordinated failure handling
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Name of the circuit (shared across tasks targeting same service)
    pub name: String,
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Time to wait before trying again (half-open state)
    pub reset_timeout: Duration,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
}

// ============================================================================
// Async Task Configuration
// ============================================================================

/// Async task configuration - Similar to Ansible's async/poll
/// Allows long-running tasks to execute in the background
#[derive(Debug, Clone)]
pub struct AsyncConfig {
    /// Maximum seconds to wait for completion (0 = fire and forget)
    pub async_timeout: u64,
    /// Seconds between status checks (0 = no polling, fire and forget)
    pub poll: u64,
    /// Maximum poll attempts before giving up
    pub retries: u32,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: String::new(),
            module: ModuleCall::Command {
                cmd: Expression::String(String::new()),
                creates: None,
                removes: None,
            },
            when: None,
            register: None,
            fail_when: None,
            changed_when: None,
            notify: Vec::new(),
            loop_expr: None,
            loop_var: "item".to_string(),
            location: None,
            sudo: None,
            run_as: None,
            tags: Vec::new(),
            retry: None,
            async_config: None,
            timeout: None,
            throttle: None,
            delegate_to: None,
            delegate_facts: false,
        }
    }
}

/// Module invocation types
#[derive(Debug, Clone)]
pub enum ModuleCall {
    /// package: nginx, state: installed
    Package {
        name: Expression,
        state: PackageState,
    },
    /// service: nginx, state: running
    Service {
        name: Expression,
        state: ServiceState,
        enabled: Option<bool>,
    },
    /// file: /path, source: template.conf
    File {
        path: Expression,
        state: FileState,
        source: Option<Expression>,
        content: Option<Expression>,
        owner: Option<Expression>,
        group: Option<Expression>,
        mode: Option<Expression>,
    },
    /// command: ls -la
    Command {
        cmd: Expression,
        creates: Option<Expression>,
        removes: Option<Expression>,
    },
    /// user: jdoe
    User {
        name: Expression,
        state: UserState,
        uid: Option<Expression>,
        gid: Option<Expression>,
        groups: Vec<Expression>,
        shell: Option<Expression>,
        home: Option<Expression>,
        create_home: Option<bool>,
    },
    /// run: function_name()
    RunFunction { name: String, args: Vec<Expression> },
    /// Template module
    Template {
        src: Expression,
        dest: Expression,
        owner: Option<Expression>,
        group: Option<Expression>,
        mode: Option<Expression>,
    },
    /// Facts gathering module
    Facts { categories: Vec<String> },
    /// Shell command - execute through /bin/sh -c
    Shell {
        command: Expression,
        chdir: Option<Expression>,
        creates: Option<Expression>,
        removes: Option<Expression>,
    },
    /// Log/debug output
    Log { message: Expression },
    /// Set variable
    Set { name: String, value: Expression },
    /// Fail with message
    Fail { message: Expression },
    /// Assert condition
    Assert {
        condition: Expression,
        message: Option<Expression>,
    },
    /// Raw command (no shell processing)
    Raw { command: Expression },
    /// Git operations
    Git {
        repo: Expression,
        dest: Expression,
        version: Option<Expression>,
        force: Option<bool>,
    },
    /// HTTP request
    Http {
        url: Expression,
        method: Option<String>,
        body: Option<Expression>,
        headers: Option<Expression>,
    },
    /// Group management
    Group {
        name: Expression,
        state: UserState,
        gid: Option<Expression>,
    },
}

impl ModuleCall {
    /// Get the module name as a string
    pub fn module_name(&self) -> &'static str {
        match self {
            ModuleCall::Package { .. } => "package",
            ModuleCall::Service { .. } => "service",
            ModuleCall::File { .. } => "file",
            ModuleCall::Command { .. } => "command",
            ModuleCall::User { .. } => "user",
            ModuleCall::RunFunction { .. } => "run",
            ModuleCall::Template { .. } => "template",
            ModuleCall::Facts { .. } => "facts",
            ModuleCall::Shell { .. } => "shell",
            ModuleCall::Log { .. } => "log",
            ModuleCall::Set { .. } => "set",
            ModuleCall::Fail { .. } => "fail",
            ModuleCall::Assert { .. } => "assert",
            ModuleCall::Raw { .. } => "raw",
            ModuleCall::Git { .. } => "git",
            ModuleCall::Http { .. } => "http",
            ModuleCall::Group { .. } => "group",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackageState {
    #[default]
    Installed,
    Latest,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceState {
    #[default]
    Running,
    Stopped,
    Restarted,
    Reloaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileState {
    #[default]
    File,
    Directory,
    Link,
    Absent,
    Touch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserState {
    #[default]
    Present,
    Absent,
}

/// Handler definition
#[derive(Debug, Clone)]
pub struct Handler {
    pub name: String,
    pub module: ModuleCall,
    pub location: Option<SourceLocation>,
}

// ============================================================================
// Role Configuration
// ============================================================================

/// A reusable role that encapsulates tasks, handlers, files, templates, and vars
#[derive(Debug, Clone)]
pub struct Role {
    /// Name of the role (directory name)
    pub name: String,
    /// Path to the role directory
    pub path: String,
    /// Role metadata from meta/main.yml
    pub meta: RoleMeta,
    /// Default variables (lowest priority)
    pub defaults: HashMap<String, Value>,
    /// Role variables (higher priority than defaults)
    pub vars: HashMap<String, Value>,
    /// Tasks to execute
    pub tasks: Vec<TaskOrBlock>,
    /// Handlers defined by this role
    pub handlers: Vec<Handler>,
    /// Files directory path (for copy operations)
    pub files_path: Option<String>,
    /// Templates directory path (for template operations)
    pub templates_path: Option<String>,
}

/// Role metadata from meta/main.yml
#[derive(Debug, Clone, Default)]
pub struct RoleMeta {
    /// Role dependencies (other roles to run first)
    pub dependencies: Vec<RoleDependency>,
    /// Minimum Nexus version required
    pub min_nexus_version: Option<String>,
    /// Platforms this role supports
    pub platforms: Vec<PlatformSupport>,
    /// Role description
    pub description: Option<String>,
    /// Role author
    pub author: Option<String>,
    /// License
    pub license: Option<String>,
    /// Whether this role allows duplicates with different parameters
    pub allow_duplicates: bool,
}

/// A role dependency with optional parameter overrides
#[derive(Debug, Clone)]
pub struct RoleDependency {
    /// Name of the dependent role
    pub role: String,
    /// Variables to pass to the dependent role
    pub vars: HashMap<String, Value>,
    /// Tags to apply when running this dependency
    pub tags: Vec<String>,
    /// Conditional - only include if true
    pub when: Option<Expression>,
}

/// Platform support specification
#[derive(Debug, Clone)]
pub struct PlatformSupport {
    /// OS name (e.g., "Ubuntu", "CentOS", "Debian")
    pub name: String,
    /// Supported versions
    pub versions: Vec<String>,
}

/// Reference to a role in a playbook
#[derive(Debug, Clone)]
pub struct RoleRef {
    /// Role name or path
    pub role: String,
    /// Variable overrides for this role invocation
    pub vars: HashMap<String, Value>,
    /// Tags for filtering
    pub tags: Vec<String>,
    /// Conditional execution
    pub when: Option<Expression>,
}

/// A block of function definitions
#[derive(Debug, Clone)]
pub struct FunctionBlock {
    pub source: String,
    pub functions: Vec<FunctionDef>,
    pub location: Option<SourceLocation>,
}

/// A function definition
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<FunctionParam>,
    pub body: Vec<Statement>,
    pub location: Option<SourceLocation>,
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct FunctionParam {
    pub name: String,
    pub default: Option<Expression>,
}

/// Statements in function bodies
#[derive(Debug, Clone)]
pub enum Statement {
    /// Variable assignment: x = expr
    Assign { target: String, value: Expression },
    /// If statement
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        elif_clauses: Vec<(Expression, Vec<Statement>)>,
        else_body: Option<Vec<Statement>>,
    },
    /// For loop
    For {
        var: String,
        iter: Expression,
        body: Vec<Statement>,
    },
    /// While loop
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    /// Try/except block
    Try {
        try_body: Vec<Statement>,
        except_clauses: Vec<(Option<String>, Option<String>, Vec<Statement>)>,
    },
    /// Return statement
    Return(Option<Expression>),
    /// Expression statement (function call, etc.)
    Expression(Expression),
    /// Break from loop
    Break,
    /// Continue to next iteration
    Continue,
}

/// Expressions in the Nexus language
#[derive(Debug, Clone)]
pub enum Expression {
    /// String literal: "hello"
    String(String),
    /// Integer literal: 42
    Integer(i64),
    /// Float literal: 3.14
    Float(f64),
    /// Boolean literal: true/false
    Boolean(bool),
    /// Null/None value
    Null,
    /// Variable reference: host, vars.foo
    Variable(Vec<String>),
    /// String with interpolation: "Hello ${name}"
    InterpolatedString(Vec<StringPart>),
    /// Binary operation: a + b
    BinaryOp {
        left: Box<Expression>,
        op: BinaryOperator,
        right: Box<Expression>,
    },
    /// Unary operation: !x, -x
    UnaryOp {
        op: UnaryOperator,
        operand: Box<Expression>,
    },
    /// Function call: func(args)
    FunctionCall {
        name: String,
        args: Vec<Expression>,
        kwargs: HashMap<String, Expression>,
    },
    /// Method call: obj.method(args)
    MethodCall {
        object: Box<Expression>,
        method: String,
        args: Vec<Expression>,
        kwargs: HashMap<String, Expression>,
    },
    /// Index access: arr[0], dict["key"]
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    /// Attribute access: obj.attr
    Attribute {
        object: Box<Expression>,
        attr: String,
    },
    /// List literal: [1, 2, 3]
    List(Vec<Expression>),
    /// Dict literal: {"key": "value"}
    Dict(Vec<(Expression, Expression)>),
    /// Filter expression: items | filter(x => x.active)
    Filter {
        input: Box<Expression>,
        filter_name: String,
        predicate: Option<Box<Expression>>,
    },
    /// Lambda/arrow function: x => x.active
    Lambda {
        params: Vec<String>,
        body: Box<Expression>,
    },
    /// Ternary: a if condition else b
    Ternary {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
}

impl Expression {
    /// Create a simple string expression
    pub fn string(s: impl Into<String>) -> Self {
        Expression::String(s.into())
    }

    /// Create a simple variable reference
    pub fn var(name: impl Into<String>) -> Self {
        Expression::Variable(vec![name.into()])
    }
}

/// Parts of an interpolated string
#[derive(Debug, Clone)]
pub enum StringPart {
    /// Literal text
    Literal(String),
    /// Interpolated expression: ${...}
    Expression(Expression),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // Membership
    In,
    NotIn,
}

impl std::fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BinaryOperator::Add => "+",
            BinaryOperator::Sub => "-",
            BinaryOperator::Mul => "*",
            BinaryOperator::Div => "/",
            BinaryOperator::Mod => "%",
            BinaryOperator::Eq => "==",
            BinaryOperator::Ne => "!=",
            BinaryOperator::Lt => "<",
            BinaryOperator::Le => "<=",
            BinaryOperator::Gt => ">",
            BinaryOperator::Ge => ">=",
            BinaryOperator::And => "and",
            BinaryOperator::Or => "or",
            BinaryOperator::In => "in",
            BinaryOperator::NotIn => "not in",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Not,
    Neg,
}

/// Runtime value type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Dict(HashMap<String, Value>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Dict(d) => !d.is_empty(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "{}", s),
            Value::List(l) => {
                write!(f, "[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Dict(d) => {
                write!(f, "{{")?;
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}
