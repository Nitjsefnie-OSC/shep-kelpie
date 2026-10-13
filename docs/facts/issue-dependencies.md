# Issue dependencies

Read 2026-09-27 with gh 2.96.

- `gh issue list --json blockedBy` lists each issue's blockers with their `number` and `state` (`OPEN` or `CLOSED`), plus a `totalCount`. No GraphQL call is needed. gh's own query asks for the first 50 blockers (`api/query_builder.go` at v2.96.0), so the board takes any blocker counted but not listed as open.
- A blocker's state is its issue's, never its pull request's. The board skips an issue while any blocker reads `OPEN`, however far that blocker's pull request has got.
- On this repo, shep-pm/shep-kelpie#27 listed eight blockers, seven open and one closed. The recording is `fixtures/gh-issue-list.json`.
- `gh issue view --json parent,blockedBy` gives the same `parent` object and the blockers' `nodes`, recorded on cli/cli in `fixtures/gh-issue-view-sub-issue.json`.
- `gh issue list --json parent,subIssuesSummary` gives each issue's `parent` (null, or an object with its `number`) and `subIssuesSummary` with `total`, `completed` and `percentCompleted`. Read 2026-09-30 from gh 2.96's `api/query_builder.go` and a recording on cli/cli, `fixtures/gh-issue-list-sub-issues.json`, since no repo of the maintainer's had sub-issues: #12438 there reads 2 of 3 completed. The board takes `completed` as the closed ones.
- The forge port's `comment` runs `gh pr comment`, which looks the number up as a pull request, so a comment on an issue goes through `post_comment` and `repos/<repo>/issues/<n>/comments`, which takes either. Not measured on an issue.
- `gh api repos/<repo>/issues/<n>/sub_issues?per_page=100` lists a parent's sub-issues (GitHub caps a parent at 100) with `state` and `state_reason`: `completed`, `not_planned`, `duplicate`, `reopened` or null. Read from GitHub's REST documentation, not measured on a repo. `subIssuesSummary.completed` counts every closed sub-issue, whatever it closed as, so the parent check reads the list.
