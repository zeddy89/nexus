// Nexus CLI - Next-Generation Infrastructure Automation

use std::io::{self, Write};
use std::str::FromStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use colored::*;
use parking_lot::Mutex;

use nexus::executor::{Scheduler, SchedulerConfig, TagFilter};
use nexus::inventory::Inventory;
use nexus::output::{NexusError, OutputFormat, OutputWriter};
use nexus::parser::{parse_playbook_file, parse_playbook_file_with_vault};
use nexus::parser::ast::TaskOrBlock;

#[derive(Parser)]
#[command(
    name = "nexus",
    about = "Next-generation infrastructure automation",
    version,
    author,
    disable_colored_help = true,
    term_width = 0,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Quiet mode - only show errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output format (text or json)
    #[arg(long, global = true, default_value = "text")]
    output_format: String,
}

#[derive(Subcommand)]
#[command(disable_colored_help = true)]
enum Commands {
    /// Run a playbook
    Run {
        /// Path to the playbook file
        playbook: PathBuf,

        /// Path to the inventory file
        #[arg(short, long)]
        inventory: PathBuf,

        /// Limit to specific hosts (comma-separated)
        #[arg(short, long)]
        limit: Option<String>,

        /// Run in check mode (dry run)
        #[arg(short, long)]
        check: bool,

        /// Show differences for changed files
        #[arg(short = 'D', long)]
        diff: bool,

        /// Maximum parallel hosts
        #[arg(long, default_value = "10")]
        forks: usize,

        /// SSH connection timeout in seconds
        #[arg(long, default_value = "30")]
        timeout: u64,

        /// Path to SSH private key
        #[arg(long)]
        private_key: Option<PathBuf>,

        /// SSH user (overrides inventory)
        #[arg(short, long)]
        user: Option<String>,

        /// SSH password (insecure - prefer --ask-pass)
        #[arg(long)]
        password: Option<String>,

        /// Prompt for SSH password
        #[arg(short = 'k', long)]
        ask_pass: bool,

        /// Run all tasks with sudo
        #[arg(short = 's', long)]
        sudo: bool,

        /// Prompt for sudo password
        #[arg(short = 'K', long)]
        ask_sudo_pass: bool,

        /// Only run tasks with these tags (comma-separated)
        #[arg(short = 't', long)]
        tags: Option<String>,

        /// Skip tasks with these tags (comma-separated)
        #[arg(long)]
        skip_tags: Option<String>,

        /// Vault password for decrypting secrets
        #[arg(long)]
        vault_password: Option<String>,

        /// File containing vault password
        #[arg(long)]
        vault_password_file: Option<PathBuf>,

        /// Prompt for vault password
        #[arg(long)]
        ask_vault_pass: bool,

        /// Callback plugins (format: name:args, can repeat)
        #[arg(long = "callback")]
        callbacks: Vec<String>,

        /// Enable checkpoints (save progress for resume)
        #[arg(long)]
        checkpoint: bool,

        /// Resume from last checkpoint
        #[arg(long)]
        resume: bool,

        /// Resume from specific checkpoint file
        #[arg(long)]
        resume_from: Option<PathBuf>,

        /// Enable live TUI dashboard
        #[arg(long)]
        tui: bool,
    },

    /// Validate a playbook without executing
    Validate {
        /// Path to the playbook file
        playbook: PathBuf,
    },

    /// List hosts in inventory
    Inventory {
        /// Path to the inventory file
        #[arg(short, long)]
        inventory: PathBuf,

        /// Host pattern to match
        #[arg(default_value = "all")]
        pattern: String,

        /// Show host variables
        #[arg(long)]
        vars: bool,
    },

    /// Parse and display a playbook
    Parse {
        /// Path to the playbook file
        playbook: PathBuf,

        /// Output format (yaml, json)
        #[arg(short, long, default_value = "yaml")]
        format: String,
    },

