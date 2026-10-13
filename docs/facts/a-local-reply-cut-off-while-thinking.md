# A local reply cut off while thinking, and a busy paddock

From 05:40 UTC on 2026-10-13 most local review rounds on `iq3-xxs-review` failed (#712). In the runner's `usage.jsonl`, lines of kind `local-round`: 40 failed and 4 answered on shep-pm/shep, 74 failed and 3 on shep-pm/shep-kelpie. Each failure held the GPU for 170 to 200 seconds and ended in `reviewer-skipped`.

- A paddock busy with another turn answers a chat request HTTP 503 `{"error":"busy","reason":"iq3_xxs is serving ... next in line","reason_kind":"turn"}`.
  - Kelpie used to fail the round on it. It now waits 30 seconds, the same as its pause between busy answers to a lease take, and asks again, until the request's own 1800 second deadline (`REQUEST_TIMEOUT`).
- A reply came back with `finish_reason: "length"`, `content: null` and only `reasoning_content`: the model spent the whole reply thinking and wrote no findings. Kelpie read that as unreadable text.
  - The two replies read (round 1 of builds 618 and 701) had `completion_tokens` 15,872 and 9,216. The request set `max_tokens` to 65,536, a quarter of the 262,144 context (`REPLY_SHARE`).
  - Both counts are multiples of 512 and far under `max_tokens`, so something below kelpie ends the reply early. Where is not measured: the worker's sandbox reaches neither the paddock nor its model server, so the cap could not be read or probed from it. It could be the paddock, the model server's per-slot context or its own token limit, or the model's settings.
  - What kelpie now does needs no cap: it reports such a reply as cut off, with the finish reason and the token counts, and asks again as two halves of the chunk (cut between lines, the second half under its file's name), up to three halvings. Less diff in a request is less to think about. A chunk too short to halve fails the round with the cut-off message.
- To measure the cap, send one chat request to the paddock with a large `max_tokens` and a diff-sized prompt, and read `usage.completion_tokens` of a reply with `finish_reason: "length"`. A count that repeats across requests is the cap. Record it here, with where it is set.
