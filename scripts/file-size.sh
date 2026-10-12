#!/bin/sh
# IR-48 on this machine: the same check CI's file-size job runs, so a file
# over 1000 lines fails before the push and not one round later. Counts
# every tracked or new `.rs` file and prints each one over.
set -eu
cd "$(git rev-parse --show-toplevel)"
git ls-files -z --cached --others --exclude-standard -- '*.rs' |
  xargs -0 wc -l |
  awk '$2 != "total" && $1 > 1000 { print "over 1000 lines: " $2 " (" $1 ")"; over = 1 }
       END { exit over }'
