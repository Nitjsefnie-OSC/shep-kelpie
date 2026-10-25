//! What a pause says in the runner's log

use super::*;

fn notes(runner: &Mutex<Runner>) -> Vec<String> {
    runner.lock().unwrap().take_notes()
}

// What `drain pause` says when the pause file cannot be written for a turn in flight
const REFUSED: &str = "drain pause: no turn was asked to end at a tool call; \
                       the pause could not be written for #7";

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

    let (asked, said) = std::thread::scope(|scope| {
        let asking = scope.spawn(|| {
            assert!(tool.entered(crate::test::CALL_PATIENCE));
            // A folder where the request file goes: the write fails.
            let request = Pause::in_build(&rig.build_7()).requested_file().to_owned();
            std::fs::create_dir_all(request).unwrap();
            // One `drain pause` may land while the step holds the runner's lock,
            // answered from the posted copy and leaving its pause for a pass this
            // step never runs. Ask again, as `shep kelpie pause` polls, until one
            // reaches the runner and notes the refusal, keeping what it noted.
            let deadline = std::time::Instant::now() + crate::test::CALL_PATIENCE;
            loop {
                rig.ask(&runner, "drain", Some("pause"));
                let told = notes(&runner);
                if told.iter().any(|n| n.contains("could not be written")) {
                    tool.release();
                    return told;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the drain pause never reached the runner: {told:?}"
                );
                std::thread::yield_now();
            }
        });
        step(&runner).unwrap();
        (asking.join().unwrap(), notes(&runner))
    });

    let said: Vec<String> = asked.into_iter().chain(said).collect();
    assert!(said.iter().any(|n| n == REFUSED), "{said:?}");
    assert!(
        !said.iter().any(|n| n.contains("paused-at-tool")),
        "a turn no pause was set for ends on its own, not paused: {said:?}"
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
