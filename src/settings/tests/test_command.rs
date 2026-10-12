//! `ci.test_command`, how a reviewer's attached test is run

use super::{EXAMPLE, parse, parse_err};

#[test]
fn test_command_is_optional() {
    let on = EXAMPLE.replace(
        "# test_command = \"cargo test --test {name}\"",
        "test_command = \"cargo test --test {name}\"",
    );
    assert_ne!(on, EXAMPLE, "the example's test_command moved");
    let command = |text: &str| {
        let ci = parse(text).unwrap().ci;
        ci.test_command.map(|c| c.as_str().to_owned())
    };
    assert_eq!(command(EXAMPLE), None);
    assert_eq!(command(&on).as_deref(), Some("cargo test --test {name}"));
    let by_path = on.replace("{name}", "{test}");
    assert_eq!(
        command(&by_path).as_deref(),
        Some("cargo test --test {test}")
    );
}

#[test]
fn test_command_without_a_placeholder_is_refused() {
    let with = |value: &str| {
        EXAMPLE.replace(
            "# test_command = \"cargo test --test {name}\"",
            &format!("test_command = \"{value}\""),
        )
    };
    for value in ["cargo test", ""] {
        let err = parse_err(&with(value));
        assert!(err.contains("`ci.test_command"), "{err}");
        assert!(err.contains("{test}"), "{err}");
        assert!(err.contains("{name}"), "{err}");
    }
}
