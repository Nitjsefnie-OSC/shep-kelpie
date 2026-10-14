//! The skills a goose call loads, and a step's prompt as goose's skill command

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::hooks::Judging;
use super::hooks_tests::{plugin as fence_plugin, policy_of, worker};
use super::tests::{FRESH, FRESH_ID, OTHER_ID, URL, World, id, stand_in, strings};
use super::*;
use crate::confine::Verdict;
use crate::guard::{Checkout, judge_goose, judge_goose_reading, judge_goose_writing};
use crate::hooks::Hooks;
use crate::local_paths::LocalPaths;

// A plugin called `name` holding a skill folder for each `(folder, SKILL.md)` of `skills`.
fn plugin(w: &World, name: &str, skills: &[(&str, &str)]) -> PathBuf {
    let plugin = w.path(name);
    std::fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    std::fs::write(
        plugin.join(".claude-plugin/plugin.json"),
        format!(r#"{{"name":"{name}"}}"#),
    )
    .unwrap();
    for (folder, text) in skills {
        let dir = plugin.join("skills").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }
    plugin
}

fn named(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\nSteps of {name}.\n")
}

fn with_plugins(w: &World, plugins: Vec<PathBuf>) -> AgentCall {
    AgentCall {
        plugin_dirs: plugins,
        ..worker(w, Session::New(id(FRESH_ID)))
    }
}

fn skills_folder(w: &World) -> PathBuf {
    w.path("settings.goose/config/skills")
}

// The links in the call's skills folder, by name.
fn linked(w: &World) -> BTreeMap<String, PathBuf> {
    let Ok(entries) = std::fs::read_dir(skills_folder(w)) else {
        return BTreeMap::new();
    };
    entries
        .map(|e| e.unwrap().path())
        .filter(|path| path.is_symlink())
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            (name, std::fs::read_link(&path).unwrap())
        })
        .collect()
}

// Every entry of the call's skills folder, links and stubs.
fn entries(w: &World) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(skills_folder(w))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    found
}

fn text_of(call: &AgentCall) -> String {
    strings(call)
        .into_iter()
        .find_map(|a| a.strip_prefix("--text=").map(str::to_owned))
        .unwrap()
}

#[test]
fn every_plugin_s_skills_are_linked_into_the_call_s_config_skills_folder_with_the_step_skill_first()
{
    let w = World::new();
    let first = plugin(
        &w,
        "first",
        &[("tdd", &named("tdd")), ("style", &named("style"))],
    );
    let second = plugin(
        &w,
        "second",
        &[("tdd", &named("tdd")), ("diagnose", &named("diagnose"))],
    );
    // A plugin with no skills, and a file among a skills folder's entries.
    let bare = w.path("bare");
    std::fs::create_dir_all(bare.join("commands")).unwrap();
    std::fs::write(first.join("skills/README.md"), "notes").unwrap();
    let mut call = with_plugins(&w, vec![first.clone(), bare, second.clone()]);

    stand_in(&w, FRESH).prepare(&call).unwrap();
    assert_eq!(
        linked(&w),
        BTreeMap::from([
            ("diagnose".to_owned(), second.join("skills/diagnose")),
            ("style".to_owned(), first.join("skills/style")),
            ("tdd".to_owned(), first.join("skills/tdd")),
        ])
    );

    // The step's skill comes first, so its plugin wins a name the others hold.
    call.prompt = "/second:tdd Implement issue #7".into();
    stand_in(&w, FRESH).prepare(&call).unwrap();
    assert_eq!(linked(&w)["tdd"], second.join("skills/tdd"));
    assert_eq!(linked(&w)["style"], first.join("skills/style"));
}

#[test]
fn two_skills_with_one_name_are_linked_once_and_the_first_plugin_wins() {
    let w = World::new();
    // The folder names differ from the `name` lines, which are what goose lists.
    let first = plugin(&w, "first", &[("one", &named("shared"))]);
    let second = plugin(
        &w,
        "second",
        &[("two", &named("shared")), ("own", &named("own"))],
    );
    // Two in one plugin: the file system's order is not stable, so the folder name decides.
    let third = plugin(&w, "third", &[("b", &named("twin")), ("a", &named("twin"))]);
    let call = with_plugins(&w, vec![first.clone(), second.clone(), third.clone()]);

    stand_in(&w, FRESH).prepare(&call).unwrap();

    assert_eq!(
        linked(&w),
        BTreeMap::from([
            ("own".to_owned(), second.join("skills/own")),
            ("shared".to_owned(), first.join("skills/one")),
            ("twin".to_owned(), third.join("skills/a")),
        ])
    );
}

