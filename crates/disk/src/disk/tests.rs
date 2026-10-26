use std::fs;

use super::*;

#[test]
fn a_folder_holds_its_files_and_counts_a_hard_link_once() {
    let dir = tempfile::tempdir().unwrap();
    let one = dir.path().join("deps/one");
    fs::create_dir_all(one.parent().unwrap()).unwrap();
    fs::write(&one, vec![1u8; 100_000]).unwrap();
    let alone = folder_bytes(dir.path()).unwrap();
    assert!(alone >= 100_000, "{alone}");

    fs::hard_link(&one, dir.path().join("one")).unwrap();
    assert_eq!(folder_bytes(dir.path()).unwrap(), alone);
}

#[test]
fn a_folder_that_is_not_there_cannot_be_measured() {
    let dir = tempfile::tempdir().unwrap();
    assert!(folder_bytes(&dir.path().join("gone")).is_err());
}

#[test]
fn the_disk_holding_a_folder_has_free_space() {
    let dir = tempfile::tempdir().unwrap();
    assert!(free_bytes(dir.path()).unwrap() > 0);
}
