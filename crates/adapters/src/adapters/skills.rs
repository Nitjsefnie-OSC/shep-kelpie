//! The skill folders a call's plugins hold

use std::path::PathBuf;

use crate::ports::AgentCall;

/// The `skills/` folder of each of `call`'s plugins that has one, in the order
/// the call lists them
pub(in crate::adapters) fn skill_dirs(call: &AgentCall) -> Vec<PathBuf> {
    (call.plugin_dirs.iter())
        .map(|plugin| plugin.join("skills"))
        .filter(|dir| dir.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{Role, Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};

    fn call_with(plugins: Vec<PathBuf>) -> AgentCall {
        AgentCall {
            plugin_dirs: plugins,
            ..AgentCall::new(
                Role::Worker,
                Tools::Work,
                AgentHarness::Codex,
                "m",
                Effort::Low,
                Session::New(SessionId("s".into())),
                PathBuf::from("/wt"),
                String::new(),
            )
        }
    }

    #[test]
    fn a_plugin_with_a_skills_folder_names_it_and_one_without_names_nothing() {
        let dir = crate::test::short_tempdir();
        let (first, bare, last) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        std::fs::create_dir_all(first.join("skills/one")).unwrap();
        std::fs::create_dir_all(bare.join("commands")).unwrap();
        std::fs::create_dir_all(last.join("skills")).unwrap();

        let call = call_with(vec![last.clone(), bare, first.clone()]);
        assert_eq!(
            skill_dirs(&call),
            [last.join("skills"), first.join("skills")]
        );
        assert_eq!(skill_dirs(&call_with(Vec::new())), Vec::<PathBuf>::new());
    }
}