#[test]
fn a_skill_with_no_name_or_a_slash_in_its_name_is_not_linked() {
    let w = World::new();
    let kept = plugin(
        &w,
        "kept",
        &[
            ("plain", &named("plain")),
            ("quoted", "---\nname: \"quoted\"\n---\nText.\n"),
            ("nameless", "---\ndescription: No name.\n---\nText.\n"),
            ("no-front", "Text with no front matter.\n"),
            ("slash", &named("a/b")),
            ("up", &named("..")),
            ("dot", &named(".")),
            ("blank", "---\nname:\n---\nText.\n"),
            // Past the front matter, a `name:` line is the skill's text.
            ("late", "---\ndescription: x\n---\nname: late\n"),
        ],
    );
    std::fs::create_dir_all(kept.join("skills/empty")).unwrap();
    let call = with_plugins(&w, vec![kept.clone()]);

    stand_in(&w, FRESH).prepare(&call).unwrap();

    assert_eq!(
        linked(&w),
        BTreeMap::from([
            ("plain".to_owned(), kept.join("skills/plain")),
            ("quoted".to_owned(), kept.join("skills/quoted")),
        ])
    );
    // Nothing was linked outside the call's own folder.
    assert!(!w.path("settings.goose/config/b").exists());
    assert!(!w.path("settings.goose/config/a").exists());
}

#[test]
fn links_from_the_last_call_are_removed() {
    let w = World::new();
    let first = plugin(
        &w,
        "first",
        &[("tdd", &named("tdd")), ("style", &named("style"))],
    );
    let goose = stand_in(&w, FRESH);
    goose
        .prepare(&with_plugins(&w, vec![first.clone()]))
        .unwrap();
    assert_eq!(linked(&w).len(), 2);
    // A file the last call left in the folder goes with them.
    std::fs::write(skills_folder(&w).join("planted"), "x").unwrap();

    let other = plugin(&w, "other", &[("review", &named("review"))]);
    std::fs::remove_dir_all(first.join("skills/style")).unwrap();
    goose
        .prepare(&with_plugins(&w, vec![first.clone(), other.clone()]))
        .unwrap();
    assert_eq!(
        linked(&w),
        BTreeMap::from([
            ("review".to_owned(), other.join("skills/review")),
            ("tdd".to_owned(), first.join("skills/tdd")),
        ])
    );
    assert!(!entries(&w).contains(&"planted".to_owned()));

    // A call with no plugins links nothing, and the plugin folders are left whole.
    goose.prepare(&with_plugins(&w, Vec::new())).unwrap();
    assert_eq!(linked(&w), BTreeMap::new());
    assert!(first.join("skills/tdd/SKILL.md").is_file());
}

// The `implement` skill of a plugin called `mattpocock`
fn implement(w: &World, text: &str) -> PathBuf {
    plugin(w, "mattpocock", &[("implement", text)])
}

#[test]
fn a_step_prompt_runs_as_goose_s_slash_command_with_the_rest_after_it() {
    let w = World::new();
    let plugin = implement(&w, &named("do-implement"));
    let mut call = with_plugins(&w, vec![plugin]);
    call.prompt = "/mattpocock:implement Implement issue #7\nIt says \"quoted\".".into();

    // The name goose lists the skill by is the `name` line, not the folder `step_skill` finds.
    assert_eq!(
        text_of(&call),
        "/do-implement Implement issue #7\nIt says \"quoted\"."
    );
    // goose resolves the command from the folder, so every call has it, a researcher included.
    for tools in [
        Tools::Work,
        Tools::Review,
        Tools::Advise,
        Tools::Retro,
        Tools::Research,
    ] {
        let call = AgentCall {
            tools,
            ..call.clone()
        };
        assert_eq!(text_of(&call).split(' ').next(), Some("/do-implement"));
    }
    // With nothing after the command there is nothing after it.
    call.prompt = "/mattpocock:implement ".into();
    assert_eq!(text_of(&call), "/do-implement");
    // `$` that is not followed by a number or `ARGUMENTS` is plain text.
    let plain = implement(
        &w,
        "---\nname: implement\n---\nRun in $HOME, costs $ five.\n",
    );
    call.plugin_dirs = vec![plain];
    call.prompt = "/mattpocock:implement go".into();
    assert_eq!(text_of(&call), "/implement go");

    // The skill is linked, which is how goose finds it.
    stand_in(&w, FRESH).prepare(&call).unwrap();
    assert!(linked(&w).contains_key("implement"));
}

