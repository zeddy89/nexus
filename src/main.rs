// Nexus CLI - Next-Generation Infrastructure Automation

use std::io::{self, Write};
use std::str::FromStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use colored::*;
use parking_lot::Mutex;

use nexus::executor::{Scheduler, SchedulerConfig, TagFilter};
use nexus::inventory::{Inventory, NetworkScanner, DiscoveredHost, DiscoveryDaemon, Notifier, Host, HostGroup, ProbeType};
use nexus::output::{NexusError, OutputFormat, OutputWriter};
use nexus::parser::{parse_playbook_file, parse_playbook_file_with_vault};
use nexus::parser::ast::{HostPattern, Playbook, TaskOrBlock, Value};
use nexus::converter::{Converter, ConversionOptions, ConversionReport, IssueSeverity};

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
        inventory: Option<PathBuf>,

        /// Comma-separated host list (alternative to inventory file)
        #[arg(short = 'H', long)]
        hosts: Option<String>,

        /// Discover hosts from subnet instead of inventory
        #[arg(long)]
        discover: Option<String>,  // e.g., "10.20.30.0/24"

        /// Filter discovered hosts
        #[arg(long)]
        discover_filter: Option<String>,

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
        inventory: Option<PathBuf>,

        /// Comma-separated host list (alternative to inventory file)
        #[arg(short = 'H', long)]
        hosts: Option<String>,

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

    /// Discover hosts on a network
    Discover {
        /// Subnet to scan (CIDR notation, e.g., 192.168.1.0/24)
        #[arg(long)]
        subnet: Option<String>,

        /// File containing list of subnets to scan
        #[arg(long)]
        subnets_from: Option<PathBuf>,

        /// Passive mode - read from ARP cache, no packets sent
        #[arg(long)]
        passive: bool,

        /// Use ARP cache for passive discovery
        #[arg(long)]
        from_arp: bool,

        /// Probe type: ssh, ping, or tcp:port1,port2
        #[arg(long, default_value = "ssh")]
        probe: String,

        /// Discovery profile file
        #[arg(long)]
        profile: Option<PathBuf>,

        /// Enable OS fingerprinting
        #[arg(long)]
        fingerprint: bool,

        /// Save discovered hosts to inventory file
        #[arg(long)]
        save_to: Option<PathBuf>,

        /// Filter expression (e.g., "port:22 AND os:linux")
        #[arg(long)]
        filter: Option<String>,

        /// Jump host for scanning remote networks
        #[arg(long)]
        via: Option<String>,

        /// Connection timeout in milliseconds
        #[arg(long, default_value = "1000")]
        timeout: u64,

        /// Max concurrent probes
        #[arg(long, default_value = "100")]
        parallel: usize,

        /// Run as daemon for continuous monitoring
        #[arg(long)]
        daemon: bool,

        /// Subnets to watch in daemon mode (comma-separated)
        #[arg(long)]
        watch: Option<String>,

        /// Scan interval for daemon mode (e.g., 5m, 1h)
        #[arg(long, default_value = "5m")]
        interval: String,

        /// Notification on changes (webhook:URL, file:PATH, or stdout)
        #[arg(long)]
        notify_on_change: Option<String>,
    },

    /// Convert Ansible playbooks to Nexus format
    Convert {
        /// Source file or directory to convert
        source: PathBuf,

        /// Output file or directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Show what would be converted without writing files
        #[arg(long)]
        dry_run: bool,

        /// Approve each file conversion interactively
        #[arg(long)]
        interactive: bool,

        /// Convert entire project (playbooks, roles, inventory)
        #[arg(long)]
        all: bool,

        /// Also convert inventory files
        #[arg(long)]
        include_inventory: bool,

        /// Also convert Jinja2 templates to Nexus syntax
        #[arg(long)]
        include_templates: bool,

        /// Keep Jinja2 syntax in templates (don't convert to ${})
        #[arg(long)]
        keep_jinja2: bool,

        /// Write conversion report to file
        #[arg(long)]
        report: Option<PathBuf>,

        /// Fail on any conversion warning
        #[arg(long)]
        strict: bool,

        /// Minimal output
        #[arg(short, long)]
        quiet: bool,

        /// Detailed conversion log
        #[arg(short, long)]
        verbose: bool,

        /// Assessment mode - scan and report without converting
        #[arg(long)]
        assess: bool,
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
            hosts,
            discover,
            discover_filter,
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
                hosts,
                discover,
                discover_filter,
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
            hosts,
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
                hosts,
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
        Commands::Discover {
            subnet,
            subnets_from,
            passive,
            from_arp,
            probe,
            profile,
            fingerprint,
            save_to,
            filter,
            via,
            timeout,
            parallel,
            daemon,
            watch,
            interval,
            notify_on_change,
        } => {
            handle_discover_command(
                subnet,
                subnets_from,
                passive,
                from_arp,
                probe,
                profile,
                fingerprint,
                save_to,
                filter,
                via,
                timeout,
                parallel,
                daemon,
                watch,
                interval,
                notify_on_change,
            )
            .await
        }
        Commands::Convert {
            source,
            output,
            dry_run,
            interactive,
            all,
            include_inventory,
            include_templates,
            keep_jinja2,
            report,
            strict,
            quiet,
            verbose,
            assess,
        } => {
            handle_convert_command(
                source,
                output,
                dry_run,
                interactive,
                all,
                include_inventory,
                include_templates,
                keep_jinja2,
                report,
                strict,
                quiet,
                verbose,
                assess,
            )
        }
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

/// Resolve inventory from various sources with priority order
///
/// Priority order:
/// 1. CLI --discover flag (live network scan) - highest priority
/// 2. CLI --hosts flag (explicit host list)
/// 3. Inventory file (--inventory / -i)
/// 4. Playbook-embedded hosts (HostPattern::Inline)
/// 5. Implicit localhost (when playbook has hosts: localhost -> HostPattern::Localhost)
/// 6. Error if none available
async fn resolve_inventory(
    inventory_path: Option<&Path>,
    cli_hosts: Option<&str>,
    discover_subnet: Option<&str>,
    discover_filter: Option<&str>,
    playbook: &Playbook,
    default_user: Option<&str>,
) -> Result<Inventory, NexusError> {
    // 1. CLI --discover flag takes highest priority (live network scan)
    if let Some(subnet) = discover_subnet {
        let scanner = NetworkScanner::new();
        let discovered_hosts = scanner.scan_subnet(subnet).await?;

        // Filter hosts if filter is provided
        let filtered_hosts = if let Some(filter_str) = discover_filter {
            // Simple filtering by OS family for now
            discovered_hosts.into_iter()
                .filter(|h| {
                    if let Some(ref os) = h.os_classification {
                        os.os_family.contains(filter_str)
                    } else {
                        false
                    }
                })
                .collect::<Vec<_>>()
        } else {
            discovered_hosts
        };

        // Convert discovered hosts to inventory
        return Ok(inventory_from_discovered_hosts(&filtered_hosts, default_user));
    }

    // 2. CLI --hosts flag
    if let Some(hosts_str) = cli_hosts {
        return Ok(Inventory::from_cli_hosts(hosts_str, default_user));
    }

    // 3. Inventory file
    if let Some(path) = inventory_path {
        return Inventory::from_file(path);
    }

    // 4. Playbook-embedded hosts (HostPattern::Inline)
    if let HostPattern::Inline(ref inline_hosts) = playbook.hosts {
        return Ok(Inventory::from_inline_hosts(inline_hosts, default_user));
    }

    // 5. Implicit localhost (when playbook has hosts: localhost)
    if let HostPattern::Localhost = playbook.hosts {
        return Ok(Inventory::localhost_only());
    }

    // 6. Error - no inventory source available
    Err(NexusError::Runtime {
        function: None,
        message: "No inventory source provided".to_string(),
        suggestion: Some(
            "Use --discover to scan a subnet, --inventory/-i to specify an inventory file, \
             --hosts/-H for a comma-separated host list, or define hosts inline in your playbook".to_string()
        ),
    })
}

/// Convert discovered hosts to an Inventory
fn inventory_from_discovered_hosts(
    discovered: &[DiscoveredHost],
    default_user: Option<&str>,
) -> Inventory {
    use nexus::inventory::Host;

    let mut inventory = Inventory::new();

    for dhost in discovered {
        let addr_str = dhost.address.to_string();
        let mut host = Host::new(&addr_str);

        // Use hostname if available, otherwise use IP
        if let Some(ref hostname) = dhost.hostname {
            host.name = hostname.clone();
        }

        host.address = addr_str;

        // Find SSH port from open ports
        if let Some(ssh_port) = dhost.open_ports.iter().find(|p| p.port == 22) {
            host.port = ssh_port.port;
        }

        // Set user if provided
        if let Some(user) = default_user {
            host.user = user.to_string();
        }

        // Add OS classification as a variable if available
        if let Some(ref os) = dhost.os_classification {
            host.vars.insert("discovered_os".to_string(),
                nexus::parser::ast::Value::String(os.os_family.clone()));
            if let Some(ref dist) = os.distribution {
                host.vars.insert("discovered_distribution".to_string(),
                    nexus::parser::ast::Value::String(dist.clone()));
            }
        }

        inventory.add_host(host);
    }

    inventory
}

#[allow(clippy::too_many_arguments)]
async fn run_playbook(
    playbook_path: PathBuf,
    inventory_path: Option<PathBuf>,
    cli_hosts: Option<String>,
    discover_subnet: Option<String>,
    discover_filter: Option<String>,
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

    // Resolve inventory from various sources
    let inventory = resolve_inventory(
        inventory_path.as_deref(),
        cli_hosts.as_deref(),
        discover_subnet.as_deref(),
        discover_filter.as_deref(),
        &playbook,
        user.as_deref(),
    ).await?;

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
    inventory_path: Option<PathBuf>,
    cli_hosts: Option<String>,
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

    // Resolve inventory from various sources
    let inventory = resolve_inventory(
        inventory_path.as_deref(),
        cli_hosts.as_deref(),
        None,  // discover_subnet not supported in plan command
        None,  // discover_filter not supported in plan command
        &playbook,
        user.as_deref(),
    ).await?;

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

#[allow(clippy::too_many_arguments)]
async fn handle_discover_command(
    subnet: Option<String>,
    subnets_from: Option<PathBuf>,
    _passive: bool,
    _from_arp: bool,
    probe: String,
    _profile: Option<PathBuf>,
    fingerprint: bool,
    save_to: Option<PathBuf>,
    filter: Option<String>,
    _via: Option<String>,
    timeout: u64,
    parallel: usize,
    daemon: bool,
    watch: Option<String>,
    interval: String,
    notify_on_change: Option<String>,
) -> Result<(), NexusError> {
    // Validate inputs - requires either --subnet or --subnets-from or daemon mode
    if subnet.is_none() && subnets_from.is_none() && !daemon {
        return Err(NexusError::Runtime {
            function: None,
            message: "No subnet specified".to_string(),
            suggestion: Some("Use --subnet, --subnets-from, or --daemon with --watch".to_string()),
        });
    }

    // Collect subnets to scan
    let mut subnets = Vec::new();

    if let Some(subnet_str) = subnet {
        subnets.push(subnet_str);
    }

    if let Some(file_path) = subnets_from {
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| NexusError::Io {
                message: format!("Failed to read subnets file: {}", e),
                path: Some(file_path.clone()),
            })?;

        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                subnets.push(line.to_string());
            }
        }
    }

    // Handle daemon mode
    if daemon {
        println!("{}", "Starting discovery daemon...".green());

        // Use watch subnets if provided, otherwise use scanned subnets
        let watch_subnets = if let Some(watch_str) = watch {
            watch_str.split(',').map(|s| s.trim().to_string()).collect()
        } else {
            subnets.clone()
        };

        if watch_subnets.is_empty() {
            return Err(NexusError::Runtime {
                function: None,
                message: "No subnets to watch in daemon mode".to_string(),
                suggestion: Some("Use --watch or --subnet to specify subnets".to_string()),
            });
        }

        // Parse interval
        let interval_duration = parse_interval(&interval)?;

        // Create daemon
        let mut daemon = DiscoveryDaemon::new(
            watch_subnets,
            interval_duration,
        );

        // Add notifier if specified
        if let Some(notify_spec) = notify_on_change {
            let notifier = parse_notifier(&notify_spec)?;
            daemon = daemon.with_notifier(notifier);
        }

        daemon.run().await;
    }

    // Normal discovery mode (non-daemon)
    println!("{}", "Starting network discovery...".cyan());

    // Parse probe type
    let probe_type = parse_probe_type(&probe)?;

    // Create scanner
    let scanner = NetworkScanner {
        timeout: Duration::from_millis(timeout),
        concurrent_probes: parallel,
        fingerprint,
        probe_type,
    };

    // Scan subnets
    let mut all_hosts = Vec::new();

    for subnet_str in &subnets {
        println!("  {} Scanning {}...", "→".cyan(), subnet_str);

        let hosts = scanner.scan_subnet(subnet_str).await?;

        println!("    {} Found {} host(s)", "✓".green(), hosts.len());
        all_hosts.extend(hosts);
    }

    // Apply filter if specified
    let filtered_hosts = if let Some(filter_expr) = filter {
        apply_filter(&all_hosts, &filter_expr)?
    } else {
        all_hosts
    };

    // Print results
    println!();
    println!("{} {} host(s) discovered", "Results:".green().bold(), filtered_hosts.len());
    println!();

    for host in &filtered_hosts {
        println!("  {} {}", "•".cyan(), host.address.to_string().white().bold());

        if let Some(hostname) = &host.hostname {
            println!("    {} {}", "Hostname:".dimmed(), hostname);
        }

        if !host.open_ports.is_empty() {
            let ports: Vec<String> = host.open_ports.iter()
                .map(|p| {
                    if let Some(ref service) = p.service {
                        format!("{}/{}", p.port, service)
                    } else {
                        p.port.to_string()
                    }
                })
                .collect();
            println!("    {} {}", "Ports:".dimmed(), ports.join(", "));
        }

        if let Some(os) = &host.os_classification {
            let os_str = if let Some(ref dist) = os.distribution {
                format!("{} ({}) - {}% confident", os.os_family, dist, (os.confidence * 100.0) as u8)
            } else {
                format!("{} - {}% confident", os.os_family, (os.confidence * 100.0) as u8)
            };
            println!("    {} {}", "OS:".dimmed(), os_str);
        }

        if host.open_ports.iter().any(|p| p.port == 22) {
            println!("    {} {}", "SSH:".dimmed(), "accessible".green());
        }

        println!();
    }

    // Save to inventory if requested
    if let Some(output_path) = save_to {
        println!("{} Saving to inventory file...", "→".cyan());

        let inventory = convert_to_inventory(&filtered_hosts);
        save_inventory_to_file(&inventory, &output_path)?;

        println!("  {} Saved to {}", "✓".green(), output_path.display());
    }

    Ok(())
}

