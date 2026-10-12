# The deep review round

Measured by the maintainer on 2026-10-02, over 16 builds with 52 known bugs, for shep-pm/shep-kelpie#284.

- The Claude round before it found 0 of the 52. Its prompt asks for quality notes, and 89% of its 44 findings were not behaviour defects. The same round with only its prompt swapped for the defect prompt found 4, at 80% precision.
- One Opus 5.5 round at high effort with the defect prompt is the best single reader: 27% of the known bugs at 82% precision, about $1.65 a bug. Sonnet 5.5 at high found 17% at 80%, and Opus at medium found 17% at 86%.
- A second fresh Opus high session shown the first's findings and asked what they missed raised that to 38% at 95% precision.
- A reviewer that runs the tests found fewer bugs, 21%, but every finding it backed with a failing test was real, 97% precision. It confirms and does not replace reading.
- A fixer's "fixed" was wrong for 1 of 4 known bugs it was handed, and that fix broke a scenario that passed before.
- Kelpie's review gate of the time, its Claude round and the loop, caught none of the 52, and its Opus 5.5 low-effort judge kept 69% of the findings that were not real.
- The case behind #284: shep-pm/shep-kelpie#272 took ten review rounds and nine worker turns, $18.53 of its $31, and still merged a feature that could not work, while one review that ran the code against its criteria found the same HIGH in one pass.
