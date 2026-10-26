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
