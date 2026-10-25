//! Whether the repo's CI runs on a pull request, and on a draft one
//!
//! No reviewer reads a head until CI has passed on it (ADR 0008), and kelpie's
//! pull request is a draft until a review bot's round marks it ready. A repo
//! whose workflows skip drafts has no run for kelpie to wait for, so with
//! `ci.block` on the review would wait for a run that never comes. A repo
//! with no workflow on a pull request has none to wait for at all.

use std::path::Path;

use super::Line;
use crate::settings::Settings;
use crate::workflows::{
    is_workflow_file, runs_on_a_pull_request, runs_on_pull_requests, skips_drafts,
};

/// The line for a project with `ci.block` on and workflows, none of which
/// runs on a pull request, or none where the repo has a workflow that does or
/// no workflows folder
pub(super) fn pull_requests(subject: String, settings: &Settings) -> Option<Line> {
    if !settings.ci.block {
        return None;
    }
    let folder = settings.git.checkout.join(".github/workflows");
    let texts = workflow_texts(&folder)?;
    if texts.iter().any(|text| runs_on_a_pull_request(text)) {
        return None;
    }
    Some(Line::missing(
        subject,
        "no workflow runs on a pull request, so no check ever appears on one and `ci.block`          would hold every work item for CI that never runs",
        "add a workflow that runs on `pull_request`, or set `ci.block` to false in the \
         project's `[app.dogs.kelpie.ci]` table",
    ))
}

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

// The name and text of each workflow file in `folder`, or none where the
// folder cannot be read.
fn workflow_files(folder: &Path) -> Option<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_workflow_file(&name) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            files.push((name, text));
        }
    }
    Some(files)
}

fn workflow_texts(folder: &Path) -> Option<Vec<String>> {
    Some(
        workflow_files(folder)?
            .into_iter()
            .map(|(_, text)| text)
            .collect(),
    )
}

// The workflow files in `folder` that run on a pull request, when every one
// of them skips a draft.
fn skipping(folder: &Path) -> Option<Vec<String>> {
    let mut on_pull_requests = Vec::new();
    for (name, text) in workflow_files(folder)? {
        if runs_on_pull_requests(&text) {
            on_pull_requests.push((name, skips_drafts(&text)));
        }
    }
    on_pull_requests.sort();
    let every_one_skips = !on_pull_requests.is_empty() && on_pull_requests.iter().all(|(_, s)| *s);
    every_one_skips.then(|| on_pull_requests.into_iter().map(|(name, _)| name).collect())
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
