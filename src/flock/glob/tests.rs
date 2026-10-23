use super::*;

fn glob(text: &str) -> Glob {
    match Named::parse(text).unwrap() {
        Named::Glob(glob) => glob,
        Named::Project(name) => panic!("{name} read as a project"),
    }
}

fn matching<'a>(pattern: &str, names: &[&'a str]) -> Vec<&'a str> {
    let glob = glob(pattern);
    names.iter().copied().filter(|n| glob.matches(n)).collect()
}

const NAMES: [&str; 6] = ["shep", "shep-kelpie", "shepherd", "koji", "Shep", "eevee"];

#[test]
fn a_name_with_no_glob_character_is_a_project() {
    for text in ["koji", "shep-kelpie", "a_b.c"] {
        let named = Named::parse(text).unwrap();
        assert_eq!(named, Named::Project(ProjectName::try_from(text).unwrap()));
    }
}

#[test]
fn a_star_a_question_mark_or_a_bracket_makes_a_glob() {
    for text in ["*", "shep*", "k?ji", "[ke]*"] {
        assert!(matches!(Named::parse(text), Ok(Named::Glob(_))), "{text}");
    }
}

#[test]
fn a_name_that_is_neither_is_refused() {
    for text in ["", "../koji", "-x", "a b"] {
        assert!(Named::parse(text).is_err(), "{text:?}");
    }
}

#[test]
fn a_star_matches_any_run_of_characters_including_none() {
    assert_eq!(
        matching("shep*", &NAMES),
        ["shep", "shep-kelpie", "shepherd"]
    );
    assert_eq!(matching("*e", &NAMES), ["shep-kelpie", "eevee"]);
    assert_eq!(matching("*", &NAMES), NAMES);
    assert_eq!(matching("s*p*d", &NAMES), ["shepherd"]);
}

#[test]
fn a_question_mark_matches_exactly_one_character() {
    assert_eq!(matching("k?ji", &NAMES), ["koji"]);
    assert_eq!(matching("shep?", &NAMES), Vec::<&str>::new());
}

#[test]
fn a_bracket_matches_one_of_its_characters_or_a_range() {
    assert_eq!(matching("[ke]*", &NAMES), ["koji", "eevee"]);
    assert_eq!(matching("[a-f]*", &NAMES), ["eevee"]);
    assert_eq!(matching("[!s]*", &NAMES), ["koji", "Shep", "eevee"]);
}

#[test]
fn matching_is_case_sensitive() {
    assert_eq!(matching("S*", &NAMES), ["Shep"]);
    assert_eq!(matching("[S]*", &NAMES), ["Shep"]);
}

#[test]
fn a_bracket_with_no_end_is_refused() {
    let err = Named::parse("shep[").unwrap_err();
    assert!(err.contains("`[`"), "{err}");
}
