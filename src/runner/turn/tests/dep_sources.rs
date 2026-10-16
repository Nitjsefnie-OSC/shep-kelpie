//! Every call reads the installed sources and docs of its dependencies and
//! toolchains, and no credential of them

use super::*;

#[test]
fn a_worker_reads_the_installed_dependency_sources_and_no_credential_of_cargo() {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| s.replace("build_env = {}", "build_env = { CARGO_HOME = \"cargo\" }"));
    // cargo's own default home, and the one this project points into the
    // work item's build folder, both there before the runner looks.
    let (cargo, build_cargo) = (rig.home.path().join(".cargo"), rig.build_7().join("cargo"));
    for (root, rel) in [
        (&cargo, "registry/src/serde-1.0.2/src/lib.rs"),
        (&cargo, "git/checkouts/shep-core-1a2b3c/HEAD/src/lib.rs"),
        (&cargo, "registry/cache/serde-1.0.2.crate"),
        (&cargo, "credentials.toml"),
        (&build_cargo, "registry/src/rand-0.9.0/src/lib.rs"),
        (&build_cargo, "registry/index/index.crates.io-6a1f/.cache"),
    ] {
        write_in(root, rel, "x\n");
    }
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();

    for read in [
        cargo.join("registry/src/serde-1.0.2/src/lib.rs"),
        cargo.join("git/checkouts/shep-core-1a2b3c/HEAD/src/lib.rs"),
        build_cargo.join("registry/src/rand-0.9.0/src/lib.rs"),
    ] {
        assert_reads(&seen, &read);
    }
    let granted: Vec<&str> = (seen.settings["permissions"]["additionalDirectories"]
        .as_array()
        .expect("the worker is granted folders outside its worktree")
        .iter())
    .filter_map(serde_json::Value::as_str)
    .collect();
    for folder in [cargo.join("registry/src"), build_cargo.join("registry/src")] {
        assert!(granted.contains(&folder.to_str().unwrap()), "{granted:?}");
    }

    // What cargo keeps beside its cache stays unreadable to every call.
    let deny = seen.settings["permissions"]["deny"].to_string();
    let sandbox = seen.sandbox["filesystem"]["denyRead"].to_string();
    for secret in [
        "~/.cargo/credentials",
        "~/.cargo/credentials.toml",
        "~/.npmrc",
        "~/.netrc",
    ] {
        assert!(
            deny.contains(&format!("Read({secret})")),
            "{secret}: {deny}"
        );
        assert!(
            sandbox.contains(&format!("\"{secret}\"")),
            "{secret}: {sandbox}"
        );
    }
}
