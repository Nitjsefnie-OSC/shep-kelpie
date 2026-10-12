use std::path::Path;

use crate::settings::Settings;

const EXAMPLE: &str = include_str!("../../../settings.example.toml");

const WORKER: &str = "allowed_domains = [\"crates.io\", \"index.crates.io\", \"static.crates.io\"]";

// The example with `domains` as `table`'s `allowed_domains`, read the way a runner reads its table.
fn parse(table: &str, domains: &str) -> Result<Settings, String> {
    assert!(
        EXAMPLE.contains(WORKER),
        "the example's worker domains moved"
    );
    let text = match table {
        "worker" => EXAMPLE.replace(WORKER, &format!("allowed_domains = {domains}")),
        _ => format!("{EXAMPLE}\n[app.dogs.kelpie.researcher]\nallowed_domains = {domains}\n"),
    };
    Settings::from_table(
        &crate::test::project_table(&text),
        "shep",
        Path::new("/home/me"),
        Path::new("/home/me/.shep/kelpie/shep"),
    )
    .map_err(|e| e.to_string())
}

#[test]
fn a_wildcard_in_the_middle_of_a_domain_is_refused_naming_the_entry() {
    for table in ["worker", "researcher"] {
        for bad in [
            "productionresultssa*.blob.core.windows.net",
            "foo.*.example.com",
            "*.example.*",
            "*",
            "**.example.com",
        ] {
            let err = parse(table, &format!("[\"docs.rs\", \"{bad}\"]")).unwrap_err();
            assert!(
                err.contains(&format!("`{bad}`")) && err.contains("allowed_domains"),
                "{table}: {err}"
            );
        }
    }
}

#[test]
fn a_leading_wildcard_label_is_accepted() {
    for table in ["worker", "researcher"] {
        let s = parse(table, "[\"*.crates.io\", \"docs.rs\"]").unwrap();
        let set = match table {
            "worker" => s.worker.allowed_domains,
            _ => s.researcher.allowed_domains,
        };
        let set: Vec<&str> = set.iter().map(|d| d.as_str()).collect();
        assert_eq!(set, ["*.crates.io", "docs.rs"], "{table}");
    }
}

#[test]
fn an_entry_with_a_scheme_a_path_or_a_bad_port_is_refused_naming_it() {
    for table in ["worker", "researcher"] {
        for bad in [
            "https://crates.io",
            "crates.io/index",
            "crates.io:",
            "crates.io:0",
            "crates.io:http",
            "crates.io:80:80",
            "user@crates.io",
            "crates .io",
            ".crates.io",
            "crates..io",
            "*.",
        ] {
            let err = parse(table, &format!("[\"{bad}\"]")).unwrap_err();
            assert!(err.contains(&format!("`{bad}`")), "{table} {bad}: {err}");
        }
    }
}

#[test]
fn a_host_with_a_port_or_an_address_is_accepted() {
    for ok in [
        "localhost",
        "box",
        "127.0.0.1",
        "host:11434",
        "[::ffff:192.0.2.9]",
        "[::1]:8080",
        "*.example.com:443",
    ] {
        parse("worker", &format!("[\"{ok}\"]")).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
}
