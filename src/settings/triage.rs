//! Whether and how kelpie triages issues other people open,
//! `[app.dogs.kelpie.triage]`

use std::num::{NonZeroU8, NonZeroU32};

use schemars::JsonSchema;
use serde::Deserialize;

use super::agents::find;
use super::{AgentName, ExpertAgent, Settings, SettingsError};
use crate::agents::{Agents, Role, Runs, TRIAGER};

/// The most questions one triage comment may ask
pub const MOST_QUESTIONS: u8 = 10;

/// Whose issues kelpie triages
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TriagePick {
    /// No one's: kelpie triages nothing
    #[default]
    Off,
    /// The maintainer's own issues
    Maintainer,
    /// Every person's issues
    All,
}

/// Whether and how kelpie triages issues other people open
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Triage {
    /// Whose issues kelpie triages. `off` when absent. Anything but `off`
    /// needs the GitHub App to cover the repo, since the triager would
    /// otherwise post as the maintainer.
    pub pick: TriagePick,
    /// Whole days without a word from the author before kelpie asks again or
    /// closes the triage, above 0. 7 when absent.
    pub wait_days: NonZeroU32,
    /// The most questions one triage comment asks, 1 to 10. 10 when absent.
    pub max_questions: NonZeroU8,
    /// The agent file whose role is `triager` that researches an issue.
    /// `triager` when absent.
    pub agent: AgentName,
    /// The most triage calls in any 24 hours. An issue past it waits for the
    /// next free place and is never dropped. 10 when absent, 0 for no cap.
    pub per_day: u32,
    /// The most triage calls on one author's issues in any 24 hours, logins
    /// compared ignoring case. An issue past it is labelled `needs-triage`
    /// with no call and no comment. 2 when absent, 0 for no cap.
    pub per_author_per_day: u32,
    /// An author whose account is younger than this many days, and who has no
    /// history in the repo, gets `needs-triage` with no call and no comment.
    /// 30 when absent, 0 for no check.
    pub min_account_days: u32,
    /// The most kilobytes of an issue body that reach the triager, which the
    /// call is given cut at that size with a line saying so. The title is
    /// never cut. 20 when absent, 0 sends the whole body.
    pub max_body_kb: u32,
}

impl Default for Triage {
    fn default() -> Self {
        Self {
            pick: TriagePick::default(),
            wait_days: NonZeroU32::new(7).unwrap_or(NonZeroU32::MIN),
            max_questions: NonZeroU8::new(MOST_QUESTIONS).unwrap_or(NonZeroU8::MIN),
            agent: AgentName::kelpies(TRIAGER),
            per_day: 10,
            per_author_per_day: 2,
            min_account_days: 30,
            max_body_kb: 20,
        }
    }
}

impl Triage {
    /// Whether kelpie triages no one's issue
    #[inline]
    pub fn is_off(&self) -> bool {
        self.pick == TriagePick::Off
    }

    /// Checks the keys against their limits
    ///
    /// # Errors
    ///
    /// [`SettingsError::Invalid`] naming `triage.max_questions` when it is
    /// above 10.
    pub fn check(&self) -> Result<(), SettingsError> {
        match self.max_questions.get() <= MOST_QUESTIONS {
            true => Ok(()),
            false => Err(SettingsError::Invalid {
                setting: "triage.max_questions",
                reason: format!(
                    "it is {} and a comment asks at most {MOST_QUESTIONS}: set it from 1 to \
                     {MOST_QUESTIONS}",
                    self.max_questions
                ),
            }),
        }
    }
}

impl Settings {
    /// The triager the project names in `triage.agent`
    ///
    /// # Errors
    ///
    /// [`SettingsError::Invalid`] naming `triage.agent` when it has no agent
    /// file, or its file's role is not `triager`.
    pub fn triager(&self, agents: &Agents) -> Result<ExpertAgent, SettingsError> {
        let at = "triage.agent";
        let name = &self.triage.agent;
        let agent = find(agents, name, at, at, Role::Triager)?;
        let Runs::Session { model, limit } = &agent.runs else {
            unreachable!("a triager's file only parses to a session")
        };
        Ok(ExpertAgent {
            name: name.clone(),
            model: model.clone(),
            limit: limit.clone(),
            prompt: agent.prompt.clone(),
        })
    }
}
