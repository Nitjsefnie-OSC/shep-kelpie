//! The skills a Codex worker finds in its home

use std::path::Path;

use super::super::skills::skill_dirs;
use super::*;

/// Where Codex looks for a user's skills in its home
const SKILLS_FOLDER: &str = "skills";

/// Links each skill of the call's plugins into the call's Codex home, and
/// removes the links the last call left
///
/// A call's shell opens a `SKILL.md`, so only a call with one (`has_shell`) gets
/// them; every other call has its step's skill in its prompt. The first
/// plugin to hold a skill's name wins it. The call cannot write this folder,
/// so it cannot plant a skill (`WRITTEN`).
pub(super) fn link_skills(call: &AgentCall, home: &Path) -> Result<(), AgentError> {
    let folder = home.join(SKILLS_FOLDER);
    let setup = |what: &Path, e: std::io::Error| {
        AgentError::Setup(format!("cannot link {}: {}", what.display(), e.kind()))
    };
    match std::fs::read_dir(&folder) {
        Ok(entries) => {
            for entry in entries {
                let path = entry.map_err(|e| setup(&folder, e))?.path();
                if path.is_symlink() {
                    crate::adapters::files::unlink(&path).map_err(|e| setup(&path, e))?;
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(setup(&folder, e)),
    }
    if !has_shell(call) {
        return Ok(());
    }
    std::fs::create_dir_all(&folder).map_err(|e| setup(&folder, e))?;
    for dir in skill_dirs(call) {
        let entries = std::fs::read_dir(&dir).map_err(|e| setup(&dir, e))?;
        for entry in entries {
            let skill = entry.map_err(|e| setup(&dir, e))?.path();
            // Codex's own `.system` skills live in a dot folder of this one.
            let named = skill
                .file_name()
                .filter(|n| !n.as_encoded_bytes().starts_with(b"."));
            let Some(name) = named else { continue };
            if !skill.join("SKILL.md").is_file() {
                continue;
            }
            let link = folder.join(name);
            match crate::adapters::files::link(&skill, &link) {
                // An earlier plugin holds this name.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && link.is_symlink() => {}
                result => result.map_err(|e| setup(&link, e))?,
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::tests::{WORKER_ID, World, id};
    use super::*;
    use crate::ports::Role;
    use crate::test::OpenSandbox;

    fn cli(w: &World) -> CodexCli {
        CodexCli::on(
            &Base::default().sandboxed(Arc::new(OpenSandbox::default()), w.path("home")),
            w.path("login"),
        )
    }

    // A plugin at `name` holding a skill folder with a `SKILL.md` for each of `skills`.
    fn plugin(w: &World, name: &str, skills: &[&str]) -> PathBuf {
        let plugin = w.path(name);
        for skill in skills {
            let dir = plugin.join("skills").join(skill);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!("---\nname: {skill}\n---\n")).unwrap();
        }
        plugin
    }

    fn worker(w: &World, plugins: Vec<PathBuf>) -> AgentCall {
        AgentCall {
            tools: Tools::Work,
            plugin_dirs: plugins,
            ..w.call(Role::Worker, Session::New(id(WORKER_ID)))
        }
    }

    // What the call's home holds in its skills folder, by name.
    fn linked(w: &World) -> Vec<(String, PathBuf)> {
        let folder = w.path("worker/settings.codex").join(SKILLS_FOLDER);
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut found: Vec<_> = entries
            .map(|e| {
                let path = e.unwrap().path();
                let to = std::fs::read_link(&path).unwrap();
                (path.file_name().unwrap().to_str().unwrap().to_owned(), to)
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_worker_s_home_links_each_plugin_skill_and_the_first_plugin_wins_a_name() {
        let w = World::new();
        let first = plugin(&w, "first", &["tdd", "style"]);
        let second = plugin(&w, "second", &["tdd", "diagnose"]);
        // A plugin with no skills, and a folder in a skills folder that holds none.
        let bare = w.path("bare");
        std::fs::create_dir_all(bare.join("commands")).unwrap();
        std::fs::create_dir_all(second.join("skills/notes")).unwrap();
        let call = worker(&w, vec![first.clone(), bare, second.clone()]);

        cli(&w).prepare(&call).unwrap();

        assert_eq!(
            linked(&w),
            [
                ("diagnose".to_owned(), second.join("skills/diagnose")),
                ("style".to_owned(), first.join("skills/style")),
                ("tdd".to_owned(), first.join("skills/tdd")),
            ]
        );
    }

    #[test]
    fn the_wording_skill_is_linked_with_the_step_skills_on_a_worker_call() {
        let w = World::new();
        let skills =
            crate::skills::Skills::load(&crate::settings::StepSkills::default(), &w.path("skills"));
        let call = worker(&w, skills.plugin_dirs().to_vec());

        cli(&w).prepare(&call).unwrap();

        let linked = linked(&w);
        assert!(
            linked.contains(&(
                "wording".to_owned(),
                w.path("skills/kelpie-wording/skills/wording")
            )),
            "{linked:?}"
        );
        assert!(
            linked.iter().any(|(name, _)| name == "implement"),
            "{linked:?}"
        );
    }

    #[test]
    fn a_worker_links_only_the_skills_its_steps_use_and_those_they_call() {
        let w = World::new();
        let skills =
            crate::skills::Skills::load(&crate::settings::StepSkills::default(), &w.path("skills"));
        let call = worker(&w, skills.plugin_dirs().to_vec());
        // Every skill folder the call's plugins hold is linked, and no other.
        assert_eq!(
            skill_dirs(&call),
            [
                w.path("skills/mattpocock/skills"),
                w.path("skills/kelpie-wording/skills")
            ]
        );

        cli(&w).prepare(&call).unwrap();

        let mattpocock = w.path("skills/mattpocock/skills");
        assert_eq!(
            linked(&w),
            [
                ("code-review".to_owned(), mattpocock.join("code-review")),
                (
                    "codebase-design".to_owned(),
                    mattpocock.join("codebase-design")
                ),
                (
                    "diagnosing-bugs".to_owned(),
                    mattpocock.join("diagnosing-bugs")
                ),
                ("implement".to_owned(), mattpocock.join("implement")),
                ("pr".to_owned(), mattpocock.join("pr")),
                ("tdd".to_owned(), mattpocock.join("tdd")),
                (
                    "wording".to_owned(),
                    w.path("skills/kelpie-wording/skills/wording")
                ),
            ]
        );
    }

    // A call has links exactly when Codex is given a shell, whatever
    // `agent_tools` says, since Claude Code alone reads that.
    #[test]
    fn a_call_with_no_shell_gets_no_skill_links() {
        let w = World::new();
        let plugin = plugin(&w, "plugin", &["tdd"]);
        for tools in [
            Tools::Work,
            Tools::Review,
            Tools::Answer,
            Tools::Issues,
            Tools::Pm,
            Tools::Retro,
        ] {
            let call = AgentCall {
                tools,
                agent_tools: Some("Bash,Read,Skill".into()),
                plugin_dirs: vec![plugin.clone()],
                ..w.call(Role::Reviewer, Session::New(id(WORKER_ID)))
            };
            cli(&w).prepare(&call).unwrap();
            let argv = argv(&call, &Files::of(&call).home, None, None).unwrap();
            let shell = !argv.windows(2).any(|w| w == ["--disable", "shell_tool"]);
            assert_eq!(!linked(&w).is_empty(), shell, "{tools:?}");
            assert_eq!(shell, tools == Tools::Work, "{tools:?}");
        }
    }

    #[test]
    fn links_from_the_last_call_are_removed() {
        let w = World::new();
        let plugin = plugin(&w, "plugin", &["tdd", "style"]);
        let cli = cli(&w);
        cli.prepare(&worker(&w, vec![plugin.clone()])).unwrap();
        assert_eq!(linked(&w).len(), 2);

        // The same home, now a call whose plugins hold one of them and one new.
        let other = self::plugin(&w, "other", &["review"]);
        std::fs::remove_dir_all(plugin.join("skills/style")).unwrap();
        cli.prepare(&worker(&w, vec![plugin.clone(), other.clone()]))
            .unwrap();
        assert_eq!(
            linked(&w),
            [
                ("review".to_owned(), other.join("skills/review")),
                ("tdd".to_owned(), plugin.join("skills/tdd")),
            ]
        );

        // Then a call with no shell, and a worker with no plugins.
        let mut review = worker(&w, vec![plugin]);
        review.tools = Tools::Review;
        cli.prepare(&review).unwrap();
        assert_eq!(linked(&w), []);
        cli.prepare(&worker(&w, Vec::new())).unwrap();
        assert_eq!(linked(&w), []);
    }

    #[test]
    fn the_sandbox_reads_the_linked_plugin_folders_and_the_call_cannot_write_the_skills_folder() {
        let w = World::new();
        let (first, second) = (
            plugin(&w, "first", &["tdd"]),
            plugin(&w, "second", &["style"]),
        );
        let call = worker(&w, vec![first.clone(), second.clone()]);
        let files = Files::of(&call);

        let policy = policy(
            &call,
            &w.path("login"),
            &files,
            Path::new("/nowhere/codex"),
            &[],
        );

        for plugin in [&first, &second] {
            assert!(policy.read.contains(plugin), "{}", plugin.display());
        }
        assert!(policy.read.contains(&files.home));
        let skills = files.home.join(SKILLS_FOLDER);
        for path in [
            skills.clone(),
            skills.join("tdd"),
            skills.join("tdd/SKILL.md"),
        ] {
            assert!(
                !policy.write.iter().any(|w| path.starts_with(w)),
                "{}",
                path.display()
            );
        }
        assert!(!policy.no_read.iter().any(|p| skills.starts_with(p)));
        assert!(policy.no_read.iter().any(|p| p == "~/.agents/**"));
    }
}
