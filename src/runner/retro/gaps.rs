//! The retro's own earlier findings that are still open
//!
//! A report ends with a `## New findings` section of bullets, each
//! `- <theme>: <what>`. Reports stay in `retro-reports/unverified` until the
//! maintainer consolidates them, so the themes of the reports still there are
//! the open known gaps. The next retro is told them, asked for new findings
//! only, and asked to count a gap that recurs by its theme.

use std::fs;
use std::path::Path;

/// The heading the report's findings sit under
pub(super) const HEADING: &str = "## New findings";

/// The most themes the prompt lists
const LISTED: usize = 40;

/// The most characters of a theme
const THEME_LEN: usize = 80;

/// The themes of `report`'s findings
pub(super) fn themes(report: &str) -> Vec<String> {
    let mut lines = report.lines().map(str::trim);
    if !lines.any(|l| l.eq_ignore_ascii_case(HEADING)) {
        return Vec::new();
    }
    (lines.take_while(|l| !l.starts_with('#')))
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")))
        .map(|bullet| bullet.replace(['*', '`'], ""))
        .map(|bullet| {
            let theme = bullet.split(':').next().unwrap_or_default();
            theme.trim().chars().take(THEME_LEN).collect::<String>()
        })
        .filter(|theme| !theme.is_empty())
        .collect()
}

/// The open known gaps: the themes in the reports kept in `folder`, oldest
/// report first, each once. A folder or report that cannot be read adds none.
pub(super) fn open(folder: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
    files.sort();
    let mut gaps: Vec<String> = Vec::new();
    for file in files {
        let text = fs::read_to_string(file).unwrap_or_default();
        for theme in themes(&text) {
            if !gaps.iter().any(|g| g.eq_ignore_ascii_case(&theme)) {
                gaps.push(theme);
            }
        }
    }
    gaps.truncate(LISTED);
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "# Batch retro: koji #1\n\n---\n\n## Patterns\n\n- not a finding: x\n\n\
        ## New findings\n\n- **Sandbox cannot bind a socket**: tests fail there\n\
        * `state VERSION` conflicts: every item\n\n## Later\n\n- nor this: y\n";

    #[test]
    fn the_themes_are_the_bullets_under_the_findings_heading() {
        assert_eq!(
            themes(REPORT),
            ["Sandbox cannot bind a socket", "state VERSION conflicts"]
        );
    }

    #[test]
    fn a_report_with_no_findings_heading_has_no_themes() {
        assert_eq!(themes("## Patterns\n\n- a: b\n"), Vec::<String>::new());
    }

    #[test]
    fn the_open_gaps_are_the_reports_themes_once_each_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("koji-batch-1.md"), REPORT).unwrap();
        fs::write(
            dir.path().join("koji-batch-2.md"),
            "## New findings\n\n- sandbox cannot bind a socket: again\n- slow CI: new\n",
        )
        .unwrap();

        assert_eq!(
            open(dir.path()),
            [
                "Sandbox cannot bind a socket",
                "state VERSION conflicts",
                "slow CI"
            ]
        );
        assert_eq!(open(&dir.path().join("none")), Vec::<String>::new());
    }
}
