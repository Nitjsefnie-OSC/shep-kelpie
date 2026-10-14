//! A goose reviewer, advisor and retro: a shell that reads, and no write

use std::process::Command as Process;

use super::hooks_tests::{env_vars, json_in, plugin, policy_of, worker};
use super::tests::{FRESH, FRESH_ID, OTHER_ID, URL, World, id, only_skills, stand_in, strings};
use super::*;
use crate::confine::Verdict;
use crate::guard::judge_goose_reading;
use crate::hooks::Hooks;

// A call of `role` on `tools` in a worker's fence, which can write.
fn fenced(w: &World, role: Role, tools: Tools, session: Session) -> AgentCall {
    AgentCall {
        role,
        tools,
        ..worker(w, session)
    }
}

// A reviewer's fence has no folder to write.
fn reviewer(w: &World, session: Session) -> AgentCall {
    let mut call = fenced(w, Role::Reviewer, Tools::Review, session);
    call.reach.fence.as_mut().unwrap().write.clear();
    call
}

// The command of the call's hook, as goose reads it from the plugin.
fn hook_command(w: &World) -> String {
    let hooks = json_in(&plugin(w).join("hooks/hooks.json"));
    let command = &hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"];
    command.as_str().unwrap().to_owned()
}

fn is_given_developer(call: &AgentCall) -> bool {
    let argv = strings(call);
    argv.windows(2)
        .any(|a| a == ["--with-builtin", "developer"])
}

fn runs_in(mode: &str, call: &AgentCall) -> bool {
    env_vars(call).contains(&format!("GOOSE_MODE={mode}"))
}

fn judged(tool: &str, input: serde_json::Value) -> Verdict {
    let payload = serde_json::json!({
        "tool_name": tool,
        "tool_input": input,
        "working_dir": "/k/wt",
    });
    judge_goose_reading(payload.to_string().as_bytes(), &Hooks::default(), &[])
}

#[test]
fn a_goose_reviewer_in_a_fence_has_a_read_only_shell_and_no_write() {
    let w = World::new();
    let mut call = reviewer(&w, Session::New(id(FRESH_ID)));
    // A stand-in kelpie that keeps the arguments it was run with.
    let kelpie = w.path("kelpie-bin");
    let seen = w.path("seen");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", seen.display());
    crate::test::write_script(&kelpie, &script);
    call.reach.fence.as_mut().unwrap().guard.kelpie = kelpie;
    let guard = call.reach.fence.as_ref().unwrap().guard.clone();
    stand_in(&w, FRESH).prepare(&call).unwrap();

    assert!(is_given_developer(&call));
    assert!(runs_in("auto", &call));
    assert!(!strings(&call).iter().any(|a| a == "--with-extension"));

    // goose runs the command with `sh -c`.
    let ran = Process::new("sh")
        .args(["-c", &hook_command(&w)])
        .status()
        .unwrap();
    assert!(ran.success());
    let seen = std::fs::read_to_string(&seen).unwrap();
    assert_eq!(
        seen.lines().collect::<Vec<_>>(),
        [
            "guard".to_owned(),
            guard.git_common_dir.display().to_string(),
            guard.worktree.display().to_string(),
            format!("--goose={}", guard.build.display()),
            "--read-only".to_owned(),
        ]
    );

    // What that guard says of the tools goose offers.
    assert_eq!(
        judged("shell", serde_json::json!({ "command": "cat src/lib.rs" })),
        Verdict::Allow
    );
    assert_eq!(judged("tree", serde_json::json!({})), Verdict::Allow);
    for tool in ["write", "edit", "read_image"] {
        let Verdict::Refuse(why) = judged(tool, serde_json::json!({})) else {
            panic!("{tool} was let through");
        };
        assert!(why.contains("writes no file"), "{tool}: {why}");
    }
}

#[test]
fn a_retro_and_an_advisor_have_a_read_only_shell_and_no_write_in_the_sandbox() {
    let w = World::new();
    for (role, tools, session) in [
        (Role::Worker, Tools::Retro, Session::Resume(id(FRESH_ID))),
        (Role::Advisor, Tools::Advise, Session::New(id(OTHER_ID))),
    ] {
        let call = fenced(&w, role, tools, session);
        assert!(
            !call.reach.fence.as_ref().unwrap().write.is_empty(),
            "the fence it runs under can write"
        );
        stand_in(&w, FRESH).prepare(&call).unwrap();

        assert!(runs_in("auto", &call), "{tools:?}");
        assert!(hook_command(&w).contains("'--read-only'"), "{tools:?}");

        let files = Files::of(&call.settings);
        let policy = policy_of(&w, &call);
        let mut written = policy.write.clone();
        written.sort();
        let mut own = vec![files.data(), files.state(), files.scratch.clone()];
        own.sort();
        assert_eq!(written, own, "{tools:?}");
    }

    // A worker keeps its write folders, and a reviewer has none to keep.
    let work = worker(&w, Session::New(id(FRESH_ID)));
    assert!(policy_of(&w, &work).write.contains(&w.path("wt")));
    let policy = policy_of(&w, &reviewer(&w, Session::New(id(FRESH_ID))));
    assert!(!policy.write.contains(&w.path("wt")), "{:?}", policy.write);
}

