//! `shep kelpie lease` on `gpu`, end to end against the real binary
//!
//! The GPU lease is gone: paddock queues the GPU. Every command is refused
//! with a pointer at `shep paddock run`, and none leaves a lock under the
//! temporary folder, which is a scratch one and never the maintainer's.

use std::path::Path;
use std::process::{Command, Output, Stdio};

const KELPIE: &str = env!("CARGO_BIN_EXE_shep-kelpie");

fn lease(temp: &Path, args: &[&str]) -> Output {
    Command::new(KELPIE)
        .arg("lease")
        .args(args)
        .env("TMPDIR", temp)
        .env("SHEP_HOME", temp.join("no-shepherd"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

// Every path under `dir`, relative to it.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            found.push(path.strip_prefix(dir).unwrap().display().to_string());
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_gpu_lease_command_is_refused_and_leaves_no_lock() {
    let temp = tempfile::tempdir().unwrap();
    for args in [
        &["take", "gpu"][..],
        &["take", "gpu", "--model", "qwen3"],
        &["run", "gpu", "--", "true"],
        &["run", "gpu", "--model", "qwen3", "--", "true"],
        &["return", "gpu"],
    ] {
        let out = lease(temp.path(), args);
        assert!(!out.status.success(), "{args:?} succeeded");
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(said.contains("shep paddock run"), "{args:?}: {said}");
    }
    assert_eq!(files(temp.path()), Vec::<String>::new(), "a lock was made");
}
