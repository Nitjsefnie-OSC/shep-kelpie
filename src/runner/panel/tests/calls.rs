//! The researcher's call: which harness it runs on decides whether its reach names a web

use std::sync::Mutex;

use super::{name_panel, parked_with, script_all, seen, settle};
use crate::ports::Role;
use crate::runner::Runner;
use crate::test::{Rig, write_in};
use crate::tools::PI_WEB_ACCESS;

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
