//! `[app.dogs.kelpie.triage]`, which says whether kelpie triages issues other
//! people open

use std::num::NonZeroU32;

use super::{EXAMPLE, parse, parse_err};
use crate::settings::TriagePick;

const TABLE: &str = "[app.dogs.kelpie.triage]\n";

// The example with `lines` under a triage table.
fn with(lines: &str) -> String {
    format!("{EXAMPLE}\n{TABLE}{lines}")
}

#[test]
fn an_absent_triage_table_means_pick_off_wait_7_questions_10_agent_triager() {
    let t = parse(EXAMPLE).unwrap().triage;
    assert_eq!(t.pick, TriagePick::Off);
    assert_eq!(t.wait_days, NonZeroU32::new(7).unwrap());
    assert_eq!(t.max_questions.get(), 10);
    assert_eq!(t.agent.as_str(), "triager");
    assert_eq!(parse(&with("")).unwrap().triage, t, "an empty table");
    let partial = parse(&with("pick = \"all\"\n")).unwrap().triage;
    assert_eq!(
        partial.wait_days, t.wait_days,
        "a key left out keeps its default"
    );
    assert_eq!(partial.max_questions, t.max_questions);
    assert_eq!(partial.agent, t.agent);
}

#[test]
fn pick_takes_off_maintainer_or_all_and_refuses_any_other_word() {
    for (word, pick) in [
        ("off", TriagePick::Off),
        ("maintainer", TriagePick::Maintainer),
        ("all", TriagePick::All),
    ] {
        let settings = parse(&with(&format!("pick = \"{word}\"\n"))).unwrap();
        assert_eq!(settings.triage.pick, pick, "{word}");
    }
    let err = parse_err(&with("pick = \"some\"\n"));
    assert!(err.contains("pick") && err.contains("some"), "{err}");
}

#[test]
fn max_questions_of_zero_or_above_10_is_refused_naming_it() {
    for n in [0, 11, 50] {
        let err = parse_err(&with(&format!("max_questions = {n}\n")));
        assert!(err.contains("max_questions"), "{n}: {err}");
    }
    for n in [1, 10] {
        let t = parse(&with(&format!("max_questions = {n}\n")))
            .unwrap()
            .triage;
        assert_eq!(t.max_questions.get(), n);
    }
}

#[test]
fn zero_wait_days_is_refused() {
    let err = parse_err(&with("wait_days = 0\n"));
    assert!(err.contains("wait_days"), "{err}");
    let t = parse(&with("wait_days = 3\n")).unwrap().triage;
    assert_eq!(t.wait_days, NonZeroU32::new(3).unwrap());
}

#[test]
fn the_limits_default_to_10_a_day_2_per_author_30_days_and_20_kb() {
    let t = parse(EXAMPLE).unwrap().triage;
    assert_eq!(t.per_day, 10);
    assert_eq!(t.per_author_per_day, 2);
    assert_eq!(t.min_account_days, 30);
    assert_eq!(t.max_body_kb, 20);
    assert_eq!(parse(&with("")).unwrap().triage, t, "an empty table");
    let partial = parse(&with("pick = \"all\"\n")).unwrap().triage;
    assert_eq!(partial.per_day, t.per_day, "a key left out keeps its default");
    assert_eq!(partial.per_author_per_day, t.per_author_per_day);
    assert_eq!(partial.min_account_days, t.min_account_days);
    assert_eq!(partial.max_body_kb, t.max_body_kb);
    for (key, want) in [
        ("per_day", 0u32),
        ("per_author_per_day", 1),
        ("min_account_days", 7),
        ("max_body_kb", 3),
    ] {
        let t = parse(&with(&format!("{key} = {want}\n"))).unwrap().triage;
        let got = match key {
            "per_day" => t.per_day,
            "per_author_per_day" => t.per_author_per_day,
            "min_account_days" => t.min_account_days,
            _ => t.max_body_kb,
        };
        assert_eq!(got, want, "{key} = {want}");
    }
}

#[test]
fn a_limit_that_is_negative_or_not_a_number_is_refused_naming_it() {
    for key in [
        "per_day",
        "per_author_per_day",
        "min_account_days",
        "max_body_kb",
    ] {
        for value in ["-1", "\"ten\"", "1.5", "true"] {
            let err = parse_err(&with(&format!("{key} = {value}\n")));
            assert!(err.contains(key), "{key} = {value}: {err}");
        }
    }
}

#[test]
fn an_unknown_key_in_triage_is_refused_naming_it() {
    let err = parse_err(&with("pik = \"all\"\n"));
    assert!(err.contains("pik"), "{err}");
}
