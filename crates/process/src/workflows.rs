//! Reading a GitHub workflow file for the events that run it
//!
//! No YAML parser: the triggers and the draft conditions are read as text, in
//! the forms GitHub's documentation shows.

/// Whether `name` is a workflow file's name
pub fn is_workflow_file(name: &str) -> bool {
    name.ends_with(".yml") || name.ends_with(".yaml")
}

/// Whether a workflow names a pull request event, apart from the context its
/// own conditions read
pub fn runs_on_pull_requests(text: &str) -> bool {
    without_comments(text)
        .replace("github.event.pull_request", "")
        .contains("pull_request")
}

/// Whether a workflow's checks show on a work branch's pull request: a pull
/// request event runs it, or a push that reaches a work branch does, `base`
/// being the branch the pull request is against
pub fn runs_on_a_pull_request(text: &str, base: &str) -> bool {
    runs_on_pull_requests(text) || runs_on_push(text, base)
}

/// Whether a push to a work branch runs a workflow, whatever its draft
/// conditions say, since a condition on the pull request is empty there
///
/// A `push` with `branches` runs on a work branch only when the list matches
/// more than `base`.
pub fn runs_on_push(text: &str, base: &str) -> bool {
    let text = without_comments(text);
    let triggers = text.split("\njobs:").next().unwrap_or("");
    let words = || triggers.split(|c: char| !(c.is_alphanumeric() || c == '_'));
    if !words().any(|word| word == "push") {
        return false;
    }
    match push_branches(triggers) {
        Some(branches) => branches.iter().any(|branch| branch != base),
        None => true,
    }
}

// The `branches` list under a `push:` key, when it has one.
fn push_branches(triggers: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = triggers.lines().collect();
    let at = lines.iter().position(|l| l.trim().starts_with("push:"))?;
    let base = indent(lines[at]);
    let inline = lines[at].trim().trim_start_matches("push:");
    let block = lines[at + 1..]
        .iter()
        .take_while(|l| l.trim().is_empty() || indent(l) > base);
    let mut branches = None;
    let mut listing = false;
    for line in std::iter::once(&inline).chain(block) {
        let trimmed = line.trim().trim_start_matches('{').trim_start();
        if let Some(rest) = trimmed.strip_prefix("branches:") {
            listing = rest.trim().is_empty();
            branches.get_or_insert_with(Vec::new).extend(entries(rest));
        } else if listing && trimmed.starts_with('-') {
            branches
                .get_or_insert_with(Vec::new)
                .extend(entries(trimmed));
        } else if !trimmed.is_empty() {
            listing = false;
        }
    }
    branches
}

