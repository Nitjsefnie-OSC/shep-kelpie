# Kelpie finishes a contribution on its contributor's branch

Kelpie reviews a contribution and talks with its contributor, and the pull request stays theirs. When the contributor goes quiet, the representative asks whether kelpie may take it from here. A yes, or no answer before `contributions.quiet_days`, is a release. Kelpie then adopts the pull request and pushes to the contributor's own branch, which their "allow edits by maintainers" lets it do. It never opens a second pull request. After the release the pull request is kelpie's, and a push of the contributor's is a foreign change.

The contributor's commits, their pull request's conversation and their credit stay in one place. A second pull request would split the history and close theirs as if their work were turned down.

## Considered options

- A new branch and a second pull request, cherry-picking the contributor's commits. It needs no push access to their fork, but splits the conversation and reads as a rejection.
- Adopting with no ask once the contributor is quiet. It is faster, but takes over someone's pull request without telling them first.
- Waiting for an explicit yes. A contributor who has moved on never answers, so the pull request would sit forever.

## Consequences

- A contribution whose fork does not allow maintainer edits cannot be released this way. When `quiet_days` run out on one, the maintainer gets a ruling: leave it open, or close it with thanks.
- A contributor who says no twice and then goes quiet goes to the maintainer as a ruling, never to an adoption.
