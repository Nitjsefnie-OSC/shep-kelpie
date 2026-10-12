#!/bin/sh
# docs/design/ is read before code, so each file stays small enough to read
# whole: under 8 KB, and every area file listed in the README's table.
# An area that grows past it splits in two.
set -eu
cd "$(git rev-parse --show-toplevel)/docs/design"
bad=0
for f in *.md; do
  size=$(wc -c < "$f" | tr -d " ")
  if [ "$size" -gt 8192 ]; then
    echo "docs/design/$f is $size bytes, over 8192: split the area in two"
    bad=1
  fi
  if [ "$f" != README.md ] && ! grep -q "^| $f |" README.md; then
    echo "docs/design/$f is missing from the table in docs/design/README.md"
    bad=1
  fi
done
exit "$bad"
