# Claude Code adapter contract

**Status:** blocked by local CLI/runtime incompatibility
**Observed environment:** macOS 26.5.1; Node.js v25.8.0; 2026-07-29

The installed `claude` executable crashed before it could print its version or
help. The failure originates in the installed CLI bundle and ends with
`TypeError: Cannot read properties of undefined (reading 'prototype')`.

This is a runner-health failure, not authentication, rate-limit, or provider
evidence. The capability registry must mark this profile unavailable; no fallback
model option may be enabled to conceal it.

## Required remediation and evidence

1. Use a Claude Code release compatible with the target VM's supported Node.js
   runtime, or provision the runtime supported by the installed release.
2. Record `claude --version` and the exact command-line support for `-p`,
   `--output-format stream-json`, explicit model, effort, structured output, and
   resume.
3. Capture one successful structured no-op run and the required failure fixtures:
   invalid model, auth, rate-limit-shaped, output-limit, cancellation, and
   malformed result.
4. Determine whether reset timestamps are machine-readable or message-only.

Until these are captured, Claude remains unavailable and Sprint 00 cannot satisfy
the two-harness exit criterion.
