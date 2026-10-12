//! Which project an e2e ask comes from
//!
//! The ask names only a working folder. The dog works out the project from
//! where that folder is, never from anything the ask says about a host, so a
//! worker cannot point a run anywhere its project's settings do not.

use std::path::Path;

use crate::lease::door::NO_E2E_HOST;
use crate::runner::{ProjectName, ProjectPaths};

/// What the door answers an `e2e` ask with
///
/// The project has no host to run on when `worktree` names none, or the
/// project has no `e2e.json` for its runner to have recorded. Otherwise this
/// dog has no run to offer yet, which is also what a dog older than its
/// runner says.
pub(super) fn reply(kelpie_home: &Path, worktree: Option<&Path>) -> String {
    let recorded = worktree
        .and_then(|worktree| project_of(kelpie_home, worktree))
        .map(|name| ProjectPaths::under(kelpie_home, kelpie_home, &name).e2e())
        .is_some_and(|file| file.is_file());
    match recorded {
        true => {
            "this dog cannot run e2e yet: the run is not built in this version of kelpie".to_owned()
        }
        false => format!(
            "{NO_E2E_HOST}: the project's settings name none in [app.dogs.kelpie.e2e], or the \
             path asked from is not one of its worktrees"
        ),
    }
}

/// The project whose `worktrees` folder holds `worktree`, once links are
/// resolved, or none for any other path
///
/// A path that names the `worktrees` folder itself, one that does not exist,
/// and one that only reaches a worktree through `..` or a link out of it
/// name no project.
pub(super) fn project_of(kelpie_home: &Path, worktree: &Path) -> Option<ProjectName> {
    let worktree = worktree.canonicalize().ok()?;
    std::fs::read_dir(kelpie_home)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = ProjectName::try_from(entry.file_name().to_str()?).ok()?;
            // Only the project's folder under kelpie's home is read here.
            let worktrees = ProjectPaths::under(kelpie_home, kelpie_home, &name).worktrees();
            let inside = worktree.strip_prefix(worktrees.canonicalize().ok()?).ok()?;
            (!inside.as_os_str().is_empty()).then_some(name)
        })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::*;

    // A kelpie home holding koji's worktrees 7 and 8 and golbat's 3, and a
    // folder outside it.
    fn home() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let home = base.join("kelpie");
        for worktree in [
            "koji/worktrees/7/src",
            "koji/worktrees/8",
            "golbat/worktrees/3",
        ] {
            std::fs::create_dir_all(home.join(worktree)).unwrap();
        }
        std::fs::create_dir_all(home.join("agents")).unwrap();
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        (dir, home, outside)
    }

    fn named(home: &Path, worktree: &str) -> Option<String> {
        project_of(home, &home.join(worktree)).map(|name| name.to_string())
    }

    #[test]
    fn a_worktree_names_its_project() {
        let (_dir, home, _outside) = home();
        assert_eq!(named(&home, "koji/worktrees/7").as_deref(), Some("koji"));
        assert_eq!(named(&home, "koji/worktrees/8").as_deref(), Some("koji"));
        assert_eq!(
            named(&home, "golbat/worktrees/3").as_deref(),
            Some("golbat")
        );
        // A worker may run it from a folder inside its worktree.
        assert_eq!(
            named(&home, "koji/worktrees/7/src").as_deref(),
            Some("koji")
        );
        // A link to a worktree is resolved first.
        let link = home.join("golbat/here");
        symlink(home.join("koji/worktrees/8"), &link).unwrap();
        assert_eq!(project_of(&home, &link).unwrap().as_str(), "koji");
    }

    #[test]
    fn a_path_outside_every_worktrees_folder_names_none() {
        let (_dir, home, outside) = home();
        let worktree = home.join("koji/worktrees/7");
        let rises = worktree.join("../../../agents");
        let sideways = worktree.join("../../../golbat/worktrees/../state.json");
        symlink(&outside, worktree.join("out")).unwrap();
        symlink(&outside, home.join("koji/worktrees/escape")).unwrap();
        let none = [
            rises,
            sideways,
            worktree.join("out"),
            home.join("koji/worktrees/escape"),
            home.join("koji/worktrees"),
            home.join("koji"),
            home.join("agents"),
            home.join("koji/worktrees/9"),
            home.clone(),
            outside,
            PathBuf::from("/"),
            PathBuf::new(),
        ];
        for path in none {
            assert_eq!(project_of(&home, &path), None, "{}", path.display());
        }
    }
}
