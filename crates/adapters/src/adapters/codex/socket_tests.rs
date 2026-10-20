//! A Codex worker's tests bind Unix sockets in its call's scratch folder

use std::path::Path;

use super::tests::{WORKER_ID, World, id};
use super::*;
use crate::ports::Role;

#[test]
fn a_workers_scratch_folder_is_where_it_may_bind_sockets_and_a_reviewers_is_not() {
    let w = World::new();
    let policy = |call: &AgentCall| {
        let files = Files::of(call);
        let policy = policy(call, &w.path("login"), &files, &[]);
        (policy.socket_folders, files.scratch)
    };

    let worker = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let (folders, scratch) = policy(&worker);
    assert_eq!(folders, [scratch]);

    let reviewer = w.call(Role::Reviewer, Session::New(id(WORKER_ID)));
    let mut retro = w.call(Role::Reviewer, Session::New(id(WORKER_ID)));
    retro.tools = Tools::Retro;
    for call in [reviewer, retro] {
        assert!(policy(&call).0.is_empty());
    }
}
