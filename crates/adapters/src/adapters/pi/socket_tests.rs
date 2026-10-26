//! A pi worker's tests bind Unix sockets in its call's scratch folder

use std::path::Path;

use super::tests::{FRESH_ID, URL, WORKER_ID, World, id};
use super::*;

#[test]
fn a_workers_scratch_folder_is_where_it_may_bind_sockets_and_a_reviewers_is_not() {
    let w = World::new();
    let socket = w.path("forwarder.sock");
    let reviewer = w.call(URL, Role::Reviewer, Session::New(id(FRESH_ID)));
    let upstream = PiCli::on(&Base::default())
        .upstream(server(&reviewer).unwrap())
        .unwrap();
    let policy = |call: &AgentCall| {
        let files = Files::of(call);
        let policy = policy(
            call,
            &files,
            &upstream,
            &socket,
            Path::new("/nowhere/pi"),
            &[],
        );
        (policy.socket_folders, files.scratch)
    };

    let worker = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let (folders, scratch) = policy(&worker);
    assert_eq!(folders, [scratch]);

    let issue_writer = w.call(URL, Role::IssueWriter, Session::New(id(FRESH_ID)));
    for call in [reviewer, issue_writer] {
        assert!(policy(&call).0.is_empty());
    }
}
