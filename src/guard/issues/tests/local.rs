//! The issue writer on a project whose tickets are the local store

use super::*;

// `ticket new` with `labels`, in project `-p`, and its body as a heredoc.
fn new_in(project: &str, labels: &str, body: &str) -> String {
    format!(
        "shep kelpie ticket new 'Add a thing' -p {project} {labels} --body \"$(cat <<'EOF'\n{body}EOF\n)\""
    )
}

fn new_ticket(labels: &str, body: &str) -> String {
    new_in("koji", labels, body)
}

fn labelled(status: &str) -> String {
    format!("--label {status} --label agent:sonnet-high")
}

impl Session {
    // Files local ticket `k<number>` in this session.
    fn filed_local(&self, number: u64) {
        let command = new_ticket(&labelled(&self.rules.status), BODY);
        self.ran(&command, &format!("k{number}\n"));
    }
}

#[test]
fn ticket_new_is_allowed_and_gh_issue_create_is_refused() {
    for status in MODES {
        let s = Session::local(status);
        assert_eq!(
            s.bash(&new_ticket(&labelled(status), BODY)),
            Verdict::Allow,
            "{status}"
        );
        refused(
            s.bash(&create(&labelled(status), BODY)),
            "this project's tickets are local",
        );
        refused(
            s.bash("gh issue new --title x --body y"),
            "shep kelpie ticket new",
        );
    }
}

#[test]
fn on_a_github_project_nothing_changes() {
    for status in MODES {
        let s = Session::new(status);
        let labels = labelled(status);
        assert_eq!(s.bash(&create(&labels, BODY)), Verdict::Allow, "{status}");
        for command in [
            new_ticket(&labels, BODY),
            "shep kelpie ticket list -p koji".to_owned(),
            "shep kelpie ticket show k1 -p koji".to_owned(),
        ] {
            refused(s.bash(&command), "runs only these commands");
        }
    }
}

#[test]
fn the_rules_name_the_local_project_through_the_hooks_command_line() {
    let s = Session::local(HUMAN);
    assert!(s.rules.flags().contains(&"--tickets=koji".to_owned()));
    let (read, rest) = issue_rules(&s.rules.flags());
    assert_eq!(read.as_ref(), Some(&s.rules));
    assert!(rest.is_empty(), "{rest:?}");
    let (read, _) = issue_rules(&Session::new(HUMAN).rules.flags());
    assert_eq!(read.and_then(|r| r.tickets), None);
}

#[test]
fn a_ticket_is_filed_under_the_rules_an_issue_is() {
    for status in MODES {
        let s = Session::local(status);
        let other = if status == HUMAN { READY } else { HUMAN };
        for (command, why) in [
            (
                new_ticket("--label agent:sonnet-high", BODY),
                format!("file the issue with the label `{status}`"),
            ),
            (
                new_ticket(&format!("--label {status} --label {other}"), BODY),
                format!("puts no `{other}` on"),
            ),
            (
                new_ticket(&format!("--label {status} --label agent:nobody"), BODY),
                "`agent:nobody` names no implementer".to_owned(),
            ),
            (
                new_ticket(
                    &format!("--label {status} --label agent:sonnet-high --label agent:opus-high"),
                    BODY,
                ),
                "at most one".to_owned(),
            ),
            (
                new_ticket(&labelled(status), "Build the thing.\n"),
                "Acceptance criteria".to_owned(),
            ),
            (
                new_ticket(&labelled(status), &format!("See /home/me/x.\n{BODY}")),
                "carries a path on this machine".to_owned(),
            ),
            (
                format!(
                    "shep kelpie ticket new \"$(whoami)\" -p koji --body x {}",
                    labelled(status)
                ),
                "only as plain words".to_owned(),
            ),
        ] {
            refused(s.bash(&command), &why);
        }
    }
}

#[test]
fn a_ticket_is_filed_in_this_project_and_no_other() {
    let s = Session::local(HUMAN);
    let labels = labelled(HUMAN);
    refused(
        s.bash(&new_in("rotom", &labels, BODY)),
        "name this project's tickets with `-p koji`",
    );
    refused(
        s.bash(&format!("shep kelpie ticket new 'x' --body 'y' {labels}")),
        "name this project's tickets with `-p koji`",
    );
    refused(
        s.bash("shep kelpie ticket list -p koji -p rotom"),
        "name this project's tickets with `-p koji`, once",
    );
    for command in [
        new_in("koji", &labels, BODY).replace("-p koji", "--project koji"),
        new_in("koji", &labels, BODY).replace("-p koji", "--project=koji"),
    ] {
        assert_eq!(s.bash(&command), Verdict::Allow, "{command}");
    }
}

