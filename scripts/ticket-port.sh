#!/bin/sh
# ADR 0014: a ticket is read and written through the `Tickets` port. Only a
# source's own `Tickets` impl may put one on the forge, and each stands under
# `src/ports` or `src/adapters`. Kelpie's App writes stay on `Voice`: which
# identity writes is a GitHub concern. The rig's forge is also its tickets, so
# a test may call these on `rig.forge` directly.
# Scans every tracked or new `.rs` file under src/ as a whole, so a call that
# rustfmt split across lines still matches, and over text with its comments
# blanked out, so a comment naming one is not a call. Prints each match as
# file:line.
set -eu
cd "$(git rev-parse --show-toplevel)"
found=$(git ls-files -z --cached --others --exclude-standard -- 'src/*.rs' |
  grep -zv '^src/ports/\|^src/adapters/\|^src/test/' |
  xargs -0 perl -0777 -ne '
    my $gap = qr{(?:\s|//[^\n]*|/\*.*?\*/)*};
    my $method = qr{create_issue|close_issue_as|close_issue|set_issue_label};
    my $ask = qr{(?<!rig\.)forge$gap\)*$gap\.$gap$method\s*\(|\bForge$gap::\s*$gap$method\s*\(};
    my $text = $_;
    $text =~ s{/\*.*?\*/}{ my $n = ($& =~ tr/\n//); "\n" x $n }gse;
    $text =~ s{//[^\n]*}{}g;
    while (m{$ask}gs) {
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
