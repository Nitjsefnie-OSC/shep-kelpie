//! A reply that names a ruling by the number of its issue or pull request

use super::*;
use crate::state::{ProjectState, Ruling, StateStore};

// A reply of `text` and the code of `at`, written at `at`
fn coded(rig: &Rig, text: &str, at: Timestamp) {
    let code = rig.code_at(at);
    rig.alerts.reply(&format!("{text} {code}"), at);
}

// When the rig's ruling 1 was raised
fn raised(runner: &Mutex<Runner>) -> Timestamp {
    lock(runner).state.rulings[0].raised.expect("a time")
}

#[test]
fn a_number_names_the_ruling_on_its_issue_or_its_pull_request() {
    for reply in ["71 yes", "7 yes", "#71 yes", "koji#7 yes", "koji 71 yes"] {
        let (rig, runner, head) = alerted("koji");
        rig.reply(reply);
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ReplyAnswered { id: 1 }),
            "{reply}"
        );
        assert_eq!(
            phase(&rig, &runner),
            json!({ "state": "merge", "head": head, "readied": null }),
            "{reply}"
        );
    }
}

#[test]
fn a_reply_by_number_written_before_the_ruling_was_raised_is_refused_as_stale() {
    let (rig, runner, head) = alerted("koji");
    let raised = raised(&runner);
    coded(&rig, "71 yes", Timestamp(raised.0 - 60));
    let asks = lock(&runner).state.rulings[0].question.clone();
    let asks = asks.lines().next().unwrap();
    let reason = format!(
        "the ruling changed since that reply was written, and it asks now: {asks}. \
         Reply again to answer it"
    );
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyRefused {
            id: 1,
            reason: reason.clone(),
            line_failed: None
        })
    );
    assert_eq!(
        lines(&rig),
        [format!("Ruling #71 was not answered: {reason}.")]
    );
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));

    rig.clock.advance(READ_EVERY);
    coded(&rig, "71 yes", rig.clock.now());
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "merge", "head": head, "readied": null })
    );
}

#[test]
fn a_reply_by_id_written_before_the_ruling_was_raised_is_taken_as_given() {
    let (rig, runner, _) = alerted("koji");
    let raised = raised(&runner);
    coded(&rig, "1 yes", Timestamp(raised.0 - 60));
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
}

// Every project reads the one topic, and a number is the same on each.
#[test]
fn a_number_two_projects_have_a_ruling_on_needs_the_project() {
    let (rig, runner, _) = alerted("koji");
    let kelpie = rig.paths().kelpie_home;
    let lab = kelpie.join("lab/state.json");
    std::fs::create_dir_all(lab.parent().unwrap()).unwrap();
    std::fs::write(&lab, std::fs::read_to_string(rig.paths().state).unwrap()).unwrap();
    let ids = RulingIds::under(&kelpie);
    assert_eq!(ids.claim("lab", 1), 2);
    rig.reply("71 yes");
    let reason = "#71 also has a ruling on lab, so name the project first: \
                  `koji 71 <answer> <code>`";
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyRefused {
            id: 1,
            reason: reason.into(),
            line_failed: None
        })
    );
    rig.clock.advance(READ_EVERY);
    rig.reply("koji#71 yes");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
}

// The reply kelpie tells the maintainer to send must work: a project named in
// it owns the number, whatever another project's ruling is called.
#[test]
fn naming_the_project_answers_though_another_has_a_ruling_with_that_id() {
    let (rig, runner, _) = alerted("koji");
    let mut lab = ProjectState::new();
    lab.rulings = vec![Ruling {
        id: 71,
        ..lock(&runner).state.rulings[0].clone()
    }];
    let path = rig.paths().kelpie_home.join("lab/state.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    StateStore::new(path).save(&lab).unwrap();
    rig.reply("koji 71 yes");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
}

#[test]
fn a_number_another_project_s_ruling_has_answers_nothing_here() {
    let (rig, runner, _) = alerted("koji");
    rig.reply("lab#71 yes");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    rig.clock.advance(READ_EVERY);
    rig.reply("72 yes");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
}
