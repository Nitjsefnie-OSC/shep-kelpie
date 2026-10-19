//! The project's e2e table, written for the dog
//!
//! A worker's fence writes only its worktree and build folder, so it cannot
//! change `e2e.json` in the project's folder. The dog reads it, not the ask,
//! to learn what host a worker's run goes to.

use std::fs;
use std::io;
use std::path::Path;

use crate::home::ProjectPaths;
use crate::settings::E2e;
use crate::state::write_atomically;

/// Writes `e2e` to `file`, or removes a stale `file` when there is no table
///
/// Says what went wrong in a line. A file that cannot be written is removed
/// too, so the dog never reads a host the settings no longer name.
pub(super) fn record(e2e: Option<&E2e>, file: &Path) -> Option<String> {
    let written = e2e.map(|e2e| {
        serde_json::to_vec(e2e)
            .map_err(io::Error::other)
            .and_then(|bytes| write_atomically(file, &bytes))
    });
    if let Some(Ok(())) = written {
        return None;
    }
    let removed = match fs::remove_file(file) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    let name = file.display();
    match (written, removed) {
        (Some(Err(w)), Err(r)) => Some(format!(
            "cannot write {name}: {w}, and cannot remove the stale file: {r}"
        )),
        (Some(Err(w)), Ok(())) => Some(format!("cannot write {name}: {w}")),
        (_, Err(r)) => Some(format!("cannot remove {name}: {r}")),
        _ => None,
    }
}

/// A note when the project sets an e2e table but its folder is not where
/// the dog looks
///
/// Shep starts the adopted dog without `KELPIE_HOME`, so it reads project
/// folders under `$SHEP_HOME/kelpie` alone. A runner started with another
/// `KELPIE_HOME` records `e2e.json` where the dog never reads it, and the
/// dog would say the project has no e2e host.
pub(super) fn dog_cannot_find(e2e: Option<&E2e>, paths: &ProjectPaths) -> Option<String> {
    let dog_reads = crate::home::under(&paths.shep_home);
    (e2e.is_some() && paths.kelpie_home != dog_reads).then(|| {
        format!(
            "this project sets [app.dogs.kelpie.e2e], but its home {} is not {}, which is the \
             only place the dog looks, so `lease e2e` will say it has no e2e host",
            paths.kelpie_home.display(),
            dog_reads.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::ProjectName;

    #[test]
    fn a_runner_whose_home_is_not_where_the_dog_looks_says_so() {
        let e2e: E2e = serde_json::from_str(r#"{"host":"e2e-box"}"#).unwrap();
        let koji = ProjectName::try_from("koji").unwrap();
        let shep = Path::new("/s");
        let under_shep = ProjectPaths::under(&crate::home::under(shep), shep, &koji);
        let elsewhere = ProjectPaths::under(Path::new("/elsewhere"), shep, &koji);

        assert_eq!(dog_cannot_find(Some(&e2e), &under_shep), None);
        assert_eq!(dog_cannot_find(None, &elsewhere), None);
        let note = dog_cannot_find(Some(&e2e), &elsewhere).expect("a note");
        assert!(note.contains("/elsewhere"), "{note}");
        assert!(note.contains("/s/kelpie"), "{note}");
    }

    #[test]
    fn a_file_that_can_be_neither_written_nor_removed_says_both() {
        let dir = tempfile::tempdir().unwrap();
        // A folder with something in it stands where the file should be.
        let file = dir.path().join("e2e.json");
        fs::create_dir_all(file.join("held")).unwrap();
        let e2e: E2e = serde_json::from_str(r#"{"host":"e2e-box"}"#).unwrap();

        let note = record(Some(&e2e), &file).expect("a failure is noted");

        assert!(note.contains("cannot write"), "{note}");
        assert!(note.contains("cannot remove the stale file"), "{note}");
    }
}
