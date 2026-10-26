//! The paths an issue names, checked against the checkout of `main` before it is filed

use serde_json::json;

use super::*;
use crate::confine::Verdict;
use crate::guard::{IssueRules, judge_issues, record_issues};
use crate::local_paths::LocalPaths;

/// A checkout of `main` holding `src/store.rs` and `src/dogs/route.rs`, and the rules of a
/// session filing from it
struct Checkout {
    dir: tempfile::TempDir,
    rules: IssueRules,
}

impl Checkout {
    fn new(local: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for file in ["src/store.rs", "src/dogs/route.rs", ".git/HEAD"] {
            write_in(dir.path(), file, "");
        }
        let rules = IssueRules {
            status: "ready-for-human".to_owned(),
            agents: vec!["sonnet-high".to_owned()],
            ledger: Some(dir.path().join("ledger.json")),
            tickets: local.then(|| "koji".to_owned()),
        };
        Self { dir, rules }
    }

    // The guard's verdict on `command`, run in the checkout.
    fn judge(&self, command: &str) -> Verdict {
        let call = json!({
            "tool_name": "Bash",
            "cwd": self.dir.path(),
            "tool_input": { "command": command },
        });
        let home = Some(Path::new("/home/me"));
        judge_issues(
            call.to_string().as_bytes(),
            home,
            LocalPaths::new(home),
            &self.rules,
        )
    }

    // The command that files `body` as the issue writer would.
    fn create(&self, body: &str) -> String {
        let labels = "--label ready-for-human --label agent:sonnet-high";
        match self.rules.tickets {
            Some(_) => format!(
                "shep kelpie ticket new 'Add a thing' -p koji {labels} --body \"$(cat <<'EOF'\n\
                 {body}\nEOF\n)\""
            ),
            None => format!(
                "gh issue create --title 'Add a thing' {labels} --body-file - <<'EOF'\n{body}\nEOF"
            ),
        }
    }
}

fn body_naming(paths: &str) -> String {
    format!("Change {paths}.\n\n## Acceptance criteria\n\n- [ ] It works\n")
}

#[track_caller]
fn sent_back(verdict: Verdict, paths: &[&str]) {
    let Verdict::Refuse(why) = verdict else {
        panic!("allowed, wanted the paths sent back");
    };
    assert!(why.contains("name no file on main"), "{why}");
    for path in paths {
        assert!(why.contains(&format!("`{path}`")), "{path}: {why}");
    }
}

#[test]
fn an_issue_naming_a_missing_path_goes_back_to_the_issue_writer_once_before_it_files() {
    for local in [false, true] {
        let s = Checkout::new(local);
        let command = s.create(&body_naming("`src/gone.rs` and `dogs_kv.rs`"));

        sent_back(s.judge(&command), &["src/gone.rs", "dogs_kv.rs"]);
        assert_eq!(
            s.judge(&command),
            Verdict::Allow,
            "the same issue is filed on its second try (local: {local})"
        );
    }
}

#[test]
fn only_a_path_not_sent_back_before_is_sent_back_again() {
    let s = Checkout::new(false);
    sent_back(
        s.judge(&s.create(&body_naming("`src/gone.rs`"))),
        &["src/gone.rs"],
    );

    let verdict = s.judge(&s.create(&body_naming("`src/gone.rs` and `src/other.rs`")));

    sent_back(verdict.clone(), &["src/other.rs"]);
    let Verdict::Refuse(why) = verdict else {
        unreachable!();
    };
    assert!(!why.contains("src/gone.rs"), "{why}");
}

#[test]
fn paths_that_resolve_pass_with_nothing_sent_back() {
    let s = Checkout::new(false);
    let body = body_naming("`src/store.rs`, `route.rs:40` and `src/dogs/route.rs`");

    assert_eq!(s.judge(&s.create(&body)), Verdict::Allow);
    // Nothing was told, so a path that is gone later is still sent back.
    sent_back(
        s.judge(&s.create(&body_naming("`src/gone.rs`"))),
        &["src/gone.rs"],
    );
}

#[test]
fn a_session_with_no_ledger_or_no_checkout_to_read_is_not_held_up() {
    let mut s = Checkout::new(false);
    let command = s.create(&body_naming("`src/gone.rs`"));
    s.rules.ledger = None;
    assert_eq!(s.judge(&command), Verdict::Allow, "nowhere to remember");

    let s = Checkout::new(false);
    let call = json!({
        "tool_name": "Bash",
        "cwd": "/no/such/checkout",
        "tool_input": { "command": s.create(&body_naming("`src/gone.rs`")) },
    });
    let home = Some(Path::new("/home/me"));
    let verdict = judge_issues(
        call.to_string().as_bytes(),
        home,
        LocalPaths::new(home),
        &s.rules,
    );
    assert_eq!(verdict, Verdict::Allow);
}

#[test]
fn what_was_sent_back_survives_the_hook_that_records_what_was_filed() {
    let s = Checkout::new(false);
    let command = s.create(&body_naming("`src/gone.rs`"));
    sent_back(s.judge(&command), &["src/gone.rs"]);

    let ran = json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "tool_response": { "stdout": "https://github.com/acme/shep/issues/9\n", "stderr": "" },
    });
    record_issues(ran.to_string().as_bytes(), &s.rules);

    assert_eq!(s.judge(&command), Verdict::Allow);
}

#[test]
fn the_prompt_asks_for_the_paths_in_backticks_and_for_every_x_to_be_listed() {
    let s = Setup::new();
    let prompt = instructions(&s.writer, &s.project(), Mode::Headless);

    assert!(prompt.contains("resolves them against `main`"), "{prompt}");
    assert!(prompt.contains("lists the Xs by name"), "{prompt}");
}
