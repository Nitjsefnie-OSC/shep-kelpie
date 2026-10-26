use serde_json::{Value, json};

use std::sync::Arc;

use super::*;
use crate::agents::Agents;
use crate::local_tickets::{LocalTickets, Routed};
use crate::test::{Rig, Scripted, git, write_in, write_script};

mod paths;
mod touches;

const BODY: &str = "Build the thing.\n\n## Acceptance criteria\n\n- [ ] It works\n";

const REQUEST: &str = "Let the maintainer pause one project from lookout";

/// A rig whose project lists two implementers, and the issue writer it runs
struct Setup {
    rig: Rig,
    settings: Settings,
    paths: ProjectPaths,
    listed: RoleAgents,
    files: Vec<AgentName>,
    writer: Writer,
}

impl Setup {
    fn new() -> Self {
        Self::with(|_| {})
    }

    // The same, after `prepare` has set the rig up
    fn with(prepare: impl FnOnce(&Rig)) -> Self {
        let rig = Rig::new("acme");
        rig.implementers(&["sonnet-high", "opus-high"]);
        prepare(&rig);
        let settings = rig.settings();
        let paths = rig.paths();
        let agents = Agents::load(&paths.agents).unwrap();
        let listed = settings.role_agents(&agents).unwrap();
        let writer = Writer::of(&agents, &settings.agents.issue_writer).unwrap();
        Self {
            rig,
            settings,
            paths,
            listed,
            files: agents.implementer_files(),
            writer,
        }
    }

    fn project(&self) -> Project<'_> {
        Project {
            settings: &self.settings,
            remote: self
                .settings
                .git
                .remote
                .as_ref()
                .expect("the example sets it"),
            paths: &self.paths,
            agents: &self.listed,
            implementers: &self.files,
            kelpie: Path::new(Rig::KELPIE),
            home: self.rig.home.path(),
        }
    }

    // Runs the issue writer on its own, its session answering `reply`.
    fn headless(&self, reply: &'static str) -> Result<Vec<String>, String> {
        self.rig.claude.script([Scripted::Text(reply)]);
        // The rig's one forge stands behind the forge port and GitHub's tickets.
        let forge = &self.rig.forge;
        let tickets = Routed::new(
            Arc::new(forge.clone()),
            LocalTickets::new(self.paths.tickets()),
            self.settings.tickets.source,
        );
        headless(
            &self.project(),
            &self.writer,
            REQUEST,
            forge,
            &tickets,
            &self.rig.claude,
            &self.rig.git,
        )
    }

    // Issue `number` on the forge, as the issue writer would have filed it.
    fn filed(&self, number: u64, title: &str, body: &str, labels: &[&str]) {
        self.rig.forge.open_issue(number, title, body);
        for label in labels {
            self.rig.forge.label(number, label);
        }
    }
}

// The guard command in a Claude Code settings file's hooks.
fn guard_hook(settings: &Value) -> String {
    let hooks = settings["hooks"]["PreToolUse"].as_array().unwrap();
    let guard = hooks
        .iter()
        .find(|h| h["matcher"] == "Bash|Agent|Task")
        .unwrap();
    guard["hooks"][0]["command"].as_str().unwrap().to_owned()
}