/// Parse interval string (e.g., "5m", "1h", "30s") into Duration
fn parse_interval(interval: &str) -> Result<Duration, NexusError> {
    let interval = interval.trim();

    if interval.is_empty() {
        return Err(NexusError::Runtime {
            function: None,
            message: "Empty interval".to_string(),
            suggestion: Some("Use format like '5m', '1h', or '30s'".to_string()),
        });
    }

    let (num_str, unit) = if let Some(pos) = interval.find(|c: char| !c.is_ascii_digit()) {
        (&interval[..pos], &interval[pos..])
    } else {
        (interval, "s") // Default to seconds
    };

    let num: u64 = num_str.parse()
        .map_err(|_| NexusError::Runtime {
            function: None,
            message: format!("Invalid interval number: {}", num_str),
            suggestion: Some("Use a positive integer".to_string()),
        })?;

    let multiplier = match unit.trim() {
        "s" | "sec" | "second" | "seconds" => 1,
        "m" | "min" | "minute" | "minutes" => 60,
        "h" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        _ => {
            return Err(NexusError::Runtime {
                function: None,
                message: format!("Unknown time unit: {}", unit),
                suggestion: Some("Use s, m, h, or d".to_string()),
            });
        }
    };

    Ok(Duration::from_secs(num * multiplier))
}

