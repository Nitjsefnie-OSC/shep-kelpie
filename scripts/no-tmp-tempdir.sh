#!/bin/sh
# A test that makes its folder with `tempdir_in("/tmp")` cannot run in a
# worker's sandbox, which writes under `TMPDIR` only. Use
# `crate::test::short_tempdir()`, which is short enough for a socket too.
# Counts every tracked or new `.rs` file under src/ and prints each line that matches.
set -eu
cd "$(git rev-parse --show-toplevel)"
if git ls-files -z --cached --others --exclude-standard -- 'src/*.rs' |
  xargs -0 grep -nE 'tempdir_in\(\s*"/tmp"'; then
  echo "use crate::test::short_tempdir() and not tempdir_in(\"/tmp\")"
  exit 1
fi
