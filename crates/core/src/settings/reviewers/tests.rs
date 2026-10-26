use super::*;
use crate::agents::Agents;
use std::collections::BTreeMap;

use crate::settings::{Effort, EndpointUrl, Gateway, GatewayKind, GatewayName, KeyVar, Settings};

const EXAMPLE: &str = include_str!("../../../../../settings.example.toml");

/// The reviewers the example lists, commented out
const LISTED: &str = "# reviewers = [\"iq3-xxs-review\", \"defect-hunter-lite\"]\n";

const OPUS: &str = "---\nrole: reviewer\nharness: claude-code\nmodel: claude-opus-5-5\n\
                    effort: high\npaths: [\"src/runner/merge/**\"]\n---\nRead it for defects.\n";

const GPU_BOX: &str = "---\nrole: reviewer\nharness: endpoint\nurl: http://gpu-box:11434/v1\n\
                       model: coder\ncontext: 32768\n---\n";

// Kelpie's own agents with every baseline bound, a Claude reviewer on some paths and an endpoint.
fn book() -> Agents {
    bound(Agents::embedded())
        .with("opus", OPUS)
        .with("gpu-box", GPU_BOX)
}

// `agents` with each baseline bound to Opus 5.5 at high effort.
fn bound(agents: Agents) -> Agents {
    agents.bind_all("harness: claude-code\nmodel: claude-opus-5-5\neffort: high\n")
}

// The example's project, listing `list` when it is given, and its home.
fn project(list: Option<&str>) -> (Settings, tempfile::TempDir) {
    assert!(EXAMPLE.contains(LISTED), "the example's reviewers moved");
    let listed = list.map_or(String::new(), |list| format!("reviewers = {list}\n"));
    let entry = EXAMPLE.replace(LISTED, &listed);
    let home = tempfile::tempdir().unwrap();
    let table = crate::test::project_table(&entry);
    let settings = Settings::from_table(&table, "shep", home.path(), Path::new("/p")).unwrap();
    (settings, home)
}

