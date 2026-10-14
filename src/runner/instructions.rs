//! What a worker is told beyond kelpie's own instructions
//!
//! Kelpie's instructions are the same for every project. Three things vary:
//! the repo's pull request template, found in the worker's worktree, the
//! body of the work item's agent file, and the project's own file of extra
//! instructions, read once when the runner starts.

use std::fs;
use std::path::Path;

use super::owner_template::OwnerTemplate;
use crate::profile;
use crate::settings::NonBlank;
use crate::settings::{E2e, Settings, SettingsError};
use crate::skills::{Skills, Step};

/// Reads the project's extra worker instructions, if its settings name a file
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming `worker.instructions_file` and the file
/// when it cannot be read.
pub(super) fn read_extra(settings: &Settings) -> Result<Option<String>, SettingsError> {
    let Some(file) = &settings.worker.instructions_file else {
        return Ok(None);
    };
    fs::read_to_string(file)
        .map(Some)
        .map_err(|e| SettingsError::Invalid {
            setting: "worker.instructions_file",
            reason: format!("cannot read {}: {}", file.display(), e.kind()),
        })
}

/// What a worker is told after kelpie's own instructions
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Extra<'a> {
    /// The body of the work item's agent file
    pub(super) agent: Option<&'a str>,
    /// The project's file of extra instructions
    pub(super) project: Option<&'a str>,
    /// Where the worker writes its pull request's text, when kelpie opens
    /// the pull request for it
    pub(super) pull_request_file: Option<&'a Path>,
    /// The project's e2e host, when it names one
    pub(super) e2e: Option<&'a E2e>,
    /// The template of the repo's owner's `.github` repository, which the
    /// worktree's own template outranks
    pub(super) owner_template: Option<&'a OwnerTemplate>,
    /// The hosts of the project's browser, when the turn is given one
    pub(super) browser: Option<&'a [NonBlank]>,
}

/// The command that runs the project's end-to-end tests, for the prompt of a worker told of it
pub(super) fn e2e_command(kelpie: &Path) -> String {
    format!("`{} lease e2e`", kelpie.display())
}

/// The instructions file for one worker turn
///
/// Kelpie's own, naming the `kelpie` binary and the deferred and disputed findings files, then the skills the worker
/// writes tests and its pull request's body with, then the agent's
/// instructions, then the project's. A pull request template in the
/// worktree stands in for the body's skill.
pub(super) fn compose(
    extra: Extra<'_>,
    worktree: &Path,
    skills: &Skills,
    kelpie: &Path,
    deferred_file: &Path,
    disputed_file: &Path,
) -> String {
    let mut text = profile::instructions(
        kelpie,
        extra.pull_request_file,
        deferred_file,
        disputed_file,
    );
    let mut lines = Vec::new();
    if let Some(tdd) = skills.command(Step::Tests) {
        lines.push(format!(
            "Write your tests the way the `{tdd}` skill says.\n"
        ));
    }
    let own = template(worktree);
    match (own, extra.owner_template, skills.command(Step::Pr)) {
        (Some(template), _, _) => lines.push(template_line(&template)),
        (None, Some(owners), _) => lines.push(owners.line()),
        (None, None, Some(pr)) => lines.push(format!(
            "Write your pull request's body with the `{pr}` skill, and still end it with \
             `Resolves #<issue>`.\n"
        )),
        (None, None, None) => {}
    }
    if !skills.off(Step::Retro) {
        lines.push(FRICTION.to_owned());
    }
    if let Some(e2e) = extra.e2e {
        lines.push(format!(
            "Your sandbox cannot run the project's end-to-end tests: run {} from your \
             worktree. It waits for the host, streams the output, and leaves the results in \
             `{}` of your worktree. If it exits without results, such as an exit of 75 or 78, \
             say so in your final message and report no e2e result.\n",
            e2e_command(kelpie),
            e2e.results.as_path().display()
        ));
    }
    if let Some(hosts) = extra.browser {
        lines.push(browser_line(hosts));
    }
    if !lines.is_empty() {
        text.push('\n');
        text.push_str(&lines.concat());
    }
    if let Some(rules) = skills.worker_rules() {
        text.push('\n');
        text.push_str(&rules);
    }
    let sections = [
        ("This agent's instructions", extra.agent),
        ("This project's instructions", extra.project),
    ];
    for (heading, section) in sections {
        let Some(section) = section.filter(|e| !e.trim().is_empty()) else {
            continue;
        };
        text.push_str(&format!("\n# {heading}\n\n"));
        text.push_str(section);
        if !section.ends_with('\n') {
            text.push('\n');
        }
    }
    text
}

