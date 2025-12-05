// Plan display formatter - Terraform-style execution plan output

use std::collections::HashMap;
use colored::*;

use crate::executor::{ChangeType, ExecutionPlan, HostPlan};

/// Display an execution plan with Terraform-style formatting
pub fn display_plan(plan: &ExecutionPlan, show_diff: bool) {
    println!();
    println!(
        "{} {} tasks across {} hosts",
        "Plan:".cyan().bold(),
        plan.total_tasks,
        plan.host_plans.len()
    );
    println!();

    // Group hosts by identical change signatures
    let grouped_hosts = group_hosts_by_changes(&plan.host_plans);

    // Display each group
    for (_signature, hosts) in grouped_hosts {
        // Get the changes from the first host (they're all identical)
        let changes = &hosts[0].changes;

        // Display host names
        let host_names: Vec<_> = hosts.iter().map(|h| h.host.as_str()).collect();
        println!("  {}:", host_names.join(", ").white().bold());

        // Display each change
        for change in changes {
            display_change(change, show_diff);
        }

        println!();
    }

    // Display summary
    display_summary(plan);
}

/// Group hosts by their change signatures
fn group_hosts_by_changes(host_plans: &[HostPlan]) -> Vec<(String, Vec<&HostPlan>)> {
    let mut groups: HashMap<String, Vec<&HostPlan>> = HashMap::new();

    for host_plan in host_plans {
        let signature = host_plan.signature();
        groups.entry(signature).or_default().push(host_plan);
    }

    // Convert to vec and sort by first host name
    let mut result: Vec<_> = groups.into_iter().collect();
    result.sort_by(|a, b| a.1[0].host.cmp(&b.1[0].host));

    result
}

/// Display a single change
fn display_change(change: &crate::executor::PlannedChange, show_diff: bool) {
    let symbol = match change.change_type {
        ChangeType::Create => "+".green(),
        ChangeType::Remove => "-".red(),
        ChangeType::Modify => "~".yellow(),
        ChangeType::NoChange => "✓".dimmed(),
        ChangeType::Unknown => "?".dimmed(),
        ChangeType::Conditional => "?".cyan(),
    };

    let module_name = change.module.cyan();

    // Build the description
    let description = if change.change_type == ChangeType::NoChange {
        if let Some(ref current) = change.current_state {
            format!("({})", current).dimmed().to_string()
        } else {
            "".to_string()
        }
    } else {
        match (&change.current_state, &change.desired_state) {
            (Some(current), Some(desired)) => {
                if current != desired {
                    format!("({} → {})", current, desired)
                } else {
                    format!("({})", desired)
                }
            }
            (None, Some(desired)) => format!("({})", desired),
            (Some(current), None) => format!("({})", current),
            (None, None) => "".to_string(),
        }
    };

    // Display the change line
    print!("    {} {}: {}", symbol, module_name, change.task_name);
    if !description.is_empty() {
        print!(" {}", description.dimmed());
    }

    // Display danger warning if present
    if change.is_dangerous {
        if let Some(ref reason) = change.danger_reason {
            print!(" {} {}", "⚠️".red(), reason.red().bold());
        } else {
            print!(" {}", "⚠️  DANGEROUS".red().bold());
        }
    }

    println!();

    // Display diff if present and requested
    if show_diff {
        if let Some(ref diff) = change.diff {
            // Indent the diff
            for line in diff.lines() {
                println!("      {}", colorize_diff_line(line));
            }
        }
    }
}

/// Colorize a diff line based on its prefix
fn colorize_diff_line(line: &str) -> ColoredString {
    if line.starts_with('+') && !line.starts_with("+++") {
        line.green()
    } else if line.starts_with('-') && !line.starts_with("---") {
        line.red()
    } else if line.starts_with("@@") {
        line.cyan()
    } else {
        line.dimmed()
    }
}

/// Display the plan summary
fn display_summary(plan: &ExecutionPlan) {
    println!("{}", "─".repeat(80).dimmed());

    let mut parts = Vec::new();

    if plan.creates > 0 {
        parts.push(format!("{} create", "+".green().to_string() + &plan.creates.to_string()));
    }
    if plan.modifies > 0 {
        parts.push(format!("{} modify", "~".yellow().to_string() + &plan.modifies.to_string()));
    }
    if plan.removes > 0 {
        parts.push(format!("{} remove", "-".red().to_string() + &plan.removes.to_string()));
    }
    if plan.no_changes > 0 {
        parts.push(format!("{} unchanged", "✓".dimmed().to_string() + &plan.no_changes.to_string()));
    }
    if plan.warnings > 0 {
        parts.push(format!("{} warnings", "!".red().bold().to_string() + &plan.warnings.to_string()));
    }

    println!("{} {}", "Summary:".bold(), parts.join(", "));

    // Display estimated time
    let duration = plan.estimated_duration;
    let time_str = format_duration(duration);
    println!("{} {}", "Estimated time:".bold(), time_str.dimmed());

    println!();
}

/// Format a duration in a human-readable way
fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs == 0 {
        return "< 1 second".to_string();
    }

    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    let mut parts = Vec::new();

    if hours > 0 {
        parts.push(format!("{} hour{}", hours, if hours > 1 { "s" } else { "" }));
    }
    if minutes > 0 {
        parts.push(format!("{} minute{}", minutes, if minutes > 1 { "s" } else { "" }));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{} second{}", seconds, if seconds > 1 { "s" } else { "" }));
    }

    format!("~{}", parts.join(", "))
}

/// Prompt for confirmation
pub fn prompt_confirmation(auto_approve: bool) -> Result<bool, std::io::Error> {
    if auto_approve {
        return Ok(true);
    }

    use std::io::{self, Write};

    print!("{} ", "Proceed? [y/N]".yellow().bold());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim().to_lowercase();
    Ok(input == "y" || input == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(std::time::Duration::from_secs(0)), "< 1 second");
        assert_eq!(format_duration(std::time::Duration::from_secs(5)), "~5 seconds");
        assert_eq!(format_duration(std::time::Duration::from_secs(60)), "~1 minute");
        assert_eq!(format_duration(std::time::Duration::from_secs(90)), "~1 minute, 30 seconds");
        assert_eq!(format_duration(std::time::Duration::from_secs(3661)), "~1 hour, 1 minute, 1 second");
    }
}
