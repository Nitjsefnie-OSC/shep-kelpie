//! The commit this build was made from, baked in as `KELPIE_COMMIT`
//!
//! `src/home/build.rs` reads it with `option_env!`, so a build made where git
//! cannot answer, outside a git checkout or with no git on the `PATH`, names
//! no commit at all, and every reader says so instead of guessing one.

use std::process::{Command, Stdio};

fn main() {
    // The commit moves with the checkout, not with this crate's own files, so
    // these are what a rebuild looks at. A branch whose ref is packed into
    // `packed-refs` has no loose file to watch, and cargo then runs this again
    // for every build, which costs two git calls.
    println!("cargo:rerun-if-changed=build.rs");
    for path in watched() {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(commit) = commit() {
        println!("cargo:rustc-env=KELPIE_COMMIT={commit}");
    }
}

// The commit `HEAD` is at, in seven characters, with `-dirty` when the tree
// holds changes that commit does not. `None` when git cannot answer.
fn commit() -> Option<String> {
    let sha = ask(&["rev-parse", "--short=7", "HEAD"])?;
    // `--no-optional-locks` keeps a build from taking the index lock a git
    // command run beside it is holding.
    let said = ask(&["--no-optional-locks", "status", "--porcelain"])?;
    Some(match said.trim().is_empty() {
        true => sha,
        false => format!("{sha}-dirty"),
    })
}

// The files a commit or a checkout changes, so a build after one is not left
// with the commit the build before it baked in.
fn watched() -> Vec<String> {
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
