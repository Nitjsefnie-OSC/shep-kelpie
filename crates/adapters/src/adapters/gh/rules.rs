//! Whether the repo's rules make a pull request's branch match its base before it merges

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::ForgeError;
use crate::settings::ForgeSlug;

/// The rules GitHub lists for `base`, read for whether they require a pull
/// request's branch to be up to date with it
///
/// `gh api` pages an array answer, so every ruleset's rules arrive in one call.
pub(super) fn requires_up_to_date(repo: &ForgeSlug, base: &str) -> Result<bool, ForgeError> {
    let path = format!("repos/{}/rules/branches/{base}", repo.as_str());
    parse_rules(&gh(&["api", &path])?)
}

// A branch no `required_status_checks` rule protects has nothing to require,
// and one several rulesets cover is up to date only if any of them is strict.
fn parse_rules(stdout: &[u8]) -> Result<bool, ForgeError> {
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
        return Ok(true);
    }
    if strict.contains(&None) {
        return Err(unreadable(stdout));
    }
    Ok(false)
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
    fn a_rule_that_does_not_require_up_to_date_is_read_as_one_that_does_not() {
        assert_eq!(parse_rules(SHEP_MAIN.as_bytes()), Ok(false));
    }

    #[test]
    fn a_branch_no_rule_protects_requires_nothing() {
        assert_eq!(parse_rules(NO_RULES.as_bytes()), Ok(false));
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
        assert_eq!(parse_rules(STRICT.as_bytes()), Ok(true));
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
        assert_eq!(parse_rules(both.as_bytes()), Ok(true));
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
}
