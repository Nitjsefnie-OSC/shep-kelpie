#!/bin/sh
# ADR 0014: a ticket is read and written through the `Tickets` port. Only a
# source's own `Tickets` impl may put one on the forge, and each stands under
# `crates/core/src/ports`, `crates/adapters/src/adapters` or `crates/core/src/local_tickets`.
# The forge code under `crates/adapters/src/github` is scanned.
# Kelpie's App writes stay on `Voice`: which
# identity writes is a GitHub concern. The rig's forge is also its tickets, so
# a test may call these on `rig.forge` directly.
# Scans every tracked or new `.rs` file under src/ and crates/*/src/ as a
# whole, so a call that rustfmt split across lines still matches, and over
# text with its comments blanked out, so a comment naming one is not a call.
# Prints each match as file:line.
set -eu
cd "$(git rev-parse --show-toplevel)"
# A skipped folder's own `mod.rs` is scanned, as its `foo.rs` was beside the folder.
found=$({ git ls-files -z --cached --others --exclude-standard -- 'src/*.rs' 'crates/*/src/*.rs' |
  grep -zv '^crates/core/src/ports/\|^crates/adapters/src/adapters/\|^crates/adapters/src/test/\|^crates/core/src/local_tickets\|^crates/core/src/test/\|^crates/runner/src/test/'
  git ls-files -z --cached --others --exclude-standard -- \
    crates/core/src/ports/mod.rs crates/adapters/src/adapters/mod.rs \
    crates/core/src/test/mod.rs crates/runner/src/test/mod.rs crates/adapters/src/test/mod.rs
  } |
  xargs -0 perl -0777 -ne '
    my $method = qr{create_issue|close_issue_as|close_issue|set_issue_label};
    my $ask = qr{(?<!rig\.)forge\s*\)*\s*\.\s*$method\s*\(|\bForge\s*::\s*$method\s*\(};
    my $text = $_;
    $text =~ s{(\/\*.*?\*\/)}{ my $n = ($1 =~ tr/\n//); "\n" x $n }gse;
    $text =~ s{//[^\n]*}{}g;
    while ($text =~ m{$ask}gs) {
      my $line = 1 + substr($text, 0, $-[0]) =~ tr/\n//;
      print "$ARGV:$line\n";
    }
    close ARGV if eof;
  ')
if [ -n "$found" ]; then
  echo "$found"
  echo "write the ticket through ports.tickets (ADR 0014)"
  exit 1
fi
