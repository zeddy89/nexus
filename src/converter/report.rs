use chrono::{DateTime, Local};
use std::fmt;
use std::path::{Path, PathBuf};

/// Severity of a conversion issue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IssueSeverity::Info => write!(f, "ℹ"),
            IssueSeverity::Warning => write!(f, "⚠"),
            IssueSeverity::Error => write!(f, "✗"),
        }
    }
}

/// A single conversion issue or note
#[derive(Debug, Clone)]
pub struct ConversionIssue {
    pub severity: IssueSeverity,
    pub line: Option<usize>,
    pub message: String,
    pub original: Option<String>,
    pub converted: Option<String>,
    pub suggestion: Option<String>,
}

impl ConversionIssue {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Info,
            line: None,
            message: message.into(),
            original: None,
            converted: None,
            suggestion: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            line: None,
            message: message.into(),
            original: None,
            converted: None,
            suggestion: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            line: None,
            message: message.into(),
            original: None,
            converted: None,
            suggestion: None,
        }
    }

    pub fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    pub fn with_original(mut self, original: impl Into<String>) -> Self {
        self.original = Some(original.into());
        self
    }

    pub fn with_converted(mut self, converted: impl Into<String>) -> Self {
        self.converted = Some(converted.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

/// Result of converting a single file
#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub source_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub success: bool,
    pub tasks_total: usize,
    pub tasks_converted: usize,
    pub tasks_modified: usize,
    pub tasks_need_review: usize,
    pub issues: Vec<ConversionIssue>,
    pub expressions_converted: usize,
    pub filters_converted: usize,
    pub unsupported_filters: Vec<String>,
    pub unsupported_modules: Vec<String>,
}

impl ConversionResult {
    pub fn new(source_path: PathBuf) -> Self {
        Self {
            source_path,
            output_path: None,
            success: true,
            tasks_total: 0,
            tasks_converted: 0,
            tasks_modified: 0,
            tasks_need_review: 0,
            issues: Vec::new(),
            expressions_converted: 0,
            filters_converted: 0,
            unsupported_filters: Vec::new(),
            unsupported_modules: Vec::new(),
        }
    }

    pub fn add_issue(&mut self, issue: ConversionIssue) {
        if matches!(issue.severity, IssueSeverity::Error) {
            self.success = false;
        }
        self.issues.push(issue);
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|i| matches!(i.severity, IssueSeverity::Warning | IssueSeverity::Error))
    }

    pub fn conversion_percentage(&self) -> f64 {
        if self.tasks_total == 0 {
            100.0
        } else {
            (self.tasks_converted as f64 / self.tasks_total as f64) * 100.0
        }
    }
}

/// Overall conversion report for a conversion run
#[derive(Debug, Clone)]
pub struct ConversionReport {
    pub source: PathBuf,
    pub output: Option<PathBuf>,
    pub timestamp: DateTime<Local>,
    pub assessment_only: bool,
    pub files: Vec<ConversionResult>,
    pub total_playbooks: usize,
    pub total_roles: usize,
    pub total_tasks: usize,
}

impl ConversionReport {
    pub fn new(source: PathBuf) -> Self {
        Self {
            source,
            output: None,
            timestamp: Local::now(),
            assessment_only: false,
            files: Vec::new(),
            total_playbooks: 0,
            total_roles: 0,
            total_tasks: 0,
        }
    }

    pub fn add_file_result(&mut self, result: ConversionResult) {
        self.total_tasks += result.tasks_total;
        self.files.push(result);
    }

    pub fn total_converted(&self) -> usize {
        self.files.iter().map(|f| f.tasks_converted).sum()
    }

    pub fn total_modified(&self) -> usize {
        self.files.iter().map(|f| f.tasks_modified).sum()
    }

    pub fn total_need_review(&self) -> usize {
        self.files.iter().map(|f| f.tasks_need_review).sum()
    }

    pub fn has_errors(&self) -> bool {
        self.files.iter().any(|f| !f.success)
    }

    pub fn has_warnings(&self) -> bool {
        self.files.iter().any(|f| f.has_warnings())
    }

    pub fn all_unsupported_modules(&self) -> Vec<String> {
        let mut modules: Vec<String> = self
            .files
            .iter()
            .flat_map(|f| f.unsupported_modules.clone())
            .collect();
        modules.sort();
        modules.dedup();
        modules
    }

