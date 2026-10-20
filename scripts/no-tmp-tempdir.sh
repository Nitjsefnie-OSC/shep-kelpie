#!/bin/sh
# A test that makes its folder with `tempdir_in("/tmp")` cannot run in a
# worker's sandbox, which writes under `TMPDIR` only. Use
# `crate::test::short_tempdir()`, which is short enough for a socket too.
# Scans every tracked or new `.rs` file under src/ and crates/*/src/ as a
# whole, so a call that rustfmt split across lines, or put a comment in, still
# matches. Prints each match as file:line.
set -eu
cd "$(git rev-parse --show-toplevel)"
found=$(git ls-files -z --cached --others --exclude-standard -- 'src/*.rs' 'crates/*/src/*.rs' |
  xargs -0 perl -0777 -ne '
    while (m{tempdir_in\s*\((?:\s|//[^\n]*|/\*.*?\*/)*"/tmp"}gs) {
      my $line = 1 + substr($_, 0, $-[0]) =~ tr/\n//;
      print "$ARGV:$line\n";
    }
    close ARGV if eof;
  ')
if [ -n "$found" ]; then
  echo "$found"
  echo "use crate::test::short_tempdir() and not tempdir_in(\"/tmp\")"
  exit 1
fi
