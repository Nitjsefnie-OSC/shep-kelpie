//! `[builds]`, the limits on a work item's build folder and on the disk

use super::{EXAMPLE, parse, parse_err};

fn with_builds(keys: &str) -> String {
    format!("{EXAMPLE}\n[app.dogs.kelpie.builds]\n{keys}\n")
}

#[test]
fn both_limits_default_to_20_gb_when_the_table_is_absent_or_empty() {
    for text in [EXAMPLE.to_owned(), with_builds("")] {
        let builds = parse(&text).unwrap().builds;
        assert_eq!((builds.max_gb, builds.min_free_gb), (20, 20));
        assert_eq!(builds.max_bytes(), Some(20_000_000_000));
        assert_eq!(builds.min_free_bytes(), Some(20_000_000_000));
    }
}

#[test]
fn zero_turns_each_limit_off_on_its_own() {
    let no_cap = parse(&with_builds("max_gb = 0")).unwrap().builds;
    assert_eq!(
        (no_cap.max_bytes(), no_cap.min_free_bytes()),
        (None, Some(20_000_000_000))
    );
    let no_floor = parse(&with_builds("min_free_gb = 0")).unwrap().builds;
    assert_eq!(
        (no_floor.max_bytes(), no_floor.min_free_bytes()),
        (Some(20_000_000_000), None)
    );
}

#[test]
fn a_mistyped_key_is_refused_by_name() {
    let err = parse_err(&with_builds("max_gbs = 5"));
    assert!(err.contains("max_gbs"), "{err}");
}
