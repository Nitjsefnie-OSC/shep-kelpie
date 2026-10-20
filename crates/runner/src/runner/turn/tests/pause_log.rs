//! What a pause says in the runner's log

use super::*;

fn notes(runner: &Mutex<Runner>) -> Vec<String> {
    runner.lock().unwrap().take_notes()
}

#[test]
fn a_pause_says_which_turns_it_asked_to_end_and_the_turn_that_ended_says_so() {
    let (rig, runner) = with_issue_7("shep");
    notes(&runner);

    step_paused_in_a_tool(&rig, &runner, Scripted::Say("done"));

    assert_eq!(
        notes(&runner),
        [
            "drain pause: turns asked to end at their next tool call: #7",
            "#7 paused-at-tool",
        ]
    );
}

#[test]
fn a_pause_with_no_turn_to_end_says_so() {
    let (rig, runner) = with_issue_7("shep");
    notes(&runner);

    rig.ask(&runner, "drain", Some("pause"));

    assert_eq!(
        notes(&runner),
        ["drain pause: no worker turn in flight can end at a tool call"]
    );
}

#[test]
fn the_start_that_resumes_a_paused_turn_says_so_once() {
    let (rig, runner) = with_issue_7("shep");
    step_paused_in_a_tool(&rig, &runner, Scripted::Say("done"));
    drop(runner);
    let runner = rig.open().unwrap();
    notes(&runner);

    rig.claude.script([Scripted::Reply(usage(1), Cost(9))]);
    step(&runner).unwrap();
    let said = notes(&runner);

    assert_eq!(
        said.iter()
            .filter(|n| *n == "#7 resumed-after-pause")
            .count(),
        1,
        "{said:?}"
    );
    step(&runner).unwrap();
    assert!(
        !notes(&runner)
            .iter()
            .any(|n| n.contains("resumed-after-pause")),
        "a later pass says nothing of the pause"
    );
}

#[test]
fn a_pause_that_could_not_be_written_does_not_say_no_turn_could_end() {
    use crate::guard::Pause;
    let (rig, runner) = with_issue_7("shep");
    notes(&runner);
    let tool = Hold::default();
    rig.claude.script([Scripted::AtTool(
        tool.clone(),
        Box::new(Scripted::Say("done")),
    )]);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            assert!(tool.entered(crate::test::CALL_PATIENCE));
            // A folder where the request file goes: the write fails.
            let request = Pause::in_build(&rig.build_7()).requested_file().to_owned();
            std::fs::create_dir_all(request).unwrap();
            rig.ask(&runner, "drain", Some("pause"));
            tool.release();
        });
        step(&runner).unwrap();
    });

    let said = notes(&runner);
    assert_eq!(
        said[0],
        "drain pause: no turn was asked to end at a tool call; \
         the pause could not be written for #7",
        "{said:?}"
    );
}

#[test]
fn a_resume_whose_state_could_not_be_saved_does_not_say_it_resumed() {
    let (rig, runner) = with_issue_7("shep");
    step_paused_in_a_tool(&rig, &runner, Scripted::Say("done"));
    drop(runner);
    let runner = rig.open().unwrap();
    notes(&runner);
    // A folder where the state file goes: the save fails.
    let state = rig.paths().state;
    std::fs::remove_file(&state).unwrap();
    std::fs::create_dir_all(&state).unwrap();

    assert!(step(&runner).is_err());

    assert!(
        !notes(&runner)
            .iter()
            .any(|n| n.contains("resumed-after-pause"))
    );
}
