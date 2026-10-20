//! `[app.dogs.kelpie.dependencies]`, which says whether kelpie merges and
//! closes the bumps bots open

use std::num::NonZeroU32;

use super::{EXAMPLE, parse, parse_err};
use crate::settings::{DependencyPick, OnRed};

const TABLE: &str = "[app.dogs.kelpie.dependencies]\n";

// The example with `lines` under a dependencies table.
fn with(lines: &str) -> String {
    format!("{EXAMPLE}\n{TABLE}{lines}")
}

#[test]
fn an_absent_dependencies_table_means_pick_off_dependabot_close_no_web_24_hours() {
    let d = parse(EXAMPLE).unwrap().dependencies;
    assert_eq!(d.pick, DependencyPick::Off);
    assert_eq!(d.bots, ["dependabot"]);
    assert_eq!(d.on_red, OnRed::Close);
    assert!(d.web.is_empty());
    assert_eq!(d.wait_hours, NonZeroU32::new(24).unwrap());
    assert_eq!(parse(&with("")).unwrap().dependencies, d, "an empty table");
}

#[test]
fn every_key_is_read() {
    let d = parse(&with(
        "pick = \"on\"\nbots = [\"renovate[bot]\"]\non_red = \"leave\"\nweb = [\"web/\"]\nwait_hours = 6\n",
    ))
    .unwrap()
    .dependencies;
    assert_eq!(d.pick, DependencyPick::On);
    assert_eq!(d.bots, ["renovate[bot]"]);
    assert_eq!(d.on_red, OnRed::Leave);
    assert_eq!(d.web, ["web/"]);
    assert_eq!(d.wait_hours, NonZeroU32::new(6).unwrap());
}

#[test]
fn an_unknown_key_in_dependencies_is_refused_naming_it() {
    let err = parse_err(&with("on_rd = \"close\"\n"));
    assert!(err.contains("on_rd"), "{err}");
}

#[test]
fn on_red_and_pick_refuse_any_other_word() {
    for (key, word) in [("on_red", "fix"), ("pick", "label")] {
        let err = parse_err(&with(&format!("{key} = \"{word}\"\n")));
        assert!(err.contains(key) && err.contains(word), "{err}");
    }
}

#[test]
fn zero_wait_hours_is_refused() {
    let err = parse_err(&with("wait_hours = 0\n"));
    assert!(err.contains("wait_hours"), "{err}");
}

#[test]
fn a_bot_is_listed_ignoring_case_and_the_bot_suffix() {
    let d = parse(&with("bots = [\"Dependabot[bot]\", \"renovate\"]\n"))
        .unwrap()
        .dependencies;
    for login in ["dependabot", "DEPENDABOT[bot]", "Renovate"] {
        assert!(d.lists(login), "{login}");
    }
    for login in ["dependabot-preview", "someone", ""] {
        assert!(!d.lists(login), "{login}");
    }
}
