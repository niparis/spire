# Sprint 16 — Interactive Onboarding CLI

**Last Verified:** 2026-07-31
**Depends on:** the shipped subset of Sprint 15 (S15.2 discovery, the base
`spire init` assembly from S15.4/S15.5)
**Unlocks:** repeated configuration changes during pilot testing
**Completes:** S15.1, which was deliberately deferred when `spire init` shipped

## Outcome

An operator can re-run `spire init` to change any answer it previously
collected, move backwards through the interview before committing, select a
model from a shipped catalog instead of typing one, and provision a missing
Linear workflow state without leaving the interview. Every answer is recorded in
a structured trace, so a later surprise can be traced to the decision that caused
it.

## Motivating evidence

The first live run against a real Linear workspace on 2026-07-31 produced three
defects that no test could have caught:

- **A total transport failure.** Every Linear request wrapped the personal API
  key in `Bearer`, which Linear rejects with 400. No Linear call had ever
  succeeded against the real API. The adapter suite passed throughout, because
  every test fed JSON directly into the `normalize_*` functions and nothing
  exercised the code that builds a request.
- **An unvalidated model.** After eight consecutive numbered menus, the model
  prompt was free text. The operator typed `1`. `ModelId` accepts any non-empty
  string under 256 characters, so `model: "1"` was written to disk and would
  next have appeared as a provider error at first dispatch.
- **No record of any of it.** `spire init` emits zero tracing events, so with
  `RUST_LOG=info` set and stderr redirected to a file, the log was empty. The
  configuration records outcomes; nothing recorded decisions.

Changing one answer currently requires hand-editing YAML, because init refuses
to run when a configuration exists and `spire config` has no `set`.

## Entry criteria

- `spire init` completes against a live Linear workspace and writes a schema-4
  configuration.
- Linear discovery returns teams, workflow states, and estimate scales.
- The Linear API key is accepted by the real API.
- `docs/decisions/first-run-onboarding-and-project-mapping.md` is at its
  2026-07-31 revision.

## Target command surface

```text
spire init
spire init --credential-file PATH
```

No new top-level command. `init` remains the single entry point and gains
re-runnability rather than a sibling `edit` verb. Any surface change updates
`docs/decisions/cli-command-surface.md` and the locks in
`crates/spire/tests/cli_surface.rs` and `crates/spire/tests/cli_smoke.rs` in the
same pull request.

No secret is accepted as a command-line argument.

## Work packages

### S16.1 Separate prompting behind a port

The interview is currently unreachable from a test because it reads the terminal
directly. Every later package in this sprint depends on removing that coupling.

Implementation:

1. Define a `PromptPort` in the application layer covering the interaction kinds
   the interview uses: single choice from a list, free text, and confirmation.
2. Move suggestion ranking, default selection, and answer validation behind that
   port so they are pure and testable.
3. Implement the terminal adapter in the CLI crate. Secret entry stays on the
   existing no-echo path and never passes through a recorded prompt.
4. Implement a scripted adapter for tests that replays a fixed answer sequence
   and fails on an unexpected prompt.

Verification:

- A complete interview runs end to end under the scripted adapter with no
  terminal.
- An answer sequence that ends early fails rather than silently accepting a
  default.
- The scripted adapter refuses to supply a secret.

### S16.2 Make the interview navigable

This is the package that carries the sprint's risk. The other six add or move
code; this one changes the interview from a straight line of seventeen blocking
reads into a graph that can be walked backwards, and every ambiguity left here
becomes a defect that only shows up when an operator revisits a step.

#### The step sequence

The shipped interview is seventeen prompts in a fixed order. Naming them is part
of the deliverable, because S16.7 asserts the sequence and a step that is not on
this list cannot be tested for.

| # | Step | Kind | Answer |
|---:|---|---|---|
| 1 | Path preview | confirm | proceed or stop |
| 2 | Linear credential | secret | API key |
| 3 | Team | choice | team ID |
| 4–10 | One per `LinearStateKind::ALL` | choice | workflow state ID |
| 11 | Complexity mapping | confirm | accept the derived mapping |
| 12 | Maker provider | choice | `HarnessId` |
| 13 | Maker model | choice, S16.4 | `ModelId` |
| 14 | Maker effort | choice | `Effort` |
| 15 | Reviewer provider | choice | `HarnessId` |
| 16 | Reviewer model | choice, S16.4 | `ModelId` |
| 17 | Reviewer effort | choice | `Effort` |

Steps 1 and 2 are outside the navigable region; see "Boundaries" below.

#### The dependency graph

Item 4 of a bare list would say "recompute dependents". The dependents have to be
enumerated, because each edge is a separate way to write a configuration that
references an object from a team the operator no longer selected.

