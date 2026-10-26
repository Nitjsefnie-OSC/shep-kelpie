use shep_kelpie_test_support::open_to_others as widen;
use std::fs;
use std::io;

use super::*;

#[test]
fn a_new_folder_is_owner_only_and_the_ones_that_were_there_are_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join("old");
    fs::create_dir(&old).unwrap();
    widen(&old);

    let deep = old.join("a/b");
    private_dir(&deep).unwrap();
    private_dir(&deep).unwrap();

    assert!(is_owner_only(&deep).unwrap());
    assert!(is_owner_only(&old.join("a")).unwrap());
    assert!(
        !is_owner_only(&old).unwrap(),
        "an old folder keeps its access"
    );
}

#[test]
fn a_new_file_is_owner_only_and_never_replaces_one() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("secret");

    let mut file = create_private(&path).unwrap();
    io::Write::write_all(&mut file, b"x").unwrap();

    assert!(is_owner_only(&path).unwrap());
    let again = create_private(&path).unwrap_err();
    assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&path).unwrap(), b"x");
}

#[test]
fn a_file_or_folder_others_may_read_is_not_owner_only() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("secret");
    let folder = home.path().join("folder");
    drop(create_private(&file).unwrap());
    private_dir(&folder).unwrap();

    widen(&file);
    widen(&folder);

    assert!(!is_owner_only(&file).unwrap());
    assert!(!is_owner_only(&folder).unwrap());
}

#[test]
fn a_folder_is_narrowed_to_its_owner_in_place() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join("folder");
    fs::create_dir(&folder).unwrap();
    widen(&folder);
    assert!(!is_owner_only(&folder).unwrap());

    restrict_dir(&folder).unwrap();

    assert!(is_owner_only(&folder).unwrap());
    assert!(is_owner(&folder).unwrap());
}

#[test]
fn a_path_that_is_not_there_is_an_error_not_an_answer() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("missing");

    assert_eq!(
        is_owner_only(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        is_owner(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        restrict_dir(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

// A file a plain `fs::write` makes takes the folder's ACL, which holds
// SYSTEM and Administrators beside the user.
#[cfg(windows)]
#[test]
fn a_file_that_inherits_its_acl_is_not_owner_only() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("inherited");
    fs::write(&path, "x").unwrap();

    assert!(!is_owner_only(&path).unwrap());
}

// Administrators is not the owner: a grant to it, in a DACL that still holds
// the user, is exposed.
#[cfg(windows)]
#[test]
fn a_grant_to_administrators_makes_a_file_exposed() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("secret");
    drop(create_private(&path).unwrap());
    assert!(is_owner_only(&path).unwrap());

    shep_kelpie_test_support::open_to_administrators(&path);

    assert!(!is_owner_only(&path).unwrap());
    assert!(is_owner(&path).unwrap());
}
