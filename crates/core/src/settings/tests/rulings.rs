//! `rulings.pm_alone`, which names kinds of ruling

use super::{EXAMPLE, parse};

#[test]
fn a_removed_kind_in_the_list_makes_it_refuse_by_name() {
    let spilled = parse(&format!(
        "{EXAMPLE}\n[app.dogs.kelpie.rulings]\npm_alone = [\"local-model-spilled\"]\n"
    ))
    .unwrap()
    .rulings;
    assert!(
        spilled
            .refusal()
            .is_some_and(|r| r.contains("local-model-spilled"))
    );
}

#[test]
fn only_a_merge_kind_makes_the_list_refuse() {
    let merge = parse(&format!(
        "{EXAMPLE}\n[app.dogs.kelpie.rulings]\npm_alone = [\"merge\"]\n"
    ))
    .unwrap()
    .rulings;
    assert!(merge.refusal().is_some_and(|r| r.contains("merge")));
    assert_eq!(parse(EXAMPLE).unwrap().rulings.refusal(), None);
}

#[test]
fn pm_alone_naming_security_is_refused_at_load() {
    let security = parse(&format!(
        "{EXAMPLE}\n[app.dogs.kelpie.rulings]\npm_alone = [\"security\"]\n"
    ))
    .unwrap()
    .rulings;
    let reason = security.refusal().expect("security is refused");
    assert!(reason.contains("`security`"), "{reason}");
}

#[test]
fn pm_alone_naming_contribution_merge_is_refused_and_the_panel_does_not_vote() {
    let merge = parse(&format!(
        "{EXAMPLE}\n[app.dogs.kelpie.rulings]\npm_alone = [\"contribution-merge\"]\n"
    ))
    .unwrap()
    .rulings;
    let reason = merge.refusal().expect("contribution-merge is refused");
    assert!(reason.contains("`contribution-merge`"), "{reason}");
}