#[test]
fn a_request_runs_one_session_that_files_and_kelpie_prints_what_it_filed() {
    let s = Setup::new();
    s.rig.forge.set_repo_labels(&[HUMAN]);
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.filed(42, "Its lookout button", BODY, &[HUMAN, "agent:opus-high"]);
    s.rig.forge.link_sub_issue(41, 42);

    let lines = s.headless("Filed both.\n{\"filed\": [41, 42], \"note\": \"Split in two.\"}");

    assert_eq!(
        lines.unwrap(),
        [
            "the issue writer filed 2 on shep-pm/shep, `ready-for-human` for you to read, then \
             label `ready-for-agent`:",
            "#41 Pause a project (agent:sonnet-high)",
            "#42 Its lookout button (agent:opus-high, sub-issue of #41)",
            "note: Split in two.",
        ]
    );
    // The repo lacked the agents' labels, so they were made before the session.
    assert_eq!(
        s.rig.forge.repo_labels_now(),
        [HUMAN, "agent:opus-high", "agent:sonnet-high"]
    );
    let seen = s.rig.claude.all_seen();
    let [seen] = seen.as_slice() else {
        panic!("one session, not {}", seen.len())
    };
    let call = &seen.call;
    assert_eq!(call.role, ports::Role::IssueWriter);
    assert_eq!(
        (call.model.as_str(), call.effort.as_str()),
        ("claude-opus-5-5", "medium")
    );
    assert!(matches!(call.session, Session::New(_)), "a fresh session");
    assert!(call.prompt.contains(REQUEST), "{}", call.prompt);
    assert!(
        call.cwd.starts_with(s.paths.issues()),
        "{}",
        call.cwd.display()
    );
    assert!(
        !call.cwd.exists(),
        "its checkout is removed once its reply is read"
    );
    // Read-only on the repo: it writes nothing, and its commands are its guard's.
    assert_eq!(seen.sandbox["filesystem"]["allowWrite"], json!([]));
    assert_eq!(
        seen.sandbox["network"]["allowedDomains"],
        json!(["github.com", "api.github.com"])
    );
    let deny = seen.settings["permissions"]["deny"].to_string();
    for tool in ["Edit", "Write", "Agent", "WebFetch"] {
        assert!(deny.contains(&format!("\"{tool}\"")), "{tool}: {deny}");
    }
    // On its own no one answers a prompt, so the guard alone holds its commands.
    assert_eq!(seen.settings["permissions"]["allow"], json!(["Bash"]));
    let scratch = call.settings.with_extension("tmp");
    let ledger = scratch.join("ledger.json");
    assert_guarded(&seen.settings, HUMAN, &ledger);
    assert_file_tools_read_only(&seen.settings, &call.cwd, &s.rig);
    // Its sandbox reads none of the secrets, the history or kelpie's homes, and
    // inside those only its checkout, the guard's binary and the installed
    // sources and docs of its dependencies and toolchains.
    let files = &seen.sandbox["filesystem"];
    let denied = files["denyRead"].to_string();
    for path in ["~/.ssh/**", "~/.zsh_history", "~/.bash_history"] {
        assert!(denied.contains(&format!("\"{path}\"")), "{path}: {denied}");
    }
    let kelpie_home = format!("\"{}/**\"", s.paths.kelpie_home.display());
    assert!(
        denied.contains(&kelpie_home)
            || denied.contains(&format!("\"{}/**\"", s.paths.shep_home.display()))
    );
    let mut read = vec![json!(call.cwd), json!(Rig::KELPIE)];
    read.extend(
        crate::dep_sources::DepSources::discover(
            s.rig.home.path(),
            &crate::dep_sources::ProcessEnv,
        )
        .folders()
        .iter()
        .map(|folder| json!(folder)),
    );
    assert_eq!(
        files["allowRead"],
        json!(read),
        "the checkout, the guard and the installed sources alone"
    );
    // The run is in the project's usage ledger.
    let lines = usage::read(&s.paths.folder.join(usage::FILE)).unwrap();
    let [usage::Line::Call(line)] = lines.as_slice() else {
        panic!("one call line, not {lines:?}")
    };
    let line = serde_json::to_value(line).unwrap();
    assert_eq!(
        (
            &line["role"],
            &line["kind"],
            &line["agent"],
            &line["harness"]
        ),
        (
            &json!("issue-writer"),
            &json!("issues"),
            &json!("issue-writer"),
            &json!("claude-code")
        )
    );
    assert_eq!(
        (
            &line["model"],
            &line["effort"],
            &line["ended"],
            &line["issue"]
        ),
        (
            &json!("claude-opus-5-5"),
            &json!("medium"),
            &json!("answered"),
            &json!(null)
        )
    );
    assert_eq!(line["session"], json!(call.session.id().0));
}

