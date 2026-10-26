use super::*;
use crate::settings::AgentName;

const UNBOUND_PM: &str = "---\nrole: pm\n---\nYou keep the project's notes.\n";

fn name(name: &str) -> AgentName {
    AgentName::try_from(name.to_owned()).unwrap()
}

// `shep kelpie agents <line>` on `folder`.
fn agents(folder: &Path, line: &str) -> Result<Vec<String>, String> {
    let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    run(folder, &args)
}

fn file(folder: &Path, name: &str) -> String {
    std::fs::read_to_string(folder.join(format!("{name}.md"))).unwrap()
}

fn files(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

const PM_ON_CLAUDE: &str = "new pm --harness claude-code --model claude-opus-5-5 --effort high";

#[test]
fn new_writes_a_file_that_extends_the_baseline_and_loads_with_its_prompt() {
    let dir = tempfile::tempdir().unwrap();

    let said = agents(dir.path(), PM_ON_CLAUDE).unwrap();

    assert_eq!(said, ["wrote pm.md"]);
    assert_eq!(
        file(dir.path(), "pm"),
        "---\nextends: kelpie:pm\nharness: claude-code\nmodel: claude-opus-5-5\neffort: high\n---\n"
    );
    let book = Agents::load(dir.path()).unwrap();
    let pm = book.get(&name("pm")).expect("the new file is in the book");
    let baseline = baseline::named("pm").unwrap();
    let shipped = baseline::check("pm", baseline.text).unwrap();
    assert_eq!(pm.prompt, shipped.prompt);
}

#[test]
fn name_gives_the_file_its_own_name_and_the_binding_flags_are_written() {
    let dir = tempfile::tempdir().unwrap();
    let line = "new reviewer --baseline spec --name spec-pi --harness pi --model qwen3.8:27b \
                --effort low --url http://box:11434/v1 --context 65536 --second-look";

    let said = agents(dir.path(), line).unwrap();

    assert_eq!(said, ["wrote spec-pi.md"]);
    assert_eq!(
        file(dir.path(), "spec-pi"),
        "---\nextends: kelpie:spec\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
         second_look: true\nurl: http://box:11434/v1\ncontext: 65536\n---\n"
    );
    assert!(
        Agents::load(dir.path())
            .unwrap()
            .get(&name("spec-pi"))
            .is_some()
    );
}

#[test]
fn an_implementer_needs_a_name_and_is_written_with_its_role_in_place_of_a_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let bare = "new implementer --harness claude-code --model claude-sonnet-5-5 --effort high";

    let err = agents(dir.path(), bare).unwrap_err();
    assert!(err.contains("--name"), "{err}");
    assert_eq!(files(dir.path()), Vec::<String>::new());

    agents(dir.path(), &format!("{bare} --name mine")).unwrap();
    assert_eq!(
        file(dir.path(), "mine"),
        "---\nrole: implementer\nharness: claude-code\nmodel: claude-sonnet-5-5\neffort: high\n---\n"
    );
    assert!(
        Agents::load(dir.path())
            .unwrap()
            .get(&name("mine"))
            .is_some()
    );
}

#[test]
fn new_never_writes_over_a_file() {
    let dir = tempfile::tempdir().unwrap();
    agents(dir.path(), PM_ON_CLAUDE).unwrap();
    let before = file(dir.path(), "pm");

    let err = agents(dir.path(), &PM_ON_CLAUDE.replace("high", "low")).unwrap_err();

    assert!(err.contains("`pm.md` already exists"), "{err}");
    assert_eq!(file(dir.path(), "pm"), before);
}

#[test]
fn new_writes_over_a_stub_that_is_unbound() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pm.md"), UNBOUND_PM).unwrap();
    assert_eq!(Agents::load(dir.path()).unwrap().unbound().len(), 1);

    let said = agents(dir.path(), PM_ON_CLAUDE).unwrap();

    assert_eq!(said, ["wrote pm.md"]);
    let book = Agents::load(dir.path()).unwrap();
    assert!(book.unbound().is_empty());
    assert!(book.get(&name("pm")).is_some());
}

