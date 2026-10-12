# The GPU's metrics

Built in shep-pm/shep-kelpie#146, for the load `/api/ps` cannot give (above).

- `nvidia_gpu_exporter` serves Prometheus text on port 9835. Kelpie reads five of its series, each labelled by the GPU's `uuid`: `nvidia_smi_utilization_gpu_ratio` (0 to 1), `nvidia_smi_memory_used_bytes`, `nvidia_smi_memory_total_bytes`, `nvidia_smi_power_draw_watts` and `nvidia_smi_temperature_gpu`.
  - Go prints large values in exponent form (`2.01326592e+10`), so every value is read as a float.
  - Recorded shape: `fixtures/gpu-exporter.txt`, written by hand from the exporter's documented output, since the maintainer's box did not answer from the worker's sandbox (a 5 second connect timeout, as for `/api/ps`).
- `gpu_metrics_url` in kelpie's own settings names the page, since the GPU is the machine's and not one project's. Without it nothing is read and `status` has no `gpu`. It takes the same `http://` or `https://` URL as an endpoint, and is read again at the runner's next wake like the rest of kelpie's settings.
- A thread reads the page every 15 seconds with `curl` (3 seconds to connect, 5 in all, 1 MiB at most, cut off mid-transfer), and `status` shows the last reading: each GPU's `utilization_percent`, `memory_used_bytes`, `memory_total_bytes`, `power_watts` and `temperature_celsius`, and `age_seconds` since the read ended.
  - A series the page lacks is left out of that GPU. A page with none of the five, or one that cannot be fetched, leaves `status` as it was and puts the reason in `gpu.error`. Until the first read, `gpu.error` says it has not been read yet.
  - The reason never names the URL, because the host is the maintainer's, and `EndpointUrl` prints as `EndpointUrl(..)` in `Debug` for the same reason.
- The read is off the request path because shep gives an action 3 seconds (`action_timeout`, which kelpie leaves at its default) and a box that is off or hung takes 5 to fail.
  - Measured against a listener that accepts and never replies: `curl` gave up after 5.01 seconds. A `status` that waited on it would time out at the shepherd, and `kelpie upgrade` counts a timed out `status` as a runner still busy.
  - Every trigger that answers with the status shows the same cached reading, and none waits on the box. Changing `gpu_metrics_url` takes effect at the next reread and reads the new page at once.
- Not done (shep-pm/shep-kelpie#251): the split of a GPU wait into the GPU busy with another round and the GPU idle. There is no `gpu_wait` phase since #331, which folds it into `review`, so the split needs a sample during the wait and new timing phases, which changes the state file's `timings` and the rollback above.
