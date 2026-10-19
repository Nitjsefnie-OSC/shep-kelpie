# Project manager

The agent kelpie wakes to pick, answer and unstick, and the board file it reads.

- Runner writes `board.md` so PM never runs git or gh (code-written board decides as well). Keeps newest 50 events + PM cursor; issue bodies quoted (600 chars) behind `> `; text naming this machine withheld. #326, `src/runner/briefing.rs`
- Worker silent 10 min reads as idle (PM told only "busy" misses a hung worker). #326
- A call that has not had its gateway lease is never idle or stuck: quiet counts from the grant, and a wait past 10 min is its own wake, not a stuck item. Only a wait for a lease names the lease holder; a gateway that answered busy named no client as the cause, and the board names what the call waits for, and what it got, in the same words the wake uses. #964, `src/runner/briefing.rs`, `src/runner/flight.rs`
- Overlap from `git diff` + pairwise `git merge-tree` (git 2.38+), cached by commit, run without runner lock (trigger never waits on git). #326
- `agents.pm` opts a project in (`pm`, Opus 5.5 at medium; Sonnet cost about the same here); without one board rule picks and every ruling waits on maintainer. `src/runner/pm.rs`, #327
- Code wakes it, never a timer: free slot with 2+ choices, new conflict, stuck item, a lease wait past 10 min, worker question, `shep kelpie tell`. A stuck item, a conflict and a lease wait are reasons nobody waits on, so each holds 3 min and the ones that arrive inside it go in one wake; a free slot, a tell, a question, a follow-up, a vote or a pick ends the hold at once, carrying them. What the board no longer shows is dropped before the call starts, whatever ends the window, and nothing runs while nothing is left. A reason already queued takes the state the board shows now, and a wake whose session cannot resume starts a new one at once, its reasons held no second time. #327, #964, #987
- An item it answered `none` for is not woken for again while it is in the same state: parked on a ruling, until that ruling is answered; an idle call, for 30 min, then 60, then 2h at most. A new turn, phase or ruling is a new state; new output in the same turn is not (the 10 min idle flapping). When the hold ends the wake goes out again even if the board showed nothing more, once per hold, so a worker hung during a hold is not left unattended until its turn ceiling ends it. Any other action on the item, or its closing, clears it. #986, `src/runner/pm/wakes.rs`
- Reads project worktrees + each item's diff (messages alone miss defects); secret-shaped files and kelpie secrets stay denied. #413, `src/runner/pm/call.rs`
- No git or gh: kelpie writes diffs (64 KiB each) + a `still-red` item's CI log. #452
- Tools `Read,Glob,Grep,Edit,Write`; writes only to the end of `pm-notes.md` via `kelpie confine --append`. Kelpie rolls that log past 32 KiB into `pm-archive.md` at a wake, and keeps what a later wake must remember in `pm-standing.md`, which only kelpie writes and asks every wake to mine the archive into until a `standing` answer lands. #327, #988
- Answer is one JSON object (`pick`, `hold`, `unstick`, `answer`, `follow_up`, `reply`) checked against the board; anything else dropped + logged. #327
- Pick of none lasts 30 min with nothing open; hold ends when an item closes. #327
- One session resumed per wake, compacted past 100k; failed call passed over 10 min, board rule picks meanwhile. #327
- Answers or escalates worker questions, holding the ruling from maintainer one wake (10 min max). #437
- Decides `follow-up` rulings, filing `ready-for-agent` or `needs-triage`. #510, #511
- `shep kelpie pm` hands its session to maintainer's terminal like `attach`; no wake runs under them. #327