#[test]
fn a_step_skill_with_an_arguments_placeholder_keeps_the_prompt_as_written() {
    let w = World::new();
    let written = "/mattpocock:implement Implement issue #7, it's \"broken";
    for placeholder in ["$ARGUMENTS", "$ARGUMENTS[0]", "$0", "$12"] {
        let text = format!("---\nname: implement\n---\nDo this: {placeholder}\n");
        let plugin = implement(&w, &text);
        let mut call = with_plugins(&w, vec![plugin.clone()]);
        call.prompt = written.into();

        assert_eq!(text_of(&call), written, "{placeholder}");

        // The model loads it itself, so it is still listed.
        stand_in(&w, FRESH).prepare(&call).unwrap();
        assert_eq!(linked(&w)["implement"], plugin.join("skills/implement"));
    }
}

#[test]
fn a_prompt_naming_a_skill_no_plugin_holds_is_left_as_it_is() {
    let w = World::new();
    let plugin = implement(&w, &named("implement"));
    let mut call = with_plugins(&w, vec![plugin]);
    for prompt in [
        "/mattpocock:tdd Implement issue #7",
        "/elsewhere:implement Implement issue #7",
        "/implement Implement issue #7",
        "Implement issue #7",
        "",
    ] {
        call.prompt = prompt.into();
        assert_eq!(text_of(&call), prompt);
    }
}

fn judged(judge: Judging, tool: &str) -> Verdict {
    let payload = serde_json::json!({
        "tool_name": tool,
        "tool_input": {},
        "working_dir": "/k/wt",
    });
    let bytes = payload.to_string();
    let hooks = Hooks::default();
    match judge {
        Judging::Work => judge_goose(
            bytes.as_bytes(),
            Some(Path::new("/home/me")),
            LocalPaths::new([Path::new("/home/me")]),
            &hooks,
            Checkout {
                git_common_dir: Path::new("/nowhere"),
                worktree: Path::new("/k/wt"),
            },
            Path::new("/k/build"),
            &[],
        ),
        Judging::Reading => judge_goose_reading(bytes.as_bytes(), &hooks, &[]),
        Judging::Tests(folder) => judge_goose_writing(bytes.as_bytes(), &hooks, &[], folder),
    }
}

#[test]
fn every_kind_of_call_loads_the_skills_extension_and_the_hook_lets_load_skill_through_and_nothing_else_it_refused()
 {
    let w = World::new();
    let research = w.call(URL, Role::Researcher, Session::New(id(OTHER_ID)));
    let fenced = |role, tools| AgentCall {
        role,
        tools,
        ..worker(&w, Session::New(id(FRESH_ID)))
    };
    let mut reviewer = fenced(Role::Reviewer, Tools::Review);
    reviewer.reach.fence.as_mut().unwrap().write.clear();
    let mut resumed = fenced(Role::Worker, Tools::Retro);
    resumed.session = Session::Resume(id(FRESH_ID));
    let calls = [
        ("Work", fenced(Role::Worker, Tools::Work)),
        ("Review", reviewer),
        ("Advise", fenced(Role::Advisor, Tools::Advise)),
        ("Research", research),
        ("Retro", resumed),
    ];
    for (kind, call) in &calls {
        let argv = strings(call);
        let at = argv.iter().position(|a| a == "skills");
        let at = at.unwrap_or_else(|| panic!("{kind} loads no skills: {argv:?}"));
        assert_eq!(argv[at - 1], "--with-builtin", "{kind}");
        assert_eq!(argv.iter().filter(|a| *a == "skills").count(), 1, "{kind}");
    }
    // A worker that has the `developer` builtin is given both.
    let argv = strings(&calls[0].1);
    let builtins: Vec<_> = argv
        .windows(2)
        .filter(|pair| pair[0] == "--with-builtin")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(builtins, ["developer", "skills"]);

    // The hook of each kind: a worker's, a reader's and a reviewer's asked for tests.
    for judge in [
        Judging::Work,
        Judging::Reading,
        Judging::Tests(Path::new("/k/tests")),
    ] {
        assert_eq!(judged(judge, "load_skill"), Verdict::Allow, "{judge:?}");
        // What it refused stays refused, and a look-alike is not the tool.
        for tool in [
            "read_image",
            "delegate",
            "load",
            "skills__load_skill",
            "load_skill_x",
            "Skill",
            "",
        ] {
            assert!(
                matches!(judged(judge, tool), Verdict::Refuse(_)),
                "{judge:?}: {tool}"
            );
        }
    }
    // The hook a worker's `load_skill` goes through is written.
    stand_in(&w, FRESH).prepare(&calls[0].1).unwrap();
    assert!(fence_plugin(&w).join("hooks/hooks.json").is_file());
}

