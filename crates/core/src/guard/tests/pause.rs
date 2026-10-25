//! The pause flag in the build folder, and what the `PostToolUse` hook does while it is set

use super::*;

#[test]
fn the_post_tool_hook_ends_the_turn_while_a_pause_is_set() {
    let build = TempDir::new().unwrap();
    assert_eq!(judge_pause(build.path()), None, "no pause is set");
    Pause::in_build(build.path()).request().unwrap();
    let answer = judge_pause(build.path()).expect("a pause is set");
    let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["continue"], false);
    assert!(answer["stopReason"].as_str().unwrap().contains("pausing"));
    assert!(
        Pause::in_build(build.path()).ended_turn(),
        "the runner tells a paused turn by what the hook left"
    );
    Pause::in_build(build.path()).clear();
    assert_eq!(
        judge_pause(build.path()),
        None,
        "a cleared pause ends nothing"
    );
}

#[test]
fn the_post_tool_hook_ends_no_turn_for_a_pause_that_could_not_be_written() {
    let build = TempDir::new().unwrap();
    let pause = Pause::in_build(build.path());
    // A folder where the flag file goes cannot be written as a file.
    std::fs::create_dir(pause.requested_file()).unwrap();
    assert!(pause.request().is_err(), "the runner's write fails");
    assert!(!pause.is_requested(), "no pause is set");
    assert_eq!(
        judge_pause(build.path()),
        None,
        "the hook ends nothing for a pause no one set"
    );
    assert!(
        !pause.ended_turn(),
        "no marker is left, so the runner tells no paused turn"
    );
}

#[test]
fn the_post_tool_hook_lets_the_turn_go_on_when_it_cannot_leave_its_marker() {
    let build = TempDir::new().unwrap();
    let pause = Pause::in_build(build.path());
    pause.request().unwrap();
    // A folder where the marker goes cannot be written as a file.
    std::fs::create_dir(pause.ended_file()).unwrap();
    assert_eq!(
        judge_pause(build.path()),
        None,
        "the runner could not tell the turn was paused, so it does not end"
    );
}

#[test]
fn the_pause_flag_is_taken_out_of_the_rest() {
    let args = ["--folder=/k".to_owned(), format!("{PAUSE_FLAG}/b")];
    let (build, rest) = pause_flag(&args);
    assert_eq!(build.as_deref(), Some(Path::new("/b")));
    assert_eq!(rest, ["--folder=/k"]);
    assert_eq!(pause_flag(&rest).0, None);
}