fn names(lineup: &[ListedReviewer]) -> Vec<&str> {
    lineup.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn a_project_lists_its_reviewers_in_the_order_the_review_runs_them_each_once() {
    let (settings, home) = project(Some(r#"["opus", "qwen", "gpu-box", "opus"]"#));
    let lineup = settings
        .lineup(&book(), &Gateways::default(), home.path())
        .unwrap();
    assert_eq!(names(&lineup), ["opus", "qwen", "gpu-box"]);

    let opus = &lineup[0];
    let (model, _) = opus.runs.session().unwrap();
    assert_eq!(
        (model.model.as_str(), model.effort),
        ("claude-opus-5-5", Effort::High)
    );
    assert_eq!(opus.paths[0].as_str(), "src/runner/merge/**");
    assert_eq!(opus.prompt.as_deref(), Some("Read it for defects."));
    assert!(!opus.is_local());

    let Runs::Command(qwen) = &lineup[1].runs else {
        panic!("{:?}", lineup[1]);
    };
    assert_eq!(
        qwen.command,
        home.path().join(".claude/scripts/qwen-review.sh"),
        "`~/` is the home folder"
    );
    assert!(lineup[1].is_local() && lineup[2].is_local());
}

// Kelpie's settings holding one gateway, `name`, of `kind`.
fn gateways_with(name: &str, kind: GatewayKind) -> Gateways {
    let gateway = Gateway {
        kind,
        url: EndpointUrl::try_from("http://gpu-box:8700".to_owned()).unwrap(),
        key_env: KeyVar::try_from("PADDOCK_KEY".to_owned()).unwrap(),
    };
    let name = GatewayName::try_from(name.to_owned()).unwrap();
    Gateways::new(BTreeMap::from([(name, gateway)]))
}

#[test]
fn a_project_that_lists_none_runs_defect_hunter_lite_alone_without_paddock() {
    let (settings, home) = project(None);
    let lineup = settings
        .lineup(&book(), &Gateways::default(), home.path())
        .unwrap();
    assert_eq!(names(&lineup), ["defect-hunter-lite"]);
    assert!(!lineup[0].second_look);
    assert!(lineup[0].prompt.is_some());

    let (empty, home) = project(Some("[]"));
    let none = empty.lineup(&book(), &Gateways::default(), home.path());
    assert!(none.unwrap().is_empty());
}

#[test]
fn a_project_that_lists_none_runs_iq3_xxs_review_then_defect_hunter_lite_with_paddock() {
    let (settings, home) = project(None);
    let paddock = gateways_with("paddock", GatewayKind::Paddock);
    let lineup = settings.lineup(&book(), &paddock, home.path()).unwrap();
    assert_eq!(names(&lineup), ["iq3-xxs-review", "defect-hunter-lite"]);
    assert!(lineup[0].is_local() && !lineup[1].is_local());
    let (gateway, model) = lineup[0].runs.gateway().unwrap();
    assert_eq!((gateway.as_str(), model), ("paddock", "iq3_xxs"));
}

#[test]
fn the_qwen_script_no_longer_changes_the_default_lineup() {
    let (settings, home) = project(None);
    let script = home.path().join(".claude/scripts/qwen-review.sh");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let lineup = settings
        .lineup(&book(), &Gateways::default(), home.path())
        .unwrap();
    assert_eq!(names(&lineup), ["defect-hunter-lite"]);
}

#[test]
fn only_a_gateway_named_paddock_adds_iq3_xxs_review() {
    let (settings, home) = project(None);
    let gateways = gateways_with("gpu", GatewayKind::Paddock);
    let lineup = settings.lineup(&book(), &gateways, home.path()).unwrap();
    assert_eq!(names(&lineup), ["defect-hunter-lite"]);
}

#[test]
fn a_reviewer_with_no_file_or_an_implementers_is_refused_naming_it() {
    let (settings, home) = project(Some(r#"["fable"]"#));
    let err = settings
        .lineup(&book(), &Gateways::default(), home.path())
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "setting `agents`: `agents.reviewers` names fable, which has no agent file: \
         write `agents/fable.md` in kelpie's home"
    );
    let (settings, home) = project(Some(r#"["sonnet-high"]"#));
    let err = settings
        .lineup(&book(), &Gateways::default(), home.path())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(
            "`agents.reviewers` names sonnet-high, whose agent file's role is `implementer`"
        ),
        "{err}"
    );
}

#[test]
fn a_commands_leading_tilde_is_the_home_folder_and_an_absolute_path_stays() {
    let command =
        |path: &str| format!("---\nrole: reviewer\nharness: command\ncommand: {path}\n---\n");
    let book = book()
        .with("mine", &command("~/bin/review"))
        .with("opt", &command("/opt/review"));
    let (settings, home) = project(Some(r#"["mine", "opt"]"#));
    let lineup = settings
        .lineup(&book, &Gateways::default(), home.path())
        .unwrap();
    let paths: Vec<_> = (lineup.iter())
        .map(|r| match &r.runs {
            Runs::Command(c) => c.command.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        paths,
        [home.path().join("bin/review"), "/opt/review".into()]
    );
}

const CODEX_ON_READY: &str = "---\nrole: reviewer\nharness: bot\nbot: codex\nreviews: 10\n\
                              hours: 168\nreviews_on_ready: true\n---\n";

#[test]
fn a_bot_that_reviews_on_ready_is_refused_after_another_bot_and_kept_before_it() {
    let book = book().with("codex", CODEX_ON_READY);
    let (settings, home) = project(Some(r#"["defect-hunter", "coderabbit", "codex"]"#));
    let err = settings
        .lineup(&book, &Gateways::default(), home.path())
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "setting `agents.reviewers`: codex reviews a pull request when it leaves draft, \
         so list it before coderabbit: a bot listed before it marks the draft ready, \
         which draws Codex's own review outside its lease"
    );
    let (settings, home) = project(Some(r#"["codex", "defect-hunter", "coderabbit"]"#));
    let lineup = settings
        .lineup(&book, &Gateways::default(), home.path())
        .unwrap();
    assert_eq!(names(&lineup), ["codex", "defect-hunter", "coderabbit"]);
    let (settings, home) = project(Some(r#"["coderabbit", "codex"]"#));
    assert!(
        settings
            .lineup(&bound(Agents::embedded()), &Gateways::default(), home.path())
            .is_ok(),
        "kelpie's own Codex file does not review on ready"
    );
}
