//! The project manager's call: its folder, files and fence
//!
//! It runs in its own folder, which holds a copy of the board as it stood
//! when it woke, each open branch's diff and its notes. Of kelpie's and the
//! shepherd's homes it reads that folder and the project's worktrees alone,
//! and it reads the checkout, never a secret-shaped file in either, `gh`'s
//! config or the credentials no worker reads. The rest of the machine stays readable to its sandbox, as
//! for a reviewer, since Claude Code reads its own files wherever they are.
//! It reaches no host but the model's, has only its read and file tools,
//! and writes only by adding to its notes, which a hook enforces.

use std::collections::BTreeMap;
use std::path::Path;

use crate::hooks::Hooks;
use crate::ports::{AgentCall, Fence, Guard, PM_NOTES, Reach, Role, Session, Tools};
use crate::runner::Runner;
use crate::settings::PmAgent;
use crate::skills::Step;

/// `gh`'s config, its token with it, which a worker reads to open its pull
/// request and the project manager never needs
const GH_CONFIG: &str = "~/.config/gh/**";

/// The files under the checkout and the worktrees that it never reads,
/// since each is where a secret is kept: a remote's URL in `.git/config`
/// can carry a token, and the rest hold keys and tokens by convention
pub(in crate::runner) const SECRET_SHAPED: [&str; 7] = [
    ".git/config",
    "**/.env",
    "**/.env.*",
    "**/*.pem",
    "**/*.key",
    "**/.npmrc",
    "**/.netrc",
];

impl Runner {
    // The call, with its folder, notes, settings and prompt on disk, and the
    // `board` where it `reads` it: a compaction or a retro reads neither.
    pub(super) fn pm_call(
        &mut self,
        agent: &PmAgent,
        session: Session,
        prompt: String,
        board: Option<String>,
    ) -> Result<AgentCall, String> {
        let call = self.pm_call_made(agent, session, prompt, board)?;
        self.prepared(call)
    }

    /// The batch retro's call: the project manager's agent in its folder, with
    /// no standing instructions, tools that read and search only, and the plugin
    /// folder its skill is in
    pub(in crate::runner) fn pm_retro_call(
        &mut self,
        agent: &PmAgent,
        session: Session,
        prompt: String,
    ) -> Result<AgentCall, String> {
        let mut call = self.pm_call_made(agent, session, prompt, None)?;
        call.tools = Tools::Retro;
        call.instructions = None;
        call.settings = self.paths.worker.join("pm-retro-settings.json");
        call.plugin_dirs = (self.skills.plugin_dir(Step::Retro).into_iter())
            .map(Path::to_owned)
            .collect();
        self.prepared(call)
    }

    fn pm_call_made(
        &mut self,
        agent: &PmAgent,
        session: Session,
        prompt: String,
        board: Option<String>,
    ) -> Result<AgentCall, String> {
        let folder = self.paths.pm.clone();
        let failed = |what: &std::path::Path, e: std::io::Error| {
            format!("cannot write {}: {}", what.display(), e.kind())
        };
        std::fs::create_dir_all(&folder).map_err(|e| failed(&folder, e))?;
        let notes = folder.join(PM_NOTES);
        if !notes.exists() {
            std::fs::write(&notes, "").map_err(|e| failed(&notes, e))?;
        }
        // A copy, so the board it reads holds still while it reads, with the
        // code to read. It is the same text the wake's prompt carried.
        if let Some(text) = &board {
            let path = folder.join("board.md");
            std::fs::write(&path, text).map_err(|e| failed(&path, e))?;
        }
        let instructions = self.paths.worker.join("pm-instructions.md");
        let text = agent.prompt.as_deref().unwrap_or_default();
        crate::runner::turn::write(&self.paths.worker, &instructions, text)?;
        Ok(AgentCall {
            settings: self.paths.worker.join("pm-settings.json"),
            instructions: Some(instructions),
            reach: self.pm_reach(),
            ..AgentCall::new(
                Role::Pm,
                Tools::Pm,
                agent.model.harness.clone(),
                agent.model.model.as_str().to_owned(),
                agent.model.effort,
                session,
                folder,
                prompt,
            )
        })
    }

