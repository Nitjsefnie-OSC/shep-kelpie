//! Whether the repo's CI runs on a draft pull request
//!
//! No reviewer reads a head until CI has passed on it (ADR 0008), and kelpie's
//! pull request is a draft until a review bot's round marks it ready. A repo
//! whose workflows skip drafts has no run for kelpie to wait for, so with
//! `ci.block` on the review would wait for a run that never comes.

use std::path::Path;

use super::Line;
use crate::settings::Settings;

/// The line for a project whose workflows all skip a draft pull request, or
/// none where `ci.block` is off, the repo has no workflow that runs on a pull
/// request, or one of them runs on a draft (a job without a draft condition,
/// or a push trigger, runs it)
pub(super) fn drafts(subject: String, settings: &Settings) -> Option<Line> {
    if !settings.ci.block {
        return None;
    }
    let folder = settings.git.checkout.join(".github/workflows");
    let skipping = skipping(&folder)?;
    let named = skipping
        .iter()
        .map(|file| format!("`{file}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Line::missing(
        subject,
        format!(
            "every workflow that runs on a pull request skips a draft ({named}), so no run \
             exists for kelpie to wait for while its pull request is a draft, and `ci.block` \
             would hold the review for a run that never comes"
        ),
        "let one workflow run on drafts by dropping its `github.event.pull_request.draft` \
         condition, or set `ci.block` to false in the project's `[app.dogs.kelpie.ci]` table",
    ))
}

// The workflow files in `folder` that run on a pull request, when every one
// of them skips a draft.
fn skipping(folder: &Path) -> Option<Vec<String>> {
    let mut on_pull_requests = Vec::new();
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if runs_on_pull_requests(&text) {
            on_pull_requests.push((name, skips_drafts(&text)));
        }
    }
    on_pull_requests.sort();
    let every_one_skips = !on_pull_requests.is_empty() && on_pull_requests.iter().all(|(_, s)| *s);
    every_one_skips.then(|| on_pull_requests.into_iter().map(|(name, _)| name).collect())
}

// Whether a workflow names a pull request event, apart from the context its
// own conditions read.
fn runs_on_pull_requests(text: &str) -> bool {
    without_comments(text)
        .replace("github.event.pull_request", "")
        .contains("pull_request")
}

// Whether a workflow runs on a draft pull request's branch by another
// trigger: a push runs it, whatever the draft conditions say, since a
// condition on the pull request is empty there.
fn runs_on_push(text: &str) -> bool {
    let text = without_comments(text);
    let triggers = text.split("\njobs:").next().unwrap_or("");
    let words = triggers.split(|c: char| !(c.is_alphanumeric() || c == '_'));
    words.into_iter().any(|word| word == "push")
}

// Whether every job of a workflow carries a condition that is false on a
// draft, in the forms GitHub's documentation shows, and no push runs it.
fn skips_drafts(text: &str) -> bool {
    if runs_on_push(text) {
        return false;
    }
    let jobs = jobs(&without_comments(text));
    !jobs.is_empty() && jobs.iter().all(|job| has_draft_condition(job))
}

// The text of each job under the workflow's `jobs:` key: from its key, at
// the indent of the first, to the next key at that indent.
fn jobs(text: &str) -> Vec<String> {
    let mut lines = text.lines().skip_while(|l| l.trim_end() != "jobs:");
    lines.next();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut jobs: Vec<String> = Vec::new();
    let mut at = None;
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let here = indent(line);
        if here == 0 {
            break;
        }
        if *at.get_or_insert(here) == here {
            jobs.push(String::new());
        }
        if let Some(job) = jobs.last_mut() {
            job.push_str(line);
            job.push('\n');
        }
    }
    jobs
}

