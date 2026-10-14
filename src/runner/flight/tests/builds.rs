//! A work item's build folder and the disk, watched while its turn runs

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::json;

use super::*;
use crate::runner::turn::paused::PAUSED;
use crate::test::{GB, write_in};

// Where the stand-in Claude was told to build: the turn in flight has begun
fn build_of(rig: &Rig) -> PathBuf {
    let seen = rig.claude.seen();
    let dir = seen[0].settings["env"]["CARGO_TARGET_DIR"].as_str();
    PathBuf::from(dir.expect("the call's build folder"))
}

// A file the turn in flight did not write
fn stale(build: &Path, file: &str) {
    write_in(build, file, "old");
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    (File::options().write(true).open(build.join(file)).unwrap())
        .set_modified(hour_ago)
        .unwrap();
}

// Holds issue 7's turn in a tool call, and returns the folder it builds in
fn mid_tool(rig: &Rig, driven: &Driven) -> (Hold, PathBuf) {
    let tool = Hold::default();
    let ends = Scripted::Reply(usage(), Cost(9));
    rig.claude
        .script([Scripted::AtTool(tool.clone(), Box::new(ends))]);
    driven.started();
    assert!(tool.entered(CALL_PATIENCE), "the tool call never began");
    // The turn's first minute counts as read.
    rig.clock.advance(60);
    (tool, build_of(rig))
}

fn idle(driven: &Driven) -> bool {
    matches!(driven.pass(), Pass::Idle)
}

fn told(runner: &Mutex<Runner>, containing: &str) -> usize {
    let notes = lock(runner).take_notes();
    notes.iter().filter(|n| n.contains(containing)).count()
}

#[test]
fn a_folder_past_the_cap_pauses_the_turn_prunes_what_it_did_not_touch_and_resumes_it() {
    let (rig, runner) = seven_open("shep");
    let driven = Driven::new(&runner);
    let (tool, build) = mid_tool(&rig, &driven);
    for n in 0..22 {
        stale(&build, &format!("debug/deps/old{n}.rlib"));
    }
    stale(&build, "debug/.fingerprint/dep-1/fp");
    stale(&build, "debug/incremental/session/old.bin");
    stale(&build, "elsewhere/deps/not-cargo.rlib");
    write_in(&build, "debug/deps/touched.rlib", "new");
    write_in(&build, "notes.txt", "the worker's own file");
    rig.disk.set_per_file(GB);

    driven.pass();
    rig.clock.advance(60);
    driven.pass();
    tool.release();
    let ended = driven.landed();

    assert_eq!(ended, None, "a paused turn reports nothing");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["turn"]["prompt"], PAUSED);
    assert_eq!(rig.claude.calls().len(), 1);
    assert!(!build.join("debug/deps/old0.rlib").exists());
    assert!(!build.join("debug/incremental/session/old.bin").exists());
    assert!(build.join("debug/.fingerprint/dep-1/fp").exists());
    assert!(build.join("debug/deps/touched.rlib").exists());
    assert!(build.join("notes.txt").exists());
    assert!(build.join("elsewhere/deps/not-cargo.rlib").exists());
    let notes = lock(&runner).take_notes().join("\n");
    assert_eq!(notes.matches("past `builds.max_gb`").count(), 1, "{notes}");
    assert!(notes.contains("its build folder holds 2"), "{notes}");
    assert!(notes.contains("removed 23 cargo cache files"), "{notes}");

    driven.started();
    let [first, resumed] = rig.claude.calls().try_into().unwrap();
    assert_eq!(resumed.prompt, PAUSED);
    assert_eq!(resumed.session, Session::Resume(first.session.id().clone()));
    driven.landed();
}

#[test]
fn a_folder_still_past_the_cap_after_the_prune_holds_the_next_turn_once() {
    let (rig, runner) = seven_open("shep");
    let driven = Driven::new(&runner);
    let (tool, build) = mid_tool(&rig, &driven);
    stale(&build, "debug/.fingerprint/dep-1/fp");
    stale(&build, "debug/deps/old.rlib");
    for n in 0..22 {
        write_in(&build, &format!("kept{n}.txt"), "the worker's own files");
    }
    rig.disk.set_per_file(GB);

    driven.pass();
    tool.release();
    driven.landed();
    assert!(idle(&driven));
    assert!(idle(&driven));

    let status = rig.ask(&runner, "status", None);
    let because = status["work_item"]["held_up"]["because"].as_str().unwrap();
    assert!(because.contains("its build folder holds"), "{because}");
    assert!(because.contains("`builds.max_gb`"), "{because}");
    assert_eq!(rig.claude.calls().len(), 1, "no turn started");
    assert_eq!(told(&runner, "its next turn is held"), 1);
}

#[test]
fn free_space_under_the_floor_pauses_the_turn_and_holds_the_next_while_it_stays_under() {
    let (rig, runner) = seven_open("shep");
    let driven = Driven::new(&runner);
    let (tool, _) = mid_tool(&rig, &driven);
    rig.disk.set_free(5 * GB);

    driven.pass();
    tool.release();
    assert_eq!(driven.landed(), None);
    assert!(idle(&driven));

    let status = rig.ask(&runner, "status", None);
    let because = status["work_item"]["held_up"]["because"].as_str().unwrap();
    assert!(because.contains("the disk has 5.0 GB free"), "{because}");
    assert!(because.contains("`builds.min_free_gb`"), "{because}");
    assert_eq!(status["disk_free_bytes"], json!(5 * GB));
    assert_eq!(rig.claude.calls().len(), 1);

    // Only a fresh reading lets the turn go.
    rig.disk.set_free(500 * GB);
    assert!(idle(&driven), "read again within a minute");
    rig.clock.advance(60);
    driven.started();
    let [_, resumed] = rig.claude.calls().try_into().unwrap();
    assert_eq!(resumed.prompt, PAUSED);
    driven.landed();
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["held_up"], json!(null));
}

#[test]
fn the_size_is_measured_no_more_than_once_a_minute_per_item() {
    let (rig, runner) = seven_open("shep");
    let driven = Driven::new(&runner);
    let (tool, build) = mid_tool(&rig, &driven);
    let measured = || rig.disk.measured().iter().filter(|p| **p == build).count();

    for _ in 0..3 {
        driven.pass();
    }
    assert_eq!(measured(), 1);
    rig.clock.advance(59);
    driven.pass();
    assert_eq!(measured(), 1);
    rig.clock.advance(1);
    driven.pass();
    assert_eq!(measured(), 2);

    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["build_bytes"], json!(0));
    tool.release();
    driven.landed();
}

#[test]
fn a_limit_of_0_measures_nothing() {
    let rig = Rig::new("shep");
    rig.edit_settings(|text| {
        format!("{text}\n[app.dogs.kelpie.builds]\nmax_gb = 0\nmin_free_gb = 0\n")
    });
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let driven = Driven::new(&runner);
    let (tool, build) = mid_tool(&rig, &driven);
    rig.disk.set_per_file(GB);
    rig.disk.set_free(0);

    driven.pass();
    rig.clock.advance(60);
    driven.pass();

    assert!(!rig.disk.measured().contains(&build));
    assert!(!build.join("kelpie-pause").exists());
    tool.release();
    driven.landed();
}
