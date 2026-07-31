# Harness process execution

**Status:** accepted 2026-07-31; supersedes
[`systemd-runner-contract.md`](systemd-runner-contract.md) for harness runs
**Applies to:** S05.3 and every later runner change

Spire launches Code Harnesses as ordinary child processes it supervises itself.
There is no systemd dependency in the harness execution path. One implementation
runs on Linux and macOS.

This supersedes the transient-unit contract, which was recorded as "selected for
the first design but have not been proven" in the architecture document and
"recommended but not proven" in the implementation document. It was never
validated against either provider.

## Why not systemd

The transient-unit design existed for one property: after an Orchestrator
restart, the runner could inspect the unit "instead of assuming an orphaned child
process is dead."

That property does not require systemd. It requires **durable run identity plus
a liveness check**, and systemd is one implementation of that, not the
requirement. SQLite already holds the run record; the identity fields below make
the same guarantee portable. The runner verifies rather than assumes.

Keeping systemd would have cost a Linux-only execution path, which makes the
development machine a second-class environment that exercises different code from
production. A single supervised-child implementation is tested identically
everywhere.

## Run identity

Persist three values with the run record before the child is treated as live:

| Field | Purpose |
|---|---|
| `pid` | the immediate child |
| `process_start_time` | disambiguates a reused PID |
| `process_group_id` | the whole harness process tree |

`pid` alone is unsafe: PIDs are reused, so a stale record can match an unrelated
process. The pair `(pid, process_start_time)` is stable for the lifetime of a
process and cannot be forged by reuse, because a new process with a recycled PID
has a later start time. Read it from `/proc/<pid>/stat` field 22 on Linux and
from `kinfo_proc.kp_proc.p_starttime` via `sysctl(KERN_PROC_PID)` on macOS. Treat
the value as an opaque comparable token; its units differ per platform and must
not be interpreted as a wall-clock time.

## Process groups are mandatory

Spawn each run in a new session and process group, and address every signal to
the group rather than the pid.

Harnesses spawn their own children. The captured Codex fixture contains:

```text
"command": "/bin/zsh -lc 'tail -n 5 NOTES.md && git diff -- NOTES.md'"
```

Signalling only the immediate child would orphan that subtree and leave work
running against a worktree Spire believes is idle. Group-directed signalling is
what systemd's cgroup was providing implicitly, and it is portable.

## Lifecycle

**Start.** Resolve an explicit executable and argument vector, never shell text.
Set the working directory to the Spire-owned worktree, apply the environment
allowlist, create a new session and process group, redirect stdout and stderr to
the per-run evidence path, and record `(pid, process_start_time,
process_group_id)` in the same transaction that marks the run live. A start that
cannot record identity must terminate the child rather than leave it unowned.

**Inspect.** Compare the persisted `(pid, process_start_time)` against the live
process. Matching means running; a missing process or mismatched start time means
the run ended and its result must be collected from evidence.

**Cancel.** Send `SIGTERM` to the negated process group id once, wait the
configured grace period, then send `SIGKILL` to the same group. Cancellation is
idempotent: a run already gone is a successful cancel.

**Collect.** Read the evidence files. Extraction is per-provider, since neither
harness emits the shared result at the top level of its stream; see the adapter
contracts.

**Resume** is not process re-adoption. `codex exec resume` and `claude -r` start
a *new* process continuing a provider-side session. Process identity and provider
session identity are separate lifetimes and must not be conflated.

## Recovery after restart

On startup, for each run not in a terminal state:

1. If `(pid, process_start_time)` matches a live process, adopt it and continue
   monitoring.
2. If it does not match, the run ended while Spire was down. Collect evidence and
   classify from the terminal event; a missing structured result is a failure to
   classify, never an absent-but-successful run.
3. If the run is live but past its deadline, terminate the group and record the
   timeout. This is where a deadline that elapsed during downtime is enforced.

Repeated start for the same run must not launch a second process. The durable
identity record is the guard.

## Accepted losses

Both were considered and accepted on 2026-07-31.

**No cgroup resource caps.** There is no portable equivalent. Resource usage is
already bounded upstream by the configured concurrency limits — three total
active harness runs and one AI-initiated — and by the operations guards for free
disk and inodes. A cgroup would be a second control on a constraint already
enforced where it matters.

**No timeout enforcement while Spire is down.** systemd would stop an
over-deadline run with Spire absent; a supervised child will not. The exposure is
the intersection of Spire being down and a run exceeding its deadline, and step 3
above enforces the deadline at adoption. Judged acceptable rather than worth a
Linux-only execution path.

## What systemd still does

`spire.service` and `cloudflared.service` are unchanged: systemd remains the
service manager on Linux, and `deploy/systemd/` stays as it is. This decision is
scoped to how harness runs are launched, not to how Spire itself is supervised.
On macOS, run `spire serve` in the foreground.

## Unknown / Unverified

- Restart-adoption has not been exercised against a live harness on either
  platform. S05.3's verification remains outstanding evidence, now portable
  rather than VM-only.
- Whether a harness reacts to `SIGTERM` by writing a usable terminal event before
  exiting, on either provider.
- Whether any harness double-forks or otherwise escapes its process group.
- Start-time resolution on each platform, and whether it is coarse enough for a
  PID to be reused within one tick.
