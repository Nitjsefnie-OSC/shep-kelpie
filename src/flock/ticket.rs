//! `shep kelpie ticket new|list|show|edit|close`: a local project's tickets, filed
//! and read as files in its `tickets/` folder (ADR 0014)
//!
//! The verbs work the files directly, so a ticket filed here is on the board at
//! the runner's next read, with the runner stopped or running. They are in
//! `shep-kelpie-core`, where the runner's rig runs them too.

pub use crate::local_tickets::verb::run;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test::short_tempdir;

    fn ticket(folder: &Path, args: &[&str]) -> Result<Vec<String>, String> {
        run(folder, args)
    }

    #[test]
    fn new_files_a_ticket_and_prints_its_id() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        let args = [
            "new",
            "Add a thing",
            "--body",
            "Do it.",
            "--label",
            "ready-for-agent",
        ];
        assert_eq!(ticket(&folder, &args).unwrap(), ["k1"]);
        assert_eq!(ticket(&folder, &["new", "Another"]).unwrap(), ["k2"]);
        let shown = ticket(&folder, &["show", "k1"]).unwrap().join("\n");
        assert!(shown.contains("title: \"Add a thing\""), "{shown}");
        assert!(shown.contains("labels: [\"ready-for-agent\"]"), "{shown}");
        assert!(shown.ends_with("Do it."), "{shown}");
    }

    #[test]
    fn list_shows_the_open_tickets_and_all_shows_the_closed_too() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One", "--label", "ready-for-agent"]).unwrap();
        ticket(&folder, &["new", "Two"]).unwrap();
        ticket(&folder, &["close", "k1"]).unwrap();
        assert_eq!(ticket(&folder, &["list"]).unwrap(), ["k2  open  Two"]);
        assert_eq!(
            ticket(&folder, &["list", "--all"]).unwrap(),
            ["k1  closed  One [ready-for-agent]", "k2  open  Two"]
        );
    }

    #[test]
    fn edit_changes_what_it_is_told_to_and_nothing_else() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One"]).unwrap();
        ticket(&folder, &["new", "Two", "--label", "a", "--label", "b"]).unwrap();
        let edit = [
            "edit",
            "k2",
            "--title",
            "Second",
            "--unlabel",
            "a",
            "--label",
            "c",
            "--blocked-by",
            "k1",
        ];
        ticket(&folder, &edit).unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(shown.contains("title: \"Second\""), "{shown}");
        assert!(shown.contains("labels: [\"b\", \"c\"]"), "{shown}");
        assert!(shown.contains("blocked_by: [\"k1\"]"), "{shown}");
        ticket(
            &folder,
            &["edit", "k2", "--unblock", "k1", "--parent", "k1"],
        )
        .unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(
            shown.contains("parent: \"k1\"") && !shown.contains("blocked_by"),
            "{shown}"
        );
        ticket(&folder, &["edit", "k2", "--no-parent"]).unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(!shown.contains("parent"), "{shown}");
    }

    #[test]
    fn close_can_say_why() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One"]).unwrap();
        ticket(&folder, &["close", "k1", "--not-planned"]).unwrap();
        let shown = ticket(&folder, &["show", "k1"]).unwrap().join("\n");
        assert!(shown.contains("state: not_planned"), "{shown}");
    }

    #[test]
    fn a_line_that_is_not_one_of_the_verbs_is_refused_with_the_usage() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        for args in [
            &["new"][..],
            &["new", "a", "b"],
            &["new", "a", "--body"],
            &["new", "a", "--nope", "x"],
            &["new", "a", "--parent", "k4"],
            &["show", "340"],
            &["show", "k9"],
            &["edit", "k1", "--title", "x"],
            &["list", "extra"],
            &["close"],
            &["frobnicate"],
            &[],
        ] {
            assert!(ticket(&folder, args).is_err(), "{args:?}");
        }
        let err = ticket(&folder, &["frobnicate"]).unwrap_err();
        assert!(err.starts_with("usage: shep kelpie ticket new"), "{err}");
        let err = ticket(&folder, &["show", "340"]).unwrap_err();
        assert!(err.contains("GitHub issue"), "{err}");
    }
}
