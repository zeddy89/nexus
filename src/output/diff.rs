// Unified diff generation for file changes

use similar::{ChangeTag, TextDiff};

/// Generate a unified diff between two file contents
///
/// Returns a formatted diff string showing line-by-line changes
/// with +/- indicators and context lines.
pub fn generate_unified_diff(
    old_content: &str,
    new_content: &str,
    old_label: &str,
    new_label: &str,
) -> String {
    let diff = TextDiff::from_lines(old_content, new_content);

    let mut output = String::new();
    output.push_str(&format!("--- {}\n", old_label));
    output.push_str(&format!("+++ {}\n", new_label));

    // Generate unified diff with 3 lines of context
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }

        let mut old_start = 0;
        let mut old_len = 0;
        let mut new_start = 0;
        let mut new_len = 0;

        // Calculate line ranges for hunk header
        for op in group {
            let old_range = op.old_range();
            let new_range = op.new_range();

            if old_start == 0 || old_range.start < old_start {
                old_start = old_range.start;
            }
            if new_start == 0 || new_range.start < new_start {
                new_start = new_range.start;
            }

            old_len += old_range.len();
            new_len += new_range.len();
        }

        // Write hunk header
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start + 1,
            old_len,
            new_start + 1,
            new_len
        ));

        // Write changes
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                output.push_str(&format!("{}{}", sign, change));
            }
        }
    }

    output
}

/// Generate a simple side-by-side diff summary
///
/// Returns a compact representation showing additions and deletions
pub fn generate_diff_summary(old_content: &str, new_content: &str) -> String {
    let diff = TextDiff::from_lines(old_content, new_content);

    let mut additions = 0;
    let mut deletions = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Insert => additions += 1,
            ChangeTag::Equal => {}
        }
    }

    if additions == 0 && deletions == 0 {
        "No changes".to_string()
    } else {
        format!("+{} -{} lines", additions, deletions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_diff() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 2 modified\nline 3\n";

        let diff = generate_unified_diff(old, new, "old.txt", "new.txt");

        assert!(diff.contains("--- old.txt"));
        assert!(diff.contains("+++ new.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+line 2 modified"));
    }

    #[test]
    fn test_diff_summary() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 2 modified\nline 3\nline 4\n";

        let summary = generate_diff_summary(old, new);

        assert!(summary.contains("+2"));
        assert!(summary.contains("-1"));
    }

    #[test]
    fn test_no_changes() {
        let content = "line 1\nline 2\n";
        let summary = generate_diff_summary(content, content);

        assert_eq!(summary, "No changes");
    }
}
