use crate::agents::{Need, Role};

#[test]
fn each_role_needs_what_the_design_says() {
    assert_eq!(Role::Implementer.needs(), &[Need::Edits, Need::ResumableSession]);
    assert_eq!(Role::Reviewer.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Advisor.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Researcher.needs(), &[Need::RepoReads]);
    assert_eq!(Role::Triager.needs(), &[Need::RepoReads]);
    assert_eq!(Role::IssueWriter.needs(), &[Need::ClaudeHooks]);
    assert_eq!(Role::Pm.needs(), &[Need::ClaudeHooks]);
    assert_eq!(Role::Representative.needs(), &[]);
}
