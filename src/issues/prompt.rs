//! The issue writer's prompt: its file's body, then what kelpie adds for
//! the project and the way it runs

use super::{Mode, Project, Writer};
use crate::board::AGENT_LABEL;
use crate::settings::{Implementer, RoleAgents, TicketSource};

/// The issue writer's whole prompt for `project`: its file's body, then the
/// project's implementers, the commands it has and how `mode` ends
pub fn instructions(writer: &Writer, project: &Project<'_>, mode: Mode) -> String {
    let status = mode.status();
    let base = project.settings.git.base();
    let local =
        (project.settings.tickets.source == TicketSource::Local).then(|| project.paths.name());
    let ending = match mode {
        Mode::Headless => format!(
            "You run on your own, and no one answers a question, so you cannot grill \
             the maintainer. Where the request leaves a choice open, take the one the \
             repo's docs point to, and say in the issue what you chose and why. \
             {FACTS}\n\n\
             A decision the repo's docs and code do not settle is one only the maintainer \
             can make. Do not guess it silently: end the issue it affects with an \
             `## Open questions` section, after the acceptance criteria, that asks it in \
             the format of the `grilling` skill, with your recommended answer, so the \
             maintainer can answer on the issue before labelling it ready. Number the \
             questions from 1 in each issue, and separate them with `---`:\n\n\
             ```\n\
             ## Open questions\n\n\
             {QUESTION}\n\n\
             ---\n\n\
             ❓ **Q2** - **<question title>**: <question body>\n\n\
             ➡️ <your recommended answer>\n\
             ```\n\n\
             Put a question in the issue it holds up; one that holds up the whole \
             request goes in the parent. Write the section only if you left a decision \
             open, and then write each acceptance criterion as if your recommended \
             answer holds.\n\n\
             File every issue with `{status}`, which holds it for the maintainer to \
             read before any agent takes it, and with the `{AGENT_LABEL}` label you \
             recommend.\n\n\
             When you have filed everything, end with one JSON object and nothing \
             else:\n\
             {{\"filed\": [<each issue's number, or local ticket's id such as \"k12\", a parent \
             first>], \"note\": \"<one \
             sentence for the maintainer, or empty>\"}}\n\
             If the request needs nothing filed, such as one an open issue already \
             covers, file nothing and say why in the note."
        ),
        Mode::Interactive => format!(
            "You are working with the maintainer in their terminal, and the session \
             starts with the `grill-with-docs` skill: grill the maintainer on the \
             request, round by round, until nothing is left silently assumed. Research \
             the repo before you ask. {FACTS}\n\n\
             File nothing until the maintainer confirms you share an understanding. \
             Then propose the issues: each one's title, what it delivers, its priority \
             and what blocks it. Ask whether the scope and any split are right. Then \
             preview the `{AGENT_LABEL}` label you would put on each, and ask whether to \
             apply those, apply none, or change some: an issue with none goes to the \
             default implementer. File only what the maintainer agrees, with `{status}`, \
             which puts it on kelpie's board, and only the agent labels they agreed. \
             Then list what you filed, by number.\n\n\
             You are in a checkout of a docs branch cut from `origin/{base}`. As decisions \
             settle, `domain-modeling` writes terms to `GLOSSARY.md` and records ADRs \
             in `docs/adr/`: write them here. Those are the only files you may write, \
             and your file tools refuse any other. Do not commit or push. When the \
             session ends, kelpie commits what you wrote, pushes the branch, opens a \
             draft pull request beside the issues and links it from each issue you \
             filed."
        ),
    };
    format!(
        "{}\n\n--- this project ---\n\n{}\n\n{}\n\n{ending}\n",
        writer.prompt.trim_end(),
        implementers(project.agents),
        commands(status, local.as_deref()),
    )
}

// One open question, in the format of the `grilling` skill.
const QUESTION: &str = "❓ **Q1** - **<question title>**: <question body, which may run to \
    several paragraphs and offer choices>\n\n➡️ <your recommended answer>";

// Facts are the repo's to give. The skills the issue writer runs would send a
// sub-agent for one, and its settings deny sub-agents.
const FACTS: &str = "Facts come from the repo, never the maintainer. A skill may tell you \
    to send a sub-agent to find one: you have none, so look it up yourself with Read, Grep \
    and Glob. Ask the maintainer only for decisions.";

