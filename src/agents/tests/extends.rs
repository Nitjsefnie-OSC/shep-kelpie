use super::*;

const PARENT: &str = "---\nrole: implementer\nharness: claude-code\nmodel: claude-opus-5-5\n\
                      effort: high\n---\nParent body.\n";

fn load(files: &[(&str, &str)]) -> Agents {
    let dir = folder(files);
    Agents::load(dir.path()).unwrap()
}

#[test]
fn a_file_with_extends_and_no_body_takes_its_parents_body() {
    let agents = load(&[
        ("base.md", PARENT),
        ("light.md", "---\nextends: base\neffort: medium\n---\n"),
    ]);
    let light = agents.get(&name("light")).unwrap();
    assert_eq!(light.prompt.as_deref(), Some("Parent body."));
    assert_eq!(pair(light), ("claude-opus-5-5", Effort::Medium));
}

#[test]
fn one_with_a_body_uses_its_own() {
    let agents = load(&[
        ("base.md", PARENT),
        ("light.md", "---\nextends: base\n---\nMy own.\n"),
    ]);
    let light = agents.get(&name("light")).unwrap();
    assert_eq!(light.prompt.as_deref(), Some("My own."));
    assert_eq!(pair(light), ("claude-opus-5-5", Effort::High));
}

#[test]
fn keys_the_file_sets_replace_the_parents_and_the_rest_come_from_it() {
    let agents = load(&[
        (
            "base.md",
            "---\n# A comment.\nrole: implementer\nharness: pi\nmodel: m1\neffort: high\n\
             url: http://box:11434/v1\ncontext: 65536\nlease: shared\n---\n",
        ),
        (
            "light.md",
            "---\nextends: base # the parent\n# Lighter.\neffort: low\nmodel: \"m2\"\n---\n",
        ),
    ]);
    let base = agents.get(&name("base")).unwrap();
    let light = agents.get(&name("light")).unwrap();
    assert_eq!(pair(light), ("m2", Effort::Low));
    assert_eq!(model(light).harness, model(base).harness);
    assert_eq!(limit(light), limit(base), "`lease` comes from the parent");
}

#[test]
fn role_and_harness_can_be_overridden() {
    let agents = load(&[
        ("base.md", PARENT),
        (
            "other.md",
            "---\nextends: base\nrole: reviewer\nharness: codex\n---\n",
        ),
    ]);
    let other = agents.get(&name("other")).unwrap();
    assert_eq!(other.role, Role::Reviewer);
    assert_eq!(model(other).harness, AgentHarness::Codex);
    assert_eq!(other.prompt.as_deref(), Some("Parent body."));
}

#[test]
fn a_merged_file_that_is_invalid_is_refused_naming_the_child() {
    let endpoint = "---\nrole: reviewer\nharness: endpoint\nurl: http://box/v1\nmodel: m\n\
                    context: 32768\n---\n";
    let err = refused(&[
        ("box.md", endpoint),
        ("look.md", "---\nextends: box\nsecond_look: true\n---\n"),
    ]);
    assert!(err.contains("look.md"), "{err}");
    assert!(err.contains("second_look"), "{err}");

    let err = refused(&[(
        "cubic.md",
        "---\nextends: coderabbit\nbot: cubic\n---\nA prompt.\n",
    )]);
    assert!(err.contains("cubic.md"), "{err}");
    assert!(err.contains("body"), "{err}");
}

#[test]
fn a_missing_parent_is_refused_naming_the_file() {
    let err = refused(&[("light.md", "---\nextends: nobody\neffort: low\n---\n")]);
    assert!(err.contains("light.md"), "{err}");
    assert!(err.contains("nobody"), "{err}");
}

#[test]
fn a_cycle_is_refused_naming_the_file() {
    let err = refused(&[
        ("a.md", "---\nextends: b\n---\n"),
        ("b.md", "---\nextends: a\n---\n"),
    ]);
    assert!(err.contains("a.md"), "{err}");
    assert!(err.contains("circle"), "{err}");

    let err = refused(&[("me.md", "---\nextends: me\n---\n")]);
    assert!(err.contains("me.md"), "{err}");
    assert!(err.contains("circle"), "{err}");
}

#[test]
fn a_parent_that_extends_another_resolves() {
    let agents = load(&[
        ("base.md", PARENT),
        ("mid.md", "---\nextends: base\neffort: medium\n---\n"),
        ("leaf.md", "---\nextends: mid\nmodel: m3\n---\n"),
    ]);
    let leaf = agents.get(&name("leaf")).unwrap();
    assert_eq!(pair(leaf), ("m3", Effort::Medium));
    assert_eq!(leaf.prompt.as_deref(), Some("Parent body."));
}

#[test]
fn a_child_that_sorts_before_its_parent_resolves() {
    let agents = load(&[
        ("a-child.md", "---\nextends: z-parent\neffort: low\n---\n"),
        ("z-parent.md", PARENT),
    ]);
    let child = agents.get(&name("a-child")).unwrap();
    assert_eq!(pair(child), ("claude-opus-5-5", Effort::Low));
    assert_eq!(child.prompt.as_deref(), Some("Parent body."));
}

#[test]
fn a_parent_that_is_only_a_default_can_be_extended() {
    let agents = load(&[(
        "hunter-light.md",
        "---\nextends: defect-hunter\neffort: medium\nsecond_look: false\n---\n",
    )]);
    let default = Agents::embedded();
    let hunter = default.get(&name("defect-hunter")).unwrap();
    let light = agents.get(&name("hunter-light")).unwrap();
    assert_eq!(pair(light), ("claude-opus-5-5", Effort::Medium));
    assert!(!light.second_look);
    assert!(hunter.second_look);
    assert_eq!(light.prompt, hunter.prompt);
    assert_eq!(light.role, Role::Reviewer);
}

#[test]
fn a_file_with_the_parents_name_wins_over_the_default() {
    let agents = load(&[
        (
            "defect-hunter.md",
            "---\nrole: reviewer\nharness: claude-code\nmodel: mine\neffort: low\n---\nMine.\n",
        ),
        (
            "light.md",
            "---\nextends: defect-hunter\neffort: medium\n---\n",
        ),
    ]);
    let light = agents.get(&name("light")).unwrap();
    assert_eq!(pair(light), ("mine", Effort::Medium));
    assert_eq!(light.prompt.as_deref(), Some("Mine."));
}

#[test]
fn an_edit_to_the_parent_reaches_the_child_on_reload() {
    let dir = folder(&[
        ("base.md", PARENT),
        ("light.md", "---\nextends: base\neffort: medium\n---\n"),
    ]);
    let before = snapshot(dir.path());
    fs::write(
        dir.path().join("base.md"),
        PARENT
            .replace("claude-opus-5-5", "m9")
            .replace("Parent body.", "New body."),
    )
    .unwrap();
    assert_ne!(snapshot(dir.path()), before, "the edit shows on the reread");
    let agents = Agents::load(dir.path()).unwrap();
    let light = agents.get(&name("light")).unwrap();
    assert_eq!(pair(light), ("m9", Effort::Medium));
    assert_eq!(light.prompt.as_deref(), Some("New body."));
}
