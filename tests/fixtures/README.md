# Sprint 00 redacted fixture contract

Fixtures are versioned, offline evidence—not hand-written representations of a
provider. Do not add an event file until it was captured from the disposable
environment and redacted.

Expected layout:

```text
linear/<scenario>.json
harness/codex/<scenario>.jsonl
harness/claude/<scenario>.jsonl
harness/fixture-manifest.json
```

Every file listed in the manifest must be valid JSON or JSONL. The manifest
records its one normalized outcome and redaction confirmation; run
`scripts/sprint00/verify-fixture-manifest.sh` with no network access to validate
it. `--require-captured` is the Sprint 00 exit gate.