/// The listed implementers, the default first, as an `agent:` label names them
pub fn implementers(agents: &RoleAgents) -> String {
    let default = &agents.default_implementer;
    let line = |i: &Implementer| {
        let what = match (i.name == default.name, i.is_local()) {
            (true, true) => ", the default, a local model",
            (true, false) => ", the default",
            (false, true) => ", a local model",
            (false, false) => "",
        };
        format!(
            "- `{}`: {} at {} effort{what}",
            i.name,
            i.model.model.as_str(),
            i.model.effort.as_str()
        )
    };
    let rest = (agents.implementers.iter()).filter(|i| i.name != default.name);
    let lines: Vec<String> = std::iter::once(default).chain(rest).map(line).collect();
    format!(
        "The implementers this project lists, the default first. Each issue's \
         `{AGENT_LABEL}` label, where it has one, names one of them, and no other:\n\n{}",
        lines.join("\n")
    )
}

// The commands the guard runs, as the issue writer should write them: `gh`'s
// on GitHub, and on the local project `local`, `shep kelpie ticket`'s.
fn commands(status: &str, local: Option<&str>) -> String {
    match local {
        Some(project) => local_commands(status, project),
        None => github_commands(status),
    }
}

fn local_commands(status: &str, project: &str) -> String {
    format!(
        "You have Read, Grep and Glob to read the repo, and Bash for these commands \
         only, one to a call where you can, written as plain words: no `$`, backticks, \
         pipes, redirects or variables, but for the body's `$(cat <<'EOF'`. Kelpie's \
         guard refuses anything else, git included. This project's tickets are local \
         files, not GitHub issues, and `gh issue create` is refused. Each ticket has an id \
         such as `k12`, which `ticket new` prints, and every command names this project \
         with `-p {project}`.\n\n\
         - File a ticket: `shep kelpie ticket new '<title>' -p {project} --label '{status}' \
         --label '<priority label>' --label '{AGENT_LABEL}<name>' --body \"$(cat \
         <<'EOF'`, then the body's lines, then `EOF` and `)\"` on the next line. Leave out \
         the `{AGENT_LABEL}` label for none, and the priority label where the repo sets no \
         priority rules. Add `--parent <id>` to make it a sub-ticket of one you filed, and \
         `--blocked-by <id>` for each ticket that blocks it\n\
         - Change a ticket you filed: `shep kelpie ticket edit <id> -p {project} \
         --unlabel '{AGENT_LABEL}<old>' --label '{AGENT_LABEL}<new>'`, or either flag \
         alone, and `--parent <id>` or `--blocked-by <id>` to link it\n\
         - Read this project's tickets: `shep kelpie ticket show <id> -p {project}`, \
         `shep kelpie ticket list -p {project}`, and `--all` to include the closed ones\n\
         - Read this repo's GitHub issues: `gh issue view <number>`, `gh issue list \
         --search '<words>'`\n\n\
         You edit and link only the tickets you filed in this session. A body names no \
         path outside the repo."
    )
}

fn github_commands(status: &str) -> String {
    format!(
        "You have Read, Grep and Glob to read the repo, and Bash for these commands \
         only, one to a call where you can, written as plain words: no `$`, backticks, \
         pipes, redirects or variables. Kelpie's guard refuses anything else, git \
         included.\n\n\
         - File an issue: `gh issue create --title '<title>' --label '{status}' --label \
         '<priority label>' --label '{AGENT_LABEL}<name>' --body-file - <<'EOF'`, then the \
         body's lines, then `EOF`. Leave out the `{AGENT_LABEL}` label for none, and the \
         priority label where the repo sets no priority rules\n\
         - Change the agent of an issue you filed: `gh issue edit <number> --remove-label \
         '{AGENT_LABEL}<old>' --add-label '{AGENT_LABEL}<new>'`, or either flag alone\n\
         - Read this repo's issues: `gh issue view <number>`, `gh issue list --search \
         '<words>'`\n\
         - An issue's id, which a link takes: `gh api \
         'repos/{{owner}}/{{repo}}/issues/<number>' --jq .id`, alone in its call\n\
         - Make an issue you filed a sub-issue of another you filed: `gh api -X POST \
         'repos/{{owner}}/{{repo}}/issues/<parent>/sub_issues' -F sub_issue_id=<its id>`\n\
         - Mark an issue you filed blocked by another: `gh api -X POST \
         'repos/{{owner}}/{{repo}}/issues/<number>/dependencies/blocked_by' -F \
         issue_id=<the other's id>`\n\n\
         You edit and link only the issues you filed in this session, by the ids you \
         read as above. Write `{{owner}}/{{repo}}` as it stands: gh fills it in from \
         this checkout."
    )
}