| Changing | Invalidates | Why |
|---|---|---|
| Team (3) | Steps 4–10 | Workflow state IDs are team-scoped. A retained ID names a state on the old team. |
| Team (3) | Step 11 | The complexity mapping is derived from that team's estimate scale, which differs per team and may be `notUsed`. |
| Maker provider (12) | Step 13 | The model catalog is per-provider. |
| Maker provider (12) | Steps 15–17 | The reviewer's option list excludes the maker's provider, so the previously confirmed reviewer may no longer be offered. |
| Reviewer provider (15) | Step 16 | Same catalog scoping. |

An invalidated step is cleared, not silently re-answered. Nothing derived from a
cleared step may reach `OnboardingAnswers`.

#### Navigation rules

1. Model the interview as an ordered sequence of steps, each holding its
   question, its offered options, and its confirmed answer.
2. Retain a revisited step's previous answer as its default rather than
   discarding it. A stack that pops on back makes a one-key correction cost a
   full re-entry, which is the pain this sprint exists to remove. Retention and
   invalidation are different mechanisms: a step is cleared only when the
   dependency table says so.
3. Accept a back token at every navigable prompt and state it on the prompt line.
   Undiscoverable navigation is not navigation.
4. Reserve the back token from the answer space of every step that accepts free
   text. The model step is the only one, and S16.4 makes it a choice list with an
   explicit off-catalog escape, so the reserved token must be rejected as a model
   ID rather than accepted as one.
5. Back from the first navigable step exits the interview. It does not underflow
   and does not silently do nothing.
6. When an edit made from the review invalidates no dependent, return directly to
   the review. When it invalidates dependents, walk forward through exactly the
   cleared steps and then return to the review. Never replay steps that are still
   valid.
7. Cache each discovery response for the run, keyed by the input it was fetched
   for. Revisiting the team step re-offers the cached team list; selecting a team
   whose configuration was already fetched reuses it. A revisit must not become a
   round trip, and the option list must not change under the operator mid-run.
8. Step 11 currently dead-ends: declining the derived mapping aborts with advice
   to edit a file that does not exist yet. Declining must instead route back to
   the team step, since the estimate scale is a property of the team and there is
   no other answer to change.

#### The review

9. Present every answer the write will use, in step order, before any write.
10. Show resolved names, not opaque identifiers. The configuration stores Linear
    UUIDs; a review that prints them cannot be checked by the person reading it.
11. Let any single step be selected from the review by its number.
12. Make the write a distinct confirmation from the review itself, so reaching
    the end of the interview is not the same act as committing it.

#### Boundaries

13. Steps 1 and 2 are not navigable. The preview confirmation precedes the
    interview, and the credential is verified and stored before discovery can
    run, so backing into it would mean unwinding a completed side effect.
    Changing the credential is a re-run, not a back step.
14. The claim in `crates/spire/src/init.rs` that an interrupted run "leaves the
    installation exactly as it found it" is already false: `authenticate` writes
    the secret store and the authentication metadata store before step 3. Either
    make the statement true by deferring those writes to the single commit point,
    or correct the statement and the operator-facing message. Do not carry the
    contradiction forward into a flow where interruption becomes common.
15. `EOF` on stdin is the only interruption the interview currently handles.
    `SIGINT` kills the process wherever it lands. State which of the two this
    package covers rather than leaving the guarantee to depend on how the
    operator quits.

Verification:

- Going back to the team step and choosing a different team clears steps 4–11 and
  re-collects them, rather than reusing state IDs from the old team.
- Going back to the team step and reselecting the same team retains the existing
  mapping as defaults and issues no new discovery request.
- Choosing a maker provider that the reviewer already holds clears the reviewer
  steps rather than producing a configuration where both roles share a provider.
- Editing the maker effort from the review returns to the review without
  re-asking the reviewer steps.
- Back at step 3 exits the interview and writes no configuration.
- The reserved back token is rejected as a model ID.
- The review renders team, state, and model names; no answer is displayed only as
  a UUID.
- Declining at the review writes nothing and says so.
- Interruption at any step, including inside the review, leaves the installation
  in the state this package commits to in item 14, and a test asserts that state
  rather than the current unverified claim.

### S16.3 Seed the interview from an existing configuration

Implementation:

1. Load an existing configuration when one is present and use each value as the
   default for its step.
2. Replace the refusal to run with a preview that names the file to be replaced
   and requires confirmation.
3. Back up the existing configuration before replacing it, and leave the backup
   on failure.
4. Preserve values the interview does not collect, including the unresolved
   GitHub, Cloudflare, and webhook fields, so re-running never reintroduces a
   placeholder over a resolved value.
5. Report which values changed relative to the loaded configuration.
6. Keep the single atomic write. An interrupted re-run leaves the original
   configuration in place and unmodified.

Verification:

- Re-running and accepting every default produces a configuration equivalent to
  the original.
- A resolved `github.installation_id` survives a re-run.
- An interrupted re-run leaves the original file byte-identical.
- The existing guard test is replaced by one asserting the backup-and-confirm
  path, not the refusal.

### S16.4 Ship a model catalog

