# The machine's load

- On 2026-09-30, with several workers on this 14-core machine, the load average sat at 25 to 39. The load came from running tests, the test binaries and the `git` processes they spawn, not from rustc, and capping compile jobs did nothing for it. So `cargo-test` leases the run and leaves the build outside.
- Under kelpie's `srt` sandbox, a process connects to a unix socket listed in `allowUnixSockets` and is refused any other with `Operation not permitted` (measured 2026-09-30 with the sandbox runtime under `~/.kelpie/tools`, one Python client against two sockets). That is what lets a worker reach the dog's door and no shep socket.