// The names in a list, inline or as one `- name` line.
fn entries(text: &str) -> Vec<String> {
    let text = text.trim();
    let text = text.strip_prefix('-').unwrap_or(text);
    text.split(|c: char| c.is_whitespace() || "[]{},'\"".contains(c))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Whether a workflow does not run on a draft: its trigger never fires on one,
/// or every job carries a condition that is false on a draft, in the forms
/// GitHub's documentation shows. A push runs it whatever either says.
pub fn skips_drafts(text: &str, base: &str) -> bool {
    if runs_on_push(text, base) {
        return false;
    }
    if trigger_skips_drafts(text) {
        return true;
    }
    let jobs = jobs(&without_comments(text));
    !jobs.is_empty() && jobs.iter().all(|job| has_draft_condition(job))
}

// Whether the pull request event is limited to types that never fire on a
// draft: a `types` list under `pull_request` with none of the types a push to a
// draft raises (`opened`, `synchronize`, `reopened`), such as
// `[ready_for_review]`. The list is read inline or as a block of `- type` lines.
fn trigger_skips_drafts(text: &str) -> bool {
    let text = without_comments(text);
    let triggers = text.split("\njobs:").next().unwrap_or("");
    let lines: Vec<&str> = triggers.lines().collect();
    let Some(at) = lines.iter().position(|l| l.trim() == "pull_request:") else {
        return false;
    };
    let base = indent(lines[at]);
    let block = lines[at + 1..]
        .iter()
        .take_while(|l| l.trim().is_empty() || indent(l) > base);
    let mut types = Vec::new();
    let mut listing = false;
    for line in block {
        let trimmed = line.trim();
        if let Some(inline) = trimmed.strip_prefix("types:") {
            listing = inline.trim().is_empty();
            types.extend(words(inline));
        } else if listing && trimmed.starts_with('-') {
            types.extend(words(trimmed));
        } else if !trimmed.is_empty() {
            listing = false;
        }
    }
    let fires_on_a_draft = ["opened", "synchronize", "reopened"];
    !types.is_empty() && !types.iter().any(|t| fires_on_a_draft.contains(&t.as_str()))
}

fn words(text: &str) -> Vec<String> {
    let word = |c: char| !(c.is_alphanumeric() || c == '_');
    text.split(word)
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

// The text of each job under the workflow's `jobs:` key: from its key, at
// the indent of the first, to the next key at that indent.
fn jobs(text: &str) -> Vec<String> {
    let mut lines = text.lines().skip_while(|l| l.trim_end() != "jobs:");
    lines.next();
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

    #[test]
    fn a_push_limited_to_main_does_not_run_on_a_work_branch() {
        for on in [
            "on:\n  push:\n    branches: [main]\n",
            "on:\n  push:\n    branches:\n      - main\n",
            "on:\n  push:\n    branches: ['main']\n",
            "on:\n  push: { branches: [main] }\n",
            "on:\n  push:\n    branches: [main]\n  workflow_dispatch:\n",
        ] {
            let text = format!("{on}jobs:\n  t:\n    runs-on: ubuntu-latest\n");
            assert!(!runs_on_push(&text, "main"), "{on}");
            assert!(!runs_on_a_pull_request(&text, "main"), "{on}");
        }
    }

    #[test]
    fn a_push_that_matches_more_than_main_runs_on_a_work_branch() {
        for on in [
            "on: push\n",
            "on: [push, pull_request]\n",
            "on:\n  push:\n",
            "on:\n  push:\n    tags: ['v*']\n",
            "on:\n  push:\n    branches: [main, 'kelpie/**']\n",
            "on:\n  push:\n    branches:\n      - main\n      - 'release/*'\n",
            "on:\n  push:\n    branches: ['**']\n",
            "on:\n  push:\n    branches-ignore: [main]\n",
            "on:\n  push:\n    branches: [my-main]\n",
        ] {
            let text = format!("{on}jobs:\n  t:\n    runs-on: ubuntu-latest\n");
            assert!(runs_on_push(&text, "main"), "{on}");
        }
    }

    #[test]
    fn a_push_limited_to_the_base_does_not_run_on_a_work_branch_whatever_it_is_called() {
        let text = "on:\n  push:\n    branches: [develop]\njobs:\n  t:\n";
        assert!(!runs_on_push(text, "develop"));
        assert!(!runs_on_a_pull_request(text, "develop"));
        assert!(runs_on_push(text, "main"), "main is not this base");
    }

    #[test]
    fn a_pull_request_event_runs_on_a_pull_request_whatever_the_push_says() {
        let text = "on:\n  push:\n    branches: [main]\n  pull_request:\njobs:\n  t:\n";
        assert!(runs_on_a_pull_request(text, "main"));
        assert!(!runs_on_push(text, "main"));
    }

    #[test]
    fn a_comment_or_a_condition_is_not_a_trigger() {
        let text = "on:\n  push:\n    branches: [main]\n# pull_request:\njobs:\n  t:\n    \
                    if: github.event.pull_request.draft == false\n";
        assert!(!runs_on_a_pull_request(text, "main"));
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
            assert!(skips_drafts(&text, "main"), "{condition}");
        }
        assert!(!skips_drafts(
            "on: pull_request\n# if: github.event.pull_request.draft == false\n",
            "main"
        ));
        assert!(!skips_drafts(
            "on: pull_request\njobs:\n  t:\n    if: github.event.pull_request.draft == true\n",
            "main"
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
        assert!(skips_drafts(&both, "main"));
        assert!(
            !skips_drafts(&one, "main"),
            "the job with no condition runs"
        );
    }

    #[test]
    fn a_pull_request_trigger_that_never_fires_on_a_draft_skips_it() {
        let job = "jobs:\n  t:\n    runs-on: ubuntu-latest\n";
        for types in [
            "    types: [ready_for_review]\n",
            "    types:\n      - ready_for_review\n      - labeled\n",
        ] {
            let text = format!("on:\n  pull_request:\n{types}{job}");
            assert!(skips_drafts(&text, "main"), "{types}");
        }
        for types in [
            "    types: [opened, ready_for_review]\n",
            "    types:\n      - synchronize\n",
            "    branches: [main]\n",
        ] {
            let text = format!("on:\n  pull_request:\n{types}{job}");
            assert!(!skips_drafts(&text, "main"), "{types}");
        }
    }

    #[test]
    fn a_workflow_that_a_push_runs_runs_on_a_drafts_branch() {
        for on in [
            "on: [push, pull_request]\n",
            "on:\n  push:\n  pull_request:\n",
        ] {
            let text =
                format!("{on}jobs:\n  t:\n    if: github.event.pull_request.draft == false\n");
            assert!(!skips_drafts(&text, "main"), "{on}");
        }
    }
}
