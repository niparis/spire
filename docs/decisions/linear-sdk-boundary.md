# Linear SDK boundary decision

**Status:** blocked pending a disposable test-team credential and pinned crate probe
**Last checked:** 2026-07-29

Sprint 03 owns the `LinearPort`; the SDK is an adapter implementation detail.
Sprint 00 must establish which operations can use a pinned `lineark-sdk` release
and which need a narrow raw GraphQL adapter.

## Required spike matrix

| Operation | Expected boundary | Evidence required | Status |
|---|---|---|---|
| Fetch canonical issue fields | SDK preferred | redacted response fixture deserializes offline | blocked |
| Query relevant issues with pagination | SDK preferred | cursor and page-boundary fixture | blocked |
| Read team workflow and estimate scale | SDK preferred | team-scoped fixture | blocked |
| Transition disposable issue status | SDK or raw GraphQL | test-team-only mutation transcript | blocked |
| Create disposable issue comment | SDK or raw GraphQL | duplicate-comment/idempotency evidence | blocked |
| GraphQL error and rate-limit headers | raw response wrapper as needed | redacted failure fixture | blocked |

## Constraints

- Pin the selected SDK version in the disposable spike's `Cargo.toml`; do not use
  a floating version requirement.
- A raw GraphQL operation must be limited to a named adapter method with typed
  request/response fixtures. It must not leak into the domain or application.
- The duplicate-comment result decides whether local outbox idempotency is solely
  responsible; no native idempotency feature may be assumed.
- The required status transition and comment creation are authorized only for the
  disposable test team and test issue named in the operator evidence.

No SDK version is recorded here because selecting one without resolving the crate
and exercising its API would be fabricated evidence.
