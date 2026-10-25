//! Whether the repo's rules make a pull request's branch match its base before it merges

use serde::Deserialize;

use super::contents::is_not_found;
use super::{gh, unreadable};
use crate::ports::ForgeError;
use crate::settings::ForgeSlug;

/// Whether `base`'s rules require a pull request's branch to be up to date with it
///
/// Rulesets and classic protection apply together and the stricter wins, so
/// the two are asked in order and only a strict ruleset answers alone. The
/// rulesets endpoint answers 30 rules at a time, so it goes through
/// `--paginate`, which merges the pages of an array answer into one array: a
/// strict rule belongs to the branch whatever page it lands on.
pub(super) fn requires_up_to_date(repo: &ForgeSlug, base: &str) -> Result<bool, ForgeError> {
    let path = format!("repos/{}/rules/branches/{base}", repo.as_str());
    match parse_rules(&gh(&["api", "--paginate", &path])?)? {
        Some(true) => Ok(true),
        said => classic_protection(repo, base, said.is_some()),
    }
}

// What the rulesets answer says: `Some(true)` when any ruleset covering the
// branch is strict, `Some(false)` when one covers it and none is, and `None`
// when no `required_status_checks` rule covers it. Only `Some(true)` answers
// the question: the rest leave it to the classic protection.
fn parse_rules(stdout: &[u8]) -> Result<Option<bool>, ForgeError> {
    #[derive(Deserialize)]
    struct Rule {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        parameters: Option<Parameters>,
    }
    #[derive(Deserialize)]
    struct Parameters {
        // Absent from the parameters of the other rule kinds, which the filter
        // drops. Absent from one of these kinds, or not a bool, is unreadable.
        strict_required_status_checks_policy: Option<bool>,
    }
    let rules: Vec<Rule> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    let strict: Vec<Option<bool>> = rules
        .iter()
        .filter(|rule| rule.kind == "required_status_checks")
        .map(|rule| {
            rule.parameters
                .as_ref()
                .and_then(|p| p.strict_required_status_checks_policy)
        })
        .collect();
    if strict.contains(&Some(true)) {
        return Ok(Some(true));
    }
    if strict.contains(&None) {
        return Err(unreadable(stdout));
    }
    Ok((!strict.is_empty()).then_some(false))
}

// The classic protection of the branch, which the rulesets endpoint does not
// cover: its `strict` decides. A 404 means none where the rulesets already
// spoke; where they were silent it can also mean protection hidden from this
// login, which the branch listing's `protected` flag splits.
fn classic_protection(repo: &ForgeSlug, base: &str, rules_said: bool) -> Result<bool, ForgeError> {
    let path = format!(
        "repos/{}/branches/{base}/protection/required_status_checks",
        repo.as_str()
    );
    let hidden = match gh(&["api", &path]) {
        Ok(stdout) => return parse_protection(&stdout),
        Err(e) if is_not_found(&e) && rules_said => return Ok(false),
        Err(e) if is_not_found(&e) => e,
        Err(e) => return Err(e),
    };
    match branch_protected(repo, base) {
        Ok(false) => Ok(false),
        Ok(true) => Err(hidden),
        Err(e) => Err(e),
    }
}

fn parse_protection(stdout: &[u8]) -> Result<bool, ForgeError> {
    #[derive(Deserialize)]
    struct Protection {
        strict: Option<bool>,
    }
    let protection: Protection = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    protection.strict.ok_or_else(|| unreadable(stdout))
}

fn branch_protected(repo: &ForgeSlug, base: &str) -> Result<bool, ForgeError> {
    let path = format!("repos/{}/branches/{base}", repo.as_str());
    parse_protected(&gh(&["api", &path])?)
}