    /// Vault operations for encrypting/decrypting secrets
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },

    /// Checkpoint management
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointAction,
    },

    /// Show execution plan without running (Terraform-style)
    Plan {
        /// Path to the playbook file
        playbook: PathBuf,

        /// Path to the inventory file
        #[arg(short, long)]
        inventory: PathBuf,

        /// Limit to specific hosts
        #[arg(short, long)]
        limit: Option<String>,

        /// SSH user
        #[arg(short, long)]
        user: Option<String>,

        /// SSH password
        #[arg(long)]
        password: Option<String>,

        /// Prompt for SSH password
        #[arg(short = 'k', long)]
        ask_pass: bool,

        /// Path to SSH private key
        #[arg(long)]
        private_key: Option<PathBuf>,

        /// Show full diffs
        #[arg(long)]
        diff: bool,

        /// Auto-approve (skip confirmation)
        #[arg(short = 'y', long)]
        yes: bool,

        /// Run all tasks with sudo
        #[arg(short = 's', long)]
        sudo: bool,

        /// Vault password for decrypting secrets
        #[arg(long)]
        vault_password: Option<String>,

        /// File containing vault password
        #[arg(long)]
        vault_password_file: Option<PathBuf>,

        /// Prompt for vault password
        #[arg(long)]
        ask_vault_pass: bool,
    },
}

#[derive(Subcommand)]
#[command(disable_colored_help = true)]
enum CheckpointAction {
    /// List all saved checkpoints
    List,

    /// Show details of a checkpoint
    Show {
        /// Checkpoint file path
        file: PathBuf,
    },

    /// Delete a checkpoint
    Clean {
        /// Playbook file (optional, cleans all if not specified)
        playbook: Option<PathBuf>,

        /// Delete checkpoints older than N days
        #[arg(long)]
        older_than: Option<u64>,
    },
}

