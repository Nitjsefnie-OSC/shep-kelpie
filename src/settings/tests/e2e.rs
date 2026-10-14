//! The `e2e` table

use std::path::Path;

use super::{EXAMPLE, parse, parse_err};

// The example with its commented e2e table taken out and `[e2e]` put in its place
fn with_e2e(keys: &str) -> String {
    let off = "# [app.dogs.kelpie.e2e]\n# host = \"e2e-box\"\n# command = \"scripts/e2e-remote.sh\"\n# results = \"test-results\"\n";
    assert!(EXAMPLE.contains(off), "the example's e2e table moved");
    EXAMPLE.replace(off, &format!("[app.dogs.kelpie.e2e]\n{keys}\n"))
}

#[test]
fn e2e_is_absent_by_default() {
    assert_eq!(parse(EXAMPLE).unwrap().e2e, None);
}

#[test]
fn an_e2e_table_reads_host_command_and_results() {
    let set = with_e2e("host = \"e2e-box\"\ncommand = \"ci/e2e.sh\"\nresults = \"out/pw\"");
    let e2e = parse(&set).unwrap().e2e.unwrap();
    assert_eq!(e2e.host.as_str(), "e2e-box");
    assert_eq!(e2e.command.as_path(), Path::new("ci/e2e.sh"));
    assert_eq!(e2e.results.as_path(), Path::new("out/pw"));
    let host_only = with_e2e("host = \"e2e-box\"");
    let e2e = parse(&host_only).unwrap().e2e.unwrap();
    assert_eq!(e2e.command.as_path(), Path::new("scripts/e2e-remote.sh"));
    assert_eq!(e2e.results.as_path(), Path::new("test-results"));
}

#[test]
fn a_path_with_a_dot_component_is_read_without_it() {
    let set = with_e2e("host = \"box\"\ncommand = \"./scripts/e2e.sh\"\nresults = \"./out/./pw\"");
    let e2e = parse(&set).unwrap().e2e.unwrap();
    assert_eq!(e2e.command.as_path(), Path::new("scripts/e2e.sh"));
    assert_eq!(e2e.results.as_path(), Path::new("out/pw"));
    for (key, bad) in [("command", "."), ("results", "./.git"), ("results", "./")] {
        let err = parse_err(&with_e2e(&format!("host = \"box\"\n{key} = {bad:?}")));
        assert!(err.contains(key), "{key} {bad:?}: {err}");
    }
}

#[test]
fn a_host_that_reads_as_an_ssh_option_is_refused() {
    for bad in [
        "-oProxyCommand=x",
        "-",
        "",
        "box name",
        "box;rm",
        "me@box",
        "a/b",
    ] {
        let err = parse_err(&with_e2e(&format!("host = {bad:?}")));
        assert!(err.contains("an ssh alias"), "{bad:?}: {err}");
    }
    for good in ["box", "e2e-box.lan", "e2e_box2", "9box"] {
        assert!(
            parse(&with_e2e(&format!("host = {good:?}"))).is_ok(),
            "{good}"
        );
    }
}

#[test]
fn a_command_or_results_path_that_leaves_the_repo_is_refused() {
    for (key, bad) in [
        ("command", "/bin/sh"),
        ("command", "../run.sh"),
        ("command", "a/../../run.sh"),
        ("command", ""),
        ("results", "/tmp/out"),
        ("results", "../out"),
        ("results", ""),
        ("results", ".git"),
        ("results", ".git/hooks"),
    ] {
        let err = parse_err(&with_e2e(&format!("host = \"box\"\n{key} = {bad:?}")));
        assert!(err.contains(key), "{key} {bad:?}: {err}");
    }
}

#[test]
fn an_unknown_e2e_key_is_refused() {
    let err = parse_err(&with_e2e("host = \"box\"\nretries = 2"));
    assert!(err.contains("retries"), "{err}");
}
