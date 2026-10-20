//! What a call's fence lets it read, judged on the settings it saw and the
//! sandbox policy its fence makes, the way each layer would judge a path

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Rig, Seen, write_in};

/// `path` as given and with its links resolved, as each tool may name it
pub fn spellings(path: &Path) -> Vec<String> {
    let resolved = path.canonicalize().ok().filter(|c| c != path);
    std::iter::once(path.to_owned())
        .chain(resolved)
        .map(|p| p.display().to_string())
        .collect()
}

/// Whether Claude Code's Read rules in the call's settings refuse `path`
pub fn tools_refuse(seen: &Seen, path: &str) -> bool {
    let deny = seen.settings["permissions"]["deny"].as_array().unwrap();
    deny.iter().filter_map(Value::as_str).any(|rule| {
        let Some(glob) = rule
            .strip_prefix("Read(/")
            .and_then(|r| r.strip_suffix(')'))
        else {
            return false;
        };
        match glob.strip_suffix("/**") {
            Some(folder) => path.starts_with(&format!("{folder}/")),
            None => path == glob,
        }
    })
}

/// Whether the sandbox refuses `path`: the narrowest rule covering it wins,
/// as `srt` emits a deny nested in an allow after the allow
pub fn sandbox_refuses(seen: &Seen, path: &str) -> bool {
    let files = &seen.sandbox["filesystem"];
    let narrowest = |key: &str| {
        let rules = files[key]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str);
        rules
            .map(|rule| rule.strip_suffix("/**").unwrap_or(rule))
            .filter(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
            .map(str::len)
            .max()
    };
    match (narrowest("denyRead"), narrowest("allowRead")) {
        (Some(denied), Some(allowed)) => denied >= allowed,
        (denied, _) => denied.is_some(),
    }
}

/// Asserts the call reads `path` in both layers. Its tools are judged on
/// `path` as given and with its links resolved; `srt` resolves its rules'
/// links itself, so the sandbox is judged as given.
pub fn assert_reads(seen: &Seen, path: &Path) {
    for spelled in spellings(path) {
        assert!(!tools_refuse(seen, &spelled), "its tools refuse {spelled}");
    }
    let spelled = path.display().to_string();
    assert!(
        !sandbox_refuses(seen, &spelled),
        "its sandbox refuses {spelled}"
    );
}

/// Asserts the call is refused `path` in both layers
pub fn assert_refuses(seen: &Seen, path: &Path) {
    for spelled in spellings(path) {
        assert!(tools_refuse(seen, &spelled), "its tools read {spelled}");
    }
    let spelled = path.display().to_string();
    assert!(
        sandbox_refuses(seen, &spelled),
        "its sandbox reads {spelled}"
    );
}

/// A cargo home and a rustup home under `rig`'s home, each holding one source,
/// with what each tool keeps beside its sources and a cargo credential among
/// them. Returns the two homes and the credential.
pub fn installed_sources(rig: &Rig) -> (PathBuf, PathBuf, PathBuf) {
    let (cargo, rustup) = (
        rig.home.path().join(".cargo"),
        rig.home.path().join(".rustup"),
    );
    for (root, rel) in [
        (&cargo, "registry/src/serde-1.0.2/src/lib.rs"),
        (&cargo, "git/checkouts/shep-core-1a2b3c/HEAD/src/lib.rs"),
        (&cargo, "registry/cache/serde-1.0.2.crate"),
        (&cargo, "credentials.toml"),
        (
            &rustup,
            "toolchains/stable-aarch64/lib/rustlib/src/rust/library/std/src/lib.rs",
        ),
        (
            &rustup,
            "toolchains/stable-aarch64/share/doc/rust/index.html",
        ),
        (&rustup, "toolchains/stable-aarch64/lib/libstd.rlib"),
    ] {
        write_in(root, rel, "x\n");
    }
    (cargo.clone(), rustup, cargo.join("credentials.toml"))
}

/// The source folders [`installed_sources`] made under `cargo` and `rustup`
pub fn source_folders(cargo: &Path, rustup: &Path) -> Vec<PathBuf> {
    vec![
        cargo.join("registry/src"),
        cargo.join("git/checkouts"),
        rustup.join("toolchains/stable-aarch64/lib/rustlib/src"),
        rustup.join("toolchains/stable-aarch64/share/doc"),
    ]
}

/// Asserts the call is granted each of `folders`, in its read list and in the
/// `additionalDirectories` its settings hold, and that no grant covers `secret`
pub fn assert_granted(seen: &Seen, folders: &[PathBuf], secret: &Path) {
    for folder in folders {
        assert!(
            seen.call.reach.read.iter().any(|held| held == folder),
            "{} is not in its read list: {:?}",
            folder.display(),
            seen.call.reach.read
        );
    }
    let granted: Vec<&str> = (seen.settings["permissions"]["additionalDirectories"]
        .as_array()
        .expect("the call is granted folders outside its working folder")
        .iter())
    .filter_map(Value::as_str)
    .collect();
    for folder in folders {
        assert!(
            granted.contains(&folder.to_str().unwrap()),
            "{}: {granted:?}",
            folder.display()
        );
    }
    assert!(
        !granted.iter().any(|held| secret.starts_with(held)),
        "{} sits under a granted folder: {granted:?}",
        secret.display()
    );
}