#[test]
fn a_reviewer_with_no_baseline_is_refused_naming_the_four() {
    let dir = tempfile::tempdir().unwrap();
    let line = "new reviewer --harness claude-code --model claude-opus-5-5 --effort high";

    let err = agents(dir.path(), line).unwrap_err();

    assert!(
        err.contains("defect-hunter, standards, spec, security-reviewer"),
        "{err}"
    );
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_baseline_of_another_role_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let line = format!("{PM_ON_CLAUDE} --baseline spec");

    let err = agents(dir.path(), &line).unwrap_err();

    assert!(
        err.contains("`spec` is not a baseline of the pm role"),
        "{err}"
    );
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_binding_the_loader_refuses_writes_nothing_and_prints_its_message() {
    let dir = tempfile::tempdir().unwrap();
    let line = "new pm --harness pi --model qwen3.8:27b --effort low";

    let err = agents(dir.path(), line).unwrap_err();

    assert!(err.contains("pi"), "{err}");
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_gateway_and_a_url_together_are_refused_and_a_missing_flag_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let both = format!("{PM_ON_CLAUDE} --gateway paddock --url http://box/v1");
    assert!(agents(dir.path(), &both).unwrap_err().contains("--gateway"));
    let missing = agents(dir.path(), "new pm --harness claude-code --effort high").unwrap_err();
    assert!(missing.contains("--model"), "{missing}");
    let unknown = agents(dir.path(), &format!("{PM_ON_CLAUDE} --fast")).unwrap_err();
    assert!(unknown.contains("--fast"), "{unknown}");
    let role = agents(dir.path(), "new janitor --harness pi").unwrap_err();
    assert!(role.contains("janitor"), "{role}");
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn list_shows_bound_agents_unbound_files_and_baselines_with_no_file() {
    let dir = tempfile::tempdir().unwrap();
    agents(dir.path(), PM_ON_CLAUDE).unwrap();
    std::fs::write(dir.path().join("scribe.md"), UNBOUND_PM).unwrap();

    let lines = agents(dir.path(), "list").unwrap();

    let has = |line: &str| assert!(lines.iter().any(|l| l == line), "{line} in {lines:#?}");
    has("pm: pm, claude-code, claude-opus-5-5, high");
    has("sonnet-high: implementer, claude-code, claude-sonnet-5-5, high");
    has("scribe: pm, unbound: run `shep kelpie agents new pm --name scribe`");
    has("triager: baseline, no file: run `shep kelpie agents new triager`");
    has("spec: baseline, no file: run `shep kelpie agents new reviewer --baseline spec`");
    assert!(
        lines.iter().all(|l| !l.starts_with("pm: baseline")),
        "pm has a file: {lines:#?}"
    );
    let bound = lines.iter().position(|l| l.starts_with("pm: pm")).unwrap();
    let unbound = lines.iter().position(|l| l.starts_with("scribe:")).unwrap();
    let baseline = lines.iter().position(|l| l.starts_with("spec:")).unwrap();
    assert!(bound < unbound && unbound < baseline, "{lines:#?}");
}

#[test]
fn an_unknown_verb_or_none_is_refused_with_the_usage() {
    let dir = tempfile::tempdir().unwrap();
    for line in ["", "make pm", "list pm"] {
        let err = agents(dir.path(), line).unwrap_err();
        assert!(err.contains("shep kelpie agents list"), "{line}: {err}");
    }
}

#[test]
fn new_over_an_unbound_file_keeps_its_body_and_the_keys_it_does_not_bind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scribe.md"), UNBOUND_PM).unwrap();
    let line = format!("{PM_ON_CLAUDE} --name scribe");

    agents(dir.path(), &line).unwrap();

    let book = Agents::load(dir.path()).unwrap();
    let prompt = book.get(&name("scribe")).unwrap().prompt.clone();
    assert_eq!(prompt.as_deref(), Some("You keep the project's notes."));

    let mine = "---\n# Mine.\nrole: implementer\ntools: Bash,Read\n---\nExtra care.\n";
    std::fs::write(dir.path().join("mine.md"), mine).unwrap();
    let bare = "new implementer --harness claude-code --model claude-sonnet-5-5 --effort high";

    agents(dir.path(), &format!("{bare} --name mine")).unwrap();

    let written = file(dir.path(), "mine");
    assert!(written.contains("tools: Bash,Read\n"), "{written}");
    assert!(written.contains("# Mine.\n"), "{written}");
    assert!(written.ends_with("---\nExtra care.\n"), "{written}");
    assert!(
        Agents::load(dir.path())
            .unwrap()
            .get(&name("mine"))
            .is_some()
    );
}

#[test]
fn an_unbound_reviewer_with_no_baseline_is_bound_by_the_command_kelpie_prints() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mine.md"),
        "---\nrole: reviewer\n---\nRead it for defects.\n",
    )
    .unwrap();
    let book = Agents::load(dir.path()).unwrap();
    let command = book.unbound()[0].command();
    assert_eq!(command, "shep kelpie agents new reviewer --name mine");

    let said = agents(
        dir.path(),
        &format!(
            "{} --harness claude-code --model claude-opus-5-5 --effort high",
            command.strip_prefix("shep kelpie agents ").unwrap()
        ),
    )
    .unwrap();

    assert_eq!(said, ["wrote mine.md"]);
    let written = file(dir.path(), "mine");
    assert!(
        written.starts_with("---\nrole: reviewer\nharness: claude-code\n"),
        "{written}"
    );
    let book = Agents::load(dir.path()).unwrap();
    let prompt = book.get(&name("mine")).unwrap().prompt.clone();
    assert_eq!(prompt.as_deref(), Some("Read it for defects."));
}

#[test]
fn a_reviewer_with_no_file_still_needs_a_baseline_when_the_name_is_free() {
    let dir = tempfile::tempdir().unwrap();
    let line =
        "new reviewer --name mine --harness claude-code --model claude-opus-5-5 --effort high";

    let err = agents(dir.path(), line).unwrap_err();

    assert!(err.contains("--baseline"), "{err}");
    assert_eq!(files(dir.path()), Vec::<String>::new());
}
