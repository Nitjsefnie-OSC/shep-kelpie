# Review outcomes

What each finding a fix turn was sent came to, kept in the journal so it outlives the work item.

## Review outcomes

- One `finding-outcomes` entry per settled fix turn (`issue`, `round`, `reviewer`, `outcomes`: severity, file, line, outcome, optional reason), written at `fix_ended` (pushed) and `went_unfixed` (pushed nothing, every finding disputed, deferred or a declined nit); none when the turn raises `FixNotPushed`, since the worker gets another turn and the entry comes then. #1034, `runner/review/outcomes/`
- Derived from what the fix turn already reads (`disputes::answers`, `thread_answers`); the worker answers nothing new. Decided on #1034
- Outcomes: `pushed` (turn pushed, worker neither disputed nor deferred it; not proof it was fixed), `disputed` (with reason), `deferred`, `nit-declined` (pushed nothing, a nit). A bot's thread (`threads_sent` as long as the findings) takes `fixed`, `declined` (with reason) or `not-addressed` from `thread_answers` instead; a `fixed` line on a turn that pushed nothing is `not-addressed`, as the thread's reply says it was not made
- `reviewer` is `review.reviewer`, as `Entry::Round` writes it. A size-skipped file's notice is no finding and gets no outcome
- Older builds skip the line (`journal::read` drops what it cannot read), so there is no migration. `Round`, `Findings`, `Held`, the state file and what a fix turn does with the answers are unchanged
