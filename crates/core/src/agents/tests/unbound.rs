use super::*;
use crate::agents::baseline::Baseline;

// A baseline with no binding keys, so a stub that extends it binds nothing.
const SCRIBE: Baseline = Baseline {
    name: "scribe",
    text: "---\nrole: pm\n---\nYou keep the project's notes.\n",
};

const STUB: &str = "---\nextends: kelpie:scribe\n---\n";

#[test]
fn a_stub_that_binds_nothing_is_unbound_and_leaves_the_other_files_loading() {
    let dir = folder(&[("notes.md", STUB), ("qwen.md", QWEN)]);
    let agents = Agents::load_with(dir.path(), &[SCRIBE]).unwrap();
    assert!(agents.get(&name("notes")).is_none());
    assert!(agents.get(&name("qwen")).is_some());
    let unbound = agents.unbound();
    assert_eq!(unbound.len(), 1);
    assert_eq!(unbound[0].name, name("notes"));
    assert_eq!(unbound[0].path, dir.path().join("notes.md"));
    assert_eq!(unbound[0].role, Role::Pm);
    assert_eq!(
        unbound[0].command(),
        "shep kelpie agents new pm --name notes"
    );
}

#[test]
fn a_stub_that_adds_harness_model_and_effort_loads_with_the_baselines_prompt() {
    let bound = "---\nextends: kelpie:scribe\nharness: claude-code\nmodel: claude-opus-5-5\n\
                 effort: medium\n---\n";
    let dir = folder(&[("notes.md", bound)]);
    let agents = Agents::load_with(dir.path(), &[SCRIBE]).unwrap();
    let notes = agents.get(&name("notes")).unwrap();
    assert_eq!(notes.role, Role::Pm);
    assert_eq!(pair(notes), ("claude-opus-5-5", Effort::Medium));
    assert_eq!(
        notes.prompt.as_deref(),
        Some("You keep the project's notes.")
    );
    assert!(agents.unbound().is_empty());
}

#[test]
fn the_review_bots_and_the_five_shipped_bindings_still_load_whole() {
    let agents = Agents::load(tempfile::tempdir().unwrap().path()).unwrap();
    for whole in [
        "sonnet-high",
        "opus-high",
        "defect-hunter-lite",
        "iq3-xxs-review",
        "qwen",
        "coderabbit",
        "cubic",
        "codex",
    ] {
        assert!(agents.get(&name(whole)).is_some(), "{whole}");
    }
    assert!(agents.unbound().is_empty());
}

#[test]
fn a_file_with_no_role_still_stops_the_folder() {
    let err = refused(&[("notes.md", "---\nmodel: x\n---\n"), ("qwen.md", QWEN)]);
    assert!(err.contains("notes.md: missing field `role`"), "{err}");
}

#[test]
fn extends_resolves_in_the_whole_agents_and_in_the_baselines() {
    let both = "---\nextends: kelpie:nope\n---\n";
    let err = refused(&[("mine.md", both)]);
    assert!(
        err.contains("defect-hunter") && err.contains("opus-high"),
        "{err}"
    );
}

#[test]
fn the_issue_writer_names_the_command_for_an_unbound_agent_and_for_a_bare_baseline_with_no_file() {
    let bare = Baseline {
        name: "issue-writer",
        text: "---\nrole: issue-writer\n---\nYou write.\n",
    };
    let dir = folder(&[("scribe.md", "---\nrole: issue-writer\n---\nYou write.\n")]);
    let agents = Agents::load_over(dir.path(), &[bare]).unwrap();
    let err = Writer::of(&agents, &name("scribe")).unwrap_err();
    assert_eq!(
        err,
        "`scribe` is unbound: run `shep kelpie agents new issue-writer --name scribe`"
    );
    let err = Writer::of(&agents, &name("issue-writer")).unwrap_err();
    assert_eq!(
        err,
        "`issue-writer` is unbound: run `shep kelpie agents new issue-writer`"
    );
}

#[test]
fn a_baseline_that_keeps_its_binding_resolves_with_no_file() {
    let agents = Agents::load(folder(&[]).path()).unwrap();
    for shipped in baseline::ALL {
        assert!(
            agents.get(&name(shipped.name)).is_some(),
            "{}",
            shipped.name
        );
        assert_eq!(agents.unbound_command(&name(shipped.name)), None);
    }
    assert!(agents.bare_baselines().is_empty());
    Writer::of(&agents, &name("issue-writer")).unwrap();
}

#[test]
fn a_file_over_a_bare_baseline_binds_it_and_leaves_the_baseline_listed_only_while_unbound() {
    let bare = Baseline {
        name: "scribe",
        text: "---\nrole: pm\n---\nYou keep the notes.\n",
    };
    let loose = Agents::load_over(folder(&[]).path(), &[bare]).unwrap();
    assert_eq!(loose.bare_baselines(), [bare]);
    let bound = "---\nextends: kelpie:scribe\nharness: claude-code\nmodel: m\neffort: low\n---\n";
    let dir = folder(&[("scribe.md", bound)]);
    let agents = Agents::load_over(dir.path(), &[bare]).unwrap();
    assert!(agents.get(&name("scribe")).is_some());
    assert_eq!(agents.unbound_command(&name("scribe")), None);
}
