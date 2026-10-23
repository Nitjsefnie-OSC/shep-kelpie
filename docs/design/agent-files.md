# Agent files

The agent file, the shipped agents and how a file builds on one. Harnesses, gateways and the agent settings are in agents.md.

- Agent = `<name>.md`, YAML frontmatter tagged `role` + Markdown body; bad file stops runner naming file + line. serde-saphyr (serde_yaml archived). ADR 0006, #313
- Reread each wake (edits land live). Item keeps its agent, runs the file as it is now. #313
- `extends` merges frontmatter keys as text before parsing (every check applies); parameters on a reference refused (each a new label + ledger key). #386, `crates/core/src/agents/extends.rs`
- Shipped agents are baselines kelpie owns, embedded from `agents/`; files in the folder say only what differs. `extends: kelpie:<name>` resolves to the embedded text, never a file: `:` is not in an `AgentName`, so it never collides with a file, every build uses its own baselines and `--rollback` needs nothing extra. A bare name in `extends` names a file only, no fallback to a shipped agent (`kelpie:nope` names the shipped ones, a bare name with no file says it names no file). #1190, `crates/core/src/agents/mod.rs`
- Baseline from the binary, not `.defaults/`: runners reread files each wake, so one still on the old build during an upgrade would read a new baseline from disk, and a key it does not know stops it. #1190
- `body: append` in a file's frontmatter puts its body after the parent's, blank line between, at each step of a chain. No `body` key: a non-empty body replaces the parent's, an empty one takes it. Any other value, or the key twice, refused with its line; blanked in the merged text so later lines keep their number. #1190, `crates/core/src/agents/extends.rs`
- `add` writes each missing shipped agent as a stub, `---\nextends: kelpie:<name>\n---\n` (`qwen` only where its script exists), never over a file. #1190, `src/flock/add/mod.rs`
- `agents/.defaults/<name>.md`: every shipped agent whole, mode 444, first comment says kelpie writes it and to override in `agents/<name>.md`. For reading and diffing, nothing loads it (the loader reads only `.md` files directly in `agents/`). Written by `add` and by the dog's start, so each upgrade's restart refreshes it; replaced by rename of a temp file named per writer (pid and counter), removed on failure, so a read never sees half and two refreshes never share one. #1190, `write_baselines`, `crates/dog/src/dog/mod.rs`
- No migration (the maintainer's install is the only one, fixed by hand), no hash tracking, no `doctor` line for a body that replaces its baseline's. #1190
