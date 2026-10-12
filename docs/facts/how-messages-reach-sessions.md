# How messages reach sessions

Read from the desktop app's bundle and the CLI binary, 2026-09-25.

- Desktop app sessions run through the Agent SDK: the app spawns `claude --input-format stream-json --output-format stream-json` and holds its stdin. Its session tools are an in-process SDK MCP server with no socket or port. Sending a message writes a user turn into the target's stdin. An outside program cannot reach a desktop session.
- CLI sessions each register `~/.claude/sessions/<pid>.json` with a messaging socket (`/tmp/cc-socks/<pid>.sock`) and a 0600 peer token file. `SendMessage` is JSON lines over that socket, with an auth line first. The protocol is undocumented.
- `claude agents --json` lists live sessions, interactive and background, with their status. It is meant for scripting and needs no TTY.
- Background sessions: `claude --bg` prints an id that `claude attach`, `logs`, `stop` and `rm` take. `--bg --resume <id>` continues a session in the background.
- An MCP server can push `notifications/claude/channel` into a session, which would wake it. It is gated per organisation and off on Bedrock and Vertex.
- A socket message wakes an idle background session, about 5 seconds, 3 times out of 3. It arrives framed as a message from another Claude session.
