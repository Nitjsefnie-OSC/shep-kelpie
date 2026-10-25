//! The commit this build was made from, baked in as `KELPIE_COMMIT`
//!
//! `home::build` reads it with `option_env!`, so a build made where git
//! cannot answer, outside a git checkout or with no git on the `PATH`, names
//! no commit at all, and every reader says so instead of guessing one.

use std::process::{Command, Stdio};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for path in watched() {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(commit) = commit() {
        println!("cargo:rustc-env=KELPIE_COMMIT={commit}");
    }
}

// The commit `HEAD` is at, in seven characters, with `-dirty` when a tracked
// file holds changes that commit does not. `None` when git cannot answer.
fn commit() -> Option<String> {
    let sha = ask(&["rev-parse", "--short=7", "HEAD"])?;
    // `--no-optional-locks` keeps a build from taking the index lock a git
    // command run beside it is holding. Untracked files are left out because
    // they are not what a build compiles: a new file only reaches the build
    // through a tracked one, and that tracked file is watched and counted.
    let said = ask(&[
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ])?;
    Some(match said.is_empty() {
        true => sha,
        false => format!("{sha}-dirty"),
    })
}

// What a rebuild has to look at, which is what the dirty check looks at:
// every entry git tracks at the top of the checkout, cargo scanning a listed
// directory for any change inside it, and the refs that say where `HEAD`
// stands. A build script is rerun only for the paths it lists, so listing
// this crate's own files alone would leave a build made from a tree with
// edits, or one made after the edits were undone, holding the answer the
// build before it baked in. A branch whose ref is packed into `packed-refs`
// has no loose file to watch, and cargo then runs this again for every build,
// which costs two git calls and stays correct.
fn watched() -> Vec<String> {
    let mut paths = head_refs();
    let Some(root) = ask(&["rev-parse", "--show-toplevel"]) else {
        return paths;
    };
    let Some(names) = ask(&["ls-tree", "--name-only", "HEAD"]) else {
        return paths;
    };
    paths.extend(names.lines().map(|name| format!("{root}/{name}")));
    paths
}

// The files a commit or a checkout moves, so a build after one is not left
// with the commit the build before it baked in.
fn head_refs() -> Vec<String> {
    let branch = symbolic_head();
    let specs: [&str; 2] = ["HEAD", branch.as_deref().unwrap_or("")];
    specs
        .into_iter()
        .filter(|spec| !spec.is_empty())
        .filter_map(|spec| ask(&["rev-parse", "--git-path", spec]))
        .filter(|path| !path.is_empty())
        .collect()
}

// The branch `HEAD` points at, and `None` when it is detached.
fn symbolic_head() -> Option<String> {
    ask(&["symbolic-ref", "--quiet", "HEAD"])
}

// git's answer, and `None` when it cannot run or refused.
fn ask(args: &[&str]) -> Option<String> {
    let answer = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    answer
        .status
        .success()
        .then(|| String::from_utf8_lossy(&answer.stdout).trim().to_owned())
}
