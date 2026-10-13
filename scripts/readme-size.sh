#!/bin/sh
# README.md is what a first-time operator reads, so it stays under 20 KB. The
# operator reference lives in docs/guide/, one page per topic, with no cap.
set -eu
cd "$(git rev-parse --show-toplevel)"
size=$(wc -c < README.md | tr -d " ")
if [ "$size" -gt 20480 ]; then
  echo "README.md is $size bytes, over 20480: move the text to a page in docs/guide/"
  exit 1
fi
