//! The experts' calls: their folders, files and fences
//!
//! The advisor and the researcher run a new session each, in a folder of
//! their own that kelpie wrote what they read into. Of kelpie's and the
//! shepherd's homes each reads its folder alone, and no secret-shaped file
//! in the code it reads. Neither writes a file or runs a command: the reply
//! is the vote. The advisor reads the worktrees and the checkout too, and
//! reaches no host but the model's. The researcher reads nothing of the
//! code, and reaches the web, read only, plus the hosts the project names
//! in `researcher.allowed_domains`. On pi, which has no web tool, the web is
//! the `pi-web-access` package, which the call's reach names when `tools
//! install` has put it there. On goose, which has none either, it is the
//! `duckduckgo-websearch` package, run as the stdio MCP server `kelpie-web`.
//!
//! The worker's vote is its own session resumed once, with the tools of a
//! retro: it reads its worktree and changes nothing.

use super::ask;
use crate::adapters::goose::web;
use crate::ports::{AgentCall, Reach, Role, Session, Tools};
use crate::runner::Runner;
use crate::settings::{AgentHarness, ExpertAgent};
use crate::state::Ruling;
use crate::tools::{GOOSE_WEB, GOOSE_WEB_FILE, PI_WEB_ACCESS};
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
            .ticket()
            .and_then(|t| self.state.item_by_ticket(t))
            .map(|i| i.worktree.clone());
        let prompt = ask::advisor(
            ruling.id,
            ruling.ticket(),
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
        let mut reach = self.fenced_reach(&folder, &[], Vec::new(), hosts);
        self.with_web(&agent.model.harness, &mut reach);
        let prompt = ask::researcher(ruling.id, ruling.ticket(), brief, &files);
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

    // On pi and goose, which have no web tool, the package that gives the
    // researcher one. A package that is not installed leaves it reading only,
    // and a note says so.
    fn with_web(&self, harness: &AgentHarness, reach: &mut Reach) {
        // parity: harness.web-research
        match harness {
            AgentHarness::Pi(_) => self.with_pi_web(reach),
            AgentHarness::Goose(_) => self.with_goose_web(reach),
            _ => {}
        }
    }

    fn with_pi_web(&self, reach: &mut Reach) {
        let tools = &self.paths.tools;
        let package = tools.pi_package(PI_WEB_ACCESS);
        if !tools.has_pi_package(PI_WEB_ACCESS) {
            self.notes.push(format!(
                "the researcher runs without the web: {} is not there, so run \
                 `shep kelpie tools install`",
                package.display()
            ));
            return;
        }
        reach.extensions.push(package);
    }

    // The server file runs under the sandbox, which reads none of kelpie's
    // home by default, so the package's `node_modules` folder is read-granted:
    // the server's own dependencies sit beside it.
    fn with_goose_web(&self, reach: &mut Reach) {
        let tools = &self.paths.tools;
        let package = tools.pi_package(GOOSE_WEB);
        let server = package.join(GOOSE_WEB_FILE);
        if !tools.has_pi_package(GOOSE_WEB) || !server.is_file() {
            self.notes.push(format!(
                "the researcher runs without the web: {} is not there, so run \
                 `shep kelpie tools install`",
                server.display()
            ));
            return;
        }
        reach.read.extend(package.parent().map(ToOwned::to_owned));
        reach.mcp_servers.insert(
            web::SERVER.to_owned(),
            serde_json::json!({
                "type": "stdio",
                "command": "node",
                "args": [server],
            }),
        );
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
        let item = (ruling.ticket().and_then(|t| self.state.item_by_ticket(t)))
            .ok_or("it has no work item")?;
        let prompt = ask::worker(ruling.id, ruling.ticket(), brief);
        let session = Session::Resume(item.session.clone());
        let mut call = self.worker_call(item, session, Some(prompt), false)?;
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
            issue: ruling.issue.unwrap_or(0),
            settings: self.paths.worker.join(format!("{name}-settings.json")),
            instructions: Some(instructions),
            reach,
            ..AgentCall::new(
                role,
                tools,
                agent.model.harness.clone(),
                agent.model.model.as_str().to_owned(),
                agent.model.effort,
                Session::New(id),
                folder,
                prompt,
            )
        })
    }
}
