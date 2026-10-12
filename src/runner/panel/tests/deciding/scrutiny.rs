//! What only ever raises the panel: a question, and a diff that touches a path
//! on `rulings.full_panel_paths`, ask all four whatever the work item's type

use std::sync::Mutex;

use super::{PANEL, asked, decided_comment, leak, pm_says, ruling, script, voted};
use crate::ports::Role;
use crate::runner::{Runner, StepReport, step};
use crate::settings::Merging;
use crate::test::{Rig, Scripted};

const QUESTION: &str = "Done so far.\n\n<kelpie-question>\nWhich flag?\n- `--dry-run`\n\
                        - `--check`\n</kelpie-question>\n";

/// A worker asking a question on the turn that opens pull request 71, titled
/// `title`, in a project with a panel under `rulings`
fn asking(project: &str, title: &str, rulings: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.with_app();
    rig.maintainer("octocat");
    super::super::name_panel(&rig, true);
    let table = format!("\n[app.dogs.kelpie.rulings]\n{rulings}\n");
    rig.edit_settings(|s| format!("{s}{table}"));
    rig.forge.set_title(71, title);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Say(QUESTION)]);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Asked { id: 1, .. })
    ));
    (rig, runner)
}

#[test]
fn a_fix_items_question_asks_all_four() {
    let (rig, runner) = asking("q-fix", "fix(guard): a bug", PANEL);
    script(&rig, ["A", "A", "A", "A"]);
    // The project manager is woken for the question first, and then to vote.
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(leak(pm_says('A', false)))]);

    voted(&rig, &runner);

    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
    let said = decided_comment(&rig).expect("the full panel decided it");
    assert!(said.contains("(4 of 4)"), "{said}");
}

#[test]
fn a_docs_item_whose_diff_touches_a_full_panel_path_asks_all_four() {
    let rulings = format!("{PANEL}\nfull_panel_paths = [\"src/**\", \"work.*\"]");
    let (rig, runner) = ruling("p-docs", "docs: say more", Merging::Auto, &rulings);
    script(&rig, ["A", "A", "A", "A"]);

    voted(&rig, &runner);

    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
}

#[test]
fn a_diff_that_touches_no_full_panel_path_leaves_the_types_panel() {
    for (project, rulings) in [
        (
            "p-none",
            format!("{PANEL}\nfull_panel_paths = [\"src/**\"]"),
        ),
        ("p-empty", format!("{PANEL}\nfull_panel_paths = []")),
        ("p-absent", PANEL.to_owned()),
    ] {
        let (rig, runner) = ruling(project, "docs: say more", Merging::Auto, &rulings);
        script(&rig, ["B", "A", "B", "B"]);

        voted(&rig, &runner);

        for role in [Role::Advisor, Role::Researcher, Role::Worker] {
            assert_eq!(asked(&rig, role), 0, "{project}: {role:?}");
        }
        let said = decided_comment(&rig).expect("the project manager decided it");
        assert!(said.contains("(1 of 1)"), "{project}: {said}");
    }
}