Implementation:

1. Add a catalog data file listing known model identifiers per provider, loadable
   without rebuilding the binary.
2. Offer the catalog entries for the selected provider as a choice list.
3. Accept a model outside the catalog through an explicit escape, record it, and
   warn that it is unverified.
4. Reject an empty model. Do not attempt to infer intent from the shape of the
   input; a syntactic guard cannot distinguish a retired model from a current
   one.
5. Record the catalog version used, so a configuration can be explained later.

Verification:

- The model step cannot be satisfied by a menu index for a step that offered a
  list.
- An off-catalog model is accepted and marked unverified in the trace.
- A missing or malformed catalog file fails at startup with the path named,
  rather than silently offering nothing.

### S16.5 Trace every onboarding decision

Implementation:

1. Emit a structured event for each confirmed answer carrying the step, the
   chosen value, whether it was the suggested default, and whether it was
   revisited.
2. Emit an event for each derived value, including the complexity mapping and its
   source estimate scale.
3. Emit an event for the write, naming the destination and the backup.
4. Never emit a credential, a raw provider payload, or untrusted issue text.
5. Document the redirect required to capture the trace on macOS, where no
   supervisor collects stderr.

Verification:

- A complete interview produces a trace from which every written value can be
  explained.
- Credential and untrusted-content sentinels never appear in the trace.
- The trace is emitted for a failed run up to the point of failure.

### S16.6 Provision a missing workflow state

A team may have no state that can carry a Spire lifecycle state. Sending the
operator to the Linear UI and requiring a restart of the interview is not an
acceptable outcome.

Implementation:

1. Detect a lifecycle state with no acceptable candidate and offer to create one.
2. Show the exact proposed state, its category, and its target team.
3. Require per-action confirmation. This confirmation authorizes one named
   change and nothing else.
4. Record a durable provisioning operation before delivery, reusing the S15.6
   contract.
5. Query existing states before creating on any retry.
6. Re-read the team's states after creation rather than assuming the local shape.

Rules:

- Setup provisioning writes and runtime ticket writes are distinct authorities
  and never share a gate. Confirming a state creation must not consult or affect
  `rollout.linear_writes_enabled`, and enabling rollout must not authorize a
  schema change.
- A setup write uses the operator's own credential and is attributed to that
  person in Linear.
- Creation is bounded to the selected team. No label, project, initiative,
  document, or ticket is created as a side effect.

Verification:

- Declining creation leaves the interview able to continue or stop cleanly.
- A crash between intent and response cannot produce two states on retry.
- Permission failure maps no lifecycle state and writes no configuration.
- A test asserts that the rollout gate is not read anywhere on this path.

### S16.7 Cover the interview in tests

Implementation:

1. Drive full interviews through the scripted prompt adapter against fixture
   discovery data.
2. Cover the first-run, re-run, back-navigation, changed-team, declined-write,
   and interrupted paths.
3. Assert the resulting configuration parses and validates as expected for each.
4. Add a transport-level test for the Linear adapter that asserts the outgoing
   request shape, including the authorization header, against a local HTTP
   server. The `Bearer` defect survived because no test ever built a request.

Verification:

- The interview suite runs without a terminal and without network access.
- A change to the authorization header format fails a test.
- Removing a step from the interview fails a test rather than silently
  shortening the flow.

## Suggested pull-request slices

1. `PromptPort`, the terminal and scripted adapters, and the interview test
   harness.
2. The step sequence, the back token, and dependent-step invalidation.
3. The review, edit-from-review re-entry, and the interruption guarantee from
   S16.2 item 14.
4. Existing-configuration seeding, backup, and change reporting.
5. Model catalog and decision trace.
6. Workflow-state provisioning and the Linear transport test.

## Sprint demo

Starting from the configuration written by the first live run, re-run
`spire init`. Accept defaults through to the harness step, correct the model that
was previously recorded as `1`, then go back and select a different team to show
the state mapping being discarded and recollected. Return to the original team,
reach the final review, and interrupt — showing the original configuration
unchanged. Re-run, complete the write, and show the backup alongside a trace that
explains every value in the new file. On a team missing a lifecycle state, create
that state from inside the interview and show the provisioning operation, with
rollout still disabled and no ticket admitted.

## Unknown / Unverified

- Whether back-navigation is better served by a full-screen interface or by
  in-line prompts is an implementation decision, and the dependency it adds
  belongs to the CLI crate only.
- Codex exposes no model alias system and no model listing. Claude Code accepts
  stable aliases. The catalog's shape must accommodate both without implying the
  two providers offer equivalent guarantees.
- Model probing is out of scope here. It depends on a harness execution path that
  can spawn a provider process, which `crates/spire-adapters/src/harness.rs` does
  not yet do.
- The Linear `workflowStateCreate` contract requires verification against the
  authenticated target workspace before S16.6 is implemented, in the same way
  `projectCreate` did for S15.7.