/// Parse notifier specification
fn parse_notifier(spec: &str) -> Result<Notifier, NexusError> {
    if let Some(url) = spec.strip_prefix("webhook:") {
        Ok(Notifier::Webhook { url: url.to_string() })
    } else if let Some(path) = spec.strip_prefix("file:") {
        Ok(Notifier::File { path: PathBuf::from(path) })
    } else if spec == "stdout" {
        Ok(Notifier::Stdout)
    } else {
        Err(NexusError::Runtime {
            function: None,
            message: format!("Invalid notifier specification: {}", spec),
            suggestion: Some("Use webhook:URL, file:PATH, or stdout".to_string()),
        })
    }
}

/// Parse probe type specification (ssh, ping, or tcp:port1,port2)
fn parse_probe_type(probe: &str) -> Result<ProbeType, NexusError> {
    let probe = probe.trim().to_lowercase();

    match probe.as_str() {
        "ssh" => Ok(ProbeType::Ssh),
        "ping" => Ok(ProbeType::Ping),
        _ if probe.starts_with("tcp:") => {
            let ports_str = &probe[4..];
            let ports: Result<Vec<u16>, _> = ports_str
                .split(',')
                .map(|s| s.trim().parse::<u16>())
                .collect();

            match ports {
                Ok(ports) if !ports.is_empty() => Ok(ProbeType::TcpPorts(ports)),
                Ok(_) => Err(NexusError::Runtime {
                    function: None,
                    message: "No ports specified".to_string(),
                    suggestion: Some("Use format like 'tcp:22,80,443'".to_string()),
                }),
                Err(_) => Err(NexusError::Runtime {
                    function: None,
                    message: format!("Invalid port in probe specification: {}", ports_str),
                    suggestion: Some("Ports must be numbers between 1 and 65535".to_string()),
                }),
            }
        }
        _ => Err(NexusError::Runtime {
            function: None,
            message: format!("Unknown probe type: {}", probe),
            suggestion: Some("Use 'ssh', 'ping', or 'tcp:port1,port2'".to_string()),
        }),
    }
}

