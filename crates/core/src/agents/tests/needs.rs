use crate::agents::{Need, Role};

#[test]
fn each_role_needs_what_the_design_says() {
    assert_eq!(
        Role::Implementer.needs(),
        &[Need::Edits, Need::ResumableSession]
    );
    assert_eq!(Role::Reviewer.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Advisor.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Researcher.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Triager.needs(), &[Need::RepoReads]);
    assert_eq!(Role::IssueWriter.needs(), &[Need::ClaudeHooks]);
    assert_eq!(Role::Pm.needs(), &[Need::ClaudeHooks]);
    assert_eq!(Role::Representative.needs(), &[]);
}

#[test]
fn every_harness_declares_every_need() {
    use crate::settings::Harness;

    let matrix: toml::Table = include_str!("../../../../../docs/parity.toml")
        .parse()
        .unwrap();
    for (harness, hooks) in [
        (Harness::ClaudeCode, true),
        (Harness::Pi, false),
        (Harness::Codex, false),
        (Harness::Goose, false),
        (Harness::StandIn, false),
    ] {
        let facts = harness.facts();
        assert!(facts.edits, "{harness:?}");
        assert!(facts.resumable_session, "{harness:?}");
        assert!(facts.repo_reads, "{harness:?}");
        assert_eq!(facts.claude_hooks, hooks, "{harness:?}");
        assert!(harness.serves(Role::Implementer));
        assert!(harness.serves(Role::Reviewer));
        assert!(harness.serves(Role::Representative));
        assert_eq!(harness.serves(Role::Pm), hooks);
        if harness == Harness::StandIn {
            continue;
        }
        for (row, declared) in [
            ("worker", facts.edits && facts.resumable_session),
            ("reviewer-reads-repo", facts.repo_reads),
            ("issue-writer", facts.claude_hooks),
            ("pm", facts.claude_hooks),
        ] {
            assert_eq!(
                matrix["harness"][row][facts.name].as_str() == Some("yes"),
                declared,
                "{row}: {}",
                facts.name
            );
        }
    }
    assert_eq!(Harness::serving(Role::Pm), vec![Harness::ClaudeCode]);
    assert_eq!(
        Harness::serving(Role::IssueWriter),
        vec![Harness::ClaudeCode]
    );
    assert_eq!(Harness::serving(Role::Implementer), Harness::ALL);
}

#[test]
fn the_needs_key_overrides_the_role_and_merges_through_extends() {
    use super::{folder, name};
    use crate::agents::Agents;

    let dir = folder(&[
        (
            "base.md",
            "---\nrole: implementer\nharness: codex\nmodel: m\neffort: high\nneeds: [claude-hooks]\n---\n",
        ),
        ("inherited.md", "---\nextends: base\n---\n"),
        (
            "relaxed.md",
            "---\nextends: inherited\nneeds: [repo-reads]\n---\n",
        ),
        ("empty.md", "---\nextends: base\nneeds: []\n---\n"),
        (
            "strict.md",
            "---\nextends: relaxed\nneeds: [claude-hooks]\n---\n",
        ),
        (
            "pm.md",
            "---\nextends: kelpie:pm\nharness: codex\nmodel: m\neffort: high\nneeds: []\n---\n",
        ),
    ]);
    let book = Agents::load(dir.path()).unwrap();
    for agent in ["base", "inherited", "strict"] {
        let err = book.broken(&name(agent)).unwrap().to_string();
        assert!(err.contains("Claude Code hook"), "{err}");
        assert!(book.get(&name(agent)).is_none());
    }
    for agent in ["relaxed", "empty", "pm"] {
        assert!(
            book.get(&name(agent)).is_some(),
            "{agent}: {:?}",
            book.broken(&name(agent))
        );
    }
}

#[test]
fn an_unknown_need_is_refused_with_its_line() {
    use super::{folder, name};
    use crate::agents::Agents;

    let dir = folder(&[
        (
            "base.md",
            "---\nrole: implementer\nharness: claude-code\nmodel: m\neffort: high\n---\n",
        ),
        ("inline.md", "---\nextends: base\nneeds: [telepathy]\n---\n"),
        (
            "block.md",
            "---\nextends: base\nneeds:\n  - telepathy\n---\n",
        ),
        ("child.md", "---\nextends: inline\n---\n"),
    ]);
    let book = Agents::load(dir.path()).unwrap();
    for (agent, line) in [("inline", Some(3)), ("block", Some(4)), ("child", None)] {
        let err = book.broken(&name(agent)).unwrap().to_string();
        assert!(err.contains("unknown variant `telepathy`"), "{err}");
        match line {
            Some(line) => assert!(err.contains(&format!("at line {line},")), "{err}"),
            None => assert!(err.contains("inherited through `inline`"), "{err}"),
        }
        for known in ["edits", "resumable-session", "repo-reads", "claude-hooks"] {
            assert!(err.contains(known), "{err}");
        }
    }
}

#[test]
fn every_roles_keys_take_needs_and_check_the_harness() {
    use crate::agents::baseline;

    for role in [
        "implementer",
        "reviewer",
        "advisor",
        "researcher",
        "triager",
        "representative",
        "issue-writer",
        "pm",
    ] {
        let text = format!(
            "---\nrole: {role}\nharness: codex\nmodel: m\neffort: high\nneeds: []\n---\nA prompt.\n"
        );
        assert!(baseline::check("mine", &text).is_ok(), "{role}");
        let strict = text.replace(
            "needs: []",
            "needs: [edits, resumable-session, repo-reads, claude-hooks]",
        );
        let err = baseline::check("mine", &strict).unwrap_err();
        assert!(err.contains("Claude Code hook"), "{role}: {err}");
        assert!(
            baseline::check("mine", &strict.replace("codex", "claude-code")).is_ok(),
            "{role}"
        );
    }
}