// The guard on every command, and the recorder of its ledger after each.
fn assert_guarded(settings: &Value, status: &str, ledger: &Path) {
    let guard = guard_hook(settings);
    let flags = [
        "guard".to_owned(),
        format!("--issues={status}"),
        "--agent=sonnet-high".to_owned(),
        "--agent=opus-high".to_owned(),
        format!("--ledger={}", ledger.display()),
    ];
    for flag in &flags {
        assert!(guard.contains(flag.as_str()), "{flag}: {guard}");
    }
    let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
    let record = post.iter().find(|h| h["matcher"] == "Bash").unwrap();
    let record = record["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(record, format!("{guard} --record"));
}

// Its file tools read the checkout, and no secret, history or folder beside it.
fn assert_file_tools_read_only(settings: &Value, checkout: &Path, rig: &Rig) {
    let deny: Vec<&str> = (settings["permissions"]["deny"].as_array().unwrap().iter())
        .filter_map(Value::as_str)
        .collect();
    for rule in [
        "Read(~/.config/gh/**)",
        "Read(~/.claude.json)",
        "Read(~/.claude/**)",
        "Read(~/.zsh_history)",
        "Read(~/.bash_history)",
    ] {
        assert!(deny.contains(&rule), "{rule}");
    }
    let rig_home = rig.home.path().canonicalize().unwrap();
    let beside = format!("Read(/{}/origin.git/**)", rig_home.display());
    assert!(deny.contains(&beside.as_str()), "{beside}");
    let checkout = checkout
        .canonicalize()
        .unwrap_or_else(|_| checkout.to_owned());
    let held = deny
        .iter()
        .filter_map(|r| r.strip_prefix("Read(/"))
        .find(|r| {
            let path = Path::new(r.trim_end_matches(')').trim_end_matches("/**"));
            checkout.starts_with(path)
        });
    assert_eq!(held, None, "the checkout stays readable");
}

#[test]
fn an_issue_without_criteria_or_one_listed_agent_is_named_with_what_it_lacks() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.filed(43, "Untidy", "Do it.\n", &[HUMAN]);
    s.filed(44, "Unlisted", BODY, &[HUMAN, "agent:haiku-low"]);
    // The board reads the prefix in any case, so this is a second agent.
    s.filed(
        46,
        "Twice",
        BODY,
        &[HUMAN, "agent:opus-high", "Agent:sonnet-high"],
    );
    s.rig.forge.remove_issue(45);

    let err = s.headless("{\"filed\": [41, 43, 44, 45, 46]}").unwrap_err();

    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines[1], "#41 Pause a project (agent:sonnet-high)");
    assert_eq!(
        lines[2],
        "#43 Untidy: no `agent:` label; no acceptance criteria"
    );
    assert!(
        lines[3].starts_with("#44 Unlisted: label `agent:haiku-low` names no implementer"),
        "{}",
        lines[3]
    );
    assert_eq!(
        lines[4],
        "#45: cannot read it back: gh failed: no issue #45"
    );
    assert_eq!(
        lines[5],
        "#46 Twice: the issue has more than one `agent:` label"
    );
    assert_eq!(lines.len(), 6, "{err}");
}

#[test]
fn on_a_local_project_it_files_local_tickets_and_kelpie_reads_them_back_from_the_store() {
    let s = Setup::with(Rig::tickets_local);
    let ticket = |args: &[&str]| s.rig.ticket_verb(args);
    ticket(&[
        "new",
        "Pause a project",
        "--label",
        HUMAN,
        "--label",
        "agent:sonnet-high",
        "--body",
        BODY,
    ]);
    ticket(&[
        "new",
        "Its lookout button",
        "--label",
        HUMAN,
        "--label",
        "agent:opus-high",
        "--parent",
        "k1",
        "--body",
        BODY,
    ]);
    ticket(&["new", "Untidy", "--label", HUMAN, "--body", "Do it."]);

    let err = s
        .headless("{\"filed\": [\"k1\", \"k2\", \"k3\"], \"note\": \"Split in two.\"}")
        .unwrap_err();

    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(
        lines[0],
        "the issue writer filed 3 on shep-pm/shep, `ready-for-human` for you to read, then \
         label `ready-for-agent`:"
    );
    assert_eq!(lines[1], "k1 Pause a project (agent:sonnet-high)");
    assert_eq!(
        lines[2],
        "k2 Its lookout button (agent:opus-high, sub-issue of k1)"
    );
    assert_eq!(
        lines[3],
        "k3 Untidy: no `agent:` label; no acceptance criteria"
    );
    assert_eq!(lines[4], "note: Split in two.");
    // A local store holds any label, so none was made on GitHub.
    assert_eq!(s.rig.forge.repo_labels_now(), Vec::<String>::new());
    // Its sandbox files into the store, which its guard names.
    let [seen] = s.rig.claude.all_seen().try_into().unwrap();
    let tickets = s.paths.tickets();
    assert_eq!(seen.sandbox["filesystem"]["allowWrite"], json!([tickets]));
    assert!(
        seen.sandbox["filesystem"]["allowRead"]
            .to_string()
            .contains(&tickets.display().to_string())
    );
    let guard = guard_hook(&seen.settings);
    assert!(guard.contains("--tickets=acme"), "{guard}");
    let prompt = instructions(&s.writer, &s.project(), Mode::Headless);
    assert!(prompt.contains("shep kelpie ticket new"), "{prompt}");
    assert!(!prompt.contains("gh issue create --title"), "{prompt}");
}

