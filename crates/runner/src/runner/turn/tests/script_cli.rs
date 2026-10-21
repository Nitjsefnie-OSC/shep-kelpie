//! A CLI written on the `Cli` trait runs a work item's turn in the runner loop

use std::sync::Arc;

use super::*;
use crate::test::ScriptCli;

// The worker's first prompt is `$1`, and the turn runs in the work item's worktree.
const SCRIPT: &str = "printf %s \"$1\" > prompt.txt; echo done";

#[test]
fn a_cli_on_the_trait_runs_a_work_item_turn_through_the_runner() {
    let rig = Rig::new("shep");
    let agents = ScriptCli(SCRIPT).adapter(rig.home.path());
    let runner = rig.open_agents(Arc::new(agents)).unwrap();
    rig.ask(&runner, "add", Some("7"));

    step(&runner).unwrap();

    let prompt = fs::read_to_string(rig.worktree_7().join("prompt.txt")).unwrap();
    assert!(prompt.contains("#7"), "{prompt}");
    let item = &rig.ask(&runner, "status", None)["work_item"];
    assert_eq!(item["phase"], json!({ "state": "implement" }));
}
