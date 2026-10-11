//! An open branch's diff from `main`, read for the project manager, which
//! runs no git, cut to a size its context can take

use std::io::{self, Read};
use std::path::Path;
use std::process::Stdio;

use crate::worktree::in_repo;

/// The most of one diff kept, in bytes: about 16k tokens, so one huge
/// branch cannot flood its context, and it reads the worktree for the rest
pub(in crate::runner) const CAP: usize = 64 * 1024;

/// `head`'s diff from where it left `main`, cut at [`CAP`], or why git
/// could not give it
///
/// `files` are the files it changes, as `git diff --name-only --no-renames`
/// lists them, and `worktree` is where the worker's copy is: a cut diff's
/// last line names them both, so the rest can be read there.
///
/// No external diff or text conversion runs, whatever git's config names,
/// and no more than [`CAP`] bytes of git's output are held.
pub(super) fn read(
    repo: &Path,
    main: &str,
    head: &str,
    files: Option<&[String]>,
    worktree: &Path,
) -> Result<Vec<u8>, String> {
    let range = format!("{main}...{head}");
    let mut child = in_repo(repo)
        .arg("--no-replace-objects")
        .args(["diff", "--no-color", "--no-ext-diff", "--no-textconv"])
        // One section a file, in the order `--name-only` lists them.
        .arg("--no-renames")
        .arg(&range)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("git did not run: {}", e.kind()))?;
    let read = match child.stdout.take() {
        Some(out) => kept(out),
        None => Err(io::Error::other("no output")),
    };
    let status = child
        .wait()
        .map_err(|e| format!("git did not end: {}", e.kind()))?;
    let (diff, total) = read.map_err(|e| format!("cannot read git's diff: {}", e.kind()))?;
    if !status.success() {
        return Err(format!("git diff {range} failed ({status})"));
    }
    Ok(capped(diff, total, files, worktree))
}

// The first `CAP` bytes of `out` and the one after, and how many it held,
// the rest drained.
fn kept(mut out: impl Read) -> io::Result<(Vec<u8>, u64)> {
    let mut diff = Vec::new();
    (&mut out).take(CAP as u64 + 1).read_to_end(&mut diff)?;
    let rest = io::copy(&mut out, &mut io::sink())?;
    let total = (diff.len() as u64).saturating_add(rest);
    Ok((diff, total))
}

// `diff`, the start of a diff `total` long, cut within `CAP` at a line's
// end, or at a character's where that would drop over half of what fits,
// as in a minified file's one line, with a last line naming its size, the
// files from the one cut on, and the worktree.
fn capped(mut diff: Vec<u8>, total: u64, files: Option<&[String]>, worktree: &Path) -> Vec<u8> {
    if total <= CAP as u64 {
        return diff;
    }
    let held = diff.get(..CAP).unwrap_or(&diff);
    let end = match held.iter().rposition(|&b| b == b'\n') {
        Some(at) if at >= CAP / 2 => at + 1,
        // A UTF-8 continuation byte is never the first of a character.
        _ => (0..=CAP)
            .rev()
            .find(|&at| diff.get(at).is_none_or(|b| b & 0xC0 != 0x80))
            .unwrap_or(0),
    };
    diff.truncate(end);
    // Each section opens with such a line, and no line of a file's own
    // starts with "diff": it starts with a space, a plus or a minus. A path
    // that changes between a file and a link has two sections under one
    // header line, which `--name-only` lists once.
    let mut headers = diff
        .split(|&b| b == b'\n')
        .filter(|line| line.starts_with(b"diff --git "))
        .collect::<Vec<_>>();
    headers.dedup();
    let shown = headers.len();
    let past = match files {
        // The last file shown is cut part-way, so it is read in the worktree too.
        Some(files) => format!(
            "; files cut or past the cut: {}",
            files[shown.saturating_sub(1).min(files.len())..]
                .iter()
                .map(|name| spelled(name))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => String::new(),
    };
    let said = format!(
        "\n[kelpie cut this diff at {} KiB of its {total} bytes{past}; read the worktree for the rest at {}]\n",
        CAP / 1024,
        worktree.display()
    );
    diff.extend_from_slice(said.as_bytes());
    diff
}

// `name`, quoted where a control character would break the line it is on.
fn spelled(name: &str) -> String {
    match name.chars().any(char::is_control) {
        true => format!("{name:?}"),
        false => name.to_owned(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test::{git, write_in};

    #[test]
    fn a_type_change_before_the_cut_is_one_file_and_the_cut_file_is_still_named() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "--quiet", "--initial-branch=main"]);
        write_in(repo, "a.txt", "was a file\n");
        git(repo, &["add", "."]);
        git(repo, &["commit", "--quiet", "-m", "main"]);
        git(repo, &["checkout", "--quiet", "-b", "feature"]);
        // A file becoming a link is two sections of the diff, one name.
        std::fs::remove_file(repo.join("a.txt")).unwrap();
        std::os::unix::fs::symlink("elsewhere", repo.join("a.txt")).unwrap();
        let block = "one line of a file that fills the cap\n".repeat(1_200);
        for file in ["b.txt", "c.txt"] {
            write_in(repo, file, &block);
        }
        write_in(repo, "d.txt", "small\n");
        git(repo, &["add", "."]);
        git(repo, &["commit", "--quiet", "-m", "feature"]);
        let files: Vec<String> = ["a.txt", "b.txt", "c.txt", "d.txt"]
            .map(str::to_owned)
            .into();

        let diff = read(repo, "main", "feature", Some(&files), Path::new("/work/7")).unwrap();

        let diff = String::from_utf8(diff).unwrap();
        let last = diff.trim_end().lines().last().unwrap();
        assert!(diff.contains("+++ b/b.txt\n"), "b.txt is shown");
        assert!(
            last.contains("files cut or past the cut: c.txt, d.txt;"),
            "{last}"
        );
    }
}