    pub fn all_unsupported_filters(&self) -> Vec<String> {
        let mut filters: Vec<String> = self
            .files
            .iter()
            .flat_map(|f| f.unsupported_filters.clone())
            .collect();
        filters.sort();
        filters.dedup();
        filters
    }

    /// Generate a summary string for console output
    pub fn summary(&self) -> String {
        let mut output = String::new();

        output.push_str("╔══════════════════════════════════════════════════════════════════╗\n");
        output.push_str("║                    Nexus Conversion Report                       ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        output.push_str(&format!(
            "║  Source: {:56} ║\n",
            truncate_path(&self.source, 56)
        ));
        if let Some(out) = &self.output {
            output.push_str(&format!("║  Output: {:56} ║\n", truncate_path(out, 56)));
        }
        output.push_str(&format!(
            "║  Time:   {:56} ║\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S")
        ));
        output.push_str("╚══════════════════════════════════════════════════════════════════╝\n\n");

        if self.assessment_only {
            output.push_str("MODE: Assessment Only (no files written)\n\n");
        }

        output.push_str("SUMMARY\n");
        output.push_str("───────────────────────────────────────────────────────────────────\n");

        let converted = self.total_converted();
        let modified = self.total_modified();
        let review = self.total_need_review();
        let total = self.total_tasks;

        if total > 0 {
            let converted_pct = (converted as f64 / total as f64 * 100.0) as usize;
            let modified_pct = (modified as f64 / total as f64 * 100.0) as usize;
            let review_pct = (review as f64 / total as f64 * 100.0) as usize;

            output.push_str(&format!("  Total tasks:        {}\n", total));
            output.push_str(&format!(
                "  ✓ Converted:        {} ({}%)\n",
                converted, converted_pct
            ));
            output.push_str(&format!(
                "  ~ Modified:         {} ({}%)\n",
                modified, modified_pct
            ));
            output.push_str(&format!(
                "  ⚠ Needs review:     {} ({}%)\n",
                review, review_pct
            ));
        } else {
            output.push_str("  No tasks found to convert.\n");
        }

        output.push('\n');

        // Show issues by file
        for file_result in &self.files {
            if !file_result.issues.is_empty() {
                output.push_str(&format!("\n{}\n", file_result.source_path.display()));
                output.push_str(
                    "───────────────────────────────────────────────────────────────────\n",
                );

                for issue in &file_result.issues {
                    let line_info = issue
                        .line
                        .map(|l| format!("Line {}: ", l))
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "  {} {}{}\n",
                        issue.severity, line_info, issue.message
                    ));

                    if let Some(original) = &issue.original {
                        output.push_str(&format!("    Before: {}\n", original));
                    }
                    if let Some(converted) = &issue.converted {
                        output.push_str(&format!("    After:  {}\n", converted));
                    }
                    if let Some(suggestion) = &issue.suggestion {
                        output.push_str(&format!("    Action: {}\n", suggestion));
                    }
                }
            }
        }

        // Unsupported items
        let unsupported_modules = self.all_unsupported_modules();
        let unsupported_filters = self.all_unsupported_filters();

        if !unsupported_modules.is_empty() || !unsupported_filters.is_empty() {
            output.push_str("\nUNSUPPORTED ITEMS\n");
            output
                .push_str("───────────────────────────────────────────────────────────────────\n");

            if !unsupported_modules.is_empty() {
                output.push_str(&format!("  Modules: {}\n", unsupported_modules.join(", ")));
            }
            if !unsupported_filters.is_empty() {
                output.push_str(&format!("  Filters: {}\n", unsupported_filters.join(", ")));
            }
        }

        // Next steps
        output.push_str("\nNEXT STEPS\n");
        output.push_str("───────────────────────────────────────────────────────────────────\n");

        if review > 0 {
            output.push_str(&format!(
                "  1. Review the {} task(s) marked for manual review\n",
                review
            ));
            output.push_str("  2. Run: nexus validate <output-file>\n");
            output.push_str("  3. Run: nexus plan <output-file> --check\n");
            output.push_str("  4. Test in non-production environment first\n");
        } else {
            output.push_str("  1. Run: nexus validate <output-file>\n");
            output.push_str("  2. Run: nexus plan <output-file>\n");
            output.push_str("  3. Test in non-production environment\n");
        }

        output
    }

    /// Generate a detailed markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Nexus Conversion Report\n\n");
        md.push_str(&format!("**Source:** `{}`\n\n", self.source.display()));
        if let Some(out) = &self.output {
            md.push_str(&format!("**Output:** `{}`\n\n", out.display()));
        }
        md.push_str(&format!(
            "**Generated:** {}\n\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S")
        ));

        md.push_str("## Summary\n\n");
        md.push_str("| Metric | Count | Percentage |\n");
        md.push_str("|--------|-------|------------|\n");

        let total = self.total_tasks;
        if total > 0 {
            let converted = self.total_converted();
            let modified = self.total_modified();
            let review = self.total_need_review();

            md.push_str(&format!("| Total Tasks | {} | - |\n", total));
            md.push_str(&format!(
                "| ✓ Converted | {} | {}% |\n",
                converted,
                (converted as f64 / total as f64 * 100.0) as usize
            ));
            md.push_str(&format!(
                "| ~ Modified | {} | {}% |\n",
                modified,
                (modified as f64 / total as f64 * 100.0) as usize
            ));
            md.push_str(&format!(
                "| ⚠ Needs Review | {} | {}% |\n",
                review,
                (review as f64 / total as f64 * 100.0) as usize
            ));
        }

        md.push_str("\n## Files Converted\n\n");
        for file_result in &self.files {
            md.push_str(&format!("### `{}`\n\n", file_result.source_path.display()));
            if let Some(out) = &file_result.output_path {
                md.push_str(&format!("→ `{}`\n\n", out.display()));
            }

            md.push_str(&format!(
                "- Tasks: {}/{} converted\n",
                file_result.tasks_converted, file_result.tasks_total
            ));
            md.push_str(&format!(
                "- Expressions converted: {}\n",
                file_result.expressions_converted
            ));

            if !file_result.issues.is_empty() {
                md.push_str("\n**Issues:**\n\n");
                for issue in &file_result.issues {
                    let icon = match issue.severity {
                        IssueSeverity::Info => "ℹ️",
                        IssueSeverity::Warning => "⚠️",
                        IssueSeverity::Error => "❌",
                    };
                    let line_info = issue
                        .line
                        .map(|l| format!(" (line {})", l))
                        .unwrap_or_default();
                    md.push_str(&format!("- {} {}{}\n", icon, issue.message, line_info));

                    if let Some(suggestion) = &issue.suggestion {
                        md.push_str(&format!("  - **Action:** {}\n", suggestion));
                    }
                }
            }
            md.push('\n');
        }

        // Unsupported items
        let unsupported_modules = self.all_unsupported_modules();
        let unsupported_filters = self.all_unsupported_filters();

        if !unsupported_modules.is_empty() || !unsupported_filters.is_empty() {
            md.push_str("## Unsupported Items\n\n");

            if !unsupported_modules.is_empty() {
                md.push_str("### Modules\n\n");
                for module in &unsupported_modules {
                    md.push_str(&format!("- `{}`\n", module));
                }
                md.push('\n');
            }

            if !unsupported_filters.is_empty() {
                md.push_str("### Filters\n\n");
                for filter in &unsupported_filters {
                    md.push_str(&format!("- `{}`\n", filter));
                }
                md.push('\n');
            }
        }

        md.push_str("## Next Steps\n\n");
        md.push_str("1. Review any items marked for manual review\n");
        md.push_str("2. Run `nexus validate <output-file>` to check syntax\n");
        md.push_str("3. Run `nexus plan <output-file>` to preview execution\n");
        md.push_str("4. Test in a non-production environment first\n");

        md
    }
}

fn truncate_path(path: &Path, max_len: usize) -> String {
    let s = path.display().to_string();
    if s.len() <= max_len {
        s
    } else {
        format!("...{}", &s[s.len() - max_len + 3..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_result() {
        let mut result = ConversionResult::new(PathBuf::from("test.yml"));
        result.tasks_total = 10;
        result.tasks_converted = 8;

        assert_eq!(result.conversion_percentage(), 80.0);
    }

    #[test]
    fn test_report_summary() {
        let mut report = ConversionReport::new(PathBuf::from("./ansible"));
        let mut result = ConversionResult::new(PathBuf::from("test.yml"));
        result.tasks_total = 5;
        result.tasks_converted = 4;
        result.tasks_need_review = 1;
        report.add_file_result(result);

        let summary = report.summary();
        assert!(summary.contains("Total tasks:"));
        assert!(summary.contains("Converted:"));
    }
}
