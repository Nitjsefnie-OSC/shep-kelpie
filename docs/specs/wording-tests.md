# Wording test

The representative writes every post a contributor reads on their pull request (#616). This test picks the writing guide its agent file carries and the effort it runs at (#870). #637 runs straight after on the same harness.

The code, corpus and raw results live in the experiments repo, under `wording/`, from commit 7e1f705. Its `PLAN.md` holds the same rules as this file.

## Material

Seven posts, one for each kind the representative writes:

| kind | slice | what kelpie tells the contributor |
|---|---|---|
| findings | #627 | a local reviewer's findings, as a review with line comments |
| question | #628 | an answer to their question about the code or a finding |
| release-ask | #629 | a reminder of the open threads, and whether kelpie may take it from here |
| no-with-note | #630 | the maintainer's no, with the maintainer's note |
| clean-pass | #627 | the reviewer found nothing, and it's with the maintainer now |
| merged-thanks | #630 | thanks after the merge |
| on-it | #635 | CI failed after the release, and kelpie is on it |

Each post is built on a real contributor thread from WatWowMap/ReactMap or TurtIeSocks/Koji, so the contributor's words are genuine. Kelpie's facts for each are written by hand from that thread. The on-it post's release is made up, since neither repo has a thread where a quiet contributor's CI was fixed by someone else. Its lint failure is real.

## The call

The call is the one #869 builds: a fresh `claude -p` with no tools, no MCP server, no user settings and no session kept, in an empty folder. The representative's draft body is appended to Claude Code's system prompt, with the arm's guide after it. The user prompt holds the kind, the facts, earlier posts and the contributor's text, fenced as `src/runner/untrusted.rs` fences it. A findings post replies with JSON of a review body and one comment per finding, as #627 has it.

## Arms

- Stage 1 runs Opus 5.5 at medium effort once per guide: none, `humanizer`, the maintainer's `rin-voice`, `i-have-adhd`, and a distilled mix of the three. That's 35 calls.
- Stage 2 runs the stage 1 guide at low effort against medium. Stage 1's medium output is reused, so that's 7 calls.

There's one run per arm. The pick is the measure and the maintainer reads every output, so repeat runs would multiply the reading without changing what it decides.

## Measures

- The maintainer's blind pick for each post, with an optional note. The page shows letters only, shuffled per post.
- Writing tells for each output:
  - length
  - em and en dashes
  - bold, headings and semicolons
  - exclamations and emoji
  - stock AI words and "not X but Y"
  - stock openers and closers
  - mentions of anyone but the contributor, and links
- Facts kept and claims invented, judged by Sonnet 5.5 at low effort against the post's facts.
- Tokens and cost for each call, with the guide's share.

## Deciding rules

Written before any run.

1. Stage 1's guide is the one with the most blind picks across the seven posts. A tie goes to the shorter guide text.
2. A guide that keeps under 90% of the facts on average, or invents a claim in two or more posts, can't win. The next guide by picks wins.
3. Stage 2 picks low effort unless medium wins a majority of the seven posts.
4. If `rin-voice` wins, the shipped file takes the next guide by rules 1 and 2, and `rin-voice` goes in the maintainer's home copy only. The shipped default is every kelpie user's, so it must not write in the maintainer's own voice.

The results go in `docs/specs/wording-results.md`. That file ends with the guide text and effort the rules pick, and says whether `rin-voice` belongs in the maintainer's home copy. The measured facts go in `docs/facts/the-representative-wording-test.md`. #871 ships what the rules pick.
