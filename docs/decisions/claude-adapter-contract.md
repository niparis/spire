# Claude Code adapter contract

**Status:** partial local feasibility evidence; successful-run fixture blocked
**Observed environment:** macOS 26.5.1; Node.js v25.8.0; Claude Code 2.1.148;
2026-07-29

`claude --version` returned `2.1.148`. Its help output confirms support for
non-interactive `--print`, `--output-format stream-json`, explicit `--model`,
`--effort`, `--json-schema`, and `--resume` options.

This verifies command shape only; it is not authentication, successful-run,
rate-limit, or provider-event evidence. No fallback model option may be enabled
to conceal an unavailable selected model.

## Required remediation and evidence

1. Capture one successful structured no-op run and the required failure fixtures:
   invalid model, auth, rate-limit-shaped, output-limit, cancellation, and
   malformed result.
2. Determine whether reset timestamps are machine-readable or message-only.

Until these are captured, Claude remains unavailable and Sprint 00 cannot satisfy
the two-harness exit criterion.
