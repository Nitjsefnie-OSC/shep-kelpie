//! The Codex pin: a command comes back checking the folder it runs in

use super::*;

// The worker's instructions name kelpie by its path, since a program from a variable is refused.
#[test]
fn a_test_run_under_the_lease_is_let_through_with_kelpie_named_outright() {
    let run = "/home/me/.kelpie/bin/kelpie lease run cargo-test -- cargo test";
    assert_eq!(bash(run), Verdict::Allow);
    let why = refusal(bash("\"$KELPIE\" lease run cargo-test -- cargo test"));
    assert!(why.contains("write the program's name out"), "{why}");
}

// Codex never tells the hook the folder it will run a command in, so an
// allowed command comes back checking where it runs (tests/guard_pin.rs
// runs it).
#[test]
fn an_allowed_command_comes_back_whole_after_a_check_of_its_folder() {
    let input = json!({
        "tool_name": "Bash",
        "cwd": "/k/wt",
        "tool_input": { "command": "bash x.sh\ncd sub" },
    })
    .to_string();
    let again = ["/k/kelpie".to_owned(), "guard".to_owned()];
    let answer: serde_json::Value =
        serde_json::from_str(&pinned(input.as_bytes(), Path::new("/k/wt"), &again).unwrap())
            .unwrap();
    let out = &answer["hookSpecificOutput"];
    assert_eq!(out["hookEventName"], "PreToolUse");
    assert_eq!(out["permissionDecision"], "allow");
    let command = out["updatedInput"]["command"].as_str().unwrap();
    assert!(command.starts_with("case \"$(pwd -P)\" in\n"), "{command}");
    assert!(command.ends_with("\nesac\nbash x.sh\ncd sub"), "{command}");

    assert!(pinned(br#"{"cwd": "/k"}"#, Path::new("/k"), &again).is_err());
}

#[test]
fn a_call_judged_again_is_judged_in_the_folder_it_runs_in() {
    let input = json!({
        "tool_name": "Bash",
        "cwd": "/k/wt",
        "tool_input": { "command": "bash x.sh" },
    })
    .to_string();
    let moved: serde_json::Value =
        serde_json::from_slice(&here(input.as_bytes(), Path::new("/k/wt/sub")).unwrap()).unwrap();
    assert_eq!(moved["cwd"], "/k/wt/sub");
    assert_eq!(moved["tool_input"]["command"], "bash x.sh");
    assert!(here(b"bash x.sh", Path::new("/k")).is_err());
}

#[test]
fn the_pin_flags_are_taken_out_of_the_rest() {
    let args = [
        "--folder=/k".to_owned(),
        PIN_FLAG.to_owned(),
        HERE_FLAG.to_owned(),
        "--folder=/j".to_owned(),
    ];
    let (pin, rest) = pin_flag(&args);
    assert!(pin);
    let (here, rest) = here_flag(&rest);
    assert!(here);
    assert_eq!(rest, ["--folder=/k", "--folder=/j"]);
    assert!(local_paths(None, &rest).is_ok());
    assert!(!pin_flag(&rest).0);
    assert!(!here_flag(&rest).0);
}
