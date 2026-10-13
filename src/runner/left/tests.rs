use serde_json::{Value, json};

use super::*;
use std::os::unix::fs::PermissionsExt;

use crate::runner::{CHECKS_SETTLE, StepReport, step};
use crate::test::Rig;

fn notes(runner: &Mutex<Runner>) -> Vec<String> {
    runner.lock().unwrap().take_notes()
}

#[test]
fn a_ruling_answered_while_the_runner_was_stopped_is_acted_on_when_it_starts() {
    let (rig, runner, head) = Rig::parked("rotom");
    drop(runner);
    let answers = rig.paths().answers;
    leave(&answers, "71 yes").unwrap();

    let runner = rig.open().unwrap();
    let waiting = rig.ask(&runner, "status", None)["rulings"].clone();
    assert_eq!(waiting[0]["id"], 71, "nothing is answered before a pass");
    step(&runner).unwrap();
    rig.clock.advance(CHECKS_SETTLE);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Finished { merged: true, .. })
    ));
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
    assert!(!answers.join("71").exists(), "the answer was left behind");
    assert!(notes(&runner).contains(
        &"answered ruling 71 as `shep kelpie rule` left it while the runner was stopped".to_owned()
    ));
}

// Rewrites the parked runner's state file as one saved before rulings were
// named by number, when its ruling on pull request 71 was ruling 1, and
// starts a runner on it.
fn saved_under_counter_id(rig: &Rig, runner: Mutex<Runner>) -> Mutex<Runner> {
    drop(runner);
    let path = rig.paths().state;
    let mut saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    saved["version"] = json!(37);
    saved["last_ruling"] = json!(1);
    saved["rulings"][0]["id"] = json!(1);
    saved["work_items"][0]["phase"] = json!({ "state": "ruling", "id": 1 });
    for key in ["heard", "unmentioned"] {
        let Some(map) = saved.get_mut(key).and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(entry) = map.remove("71") {
            map.insert("1".into(), entry);
        }
    }
    std::fs::write(&path, saved.to_string()).unwrap();
    rig.open().unwrap()
}

#[test]
fn an_answer_left_under_an_old_id_is_taken_once_by_the_ruling_it_became() {
    let (rig, runner, head) = Rig::parked("rotom");
    let answers = rig.paths().answers;
    leave(&answers, "1 yes").unwrap();
    let runner = saved_under_counter_id(&rig, runner);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);
    step(&runner).unwrap();
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap();
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert!(!answers.join("1").exists(), "the answer was left behind");
    assert!(notes(&runner).contains(
        &"answered ruling 1 as `shep kelpie rule` left it while the runner was stopped".to_owned()
    ));

    // The old id is read that once: a file left under it later is refused.
    leave(&answers, "1 yes").unwrap();
    step(&runner).unwrap();
    assert!(!answers.join("1").exists());
    let notes = notes(&runner);
    assert!(
        notes.iter().any(|n| n.starts_with(
            "the answer to ruling 1 left while the runner was stopped was not taken: "
        )),
        "{notes:?}"
    );
}

#[test]
fn an_answer_left_under_an_old_id_is_refused_when_no_ruling_became_it() {
    let (rig, runner, _) = Rig::parked("rotom");
    let runner = saved_under_counter_id(&rig, runner);
    let answers = rig.paths().answers;
    leave(&answers, "9 yes").unwrap();
    step(&runner).unwrap();
    assert!(!answers.join("9").exists());
    let notes = notes(&runner);
    assert!(
        notes.iter().any(
            |n| n.contains("ruling 9 is already answered, or was never raised")
                && n.contains("an id from before that no longer works")
        ),
        "{notes:?}"
    );
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);
}

#[test]
fn the_last_answer_left_for_a_ruling_is_the_one_taken() {
    let (rig, runner, _) = Rig::parked("rotom");
    drop(runner);
    let answers = rig.paths().answers;
    leave(&answers, "71 yes").unwrap();
    leave(&answers, "71 no rename the flag").unwrap();
    assert_eq!(
        std::fs::read_to_string(answers.join("71")).unwrap(),
        "71 no rename the flag"
    );
}

