# Dispatch policy version 1

**Status:** contract defined; concrete provider capabilities blocked
**Policy version:** 1
**Last checked:** 2026-07-29

Version 1 matches only `role` and normalized `complexity`. It never accepts a
harness, model, or effort from a Linear ticket. The effective policy must be
explicit—there are no defaults.

## Required coverage matrix

| Role | Complexity | Ordered candidates | Verification state |
|---|---|---|---|
| implementation | small | one primary candidate; fallbacks are optional advanced configuration | defined |
| implementation | medium | one primary candidate; fallbacks are optional advanced configuration | defined |
| implementation | large | one primary candidate; fallbacks are optional advanced configuration | defined |
| implementation | xlarge | one primary candidate; fallbacks are optional advanced configuration | defined |
| review | small | one primary candidate on a different provider from maker | defined |
| review | medium | one primary candidate on a different provider from maker | defined |
| review | large | one primary candidate on a different provider from maker | defined |
| review | xlarge | one primary candidate on a different provider from maker | defined |

## Binding decisions

- The first successful mutating implementation launch makes its harness sticky.
- Same-harness model fallback is prohibited after maker launch. A cross-harness
  reassignment requires explicit operator approval and an audit record.
- Before launch, dispatch may skip unhealthy candidates and select the next
  candidate only if review separation remains possible.
- A review starts with fresh context and must use a different harness than the
  sticky maker, even if the model differs.
- A capacity refusal before a run is accepted releases the provisional slot;
  capacity exhaustion after a terminal run preserves the maker and worktree.
- Schema 4 role configuration generates one deterministic all-complexity rule
  per role. Advanced configuration may replace this with exact rules and ordered
  fallback candidates, subject to the same coverage and separation validation.

## Acceptance checklist for the concrete policy

- [ ] Every raw Linear estimate maps to exactly one stable complexity class.
- [ ] Every `(role, complexity)` matches exactly one rule.
- [ ] Rule IDs are unique and precedence cannot hide overlap.
- [ ] Each model/effort pair has passed the corresponding installed CLI probe.
- [ ] Every implementation candidate has a possible different-harness reviewer.
- [ ] Removing Codex or Claude Code produces a validation error for unsupported
      maker/checker combinations.

`config/spire.example.yaml` intentionally leaves `complexity_mapping`, harness
models/efforts, and rules empty so it cannot be mistaken for deployable policy.
