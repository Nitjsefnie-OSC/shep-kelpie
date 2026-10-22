use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use super::super::{Feeder, Processes};
use super::Finishing;

// Real time. The count is the test's own: others feed at the same time.
#[test]
fn a_stdin_being_written_is_counted_until_the_writer_ends() {
    static OURS: AtomicUsize = AtomicUsize::new(0);
    let mut child = Command::new(crate::spawn::sh())
        .args(["-c", "sleep 30"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let feeder = Feeder::start_counted(
        child.stdin.take().unwrap(),
        vec![b'x'; 4 * 1024 * 1024],
        &OURS,
    );
    thread::sleep(Duration::from_millis(100));
    let while_stuck = OURS.load(Ordering::SeqCst);
    let _ = child.kill();
    let _ = child.wait();
    drop(feeder);

    assert_eq!(while_stuck, 1, "a write the child never reads is a feed");
    assert_eq!(OURS.load(Ordering::SeqCst), 0, "the feed ended");
}

#[test]
fn a_call_taken_off_the_list_at_its_deadline_is_a_call_being_ended() {
    let processes = Processes::default();
    let ended = {
        let processes = processes.clone();
        thread::spawn(move || {
            // Ignores TERM, so ending it takes the whole grace.
            processes.output_within(
                Command::new(crate::spawn::sh()).args(["-c", "trap '' TERM; sleep 30"]),
                Duration::from_millis(100),
            )
        })
    };
    thread::sleep(Duration::from_millis(1000));
    let refused = processes.seal().is_none();
    let _ = ended.join();

    assert!(refused, "sealed with a call being ended");
    assert!(processes.seal().is_some(), "the end was counted for good");
}

#[test]
fn a_seal_is_refused_while_one_stands_or_a_call_is_being_read() {
    let processes = Processes::default();
    let sealed = processes.seal().expect("nothing is sealed yet");
    assert!(processes.seal().is_none(), "sealed twice");
    drop(sealed);

    let mut counting = Finishing::new(&processes);
    processes.lock().finishing += 1;
    counting.counted = true;
    assert!(processes.seal().is_none(), "sealed with a call being read");
    drop(counting);
    assert!(processes.seal().is_some());
}

#[test]
fn a_group_left_with_no_one_to_finish_it_is_ended() {
    let mut child = Command::new(crate::spawn::sh())
        .args(["-c", "sleep 30"])
        .process_group(0)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();

    super::end_groups(&[child.id()]);

    let status = child.wait().unwrap();
    assert!(!status.success(), "it ran to its end: {status:?}");
}
