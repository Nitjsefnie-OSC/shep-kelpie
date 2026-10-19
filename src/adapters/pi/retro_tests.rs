//! A pi batch retro, whose skill loads from the plugin folder the call names

use super::tests::{FRESH_ID, URL, World, id, strings, values};
use super::*;
use crate::ports::Role;
use crate::settings::{SkillChoice, StepSkills};
use crate::skills::{Skills, Step};

#[test]
fn a_pi_retro_with_the_default_skill_loads_the_retro_skill_folder() {
    let w = World::new();
    let choice = StepSkills {
        retro: Some(SkillChoice::Kelpie {}),
        ..StepSkills::default()
    };
    let skills = Skills::load(&choice, &w.path("skills"));
    let mut call = w.call(URL, Role::Pm, Session::New(id(FRESH_ID)));
    call.tools = Tools::Retro;
    call.plugin_dirs = skills
        .plugin_dir(Step::Retro)
        .into_iter()
        .map(Path::to_owned)
        .collect();
    call.prompt = skills.invoke(Step::Retro, "10 work items of koji");

    let argv = strings(&call);
    let retro = w.path("skills/mattpocock/skills/retro");
    assert!(retro.join("SKILL.md").is_file());
    assert!(
        values(&argv, "--skill").contains(&retro.to_str().unwrap()),
        "{argv:?}"
    );
    assert!(argv.last().unwrap().starts_with("/skill:retro"), "{argv:?}");
}
