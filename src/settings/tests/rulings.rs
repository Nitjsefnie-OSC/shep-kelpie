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
