//! The researcher's call: which harness it runs on decides whether its reach names a web

use std::sync::Mutex;

use super::{name_panel, parked_with, script_all, seen, settle};
use crate::ports::{Role, Tools};
use crate::runner::Runner;
use crate::settings::AgentHarness;
use crate::test::{Rig, write_in};
use crate::tools::{GOOSE_WEB, GOOSE_WEB_FILE, PI_WEB_ACCESS};

const PI_RESEARCHER: &str = "---\nrole: researcher\nharness: pi\nmodel: qwen3.8:27b\n\
                             effort: low\nurl: http://box:11434/v1\ncontext: 65536\n---\nLook it up.\n";

// Puts the package where `tools install` does, with the file pi reads.
fn install_package(rig: &Rig) {
    let folder = rig.paths().tools.pi_package(PI_WEB_ACCESS);
    write_in(&folder, "package.json", "{}");
}

/// A ruling parked with a researcher on pi, and the web package installed when `installed`
fn parked_on_pi(project: &str, installed: bool) -> (Rig, Mutex<Runner>) {
    let (rig, runner, _) = Rig::parked_set(project, |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        name_panel(rig, false);
        rig.write_agent("pi-researcher", PI_RESEARCHER);
        rig.edit_settings(|s| {
            let named = "researcher = \"researcher\"";
            assert!(s.contains(named), "the panel's researcher moved");
            s.replace(named, "researcher = \"pi-researcher\"")
        });
        if installed {
            install_package(rig);
        }
    });
    (rig, runner)
}

#[test]
fn a_pi_researcher_names_the_package_and_one_without_it_runs_with_a_note() {
    let (rig, runner) = parked_on_pi("koji", true);
    script_all(&rig, false);
    settle(&rig, &runner);
    let package = rig.paths().tools.pi_package(PI_WEB_ACCESS);
    let calls = seen(&rig, Role::Researcher);
    let [asked] = calls.as_slice() else {
        panic!("the researcher was asked once, not {}", calls.len())
    };
    assert_eq!(asked.call.reach.extensions, [package]);
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(!notes.contains("without the web"), "{notes}");

    // The same panel, with `tools install` not run.
    let (rig, runner) = parked_on_pi("rotom", false);
    script_all(&rig, false);
    settle(&rig, &runner);
    let calls = seen(&rig, Role::Researcher);
    let [asked] = calls.as_slice() else {
        panic!("the researcher was asked once, not {}", calls.len())
    };
    assert_eq!(
        asked.call.reach.extensions,
        Vec::<std::path::PathBuf>::new()
    );
    let missing = rig.paths().tools.pi_package(PI_WEB_ACCESS);
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains(&format!(
            "the researcher runs without the web: {} is not there",
            missing.display()
        )),
        "{notes}"
    );
}

// Claude Code has its own web tools, so the package stays out of its call.
#[test]
fn a_researcher_on_claude_code_names_no_package_whether_or_not_it_is_installed() {
    let (rig, runner) = parked_with("pikachu", false, install_package);
    script_all(&rig, false);
    settle(&rig, &runner);
    let calls = seen(&rig, Role::Researcher);
    assert!(!calls.is_empty(), "the researcher made no call");
    for asked in calls {
        assert!(asked.call.reach.extensions.is_empty());
    }
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(!notes.contains("without the web"), "{notes}");
}

const GOOSE_RESEARCHER: &str = "---\nrole: researcher\nharness: goose\nmodel: qwen3.8:27b\n\
                                effort: low\nurl: http://box:11434/v1\ncontext: 65536\n\
                                usage: none\n---\nLook it up.\n";

// Puts the server where `tools install` does, with the file the entry runs.
fn install_goose_web(rig: &Rig) {
    let folder = rig.paths().tools.pi_package(GOOSE_WEB);
    write_in(&folder, "package.json", "{}");
    write_in(&folder, GOOSE_WEB_FILE, "");
}

/// A ruling parked with a researcher on goose, and the web server installed when `installed`
fn parked_on_goose(project: &str, installed: bool) -> (Rig, Mutex<Runner>) {
    let (rig, runner, _) = Rig::parked_set(project, |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        name_panel(rig, false);
        rig.write_agent("goose-researcher", GOOSE_RESEARCHER);
        rig.edit_settings(|s| {
            let named = "researcher = \"researcher\"";
            assert!(s.contains(named), "the panel's researcher moved");
            s.replace(named, "researcher = \"goose-researcher\"")
        });
        if installed {
            install_goose_web(rig);
        }
    });
    (rig, runner)
}

