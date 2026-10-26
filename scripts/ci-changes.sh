#!/bin/sh
# Reads a pull request's changed paths on stdin and prints `rust=true` when
# any of them can change what a Rust job sees, else `rust=false`. Markdown
# outside the code folders is prose, except the pages a Rust test reads.
set -eu
rust=false
while IFS= read -r path; do
  case "$path" in
    crates/* | src/* | tests/* | agents/* | skills/* | fixtures/*) rust=true ;;
    docs/specs/wording-results.md | docs/agents/sandbox-red-tests.md) rust=true ;;
    docs/guide/commands.md | docs/guide/metrics.md) rust=true ;;
    *.md) ;;
    *) rust=true ;;
  esac
done
echo "rust=$rust"
