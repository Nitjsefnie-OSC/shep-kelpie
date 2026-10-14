#!/bin/sh
# Each `{ gap = N }` in docs/parity.toml names an issue that is still open.
# A closed one means the feature landed, so its cell says "yes" now, or the
# gap needs the issue that still adds it.
#
# With PR set to a pull request's number, an issue that pull request
# resolves counts as closed too, so the pull request that adds a feature
# marks it, rather than the next one to run this after the merge.
set -eu
cd "$(git rev-parse --show-toplevel)"
resolves=""
if [ -n "${PR:-}" ]; then
  resolves=$(gh pr view "$PR" --json closingIssuesReferences --jq '.closingIssuesReferences[].number')
fi
bad=0
for n in $(grep -o 'gap = [0-9]*' docs/parity.toml | sed 's/gap = //' | sort -un); do
  if printf '%s\n' "$resolves" | grep -qx "$n"; then
    echo "docs/parity.toml: this pull request resolves #$n: say \"yes\" where it was the gap, or name the issue that still adds it"
    bad=1
    continue
  fi
  state=$(gh issue view "$n" --json state --jq .state)
  if [ "$state" != OPEN ]; then
    echo "docs/parity.toml: #$n is $state: say \"yes\" where it was the gap, or name the issue that still adds it"
    bad=1
  fi
done
exit "$bad"