#[test]
fn an_answer_to_a_ruling_no_longer_waiting_is_logged_and_let_go() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    let answers = rig.paths().answers;
    leave(&answers, "3 yes").unwrap();
    step(&runner).unwrap();
    assert!(!answers.join("3").exists());
    assert!(
        notes(&runner).contains(
            &"the answer to ruling 3 left while the runner was stopped was not taken: ruling 3 \
              is already answered, or was never raised; a ruling is named by the number of its pull \
              request or issue, so an id from before that no longer works"
                .to_owned()
        )
    );
    step(&runner).unwrap();
    assert_eq!(notes(&runner), Vec::<String>::new(), "it is let go once");
}

#[test]
fn an_id_written_with_leading_zeros_is_left_under_its_number_and_taken() {
    let (rig, runner, _) = Rig::parked("rotom");
    drop(runner);
    let answers = rig.paths().answers;
    leave(&answers, "071 yes").unwrap();
    assert!(answers.join("71").exists() && !answers.join("071").exists());
    // One an older kelpie might have left under its digits as written.
    std::fs::write(answers.join("0002"), "2 yes").unwrap();

    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
    assert_eq!(std::fs::read_dir(&answers).unwrap().count(), 0);
    let notes = notes(&runner);
    assert!(
        notes.contains(
            &"the answer to ruling 2 left while the runner was stopped was not taken: \
                         ruling 2 is already answered, or was never raised; a ruling is named by the number of its pull \
              request or issue, so an id from before that no longer works"
                .to_owned()
        ),
        "{notes:?}"
    );
}

#[test]
fn an_answer_that_cannot_be_read_stays_for_the_next_pass() {
    let (rig, runner, _) = Rig::parked("rotom");
    drop(runner);
    let answers = rig.paths().answers;
    // A folder where the file would be, which no read can take.
    std::fs::create_dir_all(answers.join("71")).unwrap();
    let runner = rig.open().unwrap();
    let _ = step(&runner);
    assert!(answers.join("71").exists(), "the answer was removed unread");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);
    let notes = notes(&runner);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("cannot read the answer left for ruling 71, so it stays: ")),
        "{notes:?}"
    );
}

#[test]
fn an_answer_that_cannot_be_saved_stays_and_is_taken_once_it_can_be() {
    let (rig, runner, head) = Rig::parked("rotom");
    drop(runner);
    let answers = rig.paths().answers;
    leave(&answers, "71 yes").unwrap();
    let runner = rig.open().unwrap();
    let folder = rig.paths().state.parent().unwrap().to_path_buf();
    let mode = |mode| std::fs::set_permissions(&folder, PermissionsExt::from_mode(mode)).unwrap();
    mode(0o555);
    let _ = step(&runner);
    mode(0o755);
    assert!(
        answers.join("71").exists(),
        "the answer was lost with its save"
    );
    let notes = notes(&runner);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("the answer to ruling 71 could not be saved, so it stays")),
        "{notes:?}"
    );

    step(&runner).unwrap();
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap();
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert!(!answers.join("71").exists());
}

#[test]
fn a_stale_temporary_file_goes_and_anything_else_is_logged_once() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    let answers = rig.paths().answers;
    std::fs::create_dir_all(&answers).unwrap();
    let stale = answers.join("7.tmp");
    let fresh = answers.join("8.tmp");
    std::fs::write(&stale, "7 yes").unwrap();
    std::fs::write(&fresh, "8 yes").unwrap();
    let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let file = std::fs::File::options().write(true).open(&stale).unwrap();
    file.set_modified(an_hour_ago).unwrap();
    std::fs::write(answers.join("notes.txt"), "mine").unwrap();

    step(&runner).unwrap();
    assert!(!stale.exists(), "a crashed write's file stayed");
    assert!(fresh.exists(), "a write under way was taken");
    assert!(answers.join("notes.txt").exists());
    let said = notes(&runner);
    let told = format!(
        "{} is no answer `shep kelpie rule` left, so the runner leaves it there",
        answers.join("notes.txt").display()
    );
    assert_eq!(said, [told]);
    step(&runner).unwrap();
    assert_eq!(notes(&runner), Vec::<String>::new(), "told once");
}

#[test]
fn an_answer_that_names_no_ruling_is_not_left() {
    let rig = Rig::new("koji");
    let answers = rig.paths().answers;
    for params in ["", "yes", "0 yes", "#1 yes", "../1 yes"] {
        let err = leave(&answers, params).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{params:?}");
    }
    assert!(!answers.exists());
}
