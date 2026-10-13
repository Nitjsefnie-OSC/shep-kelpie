use super::*;

#[tokio::test]
async fn a_project_that_spends_codex_checks_codex_answers_its_usage() {
    let scene = Scene::new().await;
    let codex = "---\nrole: implementer\nharness: stand-in\nmodel: gpt-5-codex\n\
                 effort: medium\nusage: codex\n---\n";
    crate::test::write_in(&scene.kelpie_home.join("agents"), "codex.md", codex);
    scene.runs("golbat", |t| {
        t.insert(
            "agents".into(),
            json!({ "implementers": ["sonnet-high", "codex"] }),
        );
    });
    let report = scene.report().await;
    let found = ok(&report, "golbat: codex usage");
    assert_eq!(found, "reads Codex usage: 5-hour window 0%, week 0%");
    assert!(!subjects(&report).contains(&"koji: codex usage"));

    let fixes = [
        (
            "cannot run codex: No such file or directory",
            "install the Codex CLI",
        ),
        ("codex refused: 402 Payment Required", "codex login"),
    ];
    for (reason, fix) in fixes {
        scene.codex_meter.fail(MeterError::Codex(reason.into()));
        let (what, said) = missing(&scene.report().await, "golbat: codex usage");
        assert_eq!(what, reason);
        assert!(said.contains(fix), "{said}");
    }
    scene
        .codex_meter
        .fail(MeterError::Codex("codex did not answer in time".into()));
    let report = scene.report().await;
    assert!(matches!(
        verdict(&report, "golbat: codex usage"),
        Verdict::Unsure { .. }
    ));
}

// A pi agent on a server of its own, which no gateway fronts
const QWEN: &str = "---\nrole: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                    url: http://box:11434/v1\ncontext: 65536\n---\n";

impl Scene {
    /// Lists a pi agent for `golbat`, which `koji` does not run
    fn runs_pi_on_golbat(&self) {
        crate::test::write_in(&self.kelpie_home.join("agents"), "qwen.md", QWEN);
        self.runs("golbat", |t| {
            t.insert(
                "agents".into(),
                json!({ "implementers": ["sonnet-high", "qwen"] }),
            );
        });
    }
}

fn unsure(report: &Report, subject: &str) -> (String, String) {
    match verdict(report, subject) {
        Verdict::Unsure { what, next } => (what.clone(), next.clone()),
        other => panic!("`{subject}` is {other:?}"),
    }
}

#[tokio::test]
async fn a_project_with_a_pi_agent_gets_a_pi_line_that_is_ok_at_the_minimum_and_above() {
    let mut scene = Scene::new().await;
    scene.runs_pi_on_golbat();
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "golbat: pi"),
        "pi 1.0.4, kelpie needs 1.0.4 or later"
    );
    assert!(report.passed(), "{:#?}", report.render());

    scene.host.pi_version = Some(Ok("1.0.10"));
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "golbat: pi"),
        "pi 1.0.10, kelpie needs 1.0.4 or later"
    );
}

#[tokio::test]
async fn a_pi_older_than_the_minimum_is_missing_and_says_to_upgrade() {
    let mut scene = Scene::new().await;
    scene.runs_pi_on_golbat();
    scene.host.pi_version = Some(Ok("0.85.1"));
    let report = scene.report().await;
    let (what, fix) = missing(&report, "golbat: pi");
    assert_eq!(what, "pi 0.85.1 is older than the 1.0.4 kelpie needs");
    assert!(fix.contains("brew upgrade pi-coding-agent"), "{fix}");
    assert!(!report.passed());
}

#[tokio::test]
async fn a_pi_off_the_path_is_missing_and_says_to_install() {
    let mut scene = Scene::new().await;
    scene.runs_pi_on_golbat();
    scene.host.off_path = vec!["pi"];
    let report = scene.report().await;
    let (what, fix) = missing(&report, "golbat: pi");
    assert!(what.contains("not on your PATH"), "{what}");
    assert!(fix.contains("brew install pi-coding-agent"), "{fix}");
    assert!(!report.passed());
}

#[tokio::test]
async fn a_pi_whose_version_cannot_be_read_is_unsure_and_never_fails_the_run() {
    let mut scene = Scene::new().await;
    scene.runs_pi_on_golbat();
    let unreadable = [
        Err("pi --version did not answer in 10s"),
        Ok("1.0.4-beta.1"),
        Ok("pi coding agent"),
    ];
    for answer in unreadable {
        scene.host.pi_version = Some(answer);
        let report = scene.report().await;
        let (what, next) = unsure(&report, "golbat: pi");
        assert!(what.contains("pi --version"), "{what}");
        assert_eq!(next, "run `pi --version`");
        assert!(report.passed(), "{:#?}", report.render());
    }
}

#[tokio::test]
async fn a_project_with_no_pi_agent_gets_no_pi_line() {
    let mut scene = Scene::new().await;
    scene.host.off_path = vec!["pi"];
    scene.runs_pi_on_golbat();
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"koji: pi"));
    assert!(subjects(&report).contains(&"golbat: pi"));
    assert!(subjects(&report).iter().any(|s| s.starts_with("koji: ")));
}