#[test]
fn a_reply_that_is_not_the_list_keeps_the_checkout_and_names_the_session() {
    let s = Setup::new();
    let main = s.rig.land_on_origin("landed.txt");

    let err = s.headless("I filed two issues for this.").unwrap_err();

    let seen = s.rig.claude.all_seen();
    let call = &seen[0].call;
    let id = &call.session.id().0;
    assert!(err.contains("ended without the list of issues"), "{err}");
    assert!(err.contains("I filed two issues for this."), "{err}");
    let resume = format!("`cd {} && claude --resume {id}`", call.cwd.display());
    assert!(err.contains(&resume), "{err}\nwanted: {resume}");
    // The checkout it read is `main` as just fetched, detached.
    assert_eq!(git(&call.cwd, &["rev-parse", "HEAD"]).trim(), main);
    let branch = std::process::Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(&call.cwd)
        .status()
        .unwrap();
    assert!(!branch.success(), "on no branch");
}

#[test]
fn the_prompt_lists_the_implementers_default_first_and_how_each_mode_ends() {
    let s = Setup::new();
    let headless = instructions(&s.writer, &s.project(), Mode::Headless);
    assert!(
        headless.starts_with(s.writer.prompt.trim_end()),
        "its file's body first"
    );
    let listed = "- `sonnet-high`: claude-sonnet-5-5 at high effort, the default\n\
                  - `opus-high`: claude-opus-5-5 at high effort\n";
    assert!(headless.contains(listed), "{headless}");
    assert!(headless.contains("--label 'ready-for-human'"), "{headless}");
    assert!(headless.contains("{\"filed\": ["), "{headless}");
    // It cannot grill, so what it could not settle it asks on the issue, in
    // the grilling skill's format with a recommended answer.
    for word in [
        "## Open questions",
        "❓ **Q1**",
        "➡️ <your recommended answer>",
    ] {
        assert!(headless.contains(word), "{word}: {headless}");
    }
    assert!(
        !headless.contains("`git"),
        "the guard runs no git: {headless}"
    );
    let interactive = instructions(&s.writer, &s.project(), Mode::Interactive);
    assert!(
        interactive.contains("--label 'ready-for-agent'"),
        "{interactive}"
    );
    assert!(
        !interactive.contains("{\"filed\""),
        "it talks to the maintainer"
    );
    assert!(!interactive.contains("Open questions"), "{interactive}");
    // The maintainer sees the agent labels before any goes on, and may take none.
    assert!(
        interactive.contains("preview the `agent:` label") && interactive.contains("apply none"),
        "{interactive}"
    );
    for word in [
        "`grill-with-docs`",
        "`GLOSSARY.md`",
        "`docs/adr/`",
        "only files you may write",
    ] {
        assert!(interactive.contains(word), "{word}: {interactive}");
    }
    // Facts are the repo's to give, and it has no sub-agent to fetch them.
    for prompt in [&headless, &interactive] {
        assert!(prompt.contains("Facts come from the repo"), "{prompt}");
        assert!(prompt.contains("Read, Grep and Glob"), "{prompt}");
    }
    for prompt in [&headless, &interactive] {
        for word in ["cost", "price", "cheap", "expensive", "budget"] {
            assert!(!prompt.to_lowercase().contains(word), "{word}");
        }
    }
}

// A `claude` that records how it was started and then writes `files`, each
// with its folder made, in the checkout it runs in.
fn fake_claude(dir: &Path, files: &[&str]) -> (PathBuf, PathBuf) {
    let (program, said) = (dir.join("claude"), dir.join("argv"));
    let mut script = format!(
        "#!/bin/sh\nprintf '%s\\0' \"$PWD\" \"$@\" > '{}'\n",
        said.display()
    );
    for file in files {
        script.push_str(&format!(
            "mkdir -p \"$(dirname '{file}')\"\nprintf 'written\\n' > '{file}'\n"
        ));
    }
    write_script(&program, &script);
    (program, said)
}

