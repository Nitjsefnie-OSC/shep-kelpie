//! A goose worker's tests bind Unix sockets in its call's scratch folder

use std::path::Path;

use super::tests::{FRESH_ID, OTHER_ID, URL, World, id};
use super::*;
use crate::ports::Role;

#[test]
fn a_workers_scratch_folder_is_where_it_may_bind_sockets_and_a_reviewers_is_not() {
    let w = World::new();
    let reviewer = w.call(URL, Role::Reviewer, Session::New(id(FRESH_ID)));
    let upstream = GooseCli::on(&Base::default())
        .upstream(server(&reviewer).unwrap())
        .unwrap();
    let policy = |call: &AgentCall| {
        let files = Files::of(&call.settings);
        let policy = policy(call, &files, &upstream, &w.path("forwarder.sock"), &[]);
        (policy.socket_folders, files.scratch)
    };

    let worker = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(OTHER_ID)));
    let (folders, scratch) = policy(&worker);
    assert_eq!(folders, [scratch]);

    let mut retro = w.call(URL, Role::Reviewer, Session::New(id(FRESH_ID)));
    retro.tools = Tools::Retro;
    for call in [reviewer, retro] {
        assert!(policy(&call).0.is_empty());
    }
}
