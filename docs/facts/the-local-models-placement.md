# The local model's placement

Kelpie no longer reads `/api/ps` or parks on a spilled model (#502): paddock decides where a model loads. What follows is what was measured while it did.

Built in shep-pm/shep-kelpie#145, after a model loaded on the CPU instead of the GPU ran review-sized prompts at a twentieth of normal speed on 2026-09-29 and nothing noticed for 14 minutes.

- Ollama's `GET /api/ps` lists each loaded model with its `size` and `size_vram` in bytes, its `context_length` and its `expires_at`. A model is on the GPU when `size_vram` equals `size`, and `size_vram / size` is the share on it. Recorded shape: `fixtures/ollama-ps.json`, written by hand from Ollama's documented response, since the maintainer's host did not answer from the worker's sandbox.
- It cannot say how busy the GPU is: there is no utilization, so a model on the GPU that another round is using looks the same as an idle one. That is the GPU metrics ticket's job, not this check's.
- It lists only loaded models. A model not loaded yet is not checked, and the check cannot say where it will go when the round's first request loads it. A model that loads spilled in the middle of a round is not caught until the next round.
- Kelpie reads `/api/ps` only where it holds the GPU lock around the round (a reviewer whose file names a `lease`; `gpu_lease = true` when this was built), after taking it and before the round.
  - Without the lock the read races whoever holds it: it would rule on another session's spill that a round would have queued behind, and could pass just before the holder reloads the model spilled.
  - The default command round, `qwen-review.sh`, takes the lock itself, so it is not checked, and `ollama` on a command without a `lease` is refused when the settings load, with that reason. An endpoint with no `lease` is skipped the same way.
  - The host is the endpoint's `url` without `/v1`, or the command's `ollama`. A command names its model with `ollama_model`, and only that model is checked. Without it every model the host has loaded is looked at, and one spilled for another reason fails the round, and each yes runs into it again until it unloads (two hours by default). Model names match without regard to case.
  - A model partly or wholly on the CPU fails the round without running it, and raises a ruling that says which model and how much is on the GPU, so it reaches the maintainer's webhook. A yes runs the same round again.
  - A 404, or a reply that is not `/api/ps`'s, means the server is not Ollama and skips the check. A reply that lists models with no readable `size` is Ollama-shaped, so the runner logs how many were left unchecked. A host that cannot be reached fails the round, and the runner retries it as any failed round.
- `status` shows `local_model` from the last read: the name, the percent on the GPU, the context length and when it unloads. It is absent until a round has read one, and it is the last round's view, not live.
