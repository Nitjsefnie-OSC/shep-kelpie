#!/bin/sh
# Each `{ gap = N }` in docs/parity.toml names an issue that is still open.
# A closed one means the feature landed, so its cell says "yes" now, or the
# gap needs the issue that still adds it.
set -eu
cd "$(git rev-parse --show-toplevel)"
bad=0
for n in $(grep -o 'gap = [0-9]*' docs/parity.toml | sed 's/gap = //' | sort -un); do
  state=$(gh issue view "$n" --json state --jq .state)
  if [ "$state" != OPEN ]; then
    echo "docs/parity.toml: #$n is $state: say \"yes\" where it was the gap, or name the issue that still adds it"
    bad=1
  fi
done
exit "$bad"