// Told when the turn has a browser, which its sandbox could not start itself
fn browser_line(hosts: &[NonBlank]) -> String {
    let hosts = match hosts {
        [] => "no host, since the project lists none".to_owned(),
        _ => (hosts.iter())
            .map(|h| format!("`{}`", h.as_str()))
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!(
        "Your sandbox cannot start a browser, so kelpie runs one beside it: the \
         `browser_*` tools of the `playwright` MCP server open http and https pages on \
         {hosts}, and every other address fails. They save no file: leave out `filename`, \
         and a screenshot comes back in the tool's reply. Wait for a page in the \
         foreground, since a turn that ends on a background task is never woken.\n"
    )
}

// Told while the retro runs, so the project manager's batch retro has the
// worker's own account of what slowed it down
const FRICTION: &str = "If something slowed you down, add a `## Friction` section to your pull \
    request's body above the `Resolves #<issue>` line: a line or two on what cost you time or \
    tokens. Leave the section out when nothing did. The project manager's batch retro reads it.\n";

fn template_line(template: &Template) -> String {
    match template {
        Template::File(path) => format!(
            "The repo has a pull request template, `{path}`. Fill it in for your pull request's \
             body instead of writing your own, and still end the body with `Resolves #<issue>`.\n"
        ),
        Template::Folder(path) => format!(
            "The repo has pull request templates in `{path}`. Fill in the one that fits for your \
             pull request's body instead of writing your own, and still end the body with \
             `Resolves #<issue>`.\n"
        ),
    }
}

/// A pull request template, as a path from the worktree's root
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Template {
    File(String),
    Folder(String),
}

/// Where GitHub reads a template from, in the order it prefers: `.github/`, the
/// root, then `docs/`
pub(super) const PLACES: [&str; 3] = [".github", "", "docs"];

/// The template GitHub would offer for a pull request from this worktree
///
/// GitHub reads `pull_request_template.md` (or `.txt`) from `.github/`, the root or
/// `docs/`, in any case, and a `PULL_REQUEST_TEMPLATE` folder of several from
/// the same places.
pub(super) fn template(worktree: &Path) -> Option<Template> {
    for place in PLACES {
        let Ok(entries) = fs::read_dir(worktree.join(place)) else {
            continue;
        };
        let named = entries.filter_map(Result::ok).filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            Some((name, is_dir))
        });
        let found = pick(place, named);
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The template among the `entries` of folder `place` (empty for the root),
/// each a name and whether it is a folder; the first by path when several match
pub(super) fn pick<S: AsRef<str>>(
    place: &str,
    entries: impl Iterator<Item = (S, bool)>,
) -> Option<Template> {
    entries
        .filter_map(|(name, is_dir)| {
            let name = name.as_ref();
            let path = if place.is_empty() {
                name.to_owned()
            } else {
                format!("{place}/{name}")
            };
            match name.to_lowercase().as_str() {
                "pull_request_template.md" | "pull_request_template.txt" if !is_dir => {
                    Some(Template::File(path))
                }
                "pull_request_template" if is_dir => Some(Template::Folder(path)),
                _ => None,
            }
        })
        .min()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::ports::{Cost, Usage};
    use crate::runner::Runner;
    use crate::runner::step;
    use crate::settings::StepSkills;
    use crate::test::{Rig, Scripted};

    const EXTRA: &str = "Never use an em dash in public writing.\n";

    // Kelpie's own instructions, naming the binary the rig says kelpie is
    fn kelpies(rig: &Rig) -> String {
        let deferred = rig.build_7().join("deferred-findings.md");
        let disputed = rig.build_7().join("disputed-findings.md");
        profile::instructions(Path::new(Rig::KELPIE), None, &deferred, &disputed)
    }

    // The instructions file the worker's first turn was started with
    fn first_instructions(rig: &Rig, runner: &Mutex<Runner>) -> String {
        rig.ask(runner, "add", Some("7"));
        let usage = Usage {
            input: 1,
            cache_write: 10,
            cache_write_5m: 0,
            cache_read: 100,
            output: 1000,
        };
        rig.claude.script([Scripted::Reply(usage, Cost(1))]);
        step(runner).unwrap();
        let [seen] = rig.claude.seen().try_into().unwrap();
        let file = seen.call.instructions.expect("the worker has instructions");
        fs::read_to_string(file).unwrap()
    }

    #[test]
    fn a_repo_without_a_template_has_the_body_written_with_the_pr_skill() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        let lines = text.strip_prefix(&kelpies(&rig)).expect(&text);
        let (named, rules) = lines.split_once("\n\n").expect(lines);
        assert_eq!(
            named,
            "\nWrite your tests the way the `/mattpocock:tdd` skill says.\n\
             Write your pull request's body with the `/mattpocock:pr` skill, and still end \
             it with `Resolves #<issue>`."
        );
        assert!(
            rules.starts_with("Kelpie runs this skill headless."),
            "{rules}"
        );
        assert!(rules.contains("Run no /code-review"), "{rules}");
        assert!(rules.contains("leave it to CI"), "{rules}");
    }

    #[test]
    fn the_first_turn_names_the_deferred_findings_file_kelpie_reads_at_merge() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        let path = rig.build_7().join("deferred-findings.md");
        assert!(text.contains(&format!("`{}`", path.display())), "{text}");
        assert!(!text.contains("{deferred}"), "{text}");
    }

    #[test]
    fn the_first_turn_names_the_disputed_findings_file_the_next_reader_is_shown() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        let path = rig.build_7().join("disputed-findings.md");
        assert!(text.contains(&format!("`{}`", path.display())), "{text}");
        assert!(!text.contains("{disputed}"), "{text}");
    }

    #[test]
    fn the_worker_is_told_to_run_its_own_tests_under_the_lease_and_leave_the_suite_to_ci() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        assert!(
            text.contains("`/opt/kelpie/bin/kelpie lease run cargo-test -- cargo test --lib`"),
            "{text}"
        );
        assert!(!text.contains("{kelpie}"), "{text}");
        assert!(text.contains("leave the whole suite to CI"), "{text}");
    }

    #[test]
    fn the_worker_is_told_kelpie_owns_the_draft_and_ready_state() {
        const TOLD: [&str; 2] = [
            "Kelpie owns the pull request's draft and ready state",
            "Never change it and never ask about it.",
        ];
        let own = Rig::new("koji");
        let runner = own.open().unwrap();
        let text = first_instructions(&own, &runner);
        assert!(!text.contains("pull-request.md"), "{text}");
        assert!(TOLD.iter().all(|line| text.contains(line)), "{text}");

        let handed_over = Rig::new("koji");
        handed_over.with_app();
        let runner = handed_over.open().unwrap();
        let text = first_instructions(&handed_over, &runner);
        assert!(text.contains("pull-request.md"), "{text}");
        assert!(TOLD.iter().all(|line| text.contains(line)), "{text}");
    }

    #[test]
    fn the_worker_is_told_of_the_friction_section_only_while_the_retro_runs() {
        let off = Rig::new("koji");
        let runner = off.open().unwrap();
        let text = first_instructions(&off, &runner);
        assert!(!text.contains("## Friction"), "{text}");

        let on = Rig::new("koji");
        on.retro_on();
        let runner = on.open().unwrap();
        let text = first_instructions(&on, &runner);
        assert!(text.contains("add a `## Friction` section"), "{text}");
        assert!(
            text.contains("above the `Resolves #<issue>` line"),
            "{text}"
        );
    }

    #[test]
    fn a_repo_with_a_template_tells_the_worker_to_fill_it() {
        let rig = Rig::new("koji");
        rig.land_on_origin("PULL_REQUEST_TEMPLATE.md");
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        assert!(text.starts_with(&kelpies(&rig)), "{text}");
        assert!(
            text.contains("pull request template, `PULL_REQUEST_TEMPLATE.md`"),
            "{text}"
        );
        assert!(text.contains("`Resolves #<issue>`"), "{text}");
        assert!(!text.contains("/mattpocock:pr"), "{text}");
    }

    #[test]
    fn a_project_file_reaches_the_workers_first_turn() {
        let rig = Rig::new("koji");
        let file = rig.home.path().join("extra.md");
        fs::write(&file, EXTRA).unwrap();
        let line = format!(
            "build_env = {{}}\ninstructions_file = \"{}\"\n",
            file.display()
        );
        rig.edit_settings(|s| s.replace("build_env = {}\n", &line));
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        assert!(text.starts_with(&kelpies(&rig)), "{text}");
        assert!(text.ends_with(EXTRA), "{text}");
    }

    #[test]
    fn a_worker_is_told_of_the_e2e_command_only_when_one_is_configured() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let without = first_instructions(&rig, &runner);
        assert!(!without.contains("lease e2e"), "{without}");
        let first_build = rig.build_7().display().to_string();

        let rig = Rig::new("koji");
        rig.e2e_on();
        let runner = rig.open().unwrap();
        let with = first_instructions(&rig, &runner);
        let line = "Your sandbox cannot run the project's end-to-end tests: run \
                    `/opt/kelpie/bin/kelpie lease e2e` from your worktree. It waits for the \
                    host, streams the output, and leaves the results in `test-results` of \
                    your worktree. If it exits without results, such as an exit of 75 or 78, \
                    say so in your final message and report no e2e result.\n";
        assert!(with.contains(line), "{with}");
        // Only that line and the rig's build folder differ, and the line comes
        // after the skills' lines.
        let same_build = with.replace(&rig.build_7().display().to_string(), &first_build);
        assert_eq!(same_build.replace(line, ""), without);
        assert!(with.find("/mattpocock:pr").unwrap() < with.find(line).unwrap());
    }

    #[test]
    fn a_worker_with_a_browser_is_told_its_hosts_and_to_wait_in_the_foreground() {
        let dir = tempfile::tempdir().unwrap();
        let skills = Skills::load(&StepSkills::default(), &dir.path().join("skills"));
        let told = |browser: Option<&[NonBlank]>| {
            let extra = Extra {
                browser,
                ..Extra::default()
            };
            compose(
                extra,
                dir.path(),
                &skills,
                Path::new(Rig::KELPIE),
                Path::new("deferred-findings.md"),
                Path::new("disputed-findings.md"),
            )
        };
        let hosts = [
            NonBlank::try_from("app.example.com".to_owned()).unwrap(),
            NonBlank::try_from("*.cdn.example.com".to_owned()).unwrap(),
        ];
        let text = told(Some(&hosts));
        assert!(text.contains("`playwright` MCP server"), "{text}");
        assert!(
            text.contains("`app.example.com`, `*.cdn.example.com`"),
            "{text}"
        );
        assert!(text.contains("leave out `filename`"), "{text}");
        assert!(text.contains("in the foreground"), "{text}");
        assert!(told(Some(&[])).contains("no host, since the project lists none"));
        assert!(!told(None).contains("playwright"));
    }

    #[test]
    fn the_template_line_comes_before_the_projects_instructions() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pull_request_template.md"), "## Summary\n").unwrap();
        let skills = Skills::load(&StepSkills::default(), &dir.path().join("skills"));
        let extra = Extra {
            agent: Some("Mind the migrations."),
            project: Some(EXTRA),
            ..Extra::default()
        };
        let text = compose(
            extra,
            dir.path(),
            &skills,
            Path::new(Rig::KELPIE),
            Path::new("deferred-findings.md"),
            Path::new("disputed-findings.md"),
        );
        let at = |needle: &str| text.find(needle).expect(needle);
        assert!(at("/mattpocock:tdd") < at("pull request template"));
        assert!(at("pull request template") < at("# This agent's instructions"));
        assert!(
            text.contains("# This agent's instructions\n\nMind the migrations.\n"),
            "{text}"
        );
        assert!(at("Mind the migrations.") < at("# This project's instructions"));
        assert!(text.ends_with(EXTRA), "{text}");
        let blank = Extra {
            agent: Some("\n"),
            project: Some(" \n"),
            ..Extra::default()
        };
        let (blank, none) = (
            compose(
                blank,
                dir.path(),
                &skills,
                Path::new(Rig::KELPIE),
                Path::new("deferred-findings.md"),
                Path::new("disputed-findings.md"),
            ),
            compose(
                Extra::default(),
                dir.path(),
                &skills,
                Path::new(Rig::KELPIE),
                Path::new("deferred-findings.md"),
                Path::new("disputed-findings.md"),
            ),
        );
        assert_eq!(blank, none);
    }

    #[test]
    fn an_agent_files_body_reaches_its_workers_turn() {
        let rig = Rig::new("koji");
        rig.write_agent(
            "sonnet-high",
            "---\nrole: implementer\nharness: claude-code\nmodel: claude-sonnet-5-5\n\
             effort: high\n---\n\nKeep each commit small.\n",
        );
        let runner = rig.open().unwrap();
        let text = first_instructions(&rig, &runner);
        assert!(text.starts_with(&kelpies(&rig)), "{text}");
        assert!(
            text.ends_with("\n# This agent's instructions\n\nKeep each commit small.\n"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_instructions_file_stops_the_runner() {
        let rig = Rig::new("koji");
        let line = "build_env = {}\ninstructions_file = \"gone.md\"\n";
        rig.edit_settings(|s| s.replace("build_env = {}\n", line));
        let err = rig.open().unwrap_err().to_string();
        assert!(
            err.starts_with("setting `worker.instructions_file`: cannot read "),
            "{err}"
        );
        assert!(err.contains("kelpie/koji/gone.md"), "{err}");
    }

    #[test]
    fn github_reads_a_template_from_three_places_in_any_case() {
        for file in [
            "Pull_Request_Template.md",
            ".github/pull_request_template.md",
            "docs/PULL_REQUEST_TEMPLATE.txt",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "## Summary\n").unwrap();
            assert_eq!(template(dir.path()), Some(Template::File(file.into())));
        }
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".github/PULL_REQUEST_TEMPLATE")).unwrap();
        assert_eq!(
            template(dir.path()),
            Some(Template::Folder(".github/PULL_REQUEST_TEMPLATE".into()))
        );
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(template(empty.path()), None);
        let both = tempfile::tempdir().unwrap();
        fs::create_dir_all(both.path().join(".github")).unwrap();
        fs::write(both.path().join("pull_request_template.md"), "Root\n").unwrap();
        fs::write(
            both.path().join(".github/pull_request_template.md"),
            "Github\n",
        )
        .unwrap();
        assert_eq!(
            template(both.path()),
            Some(Template::File(".github/pull_request_template.md".into()))
        );
    }
}
