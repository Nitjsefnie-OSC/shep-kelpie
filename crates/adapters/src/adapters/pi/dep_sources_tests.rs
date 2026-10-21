//! The sandbox reads the dependency and toolchain sources a call is given, and
//! no credential of them

use std::path::Path;

use super::*;
use tests::{URL, WORKER_ID, World, id};

#[test]
fn the_sandbox_reads_the_dependency_sources_a_call_is_given_and_no_credential_of_them() {
    let w = World::new();
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    // The folders `crate::dep_sources` finds, as the runner puts them in a call's reach.
    let sources = [
        w.path("cargo/registry/src/serde-1.0.2"),
        w.path("cargo/git/checkouts/shep-core-1a2b3c"),
        w.path("rustup/toolchains/stable/lib/rustlib/src"),
        w.path("rustup/toolchains/stable/share/doc"),
    ];
    for folder in &sources {
        call.reach.read.push(folder.clone());
        call.reach.fence.as_mut().unwrap().read.push(folder.clone());
    }
    let upstream = PiCli::on(&Base::default())
        .upstream(server(&call).unwrap())
        .unwrap();
    let policy = policy(
        &call,
        &Files::of(&call),
        &upstream,
        &w.path("forwarder.sock"),
        &[],
    );
    for folder in &sources {
        assert!(policy.read.contains(folder), "{}", folder.display());
        assert!(
            !policy
                .write
                .iter()
                .any(|writable| folder.starts_with(writable)),
            "{} is writable",
            folder.display()
        );
    }
    for secret in ["~/.cargo/credentials", "~/.npmrc", "~/.netrc", "~/.ssh/**"] {
        assert!(policy.no_read.iter().any(|r| r == secret), "{secret}");
    }
}
