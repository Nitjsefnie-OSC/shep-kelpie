# ntfy replies

Measured 2026-09-29 on ntfy.sh with scratch topics of kelpie's own, never the maintainer's, for #111. The recording is `fixtures/ntfy-poll.jsonl`, with the topic's name and the project's in the alert's title replaced.

- `GET <topic>/json?poll=1&since=<id>` returns the cached messages after that id, one JSON object a line, and closes. `since=<unix time>` includes messages in that same second. An id the cache does not hold (`since=zzzzzzzzzzzz`) returns everything cached, the same as `since=all`.
- A poll right after a publish can miss it: ntfy.sh writes its cache in batches. A message missed that way is returned by the next poll after the same id, so a cursor by id loses nothing.
- A message posted with `Tags: kelpie` reads back with `"tags":["kelpie"]`, so a read skips kelpie's own posts. Each message carries ntfy's own `time`, set when ntfy takes it.
- A message of 4,096 bytes or more comes back as `"message":"You received a file: attachment.txt"` with an `attachment` object, so a read treats it as having no text.
- The real adapter end to end (`a_reply_on_a_real_topic_reads_back`, ignored unless `KELPIE_NTFY_SCRATCH` names a scratch topic): the alert went up with its reply line, a typed reply posted the way the app posts one landed with ntfy's time within a minute of the machine's clock, and a read skipped the alert and returned the reply. Not checked: a reply from a phone with a real authenticator, which needs the maintainer's own.