fn has_draft_condition(text: &str) -> bool {
    const DRAFT: &str = "pull_request.draft";
    text.lines().any(|line| {
        let Some(at) = line.find(DRAFT) else {
            return false;
        };
        let (before, after) = (&line[..at], line[at + DRAFT.len()..].trim_start());
        let negated = before.trim_end().ends_with("!github.event.");
        let compared = after.starts_with("== false") || after.starts_with("!= true");
        negated || compared
    })
}

fn without_comments(text: &str) -> String {
    let lines = text.lines().map(|l| l.split('#').next().unwrap_or(""));
    lines.collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflows(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            crate::test::write_in(dir.path(), name, text);
        }
        dir
    }

    const SKIPS: &str = "on:\n  pull_request:\njobs:\n  test:\n    \
                         if: github.event.pull_request.draft == false\n    runs-on: ubuntu-latest\n";
    const RUNS: &str =
        "on:\n  pull_request:\n  push:\njobs:\n  lint:\n    runs-on: ubuntu-latest\n";

    #[test]
    fn a_repo_whose_every_pull_request_workflow_skips_drafts_is_named() {
        let dir = workflows(&[("ci.yml", SKIPS), ("lint.yaml", SKIPS)]);
        assert_eq!(
            skipping(dir.path()),
            Some(vec!["ci.yml".to_owned(), "lint.yaml".to_owned()])
        );
    }

    #[test]
    fn one_workflow_that_runs_on_drafts_is_enough() {
        let dir = workflows(&[("ci.yml", SKIPS), ("lint.yml", RUNS)]);
        assert_eq!(skipping(dir.path()), None);
    }

    #[test]
    fn the_forms_of_a_draft_condition_are_read() {
        for condition in [
            "if: github.event.pull_request.draft == false",
            "if: ${{ github.event.pull_request.draft != true }}",
            "if: ${{ !github.event.pull_request.draft }}",
            "if: '!github.event.pull_request.draft'",
        ] {
            let text = format!("on: pull_request\njobs:\n  t:\n    {condition}\n");
            assert!(skips_drafts(&text), "{condition}");
        }
        assert!(!skips_drafts(
            "on: pull_request\n# if: github.event.pull_request.draft == false\n"
        ));
        assert!(!skips_drafts(
            "on: pull_request\njobs:\n  t:\n    if: github.event.pull_request.draft == true\n"
        ));
    }

    #[test]
    fn a_workflow_skips_drafts_only_if_every_job_does() {
        let job = |name: &str, condition: &str| {
            format!("  {name}:\n{condition}    runs-on: ubuntu-latest\n")
        };
        let skip = "    if: github.event.pull_request.draft == false\n";
        let head = "on:\n  pull_request:\njobs:\n";
        let both = format!("{head}{}{}", job("a", skip), job("b", skip));
        let one = format!("{head}{}{}", job("a", skip), job("b", ""));
        assert!(skips_drafts(&both));
        assert!(!skips_drafts(&one), "the job with no condition runs");
    }

    #[test]
    fn a_workflow_that_a_push_runs_runs_on_a_drafts_branch() {
        for on in [
            "on: [push, pull_request]\n",
            "on:\n  push:\n  pull_request:\n",
        ] {
            let text =
                format!("{on}jobs:\n  t:\n    if: github.event.pull_request.draft == false\n");
            assert!(!skips_drafts(&text), "{on}");
        }
    }

    #[test]
    fn a_workflow_that_never_runs_on_a_pull_request_counts_for_nothing() {
        let push = "on: push\njobs:\n  t:\n    if: github.event.pull_request.draft == false\n";
        let dir = workflows(&[("release.yml", push)]);
        assert_eq!(skipping(dir.path()), None);
        let dir = workflows(&[("release.yml", push), ("ci.yml", SKIPS)]);
        assert_eq!(skipping(dir.path()), Some(vec!["ci.yml".to_owned()]));
    }

    #[test]
    fn no_workflows_folder_is_nothing_to_say() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(skipping(&dir.path().join("none")), None);
    }
}
