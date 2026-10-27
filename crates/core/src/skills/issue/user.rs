//! The issue skill in each harness's own skills folder, for any session the
//! maintainer runs
//!
//! The dog writes the skill to `<kelpie home>/.skills/issue` as it starts, which
//! an upgrade's restart does, and links it into the user skills folder of each
//! harness installed in the home folder. An `issue` already there that is not
//! kelpie's link is left alone and named; a link to another kelpie home's copy,
//! or to nothing, is replaced.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{SKILL, TEXT};

/// Each harness's folder in the home folder, which says it is installed, and
/// the user skills folder it reads
pub const FOLDERS: [(&str, &str); 4] = [
    (".claude", ".claude/skills"),
    (".pi", ".pi/agent/skills"),
    (".codex", ".codex/skills"),
    (".config/goose", ".config/goose/skills"),
];

/// The folder of kelpie's home that holds the copy the links point at
const COPY: &str = ".skills";

/// Writes the skill under `kelpie_home` and links it into each installed
/// harness's skills folder under `home`, returning a line for each link made
/// and each one left alone
pub fn install(kelpie_home: &Path, home: &Path) -> Vec<String> {
    let target = copy(kelpie_home);
    if let Err(e) = write_copy(&target) {
        return vec![format!(
            "cannot write kelpie's issue skill to {}: {e}, so no harness's skills folder links it",
            target.display()
        )];
    }
    let installed = FOLDERS
        .iter()
        .filter(|(harness, _)| home.join(harness).is_dir());
    let links = installed.map(|(_, folder)| home.join(folder).join(SKILL));
    links.filter_map(|link| linked(&link, &target)).collect()
}

fn write_copy(target: &Path) -> io::Result<()> {
    match fs::remove_dir_all(target) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    fs::create_dir_all(target)?;
    fs::write(target.join("SKILL.md"), TEXT)
}

// Makes `link` point at `target`, unless it already does or is someone
// else's, and says what it did. Nothing to say when it was already kelpie's.
// A link to another kelpie home's copy, or to nothing, is kelpie's to replace.
fn linked(link: &Path, target: &Path) -> Option<String> {
    let shown = link.display();
    let replaced = match fs::read_link(link) {
        Ok(to) if to == target => return None,
        Ok(to) if to.ends_with(Path::new(COPY).join(SKILL)) || !link.exists() => Some(to),
        Ok(to) => {
            return Some(format!(
                "left {shown} alone: it links to {}, not kelpie's issue skill",
                to.display()
            ));
        }
        Err(_) if fs::symlink_metadata(link).is_ok() => {
            return Some(format!(
                "left {shown} alone: it is not kelpie's link to its issue skill"
            ));
        }
        Err(_) => None,
    };
    let unlinked = replaced.as_ref().map_or(Ok(()), |_| unlink(link));
    let made = (unlinked.and_then(|()| link.parent().map_or(Ok(()), fs::create_dir_all)))
        .and_then(|()| symlink(target, link));
    let was = (replaced.as_ref()).map(|to| format!(" in place of its link to {}", to.display()));
    Some(match made {
        Ok(()) => format!(
            "linked kelpie's issue skill as {shown}{}",
            was.unwrap_or_default()
        ),
        Err(e) => format!("cannot link kelpie's issue skill as {shown}: {e}"),
    })
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn unlink(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn unlink(link: &Path) -> io::Result<()> {
    fs::remove_dir(link)
}

/// The folder each link points at, under `kelpie_home`
pub fn copy(kelpie_home: &Path) -> PathBuf {
    kelpie_home.join(COPY).join(SKILL)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A home with Claude Code and Codex installed, and neither pi nor goose.
    fn home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        for harness in [".claude", ".codex"] {
            fs::create_dir_all(home.path().join(harness)).unwrap();
        }
        home
    }

    #[test]
    fn the_skill_is_linked_into_each_installed_harnesss_skills_folder() {
        let (home, kelpie) = (home(), tempfile::tempdir().unwrap());
        let said = install(kelpie.path(), home.path());
        let target = copy(kelpie.path());
        let text = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(text.starts_with("---\nname: issue\n"), "{text}");
        for folder in [".claude/skills", ".codex/skills"] {
            let link = home.path().join(folder).join("issue");
            assert_eq!(fs::read_link(&link).unwrap(), target, "{folder}");
            assert!(said.contains(&format!(
                "linked kelpie's issue skill as {}",
                link.display()
            )));
        }
        for absent in [".pi", ".config/goose"] {
            assert!(!home.path().join(absent).exists(), "{absent}");
        }
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(install(kelpie.path(), home.path()), Vec::<String>::new());
    }

    #[test]
    fn a_link_to_another_kelpie_homes_copy_or_to_nothing_is_relinked() {
        let (home, kelpie) = (home(), tempfile::tempdir().unwrap());
        let (old, gone) = (tempfile::tempdir().unwrap(), home.path().join("gone"));
        let old_copy = copy(old.path());
        fs::create_dir_all(&old_copy).unwrap();
        let claude = home.path().join(".claude/skills/issue");
        let codex = home.path().join(".codex/skills/issue");
        for (link, to) in [(&claude, &old_copy), (&codex, &gone)] {
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(to, link).unwrap();
        }
        let said = install(kelpie.path(), home.path());
        let target = copy(kelpie.path());
        for (link, to) in [(&claude, &old_copy), (&codex, &gone)] {
            assert_eq!(fs::read_link(link).unwrap(), target);
            let line = format!(
                "linked kelpie's issue skill as {} in place of its link to {}",
                link.display(),
                to.display()
            );
            assert!(said.contains(&line), "{said:?}");
        }
    }

    #[test]
    fn an_issue_skill_that_is_not_kelpies_link_is_left_alone_and_named() {
        let (home, kelpie) = (home(), tempfile::tempdir().unwrap());
        let own = home.path().join(".claude/skills/issue");
        fs::create_dir_all(&own).unwrap();
        fs::write(own.join("SKILL.md"), "mine").unwrap();
        let elsewhere = home.path().join(".codex/skills/issue");
        fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        symlink(home.path(), &elsewhere).unwrap();
        let said = install(kelpie.path(), home.path());
        assert_eq!(fs::read_to_string(own.join("SKILL.md")).unwrap(), "mine");
        assert_eq!(fs::read_link(&elsewhere).unwrap(), home.path());
        assert_eq!(
            said,
            [
                format!(
                    "left {} alone: it is not kelpie's link to its issue skill",
                    own.display()
                ),
                format!(
                    "left {} alone: it links to {}, not kelpie's issue skill",
                    elsewhere.display(),
                    home.path().display()
                ),
            ]
        );
    }
}
