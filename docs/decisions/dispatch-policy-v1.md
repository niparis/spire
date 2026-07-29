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
| implementation | small | operator must supply two or more candidates | blocked |
| implementation | medium | operator must supply two or more candidates | blocked |
| implementation | large | operator must supply two or more candidates | blocked |
| implementation | xlarge | operator must supply two or more candidates | blocked |
| review | small | must include a harness distinct from every maker candidate | blocked |
| review | medium | must include a harness distinct from every maker candidate | blocked |
| review | large | must include a harness distinct from every maker candidate | blocked |
| review | xlarge | must include a harness distinct from every maker candidate | blocked |

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
