//! The pull request a worker hands over, for kelpie to open and edit
//!
//! A worker on a repo kelpie's App covers holds a token that pushes its
//! branch and does nothing else, so `gh pr create` and `gh pr edit` fail for
//! it. It writes the pull request's title and body into a file in its build
//! folder, and kelpie opens the draft when a turn ends with the branch
//! pushed and no pull request, and sets the title and body again when a
//! later turn changed the file.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::Runner;
use crate::worktree::BASE;

/// The file the worker writes its pull request in, in its build folder
const FILE: &str = "pull-request.md";

/// A copy of the file as kelpie last put it on the pull request, so a turn
/// that left it alone edits nothing
const APPLIED: &str = "pull-request.applied.md";

/// Where the worker writes its pull request's text, in `build`
pub(in crate::runner) fn path(build: &Path) -> PathBuf {
    build.join(FILE)
}

/// A pull request's title and body
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Text {
    pub(super) title: String,
    pub(super) body: String,
}

/// The title and body in `file`: its first line is the title, its
/// leading `#`s left off, and the rest is the body
///
/// `None` when it has no title, which GitHub would refuse.
pub(super) fn parse(file: &str) -> Option<Text> {
    let file = file.trim_start();
    let (first, rest) = file.split_once('\n').unwrap_or((file, ""));
    let title = first.trim_start_matches('#').trim();
    (!title.is_empty()).then(|| Text {
        title: title.to_owned(),
        body: rest.trim().to_owned(),
    })
}

impl Runner {
    /// Opens a draft pull request from `branch` with the text the worker left
    /// in its build folder, and returns its number
    ///
    /// None when there is no file, or when the forge refused, which is told
    /// in the log: the turn is then judged as one that opened no pull request.
    pub(super) fn open_handed_over(&self, build: &Path, branch: &str) -> Option<u64> {
        let file = self.read_handed_over(build)?;
        let text = parse(&file).or_else(|| {
            self.notes.push(format!(
                "{} has no title on its first line, so no pull request is opened from it",
                path(build).display()
            ));
            None
        })?;
        match (self.ports.forge).create_pull_request(
            &self.remote,
            branch,
            BASE,
            &text.title,
            &text.body,
        ) {
            Ok(number) => {
                self.keep_applied(build, &file);
                Some(number)
            }
            Err(e) => {
                self.notes
                    .push(format!("cannot open a pull request from {branch}: {e}"));
                None
            }
        }
    }

    /// Sets pull request `number`'s title and body to what the worker's file
    /// says now, when a turn changed it since kelpie last did
    pub(super) fn edit_handed_over(&self, build: &Path, number: u64) {
        let Some(file) = self.read_handed_over(build) else {
            return;
        };
        if fs::read_to_string(build.join(APPLIED)).is_ok_and(|applied| applied == file) {
            return;
        }
        let Some(text) = parse(&file) else {
            return;
        };
        match (self.ports.forge).edit_pull_request(&self.remote, number, &text.title, &text.body) {
            Ok(()) => self.keep_applied(build, &file),
            Err(e) => self
                .notes
                .push(format!("cannot edit pull request #{number}: {e}")),
        }
    }

    // The file, or none when the worker left none. One that cannot be read
    // is told.
    fn read_handed_over(&self, build: &Path) -> Option<String> {
        match fs::read_to_string(path(build)) {
            Ok(file) => Some(file),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => {
                self.notes.push(format!(
                    "cannot read {}: {}",
                    path(build).display(),
                    e.kind()
                ));
                None
            }
        }
    }

