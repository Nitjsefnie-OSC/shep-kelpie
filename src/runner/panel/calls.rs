//! The experts' calls: their folders, files and fences
//!
//! The advisor and the researcher run a new session each, in a folder of
//! their own that kelpie wrote what they read into. Of kelpie's and the
//! shepherd's homes each reads its folder alone, and no secret-shaped file
//! in the code it reads. Neither writes a file or runs a command: the reply
//! is the vote. The advisor reads the worktrees and the checkout too, and
//! reaches no host but the model's. The researcher reads nothing of the
//! code, and reaches the web, read only, plus the hosts the project names
//! in `researcher.allowed_domains`.
//!
//! The worker's vote is its own session resumed once, with the tools of a
//! retro: it reads its worktree and changes nothing.

use super::ask;
use crate::ports::{AgentCall, Role, Session, Tools};
use crate::runner::Runner;
use crate::settings::ExpertAgent;
use crate::state::Ruling;
use crate::work_item::new_session_id;

impl Runner {
    /// The advisor's call to vote on `ruling`, which `brief` states
    ///
    /// # Errors
    ///
    /// Why the call cannot be made.
    pub(in crate::runner) fn advisor_call(
        &self,
        ruling: &Ruling,
        brief: &str,
    ) -> Result<AgentCall, String> {
        let agent = self.agents.advisor.as_ref().ok_or("no advisor is named")?;
        self.advisor_folder(ruling, brief)?;
        let folder = self.paths.advisor.clone();
        let code = [self.paths.worktrees(), self.settings.git.checkout.clone()];
        let reach = self.fenced_reach(&folder, &code, Vec::new(), Vec::new());
        let worktree = ruling
            .issue
            .and_then(|n| self.state.item(n))
            .map(|i| i.worktree.clone());
        let prompt = ask::advisor(
            ruling.id,
            ruling.issue,
            brief,
            worktree.as_deref(),
            &self.settings.git.checkout,
        );
        self.expert_call(
            agent,
            Role::Advisor,
            Tools::Advise,
            ruling,
            folder,
            prompt,
            reach,
        )
    }

    /// The researcher's call to vote on `ruling`, which `brief` states
    ///
    /// `worker` is the worker's session as text, when it has one.
    ///
    /// # Errors
    ///
    /// Why the call cannot be made.
    pub(in crate::runner) fn researcher_call(
        &self,
        ruling: &Ruling,
        brief: &str,
        worker: Option<String>,
    ) -> Result<AgentCall, String> {
        let agent = self
            .agents
            .researcher
            .as_ref()
            .ok_or("no researcher is named")?;
        let files = self.researcher_folder(ruling, brief, worker)?;
        let folder = self.paths.researcher.clone();
        let hosts = (self.settings.researcher.allowed_domains.iter())
            .map(|d| d.as_str().to_owned())
            .collect();
        let reach = self.fenced_reach(&folder, &[], Vec::new(), hosts);
        let prompt = ask::researcher(ruling.id, ruling.issue, brief, &files);
        self.expert_call(
            agent,
            Role::Researcher,
            Tools::Research,
            ruling,
            folder,
            prompt,
            reach,
        )
    }

    /// The call that resumes the worker's session to vote on `ruling`, which
    /// `brief` states
    ///
    /// # Errors
    ///
    /// Why the call cannot be made: it has no worker, or its call cannot be set up.
    pub(in crate::runner) fn worker_vote_call(
        &self,
        ruling: &Ruling,
        brief: &str,
    ) -> Result<AgentCall, String> {
        let item = (ruling.issue.and_then(|n| self.state.item(n))).ok_or("it has no work item")?;
        let prompt = ask::worker(ruling.id, ruling.issue, brief);
        let mut call =
            self.worker_call(item, Session::Resume(item.session.clone()), Some(prompt))?;
        call.tools = Tools::Retro;
        call.settings = (self.paths.worker).join(format!("vote-settings-{}.json", item.issue));
        self.prepared(call)
    }

    // A new session of `agent`, in `folder`, with its file's prompt as its instructions.
    #[allow(clippy::too_many_arguments)]
    fn expert_call(
        &self,
        agent: &ExpertAgent,
        role: Role,
        tools: Tools,
        ruling: &Ruling,
        folder: std::path::PathBuf,
        prompt: String,
        reach: crate::ports::Reach,
    ) -> Result<AgentCall, String> {
        let id = new_session_id().map_err(|e| format!("cannot draw a session id: {e}"))?;
        let name = role.as_str();
        let instructions = self.paths.worker.join(format!("{name}-instructions.md"));
        let text = agent.prompt.as_deref().unwrap_or_default();
        crate::runner::turn::write(&self.paths.worker, &instructions, text)?;
        self.prepared(AgentCall {
            role,
            harness: agent.model.harness.clone(),
            issue: ruling.issue.unwrap_or(0),
            model: agent.model.model.as_str().to_owned(),
            effort: agent.model.effort,
            session: Session::New(id),
            cwd: folder,
            settings: self.paths.worker.join(format!("{name}-settings.json")),
            instructions: Some(instructions),
            prompt,
            plugin_dirs: Vec::new(),
            tools,
            agent_tools: None,
            reach,
            compact_window: None,
        })
    }
}
