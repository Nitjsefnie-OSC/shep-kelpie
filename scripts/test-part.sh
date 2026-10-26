#!/bin/sh
# Runs part 1, 2 or 3 of `cargo test --locked`, one per CI job. The runner
# crate's tests take most of the time, so parts 1 and 2 each take a set of its
# modules, and part 3 runs everything else, the other crates included, by
# skipping both sets. A new or renamed module runs in part 3.
set -euf
one="runner::contributions:: runner::review:: runner::panel:: runner::triage:: runner::follow_up::"
two="runner::review_bot:: runner::merge:: runner::gate:: runner::pm:: runner::turn:: runner::replies:: runner::core_tests:: runner::flight::
runner::coderabbit:: runner::thread_replies:: runner::dependencies:: runner::retro:: runner::ruling::"
# Each list splits on its spaces, with globbing off.
# shellcheck disable=SC2086
case "${1:-}" in
  1) set -- $one ;;
  2) set -- $two ;;
  3)
    set --
    for module in $one $two; do set -- "$@" --skip "$module"; done
    exec cargo test --locked -- "$@"
    ;;
  *)
    echo "usage: $0 1|2|3" >&2
    exit 2
    ;;
esac
exec cargo test --locked -p shep-kelpie-runner --lib -- "$@"