#[derive(Subcommand)]
#[command(disable_colored_help = true)]
enum VaultAction {
    /// Encrypt a file
    Encrypt {
        /// File to encrypt
        file: PathBuf,

        /// Vault password
        #[arg(long)]
        vault_password: Option<String>,

        /// File containing vault password
        #[arg(long)]
        vault_password_file: Option<PathBuf>,

        /// Output file (default: overwrites input)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Decrypt a file
    Decrypt {
        /// File to decrypt
        file: PathBuf,

        /// Vault password
        #[arg(long)]
        vault_password: Option<String>,

        /// File containing vault password
        #[arg(long)]
        vault_password_file: Option<PathBuf>,

        /// Output file (default: overwrites input)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// View decrypted content without modifying file
    View {
        /// File to view
        file: PathBuf,

        /// Vault password
        #[arg(long)]
        vault_password: Option<String>,

        /// File containing vault password
        #[arg(long)]
        vault_password_file: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Parse output format
    let output_format = OutputFormat::from_str(&cli.output_format).unwrap_or_else(|_| {
        eprintln!("Invalid output format: {}. Using 'text'.", cli.output_format);
        OutputFormat::Text
    });

    let result = match cli.command {
        Commands::Run {
            playbook,
            inventory,
            limit,
            check,
            diff,
            forks,
            timeout,
            private_key,
            user,
            password,
            ask_pass,
            sudo,
            ask_sudo_pass,
            tags,
            skip_tags,
            vault_password,
            vault_password_file,
            ask_vault_pass,
            callbacks,
            checkpoint,
            resume,
            resume_from,
            tui,
        } => {
            run_playbook(
                playbook,
                inventory,
                limit,
                check,
                diff,
                forks,
                timeout,
                private_key,
                user,
                password,
                ask_pass,
                sudo,
                ask_sudo_pass,
                tags,
                skip_tags,
                vault_password,
                vault_password_file,
                ask_vault_pass,
                callbacks,
                checkpoint,
                resume,
                resume_from,
                tui,
                cli.verbose,
                cli.quiet,
                output_format,
            )
            .await
        }
        Commands::Validate { playbook } => validate_playbook(playbook),
        Commands::Inventory {
            inventory,
            pattern,
            vars,
        } => list_inventory(inventory, &pattern, vars),
        Commands::Parse { playbook, format } => parse_and_display(playbook, &format),
        Commands::Vault { action } => handle_vault_command(action),
        Commands::Checkpoint { action } => handle_checkpoint_command(action),
        Commands::Plan {
            playbook,
            inventory,
            limit,
            user,
            password,
            ask_pass,
            private_key,
            diff,
            yes,
            sudo,
            vault_password,
            vault_password_file,
            ask_vault_pass,
        } => {
            handle_plan_command(
                playbook,
                inventory,
                limit,
                user,
                password,
                ask_pass,
                private_key,
                diff,
                yes,
                sudo,
                vault_password,
                vault_password_file,
                ask_vault_pass,
                cli.verbose,
            )
            .await
        }
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_playbook(
    playbook_path: PathBuf,
    inventory_path: PathBuf,
    _limit: Option<String>,
    check: bool,
    diff: bool,
    forks: usize,
    timeout: u64,
    private_key: Option<PathBuf>,
    user: Option<String>,
    password: Option<String>,
    ask_pass: bool,
    sudo: bool,
    ask_sudo_pass: bool,
    tags: Option<String>,
    skip_tags: Option<String>,
    vault_password: Option<String>,
    vault_password_file: Option<PathBuf>,
    ask_vault_pass: bool,
    callback_specs: Vec<String>,
    enable_checkpoints: bool,
    resume: bool,
    resume_from: Option<PathBuf>,
    use_tui: bool,
    verbose: bool,
    quiet: bool,
    output_format: OutputFormat,
) -> Result<(), NexusError> {
    // Handle SSH password prompting
    let ssh_password = if ask_pass {
        Some(prompt_password("SSH Password: ")?)
    } else {
        password
    };

    // Handle sudo password prompting
    let sudo_password = if ask_sudo_pass {
        Some(prompt_password("SUDO Password: ")?)
    } else {
        None
    };

    // Handle vault password
    let vault_pass = get_vault_password(vault_password, vault_password_file, ask_vault_pass)?;

    // Print banner (skip in TUI mode - it has its own header)
    if !quiet && !use_tui {
        print_banner();
    }

    // Parse playbook (with vault support)
    let playbook = if let Some(ref password) = vault_pass {
        parse_playbook_file_with_vault(&playbook_path, Some(password))?
    } else {
        parse_playbook_file(&playbook_path)?
    };

    // Load inventory
    let inventory = Inventory::from_file(&inventory_path)?;

    // Create output handler (silent when TUI is active to avoid conflicting output)
    let output = if use_tui {
        Arc::new(Mutex::new(OutputWriter::silent()))
    } else {
        Arc::new(Mutex::new(OutputWriter::new(output_format, verbose, quiet)))
    };

    // Create tag filter if tags specified
    let tag_filter = if tags.is_some() || skip_tags.is_some() {
        Some(TagFilter::from_args(tags.as_deref(), skip_tags.as_deref()))
    } else {
        None
    };

    // Print tag filter info if verbose (but not in TUI mode)
    if verbose && !use_tui && tag_filter.is_some() {
        println!(
            "  {} {}",
            "Tag filter:".cyan(),
            tag_filter.as_ref().unwrap().describe()
        );
    }

    // Create callback manager and load plugins
    let mut callback_manager = nexus::plugins::CallbackManager::new();
    for spec in callback_specs {
        match nexus::plugins::callbacks::create_callback_plugin(&spec) {
            Ok(plugin) => {
                if verbose && !use_tui {
                    println!("  {} Loaded callback plugin: {}", "✓".green(), plugin.name());
                }
                callback_manager.add(plugin);
            }
            Err(e) => {
                if !use_tui {
                    eprintln!("{} Failed to load callback plugin '{}': {}", "✗".red(), spec, e);
                }
                std::process::exit(1);
            }
        }
    }

    // Create scheduler config
    let config = SchedulerConfig {
        max_parallel_hosts: forks,
        max_parallel_tasks: 1,
        connect_timeout: Duration::from_secs(timeout),
        command_timeout: Duration::from_secs(300),
        check_mode: check,
        diff_mode: diff,
        verbose,
        ssh_password,
        ssh_private_key: private_key.map(|p| p.to_string_lossy().to_string()),
        ssh_user: user,
        sudo,
        sudo_password,
        tag_filter,
        enable_checkpoints,
        resume,
        resume_from,
    };

    // Create scheduler with callbacks
    let mut scheduler = Scheduler::with_callbacks(config, output.clone(), Arc::new(callback_manager));

    // Add role search path relative to playbook location
    scheduler.add_playbook_role_path(&playbook_path);

    // Execute playbook (with or without TUI)
    let recap = if use_tui {
        // Create event channel for TUI
        let (emitter, rx) = nexus::output::create_event_channel();

        // Set event emitter on scheduler
        scheduler.set_event_emitter(emitter);

        // Spawn TUI in a separate task
        let tui_handle = tokio::spawn(async move {
            let mut tui_app = nexus::output::TuiApp::new(rx);
            tui_app.run().await
        });

        // Execute playbook (events will be sent to TUI)
        let recap_result = scheduler.execute_playbook(&playbook, &inventory).await;

        // Wait for TUI to finish (it will auto-exit after playbook complete event)
        let _ = tui_handle.await;

        recap_result?
    } else {
        scheduler.execute_playbook(&playbook, &inventory).await?
    };

    // Exit with error if there were failures
    if recap.has_failures() {
        std::process::exit(2);
    }

    Ok(())
}

fn validate_playbook(playbook_path: PathBuf) -> Result<(), NexusError> {
    println!(
        "{} {}",
        "Validating:".cyan(),
        playbook_path.display()
    );

    let playbook = parse_playbook_file(&playbook_path)?;

    println!("{} Playbook is valid", "✓".green());
    println!();
    println!("  {} {:?}", "Hosts:".dimmed(), playbook.hosts);
    println!("  {} {}", "Tasks:".dimmed(), playbook.tasks.len());
    println!("  {} {}", "Handlers:".dimmed(), playbook.handlers.len());
    println!(
        "  {} {}",
        "Functions:".dimmed(),
        playbook.functions.as_ref().map(|f| f.functions.len()).unwrap_or(0)
    );

    Ok(())
}

fn list_inventory(inventory_path: PathBuf, pattern: &str, show_vars: bool) -> Result<(), NexusError> {
    let inventory = Inventory::from_file(&inventory_path)?;

    let pattern = nexus::inventory::parse_host_pattern(pattern);
    let hosts = inventory.get_hosts(&pattern);

    println!(
        "{} {} host(s) matching '{:?}'",
        "Found".green(),
        hosts.len(),
        pattern
    );
    println!();

    for host in hosts {
        println!("  {} {}", "•".cyan(), host.name.white().bold());
        println!("    {} {}", "Address:".dimmed(), host.address);
        println!("    {} {}", "Port:".dimmed(), host.port);

        if !host.user.is_empty() {
            println!("    {} {}", "User:".dimmed(), host.user);
        }

        if !host.groups.is_empty() {
            println!("    {} {}", "Groups:".dimmed(), host.groups.join(", "));
        }

        if show_vars && !host.vars.is_empty() {
            println!("    {}:", "Variables".dimmed());
            for (k, v) in &host.vars {
                println!("      {} = {}", k.yellow(), v);
            }
        }

        println!();
    }

    // Show groups
    println!("{}:", "Groups".green());
    for name in inventory.group_names() {
        if name != "all" {
            if let Some(group) = inventory.groups.get(name) {
                println!(
                    "  {} {} ({} hosts)",
                    "•".cyan(),
                    name,
                    group.hosts.len()
                );
            }
        }
    }

    Ok(())
}

fn parse_and_display(playbook_path: PathBuf, format: &str) -> Result<(), NexusError> {
    let playbook = parse_playbook_file(&playbook_path)?;

    match format {
        "yaml" => {
            println!("{}:", "Playbook".green());
            println!("  {} {:?}", "hosts:".dimmed(), playbook.hosts);
            println!();

            if !playbook.vars.is_empty() {
                println!("  {}:", "vars".dimmed());
                for (k, v) in &playbook.vars {
                    println!("    {}: {}", k, v);
                }
                println!();
            }

            println!("  {}:", "tasks".dimmed());
            let mut task_num = 0;
            for item in &playbook.tasks {
                match item {
                    TaskOrBlock::Task(task) => {
                        task_num += 1;
                        println!("    {} {}", format!("{}.", task_num).cyan(), task.name);
                        println!("       {:?}", task.module);
                        if task.when.is_some() {
                            println!("       {} {:?}", "when:".yellow(), task.when);
                        }
                    }
                    TaskOrBlock::Block(_) => {
                        task_num += 1;
                        println!("    {} (block)", format!("{}.", task_num).cyan());
                    }
                    TaskOrBlock::Import(import) => {
                        task_num += 1;
                        println!("    {} import_tasks: {}", format!("{}.", task_num).cyan(), import.file);
                    }
                    TaskOrBlock::Include(include) => {
                        task_num += 1;
                        println!("    {} include_tasks: {:?}", format!("{}.", task_num).cyan(), include.file);
                    }
                }
            }

            if !playbook.handlers.is_empty() {
                println!();
                println!("  {}:", "handlers".dimmed());
                for handler in &playbook.handlers {
                    println!("    {} {}", "•".cyan(), handler.name);
                }
            }
        }
        "json" => {
            // Simple JSON representation
            println!("{{");
            println!("  \"hosts\": \"{:?}\",", playbook.hosts);
            println!("  \"tasks\": [");
            for (i, item) in playbook.tasks.iter().enumerate() {
                let comma = if i < playbook.tasks.len() - 1 { "," } else { "" };
                match item {
                    TaskOrBlock::Task(task) => {
                        println!("  - Task: {}", task.name);
                    }
                    TaskOrBlock::Block(_) => {
                        println!("    {{ \"name\": \"(block)\" }}{}", comma);
                    }
                    TaskOrBlock::Import(import) => {
                        println!("    {{ \"name\": \"import_tasks: {}\" }}{}", import.file, comma);
                    }
                    TaskOrBlock::Include(_) => {
                        println!("    {{ \"name\": \"include_tasks\" }}{}", comma);
                    }
                }
            }
            println!("  ]");
            println!("}}");
        }
        _ => {
            return Err(NexusError::Runtime {
                function: None,
                message: format!("Unknown format: {}", format),
                suggestion: Some("Use 'yaml' or 'json'".to_string()),
            });
        }
    }

    Ok(())
}

fn print_banner() {
    println!();
    println!(
        "{}",
        r#"
  _   _
 | \ | | _____  ___   _ ___
 |  \| |/ _ \ \/ / | | / __|
 | |\  |  __/>  <| |_| \__ \
 |_| \_|\___/_/\_\\__,_|___/
"#
        .cyan()
    );
    println!(
        "  {} {}",
        "Next-Generation Infrastructure Automation".white(),
        format!("v{}", nexus::VERSION).dimmed()
    );
    println!();
}

fn prompt_password(prompt: &str) -> Result<String, NexusError> {
    // Print prompt to stderr so it appears even with redirected stdout
    eprint!("{}", prompt.cyan());
    io::stderr().flush().ok();

    // Read password with echo disabled
    let password = rpassword::read_password().map_err(|e| NexusError::Runtime {
        function: None,
        message: format!("Failed to read password: {}", e),
        suggestion: Some("Try using --password instead of --ask-pass".to_string()),
    })?;

    // Trim any trailing whitespace/newlines
    let password = password.trim().to_string();

    // Print newline after password entry (since echo was disabled)
    eprintln!();

    if password.is_empty() {
        return Err(NexusError::Runtime {
            function: None,
            message: "Password cannot be empty".to_string(),
            suggestion: Some("Enter a password when prompted".to_string()),
        });
    }

    Ok(password)
}

fn get_vault_password(
    vault_password: Option<String>,
    vault_password_file: Option<PathBuf>,
    ask_vault_pass: bool,
) -> Result<Option<String>, NexusError> {
    if let Some(password) = vault_password {
        Ok(Some(password))
    } else if let Some(file) = vault_password_file {
        let password = std::fs::read_to_string(&file)
            .map_err(|e| NexusError::Io {
                message: format!("Failed to read vault password file: {}", e),
                path: Some(file),
            })?
            .trim()
            .to_string();
        Ok(Some(password))
    } else if ask_vault_pass {
        Ok(Some(prompt_password("Vault Password: ")?))
    } else {
        Ok(None)
    }
}

fn handle_vault_command(action: VaultAction) -> Result<(), NexusError> {
    use nexus::vault;

    match action {
        VaultAction::Encrypt {
            file,
            vault_password,
            vault_password_file,
            output,
        } => {
            let password = get_vault_password(vault_password, vault_password_file, true)?
                .ok_or_else(|| NexusError::Runtime {
                    function: None,
                    message: "Vault password required".to_string(),
                    suggestion: Some("Use --vault-password or --vault-password-file".to_string()),
                })?;

            println!("{} {}", "Encrypting:".cyan(), file.display());

            let output_path = output.as_ref().unwrap_or(&file);

            vault::encrypt_file(&file, &password).map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Encryption failed: {}", e),
                suggestion: None,
            })?;

            // If output path is different, move the encrypted file
            if output.is_some() && output.as_ref() != Some(&file) {
                std::fs::copy(&file, output_path).map_err(|e| NexusError::Io {
                    message: format!("Failed to copy to output file: {}", e),
                    path: Some(output_path.clone()),
                })?;
            }

            println!("{} File encrypted successfully", "✓".green());
            Ok(())
        }

        VaultAction::Decrypt {
            file,
            vault_password,
            vault_password_file,
            output,
        } => {
            let password = get_vault_password(vault_password, vault_password_file, true)?
                .ok_or_else(|| NexusError::Runtime {
                    function: None,
                    message: "Vault password required".to_string(),
                    suggestion: Some("Use --vault-password or --vault-password-file".to_string()),
                })?;

            println!("{} {}", "Decrypting:".cyan(), file.display());

            let output_path = output.as_ref().unwrap_or(&file);

            vault::decrypt_file(&file, &password).map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Decryption failed: {}", e),
                suggestion: Some("Check that the password is correct".to_string()),
            })?;

            // If output path is different, move the decrypted file
            if output.is_some() && output.as_ref() != Some(&file) {
                std::fs::copy(&file, output_path).map_err(|e| NexusError::Io {
                    message: format!("Failed to copy to output file: {}", e),
                    path: Some(output_path.clone()),
                })?;
            }

            println!("{} File decrypted successfully", "✓".green());
            Ok(())
        }

        VaultAction::View {
            file,
            vault_password,
            vault_password_file,
        } => {
            let password = get_vault_password(vault_password, vault_password_file, true)?
                .ok_or_else(|| NexusError::Runtime {
                    function: None,
                    message: "Vault password required".to_string(),
                    suggestion: Some("Use --vault-password or --vault-password-file".to_string()),
                })?;

            let content = vault::view_file(&file, &password).map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to view file: {}", e),
                suggestion: Some("Check that the password is correct".to_string()),
            })?;

            println!("{}", content);
            Ok(())
        }
    }
}