#[test]
fn interactive_starts_claude_on_grill_with_docs_in_a_docs_checkout_with_the_guard() {
    let s = Setup::new();
    let dir = tempfile::tempdir().unwrap();
    let (program, said) = fake_claude(dir.path(), &[]);

    let mut claude = interactive(&s.project(), &s.writer, REQUEST, program.as_os_str()).unwrap();
    assert!(claude.command.status().unwrap().success());

    let said = std::fs::read_to_string(&said).unwrap();
    let words: Vec<&str> = said.trim_end_matches('\0').split('\0').collect();
    // It runs in a checkout of its own, cut from main on a branch of its own,
    // not in the maintainer's.
    let docs = claude.docs.worktree().to_owned();
    assert_eq!(
        Path::new(words[0]).canonicalize().unwrap(),
        docs.canonicalize().unwrap()
    );
    assert!(docs.starts_with(s.paths.issues()), "{}", docs.display());
    let branch = claude.docs.branch();
    assert!(branch.starts_with("kelpie/docs-"), "{branch}");
    assert_eq!(git(&docs, &["branch", "--show-current"]).trim(), branch);
    let main = git(&s.settings.git.checkout, &["rev-parse", "origin/main"]);
    assert_eq!(git(&docs, &["rev-parse", "HEAD"]), main);
    let [settings, ledger] = claude.files.as_slice() else {
        panic!("its settings and its ledger, not {:?}", claude.files)
    };
    assert!(
        settings.starts_with(s.paths.issues()),
        "{}",
        settings.display()
    );
    let prompt = instructions(&s.writer, &s.project(), Mode::Interactive);
    let settings_arg = settings.display().to_string();
    let plugin_arg = claude.plugin.display().to_string();
    let first = format!("/mattpocock:grill-with-docs {REQUEST}");
    assert_eq!(
        &words[1..],
        [
            "--plugin-dir",
            plugin_arg.as_str(),
            "--model",
            "claude-opus-5-5",
            "--effort",
            "medium",
            "--settings",
            settings_arg.as_str(),
            "--append-system-prompt",
            prompt.as_str(),
            first.as_str(),
        ]
    );
    // The plugin holds the skill the first message runs, and the two it calls.
    for skill in ["grill-with-docs", "grilling", "domain-modeling"] {
        let found = claude.plugin.join(format!("skills/{skill}/SKILL.md"));
        assert!(found.is_file(), "{}", found.display());
    }
    let written: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    assert!(
        written.get("sandbox").is_none(),
        "the maintainer's own stays"
    );
    // The maintainer is asked before each command the guard lets through.
    assert!(written["permissions"].get("allow").is_none(), "{written}");
    assert_guarded(&written, READY, ledger);
    assert_file_tools_read_only(&written, &docs, &s.rig);
    assert_writes_docs_only(&written, &docs);
    // It reads the skills' own files, wherever the plugin is.
    assert_file_tools_read_only(&written, &claude.plugin, &s.rig);
    assert!(
        s.rig.claude.all_seen().is_empty(),
        "no session of kelpie's own"
    );
}

// Its file tools may write the glossary and the ADR folder in `docs`, and
// the notebook writer stays denied.
fn assert_writes_docs_only(settings: &Value, docs: &Path) {
    let deny: Vec<&str> = (settings["permissions"]["deny"].as_array().unwrap().iter())
        .filter_map(Value::as_str)
        .collect();
    for tool in ["Edit", "Write", "MultiEdit"] {
        assert!(!deny.contains(&tool), "{tool} is allowed to ask");
    }
    assert!(deny.contains(&"NotebookEdit"), "{deny:?}");
    assert!(deny.contains(&"Agent"), "no sub-agents: {deny:?}");
    let hooks = settings["hooks"]["PreToolUse"].as_array().unwrap();
    let confines: Vec<&str> = hooks
        .iter()
        .filter(|h| h["matcher"].as_str().is_some_and(|m| m.contains("Edit")))
        .map(|h| h["hooks"][0]["command"].as_str().unwrap())
        .collect();
    let root = docs.canonicalize().unwrap();
    let root = root.display();
    assert_eq!(
        confines,
        [format!(
            "'{}' 'confine' '{root}/GLOSSARY.md' '{root}/docs/adr'",
            Rig::KELPIE
        )]
    );
}

