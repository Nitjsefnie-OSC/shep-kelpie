use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use shep_kelpie_test_support::CALL_PATIENCE;

use super::*;

// A leader that starts a grandchild and says so, both holding the same
// stdout. The grandchild lives for a minute unless the tree is stopped.
#[cfg(unix)]
fn leader_with_grandchild() -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 60 & echo up; wait"]);
    command
}

#[cfg(windows)]
fn leader_with_grandchild() -> Command {
    let mut command = Command::new("cmd");
    command.args([
        "/C",
        "start /B ping -n 60 127.0.0.1 & echo up & ping -n 60 127.0.0.1 >nul",
    ]);
    command
}

// Starts the leader and waits until its grandchild exists. What is left of
// stdout is read on a thread, which gets its end only once every process that
// held it is gone.
fn start() -> (std::process::Child, mpsc::Receiver<()>) {
    let mut command = leader_with_grandchild();
    let mut child = command
        .lead()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(line.starts_with("up"), "{line:?}");
    let (ended, every_holder_gone) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout.read_to_end(&mut Vec::new());
        let _ = ended.send(());
    });
    (child, every_holder_gone)
}

#[test]
fn a_killed_tree_leaves_no_grandchild() {
    let (mut child, gone) = start();
    let tree = Tree::of(&child).unwrap();
    assert!(tree.running());
    assert_eq!(tree.send(Signal::Kill), Reach::Whole);
    child.wait().unwrap();
    gone.recv_timeout(CALL_PATIENCE)
        .expect("the grandchild outlived the tree");
}

#[test]
fn a_terminated_tree_leaves_no_grandchild() {
    let (mut child, gone) = start();
    let tree = Tree::of(&child).unwrap();
    assert_eq!(tree.send(Signal::Terminate), Reach::Whole);
    child.wait().unwrap();
    gone.recv_timeout(CALL_PATIENCE)
        .expect("the grandchild outlived the tree");
}

#[test]
fn a_tree_stops_running_once_it_is_ended() {
    let (mut child, gone) = start();
    let tree = Tree::of(&child).unwrap();
    tree.send(Signal::Kill);
    child.wait().unwrap();
    gone.recv_timeout(CALL_PATIENCE).unwrap();
    // A killed grandchild is a zombie until its new parent reaps it.
    let started = Instant::now();
    while tree.running() {
        assert!(started.elapsed() < CALL_PATIENCE, "the tree still runs");
        thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
async fn a_tokio_child_s_tree_is_ended_too() {
    let mut command = tokio::process::Command::from(leader_with_grandchild());
    let mut child = command
        .lead()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut stdout, &mut line)
        .await
        .unwrap();
    let tree = Tree::of_tokio(&child).unwrap();
    tree.send(Signal::Kill);
    child.wait().await.unwrap();
    let mut rest = Vec::new();
    tokio::time::timeout(
        CALL_PATIENCE,
        tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut rest),
    )
    .await
    .expect("the grandchild outlived the tree")
    .unwrap();
}

#[test]
fn a_process_is_alive_until_it_is_gone() {
    let mut command = leader_with_grandchild();
    let mut child = command
        .lead()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let tree = Tree::of(&child).unwrap();
    assert!(alive(child.id()));
    tree.send(Signal::Kill);
    child.wait().unwrap();
    assert!(!alive(child.id()));
}

#[cfg(unix)]
#[test]
fn a_process_alone_is_signalled_without_its_group() {
    let (mut child, _) = start();
    let alone = Tree::of(&child).unwrap().alone();
    assert_eq!(alone.send(Signal::Kill), Reach::Whole);
    child.wait().unwrap();
    let rest = Tree::of(&child).unwrap();
    assert!(rest.running(), "the grandchild went with the leader");
    rest.send(Signal::Kill);
}