fn parse_protected(stdout: &[u8]) -> Result<bool, ForgeError> {
    #[derive(Deserialize)]
    struct Branch {
        protected: Option<bool>,
    }
    let branch: Branch = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    branch.protected.ok_or_else(|| unreadable(stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded 2026-10-25 from api.github.com on shep-pm/shep's `main`, the
    // answer `gh api repos/shep-pm/shep/rules/branches/main` makes: two
    // rulesets, one `required_status_checks` rule, not strict.
    const SHEP_MAIN: &str = include_str!("../../../../../fixtures/gh-api-rules-branches-main.json");

    // Recorded the same day on this repo's `main`: no ruleset covers it.
    const NO_RULES: &str = "[]";

    #[test]
    fn a_rule_that_does_not_require_up_to_date_leaves_the_question_open() {
        assert_eq!(parse_rules(SHEP_MAIN.as_bytes()), Ok(Some(false)));
    }

    #[test]
    fn a_branch_no_rule_protects_leaves_the_question_open() {
        assert_eq!(parse_rules(NO_RULES.as_bytes()), Ok(None));
    }

    // The shape of the recorded rule with its policy on, written from GitHub's
    // "Get all rules for a branch" answer: no repo of the maintainer's has a
    // strict ruleset to record.
    const STRICT: &str = r#"[{"type":"required_status_checks",
        "parameters":{"strict_required_status_checks_policy":true,
        "required_status_checks":[{"context":"test","integration_id":15368}]},
        "ruleset_source_type":"Repository","ruleset_source":"shep-pm/shep","ruleset_id":1}]"#;

    #[test]
    fn a_strict_rule_is_read_as_one_that_requires_up_to_date() {
        assert_eq!(parse_rules(STRICT.as_bytes()), Ok(Some(true)));
    }

    #[test]
    fn any_strict_rule_of_several_rulesets_requires_up_to_date() {
        let rules = |list: &str| {
            list.trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or_default()
                .to_owned()
        };
        let both = format!("[{},{}]", rules(SHEP_MAIN), rules(STRICT));
        assert_eq!(parse_rules(both.as_bytes()), Ok(Some(true)));
    }

    #[test]
    fn a_required_status_checks_rule_without_its_parameters_is_unreadable() {
        let bare = r#"[{"type":"required_status_checks","ruleset_id":1}]"#;
        assert!(matches!(
            parse_rules(bare.as_bytes()),
            Err(ForgeError::Unreadable(_))
        ));
    }

    #[test]
    fn an_answer_that_is_not_the_rule_list_is_unreadable() {
        for text in ["", "{}", "not json"] {
            assert!(
                matches!(parse_rules(text.as_bytes()), Err(ForgeError::Unreadable(_))),
                "{text}"
            );
        }
    }

    // Written from GitHub's "Get required status checks for a branch" answer.
    // The endpoint answers 401 to a request with no token at all, and the
    // guard runs no `gh api` for a worker, so no answer of a real repo is
    // recorded here.
    const STRICT_PROTECTION: &str =
        r#"{"strict":true,"contexts":["test"],"checks":[{"context":"test"}]}"#;

    #[test]
    fn classic_protection_that_is_strict_is_read_as_requiring_up_to_date() {
        assert_eq!(parse_protection(STRICT_PROTECTION.as_bytes()), Ok(true));
    }

    #[test]
    fn classic_protection_that_is_not_strict_is_read_as_not_requiring_it() {
        let said = r#"{"strict":false,"contexts":[],"checks":[]}"#;
        assert_eq!(parse_protection(said.as_bytes()), Ok(false));
    }

    #[test]
    fn classic_protection_that_does_not_say_strict_is_unreadable() {
        for text in ["{}", "not json", "[]"] {
            assert!(
                matches!(
                    parse_protection(text.as_bytes()),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
    }

    // Recorded 2026-10-25 from api.github.com: `GET /repos/shep-pm/shep/branches/main`,
    // protected by its rulesets, and `GET /repos/octocat/Hello-World/branches/master`,
    // protected by nothing.
    const PROTECTED_MAIN: &str = include_str!("../../../../../fixtures/gh-api-branches-main.json");
    const BARE_MASTER: &str = include_str!("../../../../../fixtures/gh-api-branches-master.json");

    #[test]
    fn a_protected_branch_is_read_as_protected() {
        assert_eq!(parse_protected(PROTECTED_MAIN.as_bytes()), Ok(true));
    }

    #[test]
    fn an_unprotected_branch_is_read_as_unprotected() {
        assert_eq!(parse_protected(BARE_MASTER.as_bytes()), Ok(false));
    }

    #[test]
    fn a_branch_answer_without_its_protected_field_is_unreadable() {
        for text in [r#"{"name":"main"}"#, "not json"] {
            assert!(
                matches!(
                    parse_protected(text.as_bytes()),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
    }
}