#[test]
fn the_sandbox_reads_every_plugin_folder_and_none_of_the_maintainers_skill_folders_and_cannot_write_the_skills_folder()
 {
    let w = World::new();
    let first = plugin(&w, "first", &[("tdd", &named("tdd"))]);
    let second = plugin(&w, "second", &[("style", &named("style"))]);
    let call = with_plugins(&w, vec![first.clone(), second.clone()]);

    let policy = policy_of(&w, &call);

    for plugin in [&first, &second] {
        assert!(policy.read.contains(plugin), "{}", plugin.display());
    }
    for folder in [
        "~/.agents/skills/**",
        "~/.claude/skills/**",
        "~/.config/agents/skills/**",
    ] {
        assert!(policy.no_read.contains(&folder.to_owned()), "{folder}");
        // The call is given none of them back.
        assert!(
            !policy.read.iter().any(|p| p.starts_with(folder)),
            "{folder}"
        );
    }
    let skills = skills_folder(&w);
    assert!(policy.no_write.contains(&skills), "{:?}", policy.no_write);
    for path in [
        skills.clone(),
        skills.join("tdd"),
        skills.join("tdd/SKILL.md"),
    ] {
        assert!(
            !policy.write.iter().any(|dir| path.starts_with(dir)),
            "{}",
            path.display()
        );
    }
    // A call with no fence is held to them too.
    let research = w.call(URL, Role::Researcher, Session::New(id(OTHER_ID)));
    let policy = policy_of(&w, &research);
    assert!(policy.no_read.contains(&"~/.claude/skills/**".to_owned()));
    assert!(policy.no_write.contains(&skills));
}

#[test]
fn the_worktree_s_skill_folders_are_write_denied_for_a_worker() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));

    let policy = policy_of(&w, &call);

    // Committed skills load, and a worker plants none.
    for folder in [".agents", ".claude", ".goose"] {
        for path in [
            w.path("wt").join(folder),
            w.path("wt").join("**").join(folder),
        ] {
            assert!(policy.no_write.contains(&path), "{}", path.display());
        }
        let skill = Path::new(folder).join("skills/x/SKILL.md");
        assert!(crate::fence::fenced(&skill), "{}", skill.display());
    }
}

#[test]
fn goose_s_two_builtin_skills_are_shadowed_by_stubs() {
    let w = World::new();
    let plugin = plugin(
        &w,
        "house",
        &[("mine", &named("web-search")), ("tdd", &named("tdd"))],
    );
    let call = with_plugins(&w, vec![plugin.clone()]);

    stand_in(&w, FRESH).prepare(&call).unwrap();

    // goose adds a builtin only when no discovered skill has its name.
    assert_eq!(entries(&w), ["goose-doc-guide", "tdd", "web-search"]);
    // A plugin's skill of that name is the one goose finds.
    assert_eq!(linked(&w)["web-search"], plugin.join("skills/mine"));

    // With none, a stub of each stands in, and says it is not available.
    stand_in(&w, FRESH)
        .prepare(&with_plugins(&w, Vec::new()))
        .unwrap();
    assert_eq!(entries(&w), ["goose-doc-guide", "web-search"]);
    for name in ["web-search", "goose-doc-guide"] {
        let stub = skills_folder(&w).join(name);
        assert!(!stub.is_symlink(), "{name}");
        let text = std::fs::read_to_string(stub.join("SKILL.md")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "---");
        assert_eq!(lines[1], format!("name: {name}"));
        assert!(
            lines[2].starts_with("description: Not available in this run"),
            "{text}"
        );
        assert_eq!(lines[3], "---");
    }
}