    /// The board a wake carries in its prompt and `board.md` in its folder
    /// holds: the board as the runner last wrote it, with the section naming
    /// each open work item's worktree and its diff
    ///
    /// # Errors
    ///
    /// A message naming the file that could not be read.
    pub(super) fn pm_board(&mut self) -> Result<String, String> {
        let folder = self.paths.pm.clone();
        std::fs::create_dir_all(&folder)
            .map_err(|e| format!("cannot make {}: {}", folder.display(), e.kind()))?;
        let mut text = match std::fs::read_to_string(&self.paths.board) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(format!(
                    "cannot read the board for the project manager: {}",
                    e.kind()
                ));
            }
        };
        text.push_str(&format!("\n{}", self.pm_diffs(&folder)));
        Ok(text)
    }

    // Its folder to read and write, the worktrees and checkout to read;
    // nothing else of the homes, nor `gh`'s token; no host but the model's.
    pub(super) fn pm_reach(&self) -> Reach {
        let folder = self.paths.pm.clone();
        let code = [self.paths.worktrees(), self.settings.git.checkout.clone()];
        self.fenced_reach(&folder, &code, vec![folder.clone()], Vec::new())
    }

    // A session that reads its `folder` and the `code` trees, writes `write`
    // and reaches `hosts` besides the model's; of kelpie's and the shepherd's
    // homes it reads its folder alone, and in the code trees no secret-shaped file.
    pub(in crate::runner) fn fenced_reach(
        &self,
        folder: &std::path::Path,
        code: &[std::path::PathBuf],
        write: Vec<std::path::PathBuf>,
        hosts: Vec<String>,
    ) -> Reach {
        let folder = folder.to_owned();
        let (home, shep, kelpie) = (&self.home, &self.paths.shep_home, &self.paths.kelpie_home);
        let homes =
            crate::profile::home_denials([home.as_path(), shep.as_path(), kelpie.as_path()]);
        // Both layers let a deny inside a folder it reads win: the sandbox
        // emits it after the folder's allow, and Claude Code's denies win.
        let secrets = code.iter().flat_map(|tree| {
            (SECRET_SHAPED.iter()).map(move |file| format!("{}/{file}", tree.display()))
        });
        let no_read = (crate::profile::CREDENTIALS.iter())
            .chain(&[GH_CONFIG])
            .map(|p| (*p).to_owned())
            .chain(homes)
            .chain(secrets)
            .collect();
        Reach {
            // Outside its folder, so its file tools may read them under `-p`.
            read: code.to_vec(),
            fence: Some(Box::new(Fence {
                write,
                no_write: Vec::new(),
                no_read,
                // The guard's own binary, which these calls' hooks run inside
                // the fence, and exec needs file-read of the file it runs.
                read: [folder.clone(), self.kelpie.clone()]
                    .into_iter()
                    .chain(code.iter().cloned())
                    .collect(),
                hosts,
                no_commands: Vec::new(),
                env: BTreeMap::new(),
                sockets: Vec::new(),
                guard: Guard {
                    kelpie: self.kelpie.clone(),
                    worktree: folder.clone(),
                    build: folder.clone(),
                    git_common_dir: folder,
                    base: self.settings.git.base().to_owned(),
                    kelpie_pushes: false,
                    folders: vec![kelpie.clone(), self.settings.git.checkout.clone()],
                    issues: None,
                    hooks: Hooks::default(),
                },
                hooks: Vec::new(),
            })),
            block_keychain: false,
            code_graph: None,
            browser: None,
            mcp_servers: BTreeMap::new(),
        }
    }
}
