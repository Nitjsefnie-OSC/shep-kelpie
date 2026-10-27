//! `shep kelpie issue check`: the issue check on one ticket, read through the
//! project's ticket port, or on a draft in a file before it is filed
//!
//! The draft form reads only its file, so it runs where a worker's sandbox
//! keeps the shepherd, the settings and the network out of reach.

use std::path::Path;

use crate::board::check::check;
use crate::ports::Tickets;
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;

/// What the check said of one issue or draft
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// It passes: the line saying so
    Passes(String),
    /// It fails: a line naming it, then a line per failure
    Fails(Vec<String>),
}

impl Checked {
    // `shown`'s verdict on the issue of `title`, `body` and `labels`.
    fn of(shown: &str, title: &str, body: &str, labels: &[String]) -> Self {
        let failures = check(title, body, labels);
        if failures.is_empty() {
            return Self::Passes(format!("{shown} passes the issue check"));
        }
        let head = format!("{shown} fails the issue check:");
        let lines = failures.iter().map(|f| format!("- {f}"));
        Self::Fails([head].into_iter().chain(lines).collect())
    }
}

/// Reads `ticket` on `repo` through `tickets` and checks it
///
/// # Errors
///
/// Why the ticket could not be read.
pub fn run(tickets: &dyn Tickets, repo: &ForgeSlug, ticket: TicketId) -> Result<Checked, String> {
    let shown = ticket.shown();
    let issue = (tickets.issue(repo, ticket)).map_err(|e| format!("cannot read {shown}: {e}"))?;
    Ok(Checked::of(
        &shown,
        &issue.title,
        &issue.body,
        &issue.labels,
    ))
}

/// Checks the draft in `file`, its first line the title and the rest the
/// body, as if filed with `labels`
///
/// # Errors
///
/// Why the file could not be read.
pub fn draft(file: &Path, labels: &[String]) -> Result<Checked, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read {}: {}", file.display(), e.kind()))?;
    let (title, body) = text.split_once('\n').unwrap_or((&text, ""));
    let shown = format!("the draft {}", file.display());
    Ok(Checked::of(
        &shown,
        title.trim_end_matches('\r'),
        body,
        labels,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test::{FakeForge, ticket};

    const BODY: &str = "Touches: `src/board.rs`\n\n## What to build\n\n- A thing\n\n\
        ## Acceptance criteria\n\n- [ ] It works\n";

    fn repo() -> ForgeSlug {
        ForgeSlug::try_from("acme/widgets".to_owned()).unwrap()
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    #[test]
    fn an_issue_in_the_format_passes() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.open_issue(12, "A thing is true", BODY);
        forge.label(12, "enhancement");
        forge.label(12, "priority: P4");
        let said = run(&forge, &repo(), ticket(12));
        assert_eq!(
            said,
            Ok(Checked::Passes("#12 passes the issue check".to_owned()))
        );
    }

    #[test]
    fn an_issue_out_of_the_format_is_told_each_failure() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.open_issue(12, "A thing is true", "Make it work.");
        forge.label(12, "bug");
        let said = run(&forge, &repo(), ticket(12));
        let lines = [
            "#12 fails the issue check:",
            "- no `Touches:` line naming the files it changes",
            "- no `## What to build` section, or nothing under it",
            "- no `## Acceptance criteria` section",
            "- no priority label, `priority: P0` to `priority: P4`",
        ];
        assert_eq!(said, Ok(Checked::Fails(labels(&lines))));
    }

    #[test]
    fn an_issue_that_cannot_be_read_is_named() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.remove_issue(12);
        let said = run(&forge, &repo(), ticket(12)).unwrap_err();
        assert!(said.starts_with("cannot read #12: "), "{said}");
    }

    #[test]
    fn a_draft_is_its_files_first_line_as_the_title_and_the_rest_as_the_body() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("issue.md");
        std::fs::write(&file, format!("A thing is true\n{BODY}")).unwrap();
        let said = draft(&file, &labels(&["enhancement", "priority: P4"]));
        let shown = format!("the draft {} passes the issue check", file.display());
        assert_eq!(said, Ok(Checked::Passes(shown)));
        std::fs::write(&file, format!("\n{BODY}")).unwrap();
        let Ok(Checked::Fails(lines)) = draft(&file, &labels(&["bug", "P4"])) else {
            panic!("an empty title fails");
        };
        assert_eq!(lines[1], "- the title is empty");
    }

    #[test]
    fn a_draft_that_cannot_be_read_is_named() {
        let said = draft(Path::new("/nowhere/issue.md"), &[]).unwrap_err();
        assert!(
            said.starts_with("cannot read /nowhere/issue.md: "),
            "{said}"
        );
    }
}
