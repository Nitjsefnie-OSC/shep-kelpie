# The runner answers on a socket of its own

This reopens one line of ADR 0002, "triggers down", for kelpie's own verbs. A runner now also answers a typed protocol on a Unix socket (a named pipe on Windows) of its own, and pushes its status down it. `shep trigger` stays.

Every verb reaches a runner through `shep trigger`: untyped params in, untyped JSON out, and nothing pushed. A verb that wants to follow a runner polls, and a param or an answer has no type to check at build time. The old limit of one reader thread per sheep, which made a slow trigger hold up every other, is fixed upstream (shep-pm/shep#1071, in shep 0.17, which kelpie's bump #1288 takes), so this ADR does not argue concurrency.

The typed protocol and the status stream are kelpie's, not shep's (ADR 0001). The types are kelpie's own vocabulary (work item, ruling, lease), and shep's channel carries strings by design, so a general feature would only move kelpie's types into shep with no other user.

This is not the socket ADR 0013 turned down. That one ran between a runner and the dog, for lease totals, and the dog keeps its notes. This one runs between a runner and kelpie's own verbs, for requests and status.

The decision:

- **Path.** `run/<project>.sock` under kelpie's home (`home::kelpie_home()`), beside the dog's `dog/lease.sock`. Not `~/.kelpie/run`: `home::shepherds` reads a `run` folder in `~/.kelpie` as a shepherd's home, which would switch the old-home check off.
- **Owner only.** `run/` is mode 0700 and the socket 0600. The runner binds under a private name and renames the socket into place, as `crates/dog/src/dog/door/mod.rs` does, so no client reaches a half-made socket. A live runner's socket is never replaced, a dead one's is.
- **Length.** `home::socket_fits` checks the path before the bind. A path too long refuses the socket with that message and the runner goes on without it, so no existing runner stops starting.
- **Windows.** A named pipe, its name from `ipc::address`, over `shep_core::transport`. One protocol on both platforms, with the transport doing the difference.
- **Local only.** No TCP listener. The socket's mode is the only access check, and a listener would need its own. A remote client is #724's separate decision.
- **Frames.** One JSON object per line, at most 64 KiB (`LONGEST_LINE` of the lease door, `crates/core/src/lease/door.rs`). A longer line closes the connection, so a stuck or hostile peer cannot make the runner buffer without end.
- **Hello.** The first frame each way carries an integer `protocol`, starting at 1. The client refuses a runner below its own version, as ADR 0016 refuses an earlier shepherd, and takes a later one, since a later runner still reads what an earlier client sends.
- **Bumping.** Only for a change an older peer cannot read. A new request or an optional field does not bump, so most releases leave the number alone.
- **Unknown request.** A typed error naming the request and the runner's version, and the connection stays open. A newer client asking an older runner for something new gets a message it can show and falls back on, not a dropped connection.
- **Requests and answers.** One typed request per action in `trigger::Request`. Answers carry the JSON `shep trigger` returns today, and `shep trigger` runs the same handler, so the two paths cannot drift.
- **Status stream.** A `watch` request, then the full snapshot each time the loop posts, with consecutive identical snapshots merged. A snapshot is small and a client needs no state to read one. Diffs wait until the size matters.
- **Fallback.** For one release the verbs use `shep trigger` when no runner answers on the socket (missing, refused, or an older protocol), logged once per command. A request that timed out never falls back, since the runner may have done it and a second try would repeat a change. The release after drops the fallback, and `shep trigger` stays as the maintainer's own path.
- **Unchanged.** The shepherd channel for pause, stop, handover and grants, and the lease notes of ADR 0013.

## Considered options

- **The socket in the project's folder.** macOS caps a socket's path at 104 bytes, and a project's folder sits deeper than `run/`. A name under `run/` stays shorter.
- **A TCP listener.** It needs authentication and a port, where a socket's mode already says who may connect.
- **The typed protocol or the stream as a shep feature.** The types are kelpie's own, and shep's channel carries strings by design. Shep takes what any dog could use (shep-pm/shep#623 and #624), and this is not that.
- **Keeping `shep trigger` as the only path.** Params and answers stay untyped, nothing is pushed, and every `watch` stays a poll.
- **Status events as diffs now.** A client would need the earlier snapshot and a rule for a missed event, to save bytes on a snapshot that is small today.
- **Refusing a runner's start over a too-long socket path.** A runner that ran yesterday would stop on a long `SHEP_HOME`, for a socket it does not need to do its work.

## Consequences

- A verb that finds no socket, a refused connection or an older runner uses `shep trigger` as before for one release, so an upgrade needs no ordering between verbs and runners.
- Two ways into a runner share one handler per action, so a test of the handler covers both.
- A runner on a path too long has no socket and no stream. Its verbs fall back to `shep trigger`.
- The protocol is a second surface to keep readable: a change an older peer cannot read bumps `protocol`, and the client's refusal says to upgrade the runner.
- The first slice is this ADR alone. The socket, the requests, the stream and the verbs' move follow in later slices of #1236.