#[test]
fn it_reads_this_projects_tickets() {
    let s = Session::local(HUMAN);
    for command in [
        "shep kelpie ticket show k4 -p koji",
        "shep kelpie ticket list -p koji",
        "shep kelpie ticket list --all -p koji",
        "shep kelpie ticket list -p koji --all && shep kelpie ticket show k2 -p koji",
    ] {
        assert_eq!(s.bash(command), Verdict::Allow, "{command}");
    }
    for (command, why) in [
        ("shep kelpie ticket show k4", "name this project's tickets"),
        ("shep kelpie ticket show 340 -p koji", "is not one"),
        ("shep kelpie ticket show -p koji", "takes one ticket"),
        ("shep kelpie ticket list k4 -p koji", "takes only its flags"),
        (
            "shep kelpie ticket list -p koji --json",
            "does not take `--json`",
        ),
    ] {
        refused(s.bash(command), why);
    }
}

#[test]
fn it_runs_no_other_ticket_verb_and_no_other_shep() {
    let s = Session::local(HUMAN);
    for command in [
        "shep kelpie ticket close k1 -p koji",
        "shep kelpie ticket -p koji",
        "shep kelpie add k1 -p koji",
        "shep kelpie issue 'more' -p koji",
        "shep kelpie rule 1 yes",
        "shep trigger koji add k1",
        "shep kelpie",
        "shep",
    ] {
        refused(s.bash(command), "runs only these commands");
    }
}

#[test]
fn it_edits_the_labels_and_links_of_the_tickets_it_filed() {
    let s = Session::local(HUMAN);
    s.filed_local(3);
    for command in [
        "shep kelpie ticket edit k3 -p koji --unlabel agent:sonnet-high --label agent:opus-high",
        "shep kelpie ticket edit k3 -p koji --label ready-for-human",
        "shep kelpie ticket edit k3 -p koji --blocked-by k1",
        "shep kelpie ticket edit k3 -p koji --parent k3",
    ] {
        assert_eq!(s.bash(command), Verdict::Allow, "{command}");
    }
    for (command, why) in [
        (
            "shep kelpie ticket edit k1 -p koji --label agent:opus-high",
            "k1 is not one",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --parent k1",
            "`--parent` must be one, and k1 is not",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --title 'new'",
            "does not take `--title`",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --body 'new'",
            "does not take `--body`",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --label ready-for-agent",
            "puts no `ready-for-agent` on",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --unlabel ready-for-human",
            "takes off only `agent:` labels",
        ),
        (
            "shep kelpie ticket edit k3 -p koji --label agent:nobody",
            "names no implementer",
        ),
        ("shep kelpie ticket edit k3 -p koji", "takes only"),
        (
            "shep kelpie ticket edit 3 -p koji --label agent:opus-high",
            "is not one",
        ),
    ] {
        refused(s.bash(command), why);
    }
}

#[test]
fn a_ticket_it_did_not_file_is_not_edited_whatever_it_printed() {
    let s = Session::local(HUMAN);
    // A command that is not the lone `ticket new` files nothing, whatever its
    // output says.
    s.ran("shep kelpie ticket show k1 -p koji", "k1\n");
    s.ran("shep kelpie ticket list -p koji", "k1\n");
    s.ran(&new_ticket(&labelled(HUMAN), BODY), "5\n");
    s.ran(&new_ticket(&labelled(HUMAN), BODY), "k6\nk7\n");
    for ticket in ["k1", "k5", "k6", "k7"] {
        refused(
            s.bash(&format!(
                "shep kelpie ticket edit {ticket} -p koji --label agent:opus-high"
            )),
            "is not one",
        );
    }
}

#[test]
fn it_still_reads_and_links_the_repos_github_issues() {
    let s = Session::local(HUMAN);
    s.read_id(7);
    for command in [
        "gh issue view 7 --comments",
        "gh issue list --search 'lease book' --state all",
        "gh api 'repos/{owner}/{repo}/issues/7' --jq .id",
    ] {
        assert_eq!(s.bash(command), Verdict::Allow, "{command}");
    }
    s.filed_local(3);
    refused(
        s.bash("gh issue edit 3 --add-label agent:opus-high"),
        "is not one",
    );
}

#[test]
fn a_heredoc_that_prints_an_id_files_nothing() {
    let s = Session::local(HUMAN);
    let forged = format!(
        "{}\ncat <<'EOF'\nk9\nEOF",
        new_ticket(&labelled(HUMAN), BODY)
    );
    refused(s.bash(&forged), "A `cat` runs only as the body");
}
