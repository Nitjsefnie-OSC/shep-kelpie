//! `shep kelpie issue check <n>`: the issue check on one ticket, read through
//! the project's ticket port

use crate::board::check::check;
use crate::ports::Tickets;
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;

/// Reads `ticket` on `repo` through `tickets` and says that it passes the
/// issue check
///
/// # Errors
///
/// A message naming each part the issue lacks or holds malformed, one to a
/// line, or why it could not be read.
pub fn run(
    tickets: &dyn Tickets,
    repo: &ForgeSlug,
    ticket: TicketId,
) -> Result<Vec<String>, String> {
    let shown = ticket.shown();
    let issue = (tickets.issue(repo, ticket)).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let failures = check(&issue.title, &issue.body, &issue.labels);
    if failures.is_empty() {
        return Ok(vec![format!("{shown} passes the issue check")]);
    }
    let mut lines = vec![format!("{shown} fails the issue check:")];
    lines.extend(failures.iter().map(|f| format!("- {f}")));
    Err(lines.join("\n"))
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

    #[test]
    fn an_issue_in_the_format_passes() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.open_issue(12, "A thing is true", BODY);
        forge.label(12, "enhancement");
        forge.label(12, "priority: P4");
        let said = run(&forge, &repo(), ticket(12));
        assert_eq!(said, Ok(vec!["#12 passes the issue check".to_owned()]));
    }

    #[test]
    fn an_issue_out_of_the_format_is_told_each_failure() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.open_issue(12, "A thing is true", "Make it work.");
        forge.label(12, "bug");
        let said = run(&forge, &repo(), ticket(12));
        assert_eq!(
            said,
            Err("#12 fails the issue check:\n\
                 - no `Touches:` line naming the files it changes\n\
                 - no `## What to build` section, or nothing under it\n\
                 - no `## Acceptance criteria` section\n\
                 - no priority label, `priority: P0` to `priority: P4`"
                .to_owned())
        );
    }

    #[test]
    fn an_issue_that_cannot_be_read_is_named() {
        let forge = FakeForge::new(PathBuf::from("/nowhere"));
        forge.remove_issue(12);
        let said = run(&forge, &repo(), ticket(12)).unwrap_err();
        assert!(said.starts_with("cannot read #12: "), "{said}");
    }
}