/// Apply filter expression to hosts
fn apply_filter(hosts: &[DiscoveredHost], filter_expr: &str) -> Result<Vec<DiscoveredHost>, NexusError> {
    let mut filtered = Vec::new();

    for host in hosts {
        if matches_filter(host, filter_expr) {
            filtered.push(host.clone());
        }
    }

    Ok(filtered)
}

/// Check if host matches filter expression
fn matches_filter(host: &DiscoveredHost, filter_expr: &str) -> bool {
    // Simple filter implementation
    // Supports: "port:22", "os:linux", "ssh:true", "port:22 AND os:linux"

    for condition in filter_expr.split("AND") {
        let condition = condition.trim();

        if let Some(port_str) = condition.strip_prefix("port:") {
            if let Ok(port) = port_str.trim().parse::<u16>() {
                if !host.open_ports.iter().any(|p| p.port == port) {
                    return false;
                }
            }
        } else if let Some(os_str) = condition.strip_prefix("os:") {
            let os_str = os_str.trim().to_lowercase();
            if let Some(ref os_info) = host.os_classification {
                if !os_info.os_family.to_lowercase().contains(&os_str) {
                    if let Some(ref dist) = os_info.distribution {
                        if !dist.to_lowercase().contains(&os_str) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            } else {
                return false;
            }
        } else if condition.starts_with("ssh:") {
            let ssh_str = condition.strip_prefix("ssh:").unwrap().trim();
            let ssh_required = ssh_str == "true" || ssh_str == "yes";
            if ssh_required && !host.open_ports.iter().any(|p| p.port == 22) {
                return false;
            }
        }
    }

    true
}

/// Convert discovered hosts to inventory
fn convert_to_inventory(hosts: &[DiscoveredHost]) -> Inventory {
    let mut inventory = Inventory::new();

    for discovered in hosts {
        let name = discovered.hostname.clone()
            .unwrap_or_else(|| discovered.address.to_string());

        let mut host = Host::new(name.clone())
            .with_address(discovered.address.to_string());

        // Set SSH port if available
        if let Some(ssh_port) = discovered.open_ports.iter().find(|p| p.port == 22) {
            host.port = ssh_port.port;
        }

        // Add discovered metadata as variables
        if let Some(ref os) = discovered.os_classification {
            host.vars.insert(
                "discovered_os_family".to_string(),
                Value::String(os.os_family.clone())
            );
            if let Some(ref dist) = os.distribution {
                host.vars.insert(
                    "discovered_os_dist".to_string(),
                    Value::String(dist.clone())
                );
            }
            host.vars.insert(
                "discovered_os_confidence".to_string(),
                Value::String(format!("{:.2}", os.confidence))
            );
        }

        if !discovered.open_ports.is_empty() {
            let ports_str = discovered.open_ports.iter()
                .map(|p| p.port.to_string())
                .collect::<Vec<_>>()
                .join(",");
            host.vars.insert("discovered_open_ports".to_string(), Value::String(ports_str));
        }

        host.groups.push("discovered".to_string());

        inventory.add_host(host);
    }

    // Create "discovered" group
    let discovered_group = HostGroup {
        name: "discovered".to_string(),
        hosts: hosts.iter()
            .map(|h| h.hostname.clone().unwrap_or_else(|| h.address.to_string()))
            .collect(),
        children: Vec::new(),
        vars: std::collections::HashMap::new(),
    };
    inventory.add_group(discovered_group);

    inventory
}

/// Save inventory to YAML file
fn save_inventory_to_file(inventory: &Inventory, path: &Path) -> Result<(), NexusError> {
    use std::collections::HashMap;

    let mut yaml_map: HashMap<String, serde_yaml::Value> = HashMap::new();

    // Add hosts to "all" group
    let mut all_hosts = HashMap::new();
    for host in inventory.hosts.values() {
        let mut host_map = HashMap::new();
        host_map.insert("ansible_host".to_string(), serde_yaml::Value::String(host.address.clone()));
        host_map.insert("ansible_port".to_string(), serde_yaml::Value::Number(host.port.into()));

        if !host.user.is_empty() {
            host_map.insert("ansible_user".to_string(), serde_yaml::Value::String(host.user.clone()));
        }

        // Add custom vars
        for (key, val) in &host.vars {
            let yaml_val = match val {
                Value::String(s) => serde_yaml::Value::String(s.clone()),
                Value::Int(n) => serde_yaml::Value::Number((*n).into()),
                Value::Float(f) => serde_yaml::Value::String(f.to_string()),
                Value::Bool(b) => serde_yaml::Value::Bool(*b),
                Value::List(l) => serde_yaml::Value::String(format!("{:?}", l)),
                Value::Dict(_) | Value::Null => serde_yaml::Value::String(format!("{:?}", val)),
            };
            host_map.insert(key.clone(), yaml_val);
        }

        all_hosts.insert(host.name.clone(), serde_yaml::Value::Mapping(
            host_map.into_iter().map(|(k, v)| (serde_yaml::Value::String(k), v)).collect()
        ));
    }

    let mut all_group = HashMap::new();
    all_group.insert("hosts".to_string(), serde_yaml::Value::Mapping(
        all_hosts.into_iter().map(|(k, v)| (serde_yaml::Value::String(k), v)).collect()
    ));

    yaml_map.insert("all".to_string(), serde_yaml::Value::Mapping(
        all_group.into_iter().map(|(k, v)| (serde_yaml::Value::String(k), v)).collect()
    ));

    // Write to file
    let yaml_string = serde_yaml::to_string(&yaml_map)
        .map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to serialize inventory: {}", e),
            suggestion: None,
        })?;

    std::fs::write(path, yaml_string)
        .map_err(|e| NexusError::Io {
            message: format!("Failed to write inventory file: {}", e),
            path: Some(path.to_path_buf()),
        })?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_convert_command(
    source: PathBuf,
    output: Option<PathBuf>,
    dry_run: bool,
    interactive: bool,
    all: bool,
    include_inventory: bool,
    include_templates: bool,
    keep_jinja2: bool,
    report_path: Option<PathBuf>,
    strict: bool,
    quiet: bool,
    verbose: bool,
    assess: bool,
) -> Result<(), NexusError> {
    // Print banner unless in quiet mode
    if !quiet {
        println!();
        println!("{}", "Nexus Ansible Converter".cyan().bold());
        println!();
    }

    // Validate source path exists
    if !source.exists() {
        return Err(NexusError::Runtime {
            function: None,
            message: format!("Source path does not exist: {}", source.display()),
            suggestion: Some("Verify the path to the Ansible playbook or project directory".to_string()),
        });
    }

    // Create conversion options
    let options = ConversionOptions {
        dry_run,
        interactive,
        convert_all: all,
        include_inventory,
        include_templates,
        keep_jinja2,
        strict,
        verbose,
        quiet,
    };

    // Create converter instance
    let converter = Converter::new(options);

    // Run assessment or conversion
    let report = if assess {
        if !quiet {
            println!("{} Analyzing Ansible project...", "Assessment:".cyan());
            println!();
        }
        converter.assess(&source)?
    } else {
        if !quiet {
            if dry_run {
                println!("{} Running in dry-run mode (no files will be written)", "Dry Run:".yellow());
            } else {
                println!("{} Converting Ansible to Nexus format...", "Converting:".cyan());
            }
            println!();
        }
        converter.convert(&source, output.as_deref())?
    };

    // Print summary unless in quiet mode
    if !quiet {
        print_conversion_summary(&report, assess);
    }

    // Write detailed report to file if requested
    if let Some(report_file) = report_path {
        write_conversion_report(&report, &report_file)?;
        if !quiet {
            println!();
            println!("  {} Detailed report written to {}", "✓".green(), report_file.display());
        }
    }

    // Handle strict mode - exit with error if there were warnings
    if strict && report.has_warnings() {
        println!();
        println!("{}", "Conversion failed: warnings detected in strict mode".red());
        std::process::exit(1);
    }

    // Exit with error code if there were errors
    if report.has_errors() {
        std::process::exit(1);
    }

    Ok(())
}

/// Print conversion summary
fn print_conversion_summary(report: &ConversionReport, assess_only: bool) {
    println!();
    println!("{}", "Conversion Summary:".green().bold());
    println!();

    if assess_only {
        println!("  {} Files analyzed: {}", "•".cyan(), report.files.len());
        println!("  {} Playbooks found: {}", "•".cyan(), report.total_playbooks);
        println!("  {} Roles found: {}", "•".cyan(), report.total_roles);
        println!("  {} Tasks total: {}", "•".cyan(), report.total_tasks);
    } else {
        println!("  {} Files converted: {}", "✓".green(), report.files.len());
        println!("  {} Tasks converted: {}", "✓".green(), report.total_converted());
        let need_review = report.total_need_review();
        if need_review > 0 {
            println!("  {} Tasks need review: {}", "⚠".yellow(), need_review);
        }
    }

    let warning_count: usize = report.files.iter()
        .map(|f| f.issues.iter().filter(|i| matches!(i.severity, IssueSeverity::Warning)).count())
        .sum();
    let error_count: usize = report.files.iter()
        .map(|f| f.issues.iter().filter(|i| matches!(i.severity, IssueSeverity::Error)).count())
        .sum();

    if warning_count > 0 {
        println!("  {} Warnings: {}", "⚠".yellow(), warning_count);
    }

    if error_count > 0 {
        println!("  {} Errors: {}", "✗".red(), error_count);
    }

    // Print detailed warnings and errors if any
    let warnings: Vec<_> = report.files.iter()
        .flat_map(|f| f.issues.iter())
        .filter(|i| matches!(i.severity, IssueSeverity::Warning))
        .collect();

    if !warnings.is_empty() && warnings.len() <= 10 {
        println!();
        println!("{}:", "Warnings".yellow().bold());
        for issue in warnings {
            println!("  {} {}", "⚠".yellow(), issue.message);
        }
    } else if warnings.len() > 10 {
        println!();
        println!("{}: {} total (showing first 10)", "Warnings".yellow().bold(), warnings.len());
        for issue in warnings.iter().take(10) {
            println!("  {} {}", "⚠".yellow(), issue.message);
        }
    }

    let errors: Vec<_> = report.files.iter()
        .flat_map(|f| f.issues.iter())
        .filter(|i| matches!(i.severity, IssueSeverity::Error))
        .collect();

    if !errors.is_empty() && errors.len() <= 10 {
        println!();
        println!("{}:", "Errors".red().bold());
        for issue in errors {
            println!("  {} {}", "✗".red(), issue.message);
        }
    } else if errors.len() > 10 {
        println!();
        println!("{}: {} total (showing first 10)", "Errors".red().bold(), errors.len());
        for issue in errors.iter().take(10) {
            println!("  {} {}", "✗".red(), issue.message);
        }
    }

    // Print unsupported modules if any
    let unsupported_modules = report.all_unsupported_modules();
    if !unsupported_modules.is_empty() {
        println!();
        println!("{}:", "Unsupported Modules".cyan().bold());
        for module in &unsupported_modules {
            println!("  {} {}", "ℹ".cyan(), module);
        }
    }
}

/// Write detailed conversion report to file
fn write_conversion_report(report: &ConversionReport, path: &Path) -> Result<(), NexusError> {
    use std::io::Write;

    let markdown = report.to_markdown();

    let mut file = std::fs::File::create(path)
        .map_err(|e| NexusError::Io {
            message: format!("Failed to create report file: {}", e),
            path: Some(path.to_path_buf()),
        })?;

    file.write_all(markdown.as_bytes())
        .map_err(|e| NexusError::Io {
            message: format!("Failed to write report: {}", e),
            path: Some(path.to_path_buf()),
        })?;

    Ok(())
}
