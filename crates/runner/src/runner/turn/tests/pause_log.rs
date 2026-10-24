//! A pause, and what it says in the runner's log

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

// Steps issue 7's turn with a tool call that a `drain pause` lands in, as
// `shep kelpie pause` sends it while the worker is mid-turn, and the hook
// then ends the turn at that call's result. `then` is how the turn would
// have gone on.
fn step_paused_in_a_tool(rig: &Rig, runner: &Mutex<Runner>, then: Scripted) -> Option<StepReport> {
    let tool = Hold::default();
    rig.claude
        .script([Scripted::AtTool(tool.clone(), Box::new(then))]);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            assert!(
                tool.entered(crate::test::CALL_PATIENCE),
                "the tool call never began"
            );
            // A `drain pause` that lands while the step holds the runner's
            // lock is answered from the posted copy, and its pause waits for a
            // pass the step will not run until the call ends. So it asks
            // again, as `shep kelpie pause` polls, until one reaches the runner.
            let deadline = std::time::Instant::now() + crate::test::CALL_PATIENCE;
            loop {
                let drained = rig.ask(runner, "drain", Some("pause"));
                if drained["draining"]["at_tool"] == json!(["7"]) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "{drained}");
                std::thread::yield_now();
            }
            tool.release();
        });
        step(runner).unwrap()
    })
}

#[test]
fn a_paused_turn_ends_at_its_next_tool_result_and_its_item_keeps_its_stage() {
    let (rig, runner) = with_issue_7("shep");
    let before = rig.ask(&runner, "status", None)["work_item"]["phase"].clone();

    let ended = step_paused_in_a_tool(&rig, &runner, Scripted::Push("a.txt", "a"));

    assert_eq!(
        ended, None,
        "a paused turn reports nothing: it did not finish"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["phase"], before);
    assert_eq!(
        status["work_item"]["phase"],
        json!({ "state": "implement" })
    );
    assert_eq!(status["work_item"]["turn"]["state"], "next");
    assert_eq!(status["work_item"]["turn"]["prompt"], PAUSED);
    assert_eq!(status["rulings"], json!([]));
    // What the turn would have gone on to push was not pushed: it ended at the tool.
    let [paused] = rig.claude.calls().try_into().unwrap();
    assert!(
        git(&rig.repo(), &["ls-remote", "origin", "kelpie/7"]).is_empty(),
        "{paused:?}"
    );
}

#[test]
fn a_paused_turn_runs_no_unpushed_check_and_raises_no_ruling() {
    let (rig, runner) = with_issue_7("shep");
    let worktree = rig.home.path().join("shep/kelpie/shep/worktrees/7");
    assert_eq!(
        step_paused_in_a_tool(&rig, &runner, Scripted::Say("done")),
        None
    );
    // Work left uncommitted and nothing pushed or opened: a turn that finished
    // so would be sent back for it, and a second one parked on a ruling.
    write_in(&worktree, "scratch.txt", "in progress\n");

    for _ in 0..2 {
        rig.ask(&runner, "undrain", None);
        let ended = step_paused_in_a_tool(&rig, &runner, Scripted::Say("done"));
        assert_eq!(ended, None);
    }

    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"], json!([]), "a pause raises no ruling");
    assert_eq!(
        status["work_item"]["phase"],
        json!({ "state": "implement" })
    );
    let calls = rig.claude.calls();
    assert_eq!(calls.len(), 3);
    assert!(
        calls[1..].iter().all(|c| c.prompt == PAUSED),
        "no unpushed check sent the worker back: {:?}",
        calls.iter().map(|c| &c.prompt).collect::<Vec<_>>()
    );
}

#[test]
fn the_turn_after_a_pause_resumes_the_session_with_a_fresh_ceiling() {
    let (rig, runner) = with_issue_7("shep");
    let ended = step_paused_in_a_tool(&rig, &runner, Scripted::Say("done"));
    assert_eq!(ended, None);
    // The runner stops, and the whole ceiling passes before it starts again.
    drop(runner);
    rig.clock.advance(3601);
    let runner = rig.open().unwrap();

    rig.claude.script([Scripted::Reply(usage(1), Cost(9))]);
    let resumed = step(&runner).unwrap();

    assert!(
        matches!(resumed, Some(StepReport::Ended { .. })),
        "a ceiling from before the pause parked it: {resumed:?}"
    );
    let [first, second] = rig.claude.calls().try_into().unwrap();
    assert_eq!(second.session, Session::Resume(first.session.id().clone()));
    assert_eq!(second.prompt, PAUSED);
}