    fn keep_applied(&self, build: &Path, file: &str) {
        if let Err(e) = fs::write(build.join(APPLIED), file) {
            self.notes.push(format!(
                "cannot write {}: {}",
                build.join(APPLIED).display(),
                e.kind()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::ApiError;
    use crate::ports::{Cost, Forge, Usage};
    use crate::runner::step;
    use crate::settings::ForgeSlug;
    use crate::test::{Asked, Rig, Scripted};

    fn usage() -> Usage {
        Usage {
            input: 1,
            cache_write: 0,
            cache_write_5m: 0,
            cache_read: 0,
            output: 1,
        }
    }

    // A running project with issue 7 in flight and its build folder made
    fn with_issue_7(app: bool) -> (Rig, std::sync::Mutex<Runner>) {
        let rig = Rig::new("shep");
        if app {
            rig.with_app();
        }
        let runner = rig.open().unwrap();
        assert_eq!(rig.ask(&runner, "add", Some("7"))["work_item"]["issue"], 7);
        std::fs::create_dir_all(rig.build_7()).unwrap();
        (rig, runner)
    }

    // The env and fence the call gets are pinned in `profile`; here the
    // runner is shown handing them to the worker's call, with a token minted
    // for that call and asked for with the permission to push alone.
    #[test]
    fn on_a_repo_the_app_covers_a_workers_call_holds_a_token_that_pushes_and_nothing_more() {
        let (rig, runner) = with_issue_7(true);
        rig.claude.script([Scripted::Reply(usage(), Cost(1))]);

        step(&runner).unwrap();

        let [seen] = rig.claude.seen().try_into().unwrap();
        let fence = seen
            .call
            .reach
            .fence
            .as_deref()
            .expect("a worker is fenced");
        let minted = rig
            .github
            .asked()
            .into_iter()
            .rev()
            .find_map(|asked| match asked {
                Asked::AccessToken(_, body) => Some(body),
                _ => None,
            });
        assert_eq!(
            minted.as_deref(),
            Some(r#"{"permissions":{"contents":"write"},"repositories":["shep"]}"#)
        );
        let login = std::fs::read_to_string(rig.build_7().join("gh/hosts.yml")).unwrap();
        assert!(login.contains("oauth_token: ghs_test"), "{login}");
        assert_eq!(
            fence.env["GH_CONFIG_DIR"],
            rig.build_7().join("gh").display().to_string()
        );
        assert!(fence.no_read.contains(&"~/.config/gh/**".to_owned()));
        let instructions = std::fs::read_to_string(seen.call.instructions.unwrap()).unwrap();
        assert!(instructions.contains("pull-request.md"), "{instructions}");
        assert!(!instructions.contains("gh pr create"), "{instructions}");
    }

    #[test]
    fn with_no_app_a_worker_opens_its_own_pull_request_as_the_maintainer() {
        let (rig, runner) = with_issue_7(false);
        rig.claude.script([Scripted::Reply(usage(), Cost(1))]);

        step(&runner).unwrap();

        let [seen] = rig.claude.seen().try_into().unwrap();
        let fence = seen
            .call
            .reach
            .fence
            .as_deref()
            .expect("a worker is fenced");
        assert!(!fence.env.contains_key("GH_CONFIG_DIR"));
        assert!(!rig.build_7().join("gh").exists());
        let instructions = std::fs::read_to_string(seen.call.instructions.unwrap()).unwrap();
        assert!(instructions.contains("gh pr create"), "{instructions}");
    }

    // An App that is set but cannot mint a token now still covers the repo,
    // so the call does not start rather than run as the maintainer.
    #[test]
    fn a_worker_waits_for_its_token_rather_than_run_as_the_maintainer() {
        let (rig, runner) = with_issue_7(true);
        rig.claude.script([Scripted::Reply(usage(), Cost(1))]);
        rig.github.fail_next(ApiError::Refused(502));

        step(&runner).unwrap();

        assert!(rig.claude.seen().is_empty(), "the call started");
        // Held back, not parked: no ruling asks the maintainer, and a pause
        // later the turn starts on a token.
        let status = rig.ask(&runner, "status", None);
        assert_ne!(status["work_item"]["phase"]["state"], "ruling", "{status}");
        step(&runner).unwrap();
        assert!(rig.claude.seen().is_empty(), "asked again before the pause");
        rig.clock.advance(31);
        step(&runner).unwrap();
        assert_eq!(rig.claude.seen().len(), 1, "the turn never started");
    }

    // The sandbox lets a worker ask the keychain `gh` keeps the maintainer's
    // login in, so a covered repo's worker does not start while it is there.
    #[test]
    fn no_covered_worker_starts_while_gh_keeps_the_maintainers_login_in_the_keychain() {
        let (rig, runner) = with_issue_7(true);
        rig.claude.script([Scripted::Reply(usage(), Cost(1))]);
        rig.github.keychain_holds_login(true);

        step(&runner).unwrap();

        assert!(rig.claude.seen().is_empty(), "the call started");
        assert!(!rig.build_7().join("gh").exists(), "a login was written");
    }

    #[test]
    fn a_turn_that_pushed_with_a_file_left_and_no_pull_request_has_one_opened_for_it() {
        let (rig, runner) = with_issue_7(true);
        let file = path(&rig.build_7());
        std::fs::write(
            &file,
            "feat: x

Why.

Resolves #7
",
        )
        .unwrap();
        rig.claude.script([Scripted::Push(
            "work.txt", "work
",
        )]);

        step(&runner).unwrap();

        let status = rig.ask(&runner, "status", None);
        assert_eq!(status["work_item"]["pull_request"], 500);
        assert_eq!(
            rig.forge.text_of(500),
            Some((
                "feat: x".to_owned(),
                "Why.

Resolves #7"
                    .to_owned()
            ))
        );
        assert!(
            rig.forge.draft(500),
            "kelpie marks it ready when its gates pass"
        );
    }

    // With no App the worker opens its own pull request, as the maintainer's
    // login, and a file it left is none of kelpie's to act on.
    #[test]
    fn a_file_left_by_a_worker_the_app_does_not_cover_opens_nothing() {
        let (rig, runner) = with_issue_7(false);
        std::fs::write(path(&rig.build_7()), "feat: x\n\nResolves #7\n").unwrap();
        rig.claude.script([Scripted::Push("work.txt", "work\n")]);

        step(&runner).unwrap();

        assert_eq!(rig.forge.text_of(500), None);
        let status = rig.ask(&runner, "status", None);
        assert_eq!(status["work_item"]["pull_request"], serde_json::Value::Null);
    }

    #[test]
    fn a_turn_that_left_no_file_has_no_pull_request_opened() {
        let (rig, runner) = with_issue_7(true);
        rig.claude.script([Scripted::Push(
            "work.txt", "work
",
        )]);

        step(&runner).unwrap();

        assert_eq!(rig.forge.text_of(500), None);
    }

    // A later turn that changes the file changes the pull request, and one
    // that leaves it alone does not undo an edit made on GitHub meanwhile.
    #[test]
    fn the_title_and_body_follow_the_file_only_when_a_turn_changed_it() {
        let (rig, runner) = with_issue_7(true);
        let (build, runner) = (rig.build_7(), runner.lock().unwrap());
        rig.forge.open_pull_request(500, "kelpie/7", &[7]);
        let edit = |title: &str| {
            std::fs::write(
                path(&build),
                format!(
                    "{title}

body
"
                ),
            )
            .unwrap();
            runner.edit_handed_over(&build, 500);
        };

        edit("feat: first");
        assert_eq!(rig.forge.text_of(500).unwrap().0, "feat: first");

        let remote = ForgeSlug::try_from(Rig::REMOTE.to_owned()).unwrap();
        let theirs = ("feat: theirs", "edited on GitHub");
        rig.forge
            .edit_pull_request(&remote, 500, theirs.0, theirs.1)
            .unwrap();
        runner.edit_handed_over(&build, 500);
        assert_eq!(rig.forge.text_of(500).unwrap().0, "feat: theirs");

        edit("feat: second");
        assert_eq!(rig.forge.text_of(500).unwrap().0, "feat: second");
    }

    #[test]
    fn the_first_line_is_the_title_and_the_rest_the_body() {
        let text = parse("feat(guard): scope the token\n\nWhy.\n\nResolves #420\n").unwrap();

        assert_eq!(
            text,
            Text {
                title: "feat(guard): scope the token".to_owned(),
                body: "Why.\n\nResolves #420".to_owned(),
            }
        );
    }

    #[test]
    fn a_heading_mark_and_leading_blank_lines_are_not_part_of_the_title() {
        let text = parse("\n\n# fix: a thing\nbody").unwrap();

        assert_eq!(text.title, "fix: a thing");
        assert_eq!(text.body, "body");
    }

    #[test]
    fn a_file_with_no_title_opens_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("  \n\n##\nbody"), None);
    }
}
