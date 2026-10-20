// pi's own `grep` and `find` run `rg` and `fd`, which pi downloads from GitHub
// when it finds neither in its own agent folder's `bin` nor on `PATH`. Each
// kelpie call has a pi home of its own, whose `bin` is always empty, and the
// sandbox refuses the download, so pi is told the call is offline and says the
// tool is missing instead of trying (`docs/facts/tool-errors.md`).

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
