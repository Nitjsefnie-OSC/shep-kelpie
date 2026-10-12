# The maintainer's reviews

Read 2026-09-28 through GitHub's GraphQL API on shep#598, #607, #614, #617, #622, #627 and #629. The query as kelpie sends it is recorded on #617 in `fixtures/`.

- A review's author has `__typename` `User` for the maintainer and `Bot` for CodeRabbit. So kelpie takes the latest non-pending review by a `User` as the maintainer's, and a later bot review does not hide it.
- The maintainer's replies in CodeRabbit's threads are reviews of their own, often with an empty body. Their comments sit in the bot's thread, tied to the maintainer's review by `pullRequestReview`.
- A comment on a line the diff has since moved past has `line` null and keeps only `originalLine`. Kelpie gives such a comment its file and no line.
