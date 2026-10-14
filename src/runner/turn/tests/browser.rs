//! A worker turn on a project with a browser is given one, and no other call is

use super::*;
use crate::ports::{CLEAN, Role};
use crate::settings::AgentHarness;

const OFF: &str = "[app.dogs.kelpie.browser]\nenabled = false\nheaded = false\ndomains = []";

fn turn_on(rig: &Rig) {
    rig.edit_settings(|s| {
        assert!(s.contains(OFF), "the example's browser moved");
        s.replace(
            OFF,
            "[app.dogs.kelpie.browser]\nenabled = true\nheaded = false\n\
             domains = [\"app.example.com\"]",
        )
    });
}

// The server and a Chromium build, as `shep kelpie tools install` leaves them
fn install(rig: &Rig) {
    let tools = rig.paths().tools;
    fs::create_dir_all(tools.playwright_mcp().parent().unwrap()).unwrap();
    fs::write(tools.playwright_mcp(), "").unwrap();
    fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
}

fn instructions_of(seen: &crate::test::Seen) -> String {
    fs::read_to_string(seen.call.instructions.as_ref().unwrap()).unwrap()
}

#[test]
fn a_worker_turn_on_a_project_with_the_browser_on_gets_a_socket_and_is_told_its_hosts() {
    let rig = Rig::new("shep");
    turn_on(&rig);
    install(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
    step(&runner).unwrap(); // the worker's first turn
    step(&runner).unwrap(); // round 1, qwen: clean by default
    step(&runner).unwrap(); // round 2, claude: scripted clean above

    let seen = rig.claude.all_seen();
    let worker = seen.iter().find(|s| s.call.role == Role::Worker).unwrap();
    let browser = worker.call.reach.browser.as_ref().expect("a browser");
    assert_eq!(browser.kelpie, std::path::Path::new(Rig::KELPIE));
    assert_eq!(browser.socket.parent(), Some(rig.paths().worker.as_path()));
    let config = fs::read_to_string(worker.call.settings.with_extension("mcp.json")).unwrap();
    assert!(config.contains("mcp-connect"), "{config}");
    let told = instructions_of(worker);
    assert!(told.contains("`app.example.com`"), "{told}");
    assert!(told.contains("`playwright` MCP server"), "{told}");
    // The turn is over, so its bridge is dropped and its socket is gone.
    assert!(!browser.socket.exists(), "{}", browser.socket.display());
    let reviewers = seen.iter().filter(|s| s.call.role == Role::Reviewer);
    for reviewer in reviewers {
        assert_eq!(reviewer.call.reach.browser, None);
    }
}

#[test]
fn a_project_without_the_table_gets_no_browser_though_the_tools_are_installed() {
    let rig = Rig::new("shep");
    install(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();

    let seen = rig.claude.all_seen();
    let worker = seen.iter().find(|s| s.call.role == Role::Worker).unwrap();
    assert_eq!(worker.call.reach.browser, None);
    assert!(!instructions_of(worker).contains("playwright"));
}

#[test]
fn a_codex_worker_gets_no_browser_and_is_not_told_of_one() {
    let rig = Rig::new("shep");
    turn_on(&rig);
    install(&rig);
    let agent =
        "---\nrole: implementer\nharness: codex\nmodel: gpt-6-sol\neffort: low\n---\nWork.\n";
    rig.write_agent("codex-box", agent);
    rig.implementers(&["codex-box"]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();

    let seen = rig.claude.all_seen();
    let worker = seen.iter().find(|s| s.call.role == Role::Worker).unwrap();
    assert_eq!(worker.call.harness, AgentHarness::Codex);
    assert_eq!(worker.call.reach.browser, None);
    assert!(!instructions_of(worker).contains("playwright"));
}

#[test]
fn a_project_whose_tools_lack_the_browser_runs_without_one_and_a_note_says_to_install() {
    let rig = Rig::new("shep");
    turn_on(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();

    let seen = rig.claude.all_seen();
    let worker = seen.iter().find(|s| s.call.role == Role::Worker).unwrap();
    assert_eq!(worker.call.reach.browser, None);
    assert!(!instructions_of(worker).contains("playwright"));
    let notes = runner.lock().unwrap().take_notes();
    let note = notes.iter().find(|n| n.contains("without a browser"));
    let note = note.unwrap_or_else(|| panic!("{notes:?}"));
    assert!(note.contains("issue #7's worker"), "{note}");
    assert!(note.contains("shep kelpie tools install"), "{note}");
}