// The checkout of a `main` that has the project's glossary, and the git
// identity a commit of the maintainer's needs.
fn identified(s: &Setup) {
    let repo = &s.settings.git.checkout;
    git(repo, &["config", "user.name", "rig"]);
    git(repo, &["config", "user.email", "rig@example.invalid"]);
}

// Runs the interactive writer to its end, with the issues in `filed` in its
// ledger, and returns what kelpie said.
fn ran_to_the_end(
    s: &Setup,
    files: &[&str],
    filed: &[u64],
) -> (Interactive, Result<Vec<String>, String>) {
    identified(s);
    let dir = tempfile::tempdir().unwrap();
    let (program, _) = fake_claude(dir.path(), files);
    let mut claude = interactive(&s.project(), &s.writer, REQUEST, program.as_os_str()).unwrap();
    assert!(claude.command.status().unwrap().success());
    let filed = json!({ "filed": filed, "ids": {} });
    std::fs::write(&claude.files[1], filed.to_string()).unwrap();
    let said = claude.end(&s.project(), &s.rig.forge);
    (claude, said)
}

#[test]
fn what_it_writes_becomes_a_draft_pull_request_linked_from_the_issues_it_filed() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[READY, "agent:sonnet-high"]);
    s.filed(42, "Its lookout button", BODY, &[READY, "agent:opus-high"]);

    let (claude, said) = ran_to_the_end(&s, &["GLOSSARY.md", "docs/adr/0001-pause.md"], &[41, 42]);

    assert_eq!(
        said.unwrap(),
        [
            "opened draft pull request #500 on shep-pm/shep with GLOSSARY.md, \
             docs/adr/0001-pause.md",
            "linked it from #41",
            "linked it from #42",
        ]
    );
    let (title, body) = s.rig.forge.pull_request_text(500).unwrap();
    assert_eq!(
        title,
        format!("docs: record what planning \"{REQUEST}\" settled")
    );
    assert!(body.contains("- `GLOSSARY.md`"), "{body}");
    assert!(body.contains("- `docs/adr/0001-pause.md`"), "{body}");
    assert!(body.contains("Filed beside it: #41, #42"), "{body}");
    assert!(s.rig.forge.draft(500));
    let link = "The glossary terms and ADRs settled while planning this are in #500.";
    assert_eq!(
        s.rig.forge.comments(),
        [(41, link.to_owned()), (42, link.to_owned())]
    );
    // The branch is on origin, one commit past main, holding both files.
    let repo = &s.settings.git.checkout;
    let branch = claude.docs.branch();
    assert!(s.rig.forge.head_of(branch).is_some(), "{branch} is pushed");
    git(repo, &["fetch", "--quiet", "origin"]);
    let remote = format!("origin/{branch}");
    assert_eq!(
        git(repo, &["rev-parse", &format!("{remote}~1")]),
        git(repo, &["rev-parse", "origin/main"])
    );
    let changed = git(repo, &["diff", "--name-only", "origin/main", &remote]);
    assert_eq!(
        changed.lines().collect::<Vec<_>>(),
        ["GLOSSARY.md", "docs/adr/0001-pause.md"]
    );
    // Kelpie's checkout of it, its plugin and its files are gone.
    assert!(!claude.docs.worktree().exists());
    assert!(!claude.plugin.exists());
    assert!(claude.files.iter().all(|f| !f.exists()));
    assert_eq!(git(repo, &["branch", "--list", branch]), "");
}

#[test]
fn a_session_that_wrote_nothing_opens_no_pull_request_and_leaves_no_checkout() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[READY, "agent:sonnet-high"]);

    let (claude, said) = ran_to_the_end(&s, &[], &[41]);

    assert_eq!(said.unwrap(), Vec::<String>::new());
    assert!(s.rig.forge.comments().is_empty());
    assert_eq!(s.rig.forge.pull_request_text(500), None);
    assert!(!claude.docs.worktree().exists());
    let repo = &s.settings.git.checkout;
    assert_eq!(git(repo, &["branch", "--list", claude.docs.branch()]), "");
}

#[test]
fn a_file_besides_the_glossary_and_adrs_is_never_committed() {
    let s = Setup::new();

    let (claude, said) = ran_to_the_end(&s, &["GLOSSARY.md", "src/stray.rs"], &[]);

    let err = said.unwrap_err();
    assert!(err.contains("changed src/stray.rs"), "{err}");
    assert!(
        err.contains("may write only GLOSSARY.md and docs/adr/"),
        "{err}"
    );
    assert!(err.contains(claude.docs.branch()), "{err}");
    assert_eq!(s.rig.forge.pull_request_text(500), None);
    assert_eq!(s.rig.forge.head_of(claude.docs.branch()), None);
    // What it wrote is kept to look at.
    assert!(claude.docs.worktree().join("src/stray.rs").is_file());
}