#[test]
fn a_call_with_no_fence_loads_no_extension_and_runs_in_chat_mode() {
    let w = World::new();
    for (role, tools) in [
        (Role::Reviewer, Tools::Review),
        (Role::Advisor, Tools::Advise),
        (Role::Worker, Tools::Retro),
    ] {
        let call = AgentCall {
            role,
            tools,
            ..w.call(URL, role, Session::New(id(FRESH_ID)))
        };

        stand_in(&w, FRESH).prepare(&call).unwrap();

        assert!(only_skills(&call));
        assert!(runs_in("chat", &call), "{tools:?}");
        assert!(!plugin(&w).exists(), "{tools:?}: no hook to write");
        let vars = env_vars(&call);
        assert!(!vars.iter().any(|v| v.starts_with("CONTEXT_FILE_NAMES=")));
    }
}

#[test]
fn the_researcher_has_no_shell() {
    let w = World::new();
    // Even in a fence, which a researcher may be given.
    let call = fenced(
        &w,
        Role::Researcher,
        Tools::Research,
        Session::New(id(FRESH_ID)),
    );

    stand_in(&w, FRESH).prepare(&call).unwrap();

    assert!(only_skills(&call));
    assert!(runs_in("chat", &call));
    assert!(!plugin(&w).exists());
}

#[test]
fn a_researcher_given_reading_tools_is_still_no_reader() {
    for tools in [Tools::Review, Tools::Advise, Tools::Retro] {
        let w = World::new();
        let call = fenced(&w, Role::Researcher, tools, Session::New(id(FRESH_ID)));

        stand_in(&w, FRESH).prepare(&call).unwrap();

        assert!(only_skills(&call));
        assert!(runs_in("chat", &call), "{tools:?}");
        assert!(!plugin(&w).exists(), "{tools:?}");
    }
}

#[test]
fn a_resumed_session_is_held_by_the_hook_whatever_its_stored_extensions_are() {
    let w = World::new();
    let call = fenced(
        &w,
        Role::Worker,
        Tools::Retro,
        Session::Resume(id(FRESH_ID)),
    );
    stand_in(&w, FRESH).prepare(&call).unwrap();

    // goose restores the session's own extension set, so it is given none.
    assert!(!is_given_developer(&call));
    assert!(strings(&call).iter().any(|a| a == "--resume"));
    // The session may hold `developer` from the worker's turns, so goose runs
    // its tools unasked and the hook is the only gate.
    assert!(runs_in("auto", &call));
    let command = hook_command(&w);
    assert!(command.contains("'--goose="), "{command}");
    assert!(command.contains("'--read-only'"), "{command}");
    let Verdict::Refuse(why) = judged(
        "write",
        serde_json::json!({ "path": "a.rs", "content": "x" }),
    ) else {
        panic!("a write was let through");
    };
    assert!(why.contains("writes no file"), "{why}");

    // A reviewer or an advisor resumed is held the same way.
    let advisor = fenced(&w, Role::Advisor, Tools::Advise, call.session.clone());
    assert!(!is_given_developer(&advisor));
    // A worker resumed is still given the builtin, which goose keeps once.
    let resumed = worker(&w, Session::Resume(id(FRESH_ID)));
    assert!(is_given_developer(&resumed));
}

#[test]
fn a_reviewer_is_told_what_its_shell_runs_and_an_advisor_and_a_retro_are_left_alone() {
    let w = World::new();
    let mut call = reviewer(&w, Session::New(id(FRESH_ID)));
    call.prompt = "Review the change.".into();

    let text = strings(&call)
        .into_iter()
        .find(|a| a.starts_with("--text="));
    let text = text.unwrap();

    assert!(text.starts_with("--text=Review the change."), "{text}");
    for command in ["cat", "rg", "find", "nl"] {
        assert!(text.contains(&format!("`{command}`")), "{text}");
    }
    assert!(text.contains("no Read, Glob or Grep"), "{text}");

    for (role, tools) in [(Role::Advisor, Tools::Advise), (Role::Worker, Tools::Retro)] {
        let mut call = fenced(&w, role, tools, Session::New(id(FRESH_ID)));
        call.prompt = "Vote.".into();
        assert!(strings(&call).contains(&"--text=Vote.".to_owned()));
    }
    // And a reviewer with no fence has no shell to be told of.
    let mut call = w.call(URL, Role::Reviewer, Session::New(id(FRESH_ID)));
    call.tools = Tools::Review;
    call.prompt = "Review the change.".into();
    assert!(strings(&call).contains(&"--text=Review the change.".to_owned()));
}
