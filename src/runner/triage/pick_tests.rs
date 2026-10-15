//! Which issues `status` lists as due for triage, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use crate::ports::{Clock, OpenIssue, Timestamp};
use crate::runner::{Runner, step};
use crate::test::Rig;

// The maintainer is not kelpie's account, which is `the-maintainer`
const MAINTAINER: &str = "Boss";

// A rig whose App covers the repo, with `[app.dogs.kelpie.triage]` set to `lines`
fn rig_with(lines: &str) -> Rig {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer(MAINTAINER);
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.triage]\n{lines}\n"));
    rig
}

fn triaging(pick: &str) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(&format!("pick = \"{pick}\""));
    let runner = rig.open().unwrap();
    (rig, runner)
}

// An issue by `author` made `after` seconds after the rig's clock now reads
fn issue(rig: &Rig, number: u64, author: &str, after: u64) -> OpenIssue {
    OpenIssue {
        number,
        title: format!("issue {number}"),
        body: String::new(),
        author: author.to_owned(),
        author_is_bot: false,
        labels: Vec::new(),
        created: Timestamp(rig.clock.now().0 + after),
    }
}

fn list(rig: &Rig, issue: OpenIssue) {
    rig.forge.list_open_issue(issue);
}

fn status(rig: &Rig, runner: &Mutex<Runner>) -> Value {
    rig.ask(runner, "status", None)
}

// The issues `status` lists as due, in the order it lists them
fn due(rig: &Rig, runner: &Mutex<Runner>) -> Vec<u64> {
    let status = status(rig, runner);
    let listed = status["triage"]["due"].as_array().cloned();
    let numbers = listed.unwrap_or_default();
    numbers
        .iter()
        .map(|i| i["issue"].as_u64().unwrap())
        .collect()
}

#[test]
fn status_lists_an_unlabelled_issue_opened_after_pick_was_turned_on() {
    let (rig, runner) = triaging("all");
    list(&rig, issue(&rig, 31, "newcomer", 20));
    list(&rig, issue(&rig, 30, "newcomer", 10));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [30, 31], "oldest first");
    let status = status(&rig, &runner);
    assert_eq!(status["triage"]["due"][0]["title"], "issue 30");
    assert_eq!(status["triage"]["due"][0]["author"], "newcomer");
    assert_eq!(rig.forge.comments(), [], "nothing is posted");
}

#[test]
fn an_issue_opened_before_pick_was_turned_on_is_not_listed() {
    let rig = rig_with("pick = \"all\"");
    let old = issue(&rig, 30, "newcomer", 0);
    rig.clock.advance(100);
    list(&rig, old);
    let runner = rig.open().unwrap();
    list(&rig, issue(&rig, 31, "newcomer", 1));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [31]);
}

#[test]
fn an_issue_with_any_triage_label_is_not_listed() {
    let (rig, runner) = triaging("all");
    for (number, label) in [
        (30, "needs-triage"),
        (31, "needs-info"),
        (32, "ready-for-agent"),
        (33, "ready-for-human"),
        (34, "wontfix"),
    ] {
        let mut labelled = issue(&rig, number, "newcomer", 5);
        labelled.labels = vec!["bug".to_owned(), label.to_owned()];
        list(&rig, labelled);
    }
    let mut other = issue(&rig, 35, "newcomer", 5);
    other.labels = vec!["bug".to_owned()];
    list(&rig, other);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [35]);
}

#[test]
fn a_bots_issue_is_not_listed() {
    let (rig, runner) = triaging("all");
    for (number, author) in [(30, "dependabot[bot]"), (31, "kelpie-app[bot]")] {
        let mut bot = issue(&rig, number, author, 5);
        bot.author_is_bot = true;
        list(&rig, bot);
    }
    list(&rig, issue(&rig, 32, "newcomer", 5));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [32]);
}

#[test]
fn pick_maintainer_lists_only_the_maintainers_issues_ignoring_case() {
    let (rig, runner) = triaging("maintainer");
    list(&rig, issue(&rig, 30, "boss", 5));
    list(&rig, issue(&rig, 31, "stranger", 6));
    list(&rig, issue(&rig, 32, "BOSS", 7));
    list(&rig, issue(&rig, 33, "", 8));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [30, 32]);
}

#[test]
fn pick_all_lists_other_people_s_issues_too() {
    let (rig, runner) = triaging("all");
    list(&rig, issue(&rig, 30, "stranger", 5));
    list(&rig, issue(&rig, 31, "", 6));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [30, 31], "a deleted account's too");
}

#[test]
fn pick_off_starts_with_no_triager_file_and_lists_nothing() {
    let rig = rig_with("pick = \"off\"\nagent = \"nobody\"");
    let runner = rig.open().unwrap();
    list(&rig, issue(&rig, 30, "newcomer", 5));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), Vec::<u64>::new());
    assert!(status(&rig, &runner).get("triage").is_none());
}

#[test]
fn a_pick_other_than_off_without_the_app_refuses_to_start_naming_triage_pick() {
    for pick in ["maintainer", "all"] {
        let rig = Rig::new("shep");
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.triage]\npick = \"{pick}\"\n"));
        let err = rig.open().unwrap_err().to_string();
        assert!(err.contains("triage.pick"), "{err}");
        assert!(err.contains("as the maintainer"), "{err}");
    }
}

#[test]
fn a_missing_or_wrong_role_triage_agent_refuses_to_start_naming_triage_agent() {
    for agent in ["nobody", "opus-high"] {
        let rig = rig_with(&format!("pick = \"all\"\nagent = \"{agent}\""));
        let err = rig.open().unwrap_err().to_string();
        assert!(err.contains("triage.agent"), "{agent}: {err}");
    }
}

#[test]
fn since_survives_a_restart_and_pick_off_at_start_clears_it() {
    let rig = rig_with("pick = \"all\"");
    let first = rig.open().unwrap();
    assert_eq!(step(&first).unwrap(), None);
    rig.clock.advance(100);
    list(&rig, issue(&rig, 30, "newcomer", 0));
    // It was opened between the first start and the second.
    list(&rig, {
        let mut between = issue(&rig, 31, "newcomer", 0);
        between.created = Timestamp(between.created.0 - 50);
        between
    });

    let second = rig.open().unwrap();
    assert_eq!(step(&second).unwrap(), None);
    assert_eq!(
        due(&rig, &second),
        [31, 30],
        "since stayed at the first start"
    );

    rig.edit_settings(|s| s.replace("pick = \"all\"", "pick = \"off\""));
    let off = rig.open().unwrap();
    assert_eq!(step(&off).unwrap(), None);
    assert_eq!(due(&rig, &off), Vec::<u64>::new());

    rig.edit_settings(|s| s.replace("pick = \"off\"", "pick = \"all\""));
    rig.clock.advance(100);
    let back = rig.open().unwrap();
    assert_eq!(step(&back).unwrap(), None);
    assert_eq!(
        due(&rig, &back),
        Vec::<u64>::new(),
        "since is the new start"
    );
}

#[test]
fn an_unreadable_listing_lists_nothing_and_fails_no_poll() {
    let (rig, runner) = triaging("all");
    list(&rig, issue(&rig, 30, "newcomer", 5));
    rig.forge.set_issues_down(true);

    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(due(&rig, &runner), Vec::<u64>::new());

    rig.forge.set_issues_down(false);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(due(&rig, &runner), [30]);
}