#[test]
fn a_pull_request_that_fails_to_open_leaves_no_branch_on_the_remote_but_keeps_the_work() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[READY, "agent:sonnet-high"]);
    s.rig.forge.set_down(true);

    let (claude, said) = ran_to_the_end(&s, &["GLOSSARY.md"], &[41]);

    let err = said.unwrap_err();
    assert!(err.contains("cannot open a pull request"), "{err}");
    assert!(err.contains("removed it from the remote again"), "{err}");
    assert!(err.contains(claude.docs.branch()), "{err}");
    assert_eq!(s.rig.forge.head_of(claude.docs.branch()), None);
    // The commit is still in the local branch, in the checkout named.
    let repo = &s.settings.git.checkout;
    assert_ne!(git(repo, &["branch", "--list", claude.docs.branch()]), "");
    assert!(claude.docs.worktree().is_dir());
}

#[test]
fn a_pull_request_that_fails_to_open_removes_its_branch_past_a_failing_pre_push_hook() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[READY, "agent:sonnet-high"]);
    s.rig.forge.set_down(true);
    let common = git(
        &s.settings.git.checkout,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    let hook = Path::new(&common).join("hooks/pre-push");
    crate::test::write_script(&hook, "#!/bin/sh\nexit 1\n");

    let (claude, said) = ran_to_the_end(&s, &["GLOSSARY.md"], &[41]);

    let err = said.unwrap_err();
    assert!(err.contains("removed it from the remote again"), "{err}");
    assert_eq!(s.rig.forge.head_of(claude.docs.branch()), None);
}

#[test]
fn a_link_that_fails_is_named_beside_the_pull_request_that_opened() {
    let s = Setup::new();
    s.filed(41, "Pause a project", BODY, &[READY, "agent:sonnet-high"]);
    s.rig.forge.set_comments_down(true);

    let (_, said) = ran_to_the_end(&s, &["GLOSSARY.md"], &[41]);

    let err = said.unwrap_err();
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(
        lines[0],
        "opened draft pull request #500 on shep-pm/shep with GLOSSARY.md"
    );
    assert!(lines[1].starts_with("cannot link it from #41: "), "{err}");
    assert!(s.rig.forge.pull_request_text(500).is_some());
}

#[test]
fn a_reply_is_read_from_its_first_brace_to_its_last() {
    let fenced = "Done.\n```json\n{\"filed\": [7], \"note\": \"\"}\n```";
    assert_eq!(
        read_reply(fenced),
        Some(Filed {
            filed: vec![TicketId::from_issue(7).unwrap()],
            note: String::new()
        })
    );
    for not in ["Done.", "{\"issues\": [7]}", "{\"filed\": \"7\"}"] {
        assert_eq!(read_reply(not), None, "{not}");
    }
}

// Lands a marketplace `house` holding the plugin `style` on the default branch, the repo's
// settings turning it on, and the project's `repo_setup` roles
fn style_for(roles: &str) -> impl FnOnce(&Rig) + '_ {
    move |rig| {
        let market = r#"{"name":"house","plugins":[{"name":"style","source":"./style"}]}"#;
        rig.land("markets/house/.claude-plugin/marketplace.json", market);
        rig.land(
            "markets/house/style/skills/style/SKILL.md",
            "---\nname: style\n---\nRules.\n",
        );
        let settings = r#"{
            "extraKnownMarketplaces": {"house": {"source": {"source": "directory", "path": "markets/house"}}},
            "enabledPlugins": {"style@house": true}
        }"#;
        rig.land(".claude/settings.json", settings);
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\nroles = {roles}\n"));
    }
}

fn plugin_dirs_of_the_run(roles: &str) -> Vec<PathBuf> {
    let s = Setup::with(style_for(roles));
    s.rig.forge.set_repo_labels(&[HUMAN]);
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.headless("{\"filed\": [41]}").unwrap();
    let calls = s.rig.claude.all_calls();
    assert_eq!(calls.len(), 1);
    calls[0].plugin_dirs.clone()
}

