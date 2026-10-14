---
name: wording
description: How to word a pull request's title and body for the maintainer. Use when writing or changing `pull-request.md`.
---

# How to write a pull request body

This is for the pull request's title and body. Its reader is the maintainer, so read "the contributor" below as the maintainer and "a comment" or "the post" as your body.

Distilled from three guides: `humanizer` (MIT, Siqi Chen), `i-have-adhd` (MIT, Ayoub Ghriss) and the maintainer's own measured voice. Short enough to send with every call. `mix-v2` adds the maintainer's notes from the stage 1 picks and the praise rule settled after them.

## Lead with what they need

- The first sentence tells the contributor what happened or what they need to do. A thanks can share that sentence. No warm-up, no restating their pull request back to them.
- Put things in the order the reader needs them: what happened, what it means for them, what happens next.
- When there is more than one thing to do, number the steps. Name the file and line in backticks.
- Say what happens next and when, in plain terms: "the maintainer decides the merge", "I'll ask again in three days".
- Give time the way people say it: "three days ago", "if I don't hear back by Sunday". Use a date only when the facts give no other way to say it, and never work out a weekday the facts don't give. Never give a date and a relative time together ("2024-10-27, three days ago"): pick one.

## Keep it short

- A comment runs about 20 to 60 words. A review body runs a few sentences, and each line comment says one thing.
- Cut rather than rephrase. If a sentence restates another, delete it.
- One paragraph is the default. Use a short bulleted list when there are two or more separate items, such as the changes they made after a review.
- A short closing line is fine when it adds something, such as a thanks. A line that sums up the post isn't.

## Patterns that read as a bot

- No em dashes or en dashes. Use a comma, a colon, a period or parentheses.
- No bold, no headings in a comment, no semicolons.
- No "not X, but Y" or "it's not just X, it's Y". State the thing.
- No one-line closer that sums up what you just said, and no "Let me know if you have any questions!"
- No three-item lists made for rhythm, and no "-ing" clause tacked on to sound weighty ("highlighting the importance of...").
- No stock words: delve, crucial, seamless, robust, leverage, ensure, navigate, landscape, testament, tapestry, journey, elevate, streamline, game-changer.
- No praise inflation ("amazing", "fantastic contribution") and no apology padding.
- No "Great question!" or "Happy to help!" openers.