fn handle_checkpoint_command(action: CheckpointAction) -> Result<(), NexusError> {
    use nexus::executor::CheckpointManager;

    let manager = CheckpointManager::new()?;

    match action {
        CheckpointAction::List => {
            let checkpoints = manager.list_all()?;

            if checkpoints.is_empty() {
                println!("{}", "No checkpoints found".dimmed());
                return Ok(());
            }

            println!("{} checkpoint(s):", checkpoints.len());
            println!();

            for info in checkpoints {
                println!("  {} {}", "•".cyan(), info.path.display());
                println!("    {} {}", "Playbook:".dimmed(), info.playbook_path.display());
                println!("    {} {}", "Timestamp:".dimmed(), info.timestamp.format("%Y-%m-%d %H:%M:%S"));
                println!("    {} {}", "Tasks completed:".dimmed(), info.completed_tasks);

                if let Some(ref task) = info.last_task {
                    println!("    {} {}", "Last task:".dimmed(), task);
                }
                if let Some(ref host) = info.last_host {
                    println!("    {} {}", "Last host:".dimmed(), host);
                }
                println!();
            }

            Ok(())
        }

        CheckpointAction::Show { file } => {
            let checkpoint = manager.load(&file)?;

            println!("{} {}", "Checkpoint:".cyan(), file.display());
            println!();
            println!("  {} {}", "Version:".dimmed(), checkpoint.version);
            println!("  {} {}", "Playbook:".dimmed(), checkpoint.playbook_path.display());
            println!("  {} {}", "Inventory:".dimmed(), checkpoint.inventory_path.display());
            println!("  {} {}", "Hash:".dimmed(), checkpoint.playbook_hash);
            println!("  {} {}", "Timestamp:".dimmed(), checkpoint.timestamp.format("%Y-%m-%d %H:%M:%S"));
            println!("  {} {}", "Tasks completed:".dimmed(), checkpoint.completed_tasks.len());
            println!("  {} {}", "Variables:".dimmed(), checkpoint.variables.len());
            println!("  {} {}", "Registered results:".dimmed(), checkpoint.registered_results.len());
            println!("  {} {}", "Handler notifications:".dimmed(), checkpoint.handler_notifications.len());

            if let Some(ref task) = checkpoint.last_task {
                println!();
                println!("  {} {}", "Last task:".yellow(), task);
            }
            if let Some(ref host) = checkpoint.last_host {
                println!("  {} {}", "Last host:".yellow(), host);
            }

            Ok(())
        }

        CheckpointAction::Clean { playbook, older_than } => {
            if let Some(days) = older_than {
                let cleaned = manager.clean_old(days)?;
                println!("{} Cleaned {} checkpoint(s) older than {} days", "✓".green(), cleaned, days);
            } else if let Some(playbook_path) = playbook {
                manager.cleanup(&playbook_path)?;
                println!("{} Cleaned checkpoint for {}", "✓".green(), playbook_path.display());
            } else {
                return Err(NexusError::Runtime {
                    function: None,
                    message: "Must specify either --playbook or --older-than".to_string(),
                    suggestion: Some("Use 'nexus checkpoint clean --older-than 7' or 'nexus checkpoint clean playbook.yml'".to_string()),
                });
            }

            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_plan_command(
    playbook_path: PathBuf,
    inventory_path: PathBuf,
    limit: Option<String>,
    user: Option<String>,
    password: Option<String>,
    ask_pass: bool,
    private_key: Option<PathBuf>,
    show_diff: bool,
    auto_approve: bool,
    sudo: bool,
    vault_password: Option<String>,
    vault_password_file: Option<PathBuf>,
    ask_vault_pass: bool,
    verbose: bool,
) -> Result<(), NexusError> {
    use nexus::executor::{PlanGenerator, Scheduler, SchedulerConfig, SshConfig};
    use nexus::output::plan::{display_plan, prompt_confirmation};

    // Handle SSH password prompting
    let ssh_password = if ask_pass {
        Some(prompt_password("SSH Password: ")?)
    } else {
        password
    };

    // Handle vault password
    let vault_pass = get_vault_password(vault_password, vault_password_file, ask_vault_pass)?;

    // Print banner
    print_banner();

    // Parse playbook (with vault support)
    let playbook = if let Some(ref password) = vault_pass {
        parse_playbook_file_with_vault(&playbook_path, Some(password))?
    } else {
        parse_playbook_file(&playbook_path)?
    };

    // Load inventory
    let inventory = Inventory::from_file(&inventory_path)?;

    if verbose {
        println!(
            "  {} Generating execution plan...",
            "Planning:".cyan()
        );
        println!();
    }

    // Create SSH config
    let ssh_user = user.clone();
    let ssh_config = SshConfig {
        user,
        password: ssh_password.clone(),
        private_key: private_key.as_ref().map(|p| p.to_string_lossy().to_string()),
    };

    // Generate plan
    let generator = PlanGenerator::new();
    let plan = generator
        .generate_plan(&playbook, &inventory, ssh_config, limit.as_deref())
        .await?;

    // Display the plan
    display_plan(&plan, show_diff);

    // Prompt for confirmation
    let proceed = prompt_confirmation(auto_approve).map_err(|e| NexusError::Runtime {
        function: None,
        message: format!("Failed to read confirmation: {}", e),
        suggestion: None,
    })?;

    if !proceed {
        println!();
        println!("{}", "Plan cancelled.".yellow());
        return Ok(());
    }

    println!();
    println!("{}", "Executing plan...".green().bold());
    println!();

    // Execute the playbook using the normal scheduler
    let output = Arc::new(Mutex::new(OutputWriter::new(
        OutputFormat::Text,
        verbose,
        false,
    )));

    let config = SchedulerConfig {
        max_parallel_hosts: 10,
        max_parallel_tasks: 1,
        connect_timeout: Duration::from_secs(30),
        command_timeout: Duration::from_secs(300),
        check_mode: false,
        diff_mode: show_diff,
        verbose,
        ssh_password,
        ssh_private_key: private_key.map(|p| p.to_string_lossy().to_string()),
        ssh_user,
        sudo,
        sudo_password: None,
        tag_filter: None,
        enable_checkpoints: false,
        resume: false,
        resume_from: None,
    };

    let scheduler = Scheduler::new(config, output.clone());
    scheduler.add_playbook_role_path(&playbook_path);

    let recap = scheduler.execute_playbook(&playbook, &inventory).await?;

    // Exit with error if there were failures
    if recap.has_failures() {
        std::process::exit(2);
    }

    Ok(())
}