#[test]
fn the_issue_writer_reads_the_installed_dependency_sources_and_no_credential_of_them() {
    let s = Setup::with(|rig| {
        write_in(
            &rig.home.path().join(".cargo"),
            "registry/src/serde-1.0.2/src/lib.rs",
            "x\n",
        );
    });
    s.rig.forge.set_repo_labels(&[HUMAN]);
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.headless("{\"filed\": [41]}").unwrap();
    let calls = s.rig.claude.all_calls();
    let reach = &calls[0].reach;
    let crate_src = s
        .rig
        .home
        .path()
        .join(".cargo/registry/src/serde-1.0.2/src/lib.rs");
    let fence = reach.fence.as_ref().expect("the writer's fence");
    for held in [reach.read.clone(), fence.read.clone()] {
        assert!(
            held.iter().any(|folder| crate_src.starts_with(folder)),
            "{held:?}"
        );
    }
    for secret in [
        "~/.cargo/credentials",
        "~/.cargo/credentials.toml",
        "~/.npmrc",
        "~/.netrc",
    ] {
        assert!(fence.no_read.iter().any(|r| r == secret), "{secret}");
    }
}

#[test]
fn the_repos_plugins_reach_the_issue_writer_when_its_role_is_listed() {
    let dirs = plugin_dirs_of_the_run("[\"issue-writer\"]");
    assert!(dirs.iter().any(|d| d.ends_with("repo/style")), "{dirs:?}");
}

#[test]
fn the_repos_plugins_do_not_reach_the_issue_writer_when_its_role_is_left_out() {
    assert!(plugin_dirs_of_the_run("[\"worker\"]").is_empty());
}

// The plugin dirs of a run whose default branch holds a skill of the repo's own, the
// issue writer running on `harness`
fn own_dirs_of_the_run(harness: crate::settings::AgentHarness) -> Vec<PathBuf> {
    let mut s = Setup::with(|rig| {
        rig.land(
            ".claude/skills/house/SKILL.md",
            "---\nname: house\n---\nRules.\n",
        );
    });
    s.writer.model.harness = harness;
    s.rig.forge.set_repo_labels(&[HUMAN]);
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.headless("{\"filed\": [41]}").unwrap();
    let calls = s.rig.claude.all_calls();
    assert_eq!(calls.len(), 1);
    calls[0].plugin_dirs.clone()
}

#[test]
fn a_claude_code_issue_writer_reads_the_repos_own_files_from_its_view_and_gets_no_plugin() {
    let dirs = own_dirs_of_the_run(crate::settings::AgentHarness::StandIn);
    assert!(dirs.is_empty(), "{dirs:?}");
}

#[test]
fn a_codex_issue_writer_gets_the_repos_own_files_as_a_plugin() {
    let dirs = own_dirs_of_the_run(crate::settings::AgentHarness::Codex);
    assert!(
        dirs.iter().any(|d| d.ends_with("repo/kelpie-repo")),
        "{dirs:?}"
    );
}

// The MCP servers the issue writer's call was given, for a run whose default branch holds the
// repo's `.mcp.json` and the plugin `style`, under `repo_setup`'s `roles`
fn servers_of_the_run(roles: &str) -> Vec<String> {
    let s = Setup::with(|rig| {
        style_for(roles)(rig);
        rig.land(".mcp.json", r#"{"mcpServers":{"docs":{"command":"docs"}}}"#);
        rig.land(
            "markets/house/style/.mcp.json",
            r#"{"mcpServers":{"search":{"command":"search"}}}"#,
        );
    });
    s.rig.forge.set_repo_labels(&[HUMAN]);
    s.filed(41, "Pause a project", BODY, &[HUMAN, "agent:sonnet-high"]);
    s.headless("{\"filed\": [41]}").unwrap();
    let calls = s.rig.claude.all_calls();
    assert_eq!(calls.len(), 1);
    calls[0].reach.mcp_servers.keys().cloned().collect()
}

#[test]
fn the_repos_mcp_servers_reach_the_issue_writer_when_its_role_is_listed() {
    assert_eq!(
        servers_of_the_run("[\"issue-writer\"]"),
        ["docs", "plugin_style_search"]
    );
}

#[test]
fn the_repos_mcp_servers_do_not_reach_the_issue_writer_when_its_role_is_left_out() {
    assert!(servers_of_the_run("[\"worker\"]").is_empty());
}