#[test]
fn a_goose_researcher_names_the_web_server_and_one_without_the_package_runs_with_a_note() {
    let (rig, runner) = parked_on_goose("koji", true);
    script_all(&rig, false);
    settle(&rig, &runner);
    let package = rig.paths().tools.pi_package(GOOSE_WEB);
    let calls = seen(&rig, Role::Researcher);
    let [asked] = calls.as_slice() else {
        panic!("the researcher was asked once, not {}", calls.len())
    };
    assert!(matches!(asked.call.harness, AgentHarness::Goose(_)));
    let names: Vec<_> = asked.call.reach.mcp_servers.keys().collect();
    assert_eq!(names, ["kelpie-web"]);
    assert_eq!(
        asked.call.reach.mcp_servers["kelpie-web"],
        serde_json::json!({
            "type": "stdio",
            "command": "node",
            "args": [package.join(GOOSE_WEB_FILE)],
        })
    );
    // The server runs from kelpie's tools folder, which the fence hides, so the call reads it.
    assert!(
        asked
            .call
            .reach
            .read
            .contains(&package.parent().unwrap().to_owned())
    );
    // The note is the one the call has none of.
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(!notes.contains("without the web"), "{notes}");

    // The same panel, with `tools install` not run.
    let (rig, runner) = parked_on_goose("rotom", false);
    script_all(&rig, false);
    settle(&rig, &runner);
    let calls = seen(&rig, Role::Researcher);
    let [asked] = calls.as_slice() else {
        panic!("the researcher was asked once, not {}", calls.len())
    };
    assert!(asked.call.reach.mcp_servers.is_empty());
    assert!(asked.call.reach.read.is_empty());
    let missing = rig.paths().tools.pi_package(GOOSE_WEB).join(GOOSE_WEB_FILE);
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains(&format!(
            "the researcher runs without the web: {} is not there",
            missing.display()
        )),
        "{notes}"
    );
}

const GOOSE_IMPLEMENTER: &str = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\n\
                                 effort: low\nurl: http://box:11434/v1\ncontext: 65536\n---\n";

const GOOSE_ADVISOR: &str = "---\nrole: advisor\nharness: goose\nmodel: qwen3.8:27b\n\
                             effort: low\nurl: http://box:11434/v1\ncontext: 65536\n\
                             usage: none\n---\nAdvise.\n";

// The server is the goose researcher's alone. Every call a whole panel makes with the worker, the
// advisor and the researcher on goose, the web server installed, is looped over: each role and
// each kind of tools it was made with. Then the researcher on pi and on Claude Code.
#[test]
fn no_other_goose_call_and_no_other_harness_gets_the_web_server() {
    let (rig, runner, _) = Rig::parked_set("pikachu", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        name_panel(rig, true);
        rig.write_agent("goose-box", GOOSE_IMPLEMENTER);
        rig.implementers(&["goose-box"]);
        rig.write_agent("goose-advisor", GOOSE_ADVISOR);
        rig.write_agent("goose-researcher", GOOSE_RESEARCHER);
        rig.edit_settings(|s| {
            let s = s.replace("advisor = \"advisor\"", "advisor = \"goose-advisor\"");
            s.replace(
                "researcher = \"researcher\"",
                "researcher = \"goose-researcher\"",
            )
        });
        install_goose_web(rig);
        install_package(rig);
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    let all = rig.claude.all_seen();
    // The panel made calls of each role, the worker's vote among them.
    for role in [Role::Worker, Role::Advisor, Role::Researcher, Role::Pm] {
        assert!(all.iter().any(|s| s.call.role == role), "{role:?}");
    }
    assert!(all.iter().any(|s| s.call.tools == Tools::Retro));
    for asked in &all {
        let (call, goose) = (
            &asked.call,
            matches!(asked.call.harness, AgentHarness::Goose(_)),
        );
        let has_web = call.reach.mcp_servers.contains_key("kelpie-web");
        let expected = goose && call.role == Role::Researcher && call.tools == Tools::Research;
        assert_eq!(has_web, expected, "{:?} {:?}", call.role, call.tools);
    }

    // The researcher on Claude Code and on pi, with the goose server installed too.
    let (rig, runner) = parked_with("rotom", false, |rig| {
        install_package(rig);
        install_goose_web(rig);
    });
    script_all(&rig, false);
    settle(&rig, &runner);
    let (pi_rig, pi_runner) = parked_on_pi("snorlax", true);
    install_goose_web(&pi_rig);
    script_all(&pi_rig, false);
    settle(&pi_rig, &pi_runner);
    for rig in [&rig, &pi_rig] {
        let researchers = seen(rig, Role::Researcher);
        assert!(!researchers.is_empty(), "the researcher made no call");
        for asked in researchers {
            assert!(!asked.call.reach.mcp_servers.contains_key("kelpie-web"));
        }
    }
}
