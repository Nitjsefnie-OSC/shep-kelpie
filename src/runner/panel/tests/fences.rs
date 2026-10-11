//! What the advisor and the researcher may reach: their reads, no writes, no
//! commands, and for the researcher the web

use std::path::Path;

use serde_json::Value;

use super::{parked_with, script_all, seen, settle};
use crate::ports::{Role, Tools};
use crate::runner::pm::SECRET_SHAPED;
use crate::test::{
    Rig, Seen, assert_reads, assert_refuses, sandbox_refuses, tools_refuse, write_in,
};

/// Every tool that writes a file, runs a command or starts a crew
const NEVER: [&str; 8] = [
    "Bash",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "Agent",
    "Task",
    "Skill",
];

/// The expert's one call on the panel, after a panel has voted
fn voted(project: &str, role: Role, setup: impl FnOnce(&Rig)) -> (Rig, Seen) {
    let (rig, runner) = parked_with(project, false, setup);
    script_all(&rig, false);
    settle(&rig, &runner);
    let call = seen(&rig, role).pop().expect("the expert was asked");
    (rig, call)
}

fn denied(seen: &Seen) -> Vec<String> {
    let deny = seen.settings["permissions"]["deny"].as_array().unwrap();
    deny.iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The domains the sandbox lets the call reach
fn reaches(seen: &Seen) -> Vec<String> {
    let allowed = seen.sandbox["network"]["allowedDomains"]
        .as_array()
        .unwrap();
    allowed
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The folders and files the sandbox lets the call write
fn writes(seen: &Seen) -> Vec<String> {
    let allowed = seen.sandbox["filesystem"]["allowWrite"].as_array().unwrap();
    allowed
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Asserts the settings refuse every file tool before it runs, whatever the allow-list lets through
fn assert_refuses_file_tools(seen: &Seen) {
    let hooks = seen.settings["hooks"]["PreToolUse"].as_array().unwrap();
    let file_tools = hooks
        .iter()
        .find(|h| h["matcher"].as_str().is_some_and(|m| m.contains("Edit")))
        .expect("a hook on the file tools");
    let command = file_tools["hooks"][0]["command"].as_str().unwrap();
    assert!(command.ends_with("exit 2"), "{command}");
}

#[test]
fn the_advisor_reads_the_codebase_and_writes_and_runs_nothing() {
    let (rig, advisor) = voted("koji", Role::Advisor, |_| {});
    let paths = rig.paths();
    write_in(&rig.repo(), "NOTES.md", "the checkout\n");

    assert_eq!(advisor.call.tools, Tools::Advise);
    let fence = advisor.call.reach.fence.as_deref().unwrap();
    assert!(
        fence.write.is_empty(),
        "it writes no folder: {:?}",
        fence.write
    );
    let deny = denied(&advisor);
    for tool in NEVER.into_iter().chain(["WebFetch", "WebSearch"]) {
        assert!(
            deny.iter().any(|d| d == tool),
            "{tool} is not denied: {deny:?}"
        );
    }
    assert_refuses_file_tools(&advisor);
    // The harness adds the model's host to every call's, and the fence none.
    assert_eq!(
        reaches(&advisor),
        Vec::<String>::new(),
        "it reaches no host of its own"
    );
    // Its writes are the harness's own transcripts and scratch, never the code.
    let writable = writes(&advisor);
    let code = [paths.worktrees(), rig.repo()];
    for folder in &writable {
        assert!(
            !code.iter().any(|c| Path::new(folder).starts_with(c)),
            "it may write {folder}"
        );
    }

    assert_reads(&advisor, &paths.worktree(7).join("work.txt"));
    assert_reads(&advisor, &rig.repo().join("NOTES.md"));
    for file in ["ruling.md", "diff.patch", "where.md"] {
        assert_reads(&advisor, &paths.advisor.join(file));
    }
    for unread in [
        paths.state.clone(),
        paths.folder.join("settings.toml"),
        paths.researcher.join("ruling.md"),
    ] {
        assert_refuses(&advisor, &unread);
    }
    let deny = denied(&advisor).join("\n");
    for tree in code {
        for file in SECRET_SHAPED {
            let rule = format!("Read(/{}/{file})", tree.display());
            assert!(deny.contains(&rule), "{rule}");
        }
    }
}

#[test]
fn the_researcher_reads_its_folder_and_the_web_and_writes_and_runs_nothing() {
    let (rig, researcher) = voted("rotom", Role::Researcher, |rig| {
        rig.edit_settings(|s| {
            s + "\n[app.dogs.kelpie.researcher]\nallowed_domains = [\"docs.rs\"]\n"
        });
    });
    let paths = rig.paths();

    assert_eq!(researcher.call.tools, Tools::Research);
    let fence = researcher.call.reach.fence.as_deref().unwrap();
    assert!(
        fence.write.is_empty(),
        "it writes no folder: {:?}",
        fence.write
    );
    let deny = denied(&researcher);
    for tool in NEVER {
        assert!(
            deny.iter().any(|d| d == tool),
            "{tool} is not denied: {deny:?}"
        );
    }
    for web in ["WebFetch", "WebSearch"] {
        assert!(!deny.iter().any(|d| d == web), "{web} is denied: {deny:?}");
    }
    assert_refuses_file_tools(&researcher);
    assert_eq!(reaches(&researcher), ["docs.rs"]);
    let writable = writes(&researcher);
    let code = [paths.worktrees(), rig.repo()];
    for folder in &writable {
        assert!(
            !code.iter().any(|c| Path::new(folder).starts_with(c)),
            "it may write {folder}"
        );
    }

    for file in ["ruling.md", "rulings.md"] {
        assert_reads(&researcher, &paths.researcher.join(file));
    }
    // The code is the advisor's: the researcher's tools are shut out of it.
    for unread in [
        paths.worktree(7).join("work.txt"),
        paths.advisor.join("ruling.md"),
    ] {
        let spelled = unread.display().to_string();
        assert!(
            tools_refuse(&researcher, &spelled),
            "its tools read {spelled}"
        );
    }
    for unread in [paths.state.clone(), paths.folder.join("settings.toml")] {
        assert_refuses(&researcher, &unread);
    }
    assert!(sandbox_refuses(
        &researcher,
        &paths.state.display().to_string()
    ));
}
