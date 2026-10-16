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

#[test]
fn a_worker_reads_the_toolchain_its_build_env_points_into_the_build_folder() {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| {
        s.replace(
            "build_env = {}",
            "build_env = { CARGO_HOME = \"cargo\", RUSTUP_HOME = \"rustup\" }",
        )
    });
    let rustup = rig.build_7().join("rustup");
    for rel in [
        "toolchains/stable-aarch64/lib/rustlib/src/rust/library/std/src/lib.rs",
        "toolchains/stable-aarch64/share/doc/rust/index.html",
        "toolchains/stable-aarch64/lib/libstd.rlib",
        "downloads/crates",
    ] {
        write_in(&rustup, rel, "x\n");
    }
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();

    for read in [
        rustup.join("toolchains/stable-aarch64/lib/rustlib/src/rust/library/std/src/lib.rs"),
        rustup.join("toolchains/stable-aarch64/share/doc/rust/index.html"),
    ] {
        assert_reads(&seen, &read);
    }
    let granted: Vec<&str> = (seen.settings["permissions"]["additionalDirectories"]
        .as_array()
        .expect("the worker is granted folders outside its worktree")
        .iter())
    .filter_map(serde_json::Value::as_str)
    .collect();
    for folder in [
        rustup.join("toolchains/stable-aarch64/lib/rustlib/src"),
        rustup.join("toolchains/stable-aarch64/share/doc"),
    ] {
        assert!(granted.contains(&folder.to_str().unwrap()), "{granted:?}");
    }
    // A toolchain's own binaries and rustup's download cache are no source to read.
    for other in [
        rustup.join("toolchains/stable-aarch64/lib"),
        rustup.join("downloads"),
    ] {
        assert!(
            !granted.contains(&other.to_str().unwrap()),
            "{}: {granted:?}",
            other.display()
        );
    }
}

#[test]
fn a_worker_reads_the_build_folder_source_folders_its_cargo_has_not_made_yet() {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| {
        s.replace(
            "build_env = {}",
            "build_env = { CARGO_HOME = \"cargo\", RUSTUP_HOME = \"rustup\" }",
        )
    });
    // Nothing has been built for this item: its build folder holds no crate and
    // no toolchain, and the call's own cargo is what will make them.
    let (cargo, rustup) = (rig.build_7().join("cargo"), rig.build_7().join("rustup"));
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();
    let granted: Vec<&str> = (seen.settings["permissions"]["additionalDirectories"]
        .as_array()
        .expect("the worker is granted folders outside its worktree")
        .iter())
    .filter_map(serde_json::Value::as_str)
    .collect();

    for folder in [
        cargo.join("registry/src"),
        cargo.join("git/checkouts"),
        rustup.join("toolchains"),
    ] {
        assert!(
            folder.is_dir(),
            "{} is not there for the call to fill",
            folder.display()
        );
        assert!(granted.contains(&folder.to_str().unwrap()), "{granted:?}");
    }

    // The grant is no wider than the source folders: the home itself and what
    // cargo keeps beside its unpacked crates stay outside it.
    for other in [
        cargo.as_path(),
        &cargo.join("registry"),
        &cargo.join("registry/index"),
    ] {
        assert!(
            !granted.contains(&other.to_str().unwrap()),
            "{}: {granted:?}",
            other.display()
        );
    }
}
