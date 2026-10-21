use std::sync::Mutex;

use serde_json::Value;

use crate::ports::{AuthorAssociation, Clock, OpenIssue, Timestamp};
use crate::runner::{Runner, step};
use crate::test::Rig;

use super::settings_with;

const TABLE: &str = "\n[app.dogs.kelpie.triage]\n";

// A rig whose App covers the repo, with a runner opened under `pick`
fn rig_picking(pick: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.edit_settings(|s| format!("{s}{TABLE}pick = \"{pick}\"\n"));
    let runner = rig.open().unwrap();
    (rig, runner)
}

fn pick_now(rig: &Rig, from: &str, to: &str) -> crate::settings::Settings {
    settings_with(rig, |s| {
        s.replace(
            &format!("{TABLE}pick = \"{from}\""),
            &format!("{TABLE}pick = \"{to}\""),
        )
    })
}

// An issue by someone else made `after` seconds after the rig's clock now reads
fn issue(rig: &Rig, number: u64, after: u64) -> OpenIssue {
    OpenIssue {
        number,
        title: format!("issue {number}"),
        body: String::new(),
        author: "newcomer".to_owned(),
        author_is_bot: false,
        author_association: Some(AuthorAssociation::Contributor),
        labels: Vec::new(),
        created: Timestamp(rig.clock.now().0 + after),
    }
}

fn due(rig: &Rig, runner: &Mutex<Runner>) -> Vec<u64> {
    let status = rig.ask(runner, "status", None);
    let listed = status["triage"]["due"].as_array().cloned();
    let numbers = listed.unwrap_or_default();
    numbers
        .iter()
        .map(|i| i["issue"].as_u64().unwrap())
        .collect()
}

fn refusal(reply: &Value) -> Option<&str> {
    reply["error"].as_str()
}

#[test]
fn pick_turned_on_takes_effect_at_once_and_the_verb_is_taken() {
    let (rig, runner) = rig_picking("off");
    rig.forge.list_open_issue(issue(&rig, 30, 5));
    let refused = rig.ask(&runner, "triage", Some("30"));
    assert_eq!(
        refusal(&refused),
        Some("`triage.pick` is `off`, so no issue is triaged")
    );

    let next = pick_now(&rig, "off", "all");
    let line = runner
        .lock()
        .unwrap()
        .reread(next, rig.kelpie_settings())
        .unwrap();
    assert_eq!(
        line.as_deref(),
        Some("settings changed: triage now in effect")
    );

    let reply = rig.ask(&runner, "triage", Some("30"));
    assert_eq!(refusal(&reply), None, "{reply}");
}

#[test]
fn the_next_poll_lists_the_issues_opened_after_the_reread() {
    let (rig, runner) = rig_picking("off");
    rig.forge.list_open_issue(issue(&rig, 29, 0));
    rig.clock.advance(100);
    let next = pick_now(&rig, "off", "all");
    runner
        .lock()
        .unwrap()
        .reread(next, rig.kelpie_settings())
        .unwrap();
    rig.forge.list_open_issue(issue(&rig, 31, 20));
    rig.forge.list_open_issue(issue(&rig, 30, 10));

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        due(&rig, &runner),
        [30, 31],
        "the older issue is left alone"
    );
}

#[test]
fn pick_turned_off_forgets_the_listing_and_the_verb_refuses() {
    let (rig, runner) = rig_picking("all");
    rig.forge.list_open_issue(issue(&rig, 30, 5));
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(due(&rig, &runner), [30]);

    let next = pick_now(&rig, "all", "off");
    let line = runner
        .lock()
        .unwrap()
        .reread(next, rig.kelpie_settings())
        .unwrap();
    assert_eq!(
        line.as_deref(),
        Some("settings changed: triage now in effect")
    );

    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(due(&rig, &runner), Vec::<u64>::new());
    let reply = rig.ask(&runner, "triage", Some("30"));
    assert_eq!(
        refusal(&reply),
        Some("`triage.pick` is `off`, so no issue is triaged")
    );

    // Turned on again, it begins from the reread and not from the first start.
    rig.clock.advance(100);
    let next = pick_now(&rig, "off", "all");
    runner
        .lock()
        .unwrap()
        .reread(next, rig.kelpie_settings())
        .unwrap();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(due(&rig, &runner), Vec::<u64>::new());
}

#[test]
fn a_change_between_maintainer_and_all_keeps_since() {
    let (rig, runner) = rig_picking("maintainer");
    rig.clock.advance(100);
    rig.forge.list_open_issue(issue(&rig, 30, 0));
    let next = pick_now(&rig, "maintainer", "all");
    runner
        .lock()
        .unwrap()
        .reread(next, rig.kelpie_settings())
        .unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(due(&rig, &runner), [30], "since is still the first start");
}

#[test]
fn pick_turned_on_where_the_app_does_not_cover_the_repo_is_refused_and_the_old_settings_stay() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    let next = settings_with(&rig, |s| format!("{s}{TABLE}pick = \"maintainer\"\n"));
    let mut runner = runner.lock().unwrap();

    let err = runner.reread(next, rig.kelpie_settings()).unwrap_err();

    assert!(err.to_string().contains("triage.pick"), "{err}");
    assert!(runner.settings().triage.is_off(), "the old pick stays");
}

#[test]
fn a_missing_triager_file_is_refused_naming_triage_agent() {
    let (rig, runner) = rig_picking("off");
    let next = settings_with(&rig, |s| {
        s.replace(
            &format!("{TABLE}pick = \"off\"\n"),
            &format!("{TABLE}pick = \"all\"\nagent = \"nobody\"\n"),
        )
    });
    let mut runner = runner.lock().unwrap();

    let err = runner.reread(next, rig.kelpie_settings()).unwrap_err();

    assert!(err.to_string().contains("triage.agent"), "{err}");
    assert!(runner.settings().triage.is_off(), "the old pick stays");
}
