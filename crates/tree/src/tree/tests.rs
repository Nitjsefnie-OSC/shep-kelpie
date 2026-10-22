use std::io::{BufRead, BufReader, Read, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use shep_kelpie_test_support::CALL_PATIENCE;

use super::*;

// A leader that starts a grandchild once it is told to, and says so, both
// holding the same stdout. The grandchild lives for ten minutes unless the
// tree is stopped, so one that escapes the tree outlasts the wait for it.
#[cfg(unix)]
fn leader_with_grandchild() -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", "read go; sleep 600 & echo up; wait"]);
    command
}

#[cfg(windows)]
fn leader_with_grandchild() -> Command {
    let mut command = Command::new("cmd");
    command.args([
        "/C",
        "set /p go= & start /B ping -n 600 127.0.0.1 & echo up & ping -n 600 127.0.0.1 >nul",
    ]);
    command
}

// Starts the leader and puts it in its tree before it starts the grandchild,
// then lets it. What is left of stdout is read on a thread, which gets its end
// only once every process that held it is gone.
fn start() -> (std::process::Child, Tree, mpsc::Receiver<()>) {
    let mut command = leader_with_grandchild();
    let mut child = command
        .lead()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let tree = Tree::of(&child).unwrap();
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(line.starts_with("up"), "{line:?}");
    let (ended, every_holder_gone) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout.read_to_end(&mut Vec::new());
        let _ = ended.send(());
    });
    (child, tree, every_holder_gone)
}

#[test]
fn a_killed_tree_leaves_no_grandchild() {
    let (mut child, tree, gone) = start();
    assert!(tree.running());
    assert_eq!(tree.send(Signal::Kill), Reach::Whole);
    child.wait().unwrap();
    gone.recv_timeout(CALL_PATIENCE)
        .expect("the grandchild outlived the tree");
}

#[test]
fn a_terminated_tree_leaves_no_grandchild() {
    let (mut child, tree, gone) = start();
    assert_eq!(tree.send(Signal::Terminate), Reach::Whole);
    child.wait().unwrap();
    gone.recv_timeout(CALL_PATIENCE)
        .expect("the grandchild outlived the tree");
}

#[test]
fn a_tree_stops_running_once_it_is_ended() {
    let (mut child, tree, gone) = start();
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
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let tree = Tree::of_tokio(&child).unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut child.stdin.take().unwrap(), b"go\n")
        .await
        .unwrap();
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut stdout, &mut line)
        .await
        .unwrap();
    assert!(line.starts_with("up"), "{line:?}");
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
    let (mut child, tree, _) = start();
    assert!(alive(child.id()));
    tree.send(Signal::Kill);
    child.wait().unwrap();
    assert!(!alive(child.id()));
}

#[cfg(unix)]
#[test]
fn a_process_of_another_user_is_not_alive() {
    // Pid 1 exists, and only root may signal it.
    if !rustix::process::geteuid().is_root() {
        assert!(!alive(1));
    }
}

#[cfg(unix)]
#[test]
fn a_process_alone_is_signalled_without_its_group() {
    let (mut child, tree, _) = start();
    let rest = Tree::of(&child).unwrap();
    assert_eq!(tree.alone().send(Signal::Kill), Reach::Whole);
    child.wait().unwrap();
    assert!(rest.running(), "the grandchild went with the leader");
    rest.send(Signal::Kill);
}
