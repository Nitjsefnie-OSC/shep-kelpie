// pi's `grep` and `find` download `rg` and `fd` when neither is in its `bin` or
// on `PATH`. A call's pi home has an empty `bin` and the sandbox refuses the
// download, so pi is told it is offline and reports the tool missing
// (`docs/facts/tool-errors.md`).

use super::tests::{URL, WORKER_ID, World, id};
use super::*;

#[test]
fn a_pi_call_runs_offline_so_pi_never_downloads_a_tool_of_its_own() {
    let w = World::new();
    for role in [Role::Worker, Role::Reviewer] {
        let call = w.call(URL, role, Session::New(id(WORKER_ID)));
        let vars: Vec<String> = env_args(&call, &Files::of(&call))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(
            vars.contains(&"PI_OFFLINE=1".to_owned()),
            "{role:?}: {vars:?}"
        );
    }
}
