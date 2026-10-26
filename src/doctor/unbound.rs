//! Each agent file that binds nothing, which no project can name until it does

use super::Line;
use crate::agents::Agents;

/// A line for each file in `agents` that binds no harness, with the command that binds it
pub(super) fn checks(agents: &Agents) -> Vec<Line> {
    (agents.unbound().iter())
        .map(|unbound| {
            Line::missing(
                format!("agent {}", unbound.name),
                format!("`{}.md` binds no harness, model or effort", unbound.name),
                unbound.command(),
            )
        })
        .collect()
}
