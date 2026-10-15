//! Agent calls routed to the adapter for the harness each one names

use std::fmt;
use std::sync::Arc;

use super::{
    Adopted, AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, SessionId, Transcript,
};
use crate::settings::Harness;

/// One adapter per harness, each taking the calls that name it
pub struct Routed {
    claude_code: Arc<dyn Agents>,
    pi: Arc<dyn Agents>,
    codex: Arc<dyn Agents>,
    goose: Arc<dyn Agents>,
}

impl Routed {
    /// Claude Code's calls to `claude_code`, pi's to `pi`, Codex's to `codex`
    /// and goose's to `goose`
    pub fn new(
        claude_code: Arc<dyn Agents>,
        pi: Arc<dyn Agents>,
        codex: Arc<dyn Agents>,
        goose: Arc<dyn Agents>,
    ) -> Self {
        Self {
            claude_code,
            pi,
            codex,
            goose,
        }
    }

    fn adapter(&self, call: &AgentCall) -> &dyn Agents {
        self.adapter_of(call.harness.harness())
    }

    fn adapter_of(&self, harness: Harness) -> &dyn Agents {
        match harness {
            Harness::ClaudeCode => self.claude_code.as_ref(),
            Harness::Pi => self.pi.as_ref(),
            Harness::Codex => self.codex.as_ref(),
            Harness::Goose => self.goose.as_ref(),
            #[cfg(test)]
            Harness::StandIn => self.claude_code.as_ref(),
        }
    }
}

impl fmt::Debug for Routed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Routed").finish_non_exhaustive()
    }
}

impl Agents for Routed {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.adapter(call).prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.adapter(call).run(call, ending)
    }

    fn adoption(&self, call: &AgentCall) -> Result<Adopted, String> {
        self.adapter(call).adoption(call)
    }

    fn adopt(&self, adopted: &Adopted, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.adapter_of(adopted.harness).adopt(adopted, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        self.adapter(call).transcript(call)
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        self.adapter(call).last_active(call)
    }

    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        self.adapter(call).foreground(call)
    }

    // Only the call names a harness, and a session id is unique to one.
    fn foreground_ended(&self, session: &SessionId) {
        for adapter in [&self.claude_code, &self.pi, &self.codex, &self.goose] {
            adapter.foreground_ended(session);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ports::{Role, Session, SessionId, Tools, Usage};
    use crate::settings::{AgentHarness, ContextSize, Effort, EndpointUrl, ModelServer};

    // An adapter that answers with its own name, so a reply says who ran it.
    struct Named(&'static str);

    impl Agents for Named {
        fn prepare(&self, _: &AgentCall) -> Result<(), AgentError> {
            Err(AgentError::Setup(self.0.into()))
        }

        fn run(&self, call: &AgentCall, _: &Ending) -> Result<AgentReply, AgentError> {
            Ok(AgentReply {
                session_id: call.session.id().clone(),
                text: self.0.into(),
                usage: Usage::default(),
                session_cost: None,
                context: None,
                compactions: 0,
            })
        }
    }

    fn call(harness: AgentHarness) -> AgentCall {
        AgentCall {
            issue: 7,
            settings: PathBuf::from("/k/worker/settings.json"),
            ..AgentCall::new(
                Role::Worker,
                Tools::Work,
                harness,
                "m",
                Effort::Low,
                Session::New(SessionId("s".into())),
                PathBuf::from("/k/wt/7"),
                "go",
            )
        }
    }

    #[test]
    fn each_call_goes_to_the_adapter_for_its_harness() {
        let routed = Routed::new(
            Arc::new(Named("claude")),
            Arc::new(Named("pi")),
            Arc::new(Named("codex")),
            Arc::new(Named("goose")),
        );
        let pi = AgentHarness::Pi(ModelServer {
            host: crate::settings::ModelHost::Url(
                EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap(),
            ),
            context: ContextSize::try_from(65_536).unwrap(),
        });
        let goose = AgentHarness::Goose(ModelServer {
            host: crate::settings::ModelHost::Url(
                EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap(),
            ),
            context: ContextSize::try_from(65_536).unwrap(),
        });
        let harnesses = [
            (AgentHarness::ClaudeCode, "claude"),
            (pi, "pi"),
            (AgentHarness::Codex, "codex"),
            (goose, "goose"),
        ];
        for (harness, name) in harnesses {
            let call = call(harness);
            assert_eq!(routed.run(&call, &Ending::default()).unwrap().text, name);
            assert_eq!(routed.prepare(&call), Err(AgentError::Setup(name.into())));
        }
    }

    // An adapter that notes the terminal sessions it was told ended.
    struct Noting(Arc<std::sync::Mutex<Vec<String>>>, &'static str);

    impl Agents for Noting {
        fn prepare(&self, _: &AgentCall) -> Result<(), AgentError> {
            Ok(())
        }

        fn run(&self, _: &AgentCall, _: &Ending) -> Result<AgentReply, AgentError> {
            Err(AgentError::Stopped)
        }

        fn foreground_ended(&self, session: &SessionId) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{}:{}", self.1, session.0));
        }
    }

    #[test]
    fn the_end_of_a_terminal_session_reaches_every_adapter() {
        let told = Arc::new(std::sync::Mutex::new(Vec::new()));
        let noting = |name| Arc::new(Noting(Arc::clone(&told), name));
        let routed = Routed::new(
            noting("claude"),
            noting("pi"),
            noting("codex"),
            noting("goose"),
        );

        routed.foreground_ended(&SessionId("s".into()));

        assert_eq!(
            *told.lock().unwrap(),
            ["claude:s", "pi:s", "codex:s", "goose:s"]
        );
    }
}
