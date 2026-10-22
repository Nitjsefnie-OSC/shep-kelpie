# Kelpie takes any shepherd from its pinned shep on

This reopens the version half of ADR 0003, which kept kelpie on its pinned shep minor line and refused every other. Kelpie now takes a shepherd on the shep version it is built with or any later one, on any line, and refuses an earlier one (decided by the maintainer on 2026-10-22).

The minor line guarded against requests that "may not mean the same" on another shep. shep already decides that in its handshake: it carries a protocol number that moves whenever a message shape does, and a shepherd refuses a client below the oldest protocol it accepts (`MIN_SUPPORTED`). The line check added nothing on top of it but a refusal at every shep release. shep 0.16.0 kept protocol 11, the one 0.15 speaks, and kelpie refused it anyway, so `shep kelpie finish` failed on a shepherd it could talk to.

The floor stays. Kelpie leans on what its pinned shep shipped, such as the stop that ends every lamb (ADR 0005, from 0.12.4) and the notes a runner sends (a 0.14 shepherd drops them as malformed), and an earlier shepherd may not have it.

## Considered options

- **Keep the pinned minor line.** Every shep minor needs a kelpie release before kelpie runs on it, protocol change or not.
- **Refuse another major only.** Nothing on the 0.x line, so it adds nothing now, and shep's protocol floor already covers a break that comes with a major.
- **Refuse a shepherd on a newer protocol than kelpie's.** shep takes a newer client on purpose, since a change that only adds keeps every older peer readable. The handshake is shep's to judge.

## Consequences

- `connect` refuses a shepherd on an earlier shep than `SHEP_VERSION`, and shep's handshake refuses a kelpie whose protocol the shepherd no longer accepts. `doctor` names either one on its `shepherd` line.
- `version --json` names the protocol the build speaks, and `upgrade` refuses a build below the shepherd's oldest protocol, saying to install a kelpie built for its shep. A build from before the protocol was named is checked by its shep version alone.
- Moving the shep pins is still how kelpie gets a newer shep's features, but no longer how it keeps running on one.
