# Sprint 16 — Interactive Onboarding CLI

**Last Verified:** 2026-07-31
**Depends on:** the shipped subset of Sprint 15 (S15.2 discovery, the base
`spire init` assembly from S15.4/S15.5)
**Unlocks:** repeated configuration changes during pilot testing
**Completes:** S15.1, which was deliberately deferred when `spire init` shipped

## Outcome

`spire init` becomes a full-screen configuration editor rather than a
questionnaire. An operator opens it on a fresh machine or on a configured one,
moves between sections in any order, corrects a single wrong value without
re-answering anything else, toggles multi-valued settings with space, and writes
only when every section reports itself complete. Every committed value is
recorded in a structured trace, so a later surprise can be traced to the decision
that caused it.

## The interaction model

This sprint replaces a linear interview with a menu. The distinction is not
cosmetic and it is the reason most of the packages below look different from a
prompt-by-prompt plan:

- **An interview enforces correctness through order.** Step 4 may assume step 3
  answered. A menu has no order, so ordering is replaced by **continuously
  recomputed per-section status**. This single substitution is the sprint's
  central design move.
- **An interview port is pull-based** — the application asks one question and
  receives one answer. **A menu port is push-based** — the application hands over
  a whole editable model and receives it back, mutated, on commit. The port must
  be defined the second way.
- **Invalidation becomes visible rather than procedural.** Changing the team does
  not march the operator back through seven prompts. It marks the dependent
  sections stale, states why, and refuses the write until they are revisited.

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

### S16.1 Define the editor port and its model

The interview is currently unreachable from a test because it reads the terminal
directly. Every later package depends on removing that coupling, and on removing
it in the push-based direction.

Implementation:

1. Define an `OnboardingModel` in the application layer holding every value the
   configuration needs, each field independently optional and independently
   invalidatable. This is the document the editor edits.
2. Define an `OnboardingEditorPort` with essentially one operation: given a model
   and the discovery data available, return either a committed model or a
   cancellation. Do not define one method per question; that reintroduces the
   pull-based interview behind a new name.
3. Define a `DiscoveryRequest`/`DiscoveryResponse` channel contract so the editor
   can ask for teams or a team configuration while remaining responsive. The
   editor never calls Linear itself.
4. Keep ranking, defaulting, derivation, and validation as pure functions over
   the model in the application layer. The adapter renders and dispatches key
   events; it decides nothing.
5. Implement a headless adapter for tests that applies a scripted event sequence
   and fails on an event the current screen cannot accept.

Verification:

- A complete configuration is produced end to end under the headless adapter with
  no terminal.
- The application crate compiles with no terminal, rendering, or `crossterm`
  dependency, and `./scripts/check-architecture.sh` passes.
- Model validation is exercised directly, without constructing an editor.

### S16.2 Build the full-screen editor

This is the package that carries the sprint's risk. Every other package adds or
moves code; this one introduces an event loop, an alternate screen, and raw mode
into a binary that has never had any of them.

#### Layout

A home screen lists the sections with a status marker each. Enter opens a
section; Esc returns to the home screen. Sections may be opened in any order and
any number of times.

| Section | Contents | Status is complete when |
|---|---|---|
| Linear | credential state, selected team | a team is selected against a verified credential |
| Workflow states | seven rows, one per `LinearStateKind::ALL` | all seven bind to a state on the selected team |
| Complexity | estimate scale and the derived mapping | the scale is usable and the mapping is accepted |
| Maker | provider, model, effort | all three set and the model is catalog-resolved or explicitly off-catalog |
| Reviewer | provider, model, effort | as above, and the provider differs from the maker's |
| Type labels | multi-select over the team's labels | at least one label selected |
| Rollout | multi-select over teams for `rollout.allowed_team_ids` | optional; may be left empty while writes are disabled |
| Paths | read-only preview of what will be written | always |
| Review and write | every committed value, then the write | terminal action |

The seven workflow-state rows are pre-filled from `rank_states` rather than
asked one at a time. The operator overrides the rows that are wrong. This is the
main ergonomic win over the shipped flow, which asks seven questions to change
an average of one answer.

#### Key contract

6. `↑`/`↓` move, `Enter` opens or confirms, `Esc` goes back one level, `q` quits
   from the home screen, `r` jumps to review.
7. `Space` toggles membership in the multi-select sections. It has no meaning in
   single-select lists and must not silently act as `Enter` there.
8. Render the active key contract in a footer on every screen. An undiscoverable
   binding is not a feature.
9. `Esc` on the home screen is a quit confirmation, never a silent exit.

#### Status, not order

10. Recompute every section's status after every model mutation. Correctness
    comes from status, not from the order sections were visited.
11. Give a section three distinguishable states — complete, incomplete, and
    stale — and render stale differently from never-answered. Stale must state
    what invalidated it.
12. Apply these invalidation edges on mutation:

| Changing | Marks stale | Why |
|---|---|---|
| Team | Workflow states | State IDs are team-scoped; a retained ID names a state on the old team. |
| Team | Complexity | The mapping derives from that team's estimate scale, which differs per team and may be `notUsed`. |
| Team | Type labels | Labels are team-scoped. |
| Maker provider | Maker model | The catalog is per-provider. |
| Maker provider | Reviewer | The reviewer's options exclude the maker's provider, so a confirmed reviewer may no longer be legal. |
| Reviewer provider | Reviewer model | Same catalog scoping. |
| Model | Effort of the same role | Effort is a property of the model, not the provider: two models behind one provider accept different levels, so a model with a lower ceiling strands the confirmed effort. |

The model-to-effort edge differs from the others in its recovery. A stranded
effort has a single unambiguous replacement — the new model's own default — so
the editor substitutes it and reports the substitution, rather than marking the
section stale and demanding a re-confirmation that could only produce the same
answer. Retaining the illegal level would send the provider a pair it rejects.

13. Retain the stale value and offer it as the default when the section is
    reopened, unless it is no longer among the legal options. Discarding a value
    the operator may want to re-confirm is the cost the shipped flow already
    imposes.
14. Refuse the write while any section is incomplete or stale, naming each one.
    Refusal is the only ordering mechanism this design has.

#### Asynchronous discovery

15. Run Linear discovery on the Tokio runtime and deliver results to the editor
    over a channel. A blocking network call inside the event loop freezes the
    frame and is not acceptable.
16. Render an explicit in-progress state for a section awaiting discovery, and
    keep navigation and quit responsive while it is outstanding.
17. Render a discovery failure inside the frame with a retry, rather than
    unwinding the whole editor. A transient Linear error must not discard
    collected work.
18. Cache each discovery response for the run, keyed by the input it was fetched
    for. Reselecting a previously fetched team must not issue a second request,
    and the option lists must not shift under the operator mid-run.

#### Terminal safety

19. Install a panic hook that leaves the alternate screen and disables raw mode
    before the panic message prints. Without it, any panic leaves the operator
    with an unusable terminal and no visible error.
20. Restore the terminal on every exit path, including `Esc` quit, discovery
    failure, and error propagation out of `main`.
21. Enter the alternate screen only after the terminal is confirmed usable.
    Refuse with an actionable message when stdin is not a TTY, when `TERM` is
    unset or `dumb`, or when the window is below the stated minimum size.
22. Mask secret entry in the render buffer. Raw mode already suppresses echo, so
    the risk moves from the terminal to the frame: the credential must never
    enter a buffer that a test snapshot or a panic message could surface.

#### Boundaries

23. Multi-team ingestion is out of scope. `linear.team_id` is a single `String`
    that filters the issue query, and both the state mapping and the complexity
    mapping hang off that one team, so plural ingestion is a schema-5 change with
    its own reconciliation work. The Rollout section's multi-select populates
    `rollout.allowed_team_ids`, which is already a `Vec<String>` that init leaves
    empty today, forcing a hand edit before writes can ever be enabled.
24. The seven lifecycle states cannot be partially mapped. They are seven
    required named fields on the configuration, not a map, so a state left
    unbound has no representable value. Pre-filling and overriding replaces
    partial selection.
25. The claim in `crates/spire/src/init.rs` that an interrupted run "leaves the
    installation exactly as it found it" is already false: `authenticate` writes
    the secret store and the authentication metadata store before any other
    answer is collected. Either make it true by deferring those writes to the
    single commit point, or correct the statement, the operator-facing message,
    and the matching assertion in the decision record. Do not carry the
    contradiction into a design where abandoning a session becomes routine.
26. Raw mode delivers `Ctrl-C` as a key event rather than `SIGINT`, so the
    editor owns the decision. Treat it as an abandon that restores the terminal
    and writes nothing, and state that it is not a kill.

Verification:

- The full editor runs under `TestBackend` with a synthetic key sequence, with
  no terminal and no network.
- Selecting a different team marks workflow states, complexity, and type labels
  stale, and the write is refused while they are.
- Reselecting the same team issues no second discovery request.
- Choosing a maker provider the reviewer already holds marks the reviewer stale
  rather than producing a configuration where both roles share a provider.
- Editing the maker effort marks nothing else stale and leaves the write
  available.
- `Space` in a single-select list does not advance or confirm.
- A panic raised mid-frame restores the terminal; a test asserts the hook runs.
- A non-TTY, a `dumb` terminal, and an undersized window each fail with the
  reason named, before the alternate screen is entered.
- No rendered buffer in any test snapshot contains the credential sentinel.
- Abandoning a session at any point, including with `Ctrl-C`, leaves the
  installation in the state item 25 commits to, and a test asserts that state
  rather than the current unverified claim.

### S16.3 Seed the editor from an existing configuration

Implementation:

1. Load an existing configuration when one is present and use it to populate the
   model, so every section opens already complete.
2. Replace the refusal to run with a preview that names the file to be replaced
   and requires confirmation.
3. Back up the existing configuration before replacing it, and leave the backup
   on failure.
4. Preserve values the editor does not expose, including the unresolved GitHub,
   Cloudflare, and webhook fields, so re-running never reintroduces a placeholder
   over a resolved value.
5. Mark changed values in the review, relative to the loaded configuration.
6. Keep the single atomic write. An interrupted re-run leaves the original
   configuration in place and unmodified.

Verification:

- Opening and immediately writing produces a configuration equivalent to the
  original.
- A resolved `github.installation_id` survives a re-run.
- An interrupted re-run leaves the original file byte-identical.
- The existing guard test is replaced by one asserting the backup-and-confirm
  path, not the refusal.

### S16.4 Ship a model catalog

Implementation:

1. Add a catalog data file listing known model identifiers per provider, loadable
   without rebuilding the binary.
2. Give each catalog entry the effort levels it accepts and the provider's own
   default, rather than one effort list per provider. A provider serves models
   with different reasoning ceilings, so a provider-wide list offers pairs the
   provider rejects at dispatch. Carry the same shape through to
   `HarnessCapabilityRegistry` and to `harnesses.advanced` in the configuration.
3. Offer the catalog entries for the selected provider as the section's list, and
   offer only the selected model's efforts as the effort list.
4. Accept a model outside the catalog through an explicit escape, record it, and
   mark it unverified in the section and the review. An off-catalog model
   declares no ceiling, so every effort stays offered and the operator owns the
   choice.
5. Reject an empty model. Do not attempt to infer intent from the shape of the
   input; a syntactic guard cannot distinguish a retired model from a current
   one.
6. Reject a catalog entry whose default effort is absent from its own effort
   list, at load time, with the model named.
7. Record the catalog version used, so a configuration can be explained later.

Verification:

- The model section offers only the selected provider's entries, and cycling
  reaches every one of them rather than repeatedly selecting the first.
- The effort section offers only the levels the selected model accepts.
- Selecting a model with a lower ceiling substitutes that model's default effort
  and reports the substitution.
- The capability registry refuses a model/effort pair that neither the catalog
  nor the configuration declared together, even when both appear separately
  under the same provider.
- An off-catalog model is accepted and marked unverified in both the section and
  the trace.
- A missing or malformed catalog file fails at startup with the path named,
  before the alternate screen is entered, rather than silently offering nothing.

### S16.5 Trace every onboarding decision

Implementation:

1. Emit a structured event for each committed model mutation carrying the
   section, the field, the new value, and whether it replaced a suggested
   default.
2. Emit an event for each invalidation, naming the mutation that caused it and
   the sections marked stale.
3. Emit an event for each derived value, including the complexity mapping and its
   source estimate scale.
4. Emit an event for the write, naming the destination and the backup.
5. Never emit a credential, a raw provider payload, or untrusted issue text.
6. Write the trace to a file rather than stderr. Stderr belongs to the alternate
   screen for the duration of the editor, so the previously documented redirect
   is no longer available.

Verification:

- A completed session produces a trace from which every written value can be
  explained.
- Credential and untrusted-content sentinels never appear in the trace.
- The trace is emitted for an abandoned session up to the point it was abandoned.
- Trace output never corrupts the rendered frame.

### S16.6 Provision a missing workflow state

A team may have no state that can carry a Spire lifecycle state. Sending the
operator to the Linear UI and requiring a restart is not an acceptable outcome.

Implementation:

1. Detect a lifecycle row with no acceptable candidate and offer creation as an
   action on that row.
2. Show the exact proposed state, its category, and its target team.
3. Require per-action confirmation. This confirmation authorizes one named
   change and nothing else.
4. Record a durable provisioning operation before delivery, reusing the S15.6
   contract.
5. Query existing states before creating on any retry.
6. Re-read the team's states after creation rather than assuming the local shape,
   and refresh the section from the re-read.

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

- Declining creation leaves the section usable and the editor running.
- A crash between intent and response cannot produce two states on retry.
- Permission failure maps no lifecycle state, marks the section incomplete, and
  writes no configuration.
- A test asserts that the rollout gate is not read anywhere on this path.

### S16.7 Cover the editor in tests

Implementation:

1. Drive full sessions through the headless adapter against fixture discovery
   data, asserting on both the resulting model and the rendered buffer.
2. Cover the first-run, re-run, invalidation, declined-write, discovery-failure,
   and abandoned paths.
3. Assert the resulting configuration parses and validates as expected for each.
4. Add a transport-level test for the Linear adapter that asserts the outgoing
   request shape, including the authorization header, against a local HTTP
   server. The `Bearer` defect survived because no test ever built a request.

Verification:

- The suite runs without a terminal and without network access.
- A change to the authorization header format fails a test.
- Removing a section from the editor fails a test rather than silently
  shortening the flow.

## Suggested pull-request slices

1. `OnboardingModel`, `OnboardingEditorPort`, the pure status and invalidation
   functions, and the headless adapter.
2. The event loop, home screen, terminal guards, and panic-safe restoration —
   with one section wired end to end.
3. The remaining sections, including multi-select and pre-filled workflow states.
4. Asynchronous discovery, caching, and in-frame failure handling.
5. Existing-configuration seeding, backup, and change marking.
6. Model catalog and decision trace.
7. Workflow-state provisioning and the Linear transport test.

## Sprint demo

Open `spire init` on the configuration written by the first live run. Every
section shows complete. Open Maker, correct the model previously recorded as `1`
from the catalog, and return. Open Linear, select a different team, and show
workflow states, complexity, and type labels all marked stale with the reason,
and the write refused. Return to the original team, show the retained values
offered back, and complete the sections. Toggle type labels with space. Quit
without writing and show the original configuration unchanged. Reopen, write, and
show the backup alongside a trace explaining every value in the new file. On a
team missing a lifecycle state, create that state from its row and show the
provisioning operation, with rollout still disabled and no ticket admitted.

## Unknown / Unverified

- The editor is a full-screen `ratatui` application on the alternate screen. The
  rendering dependency belongs to the CLI crate only and must not reach the
  application layer.
- Codex CLI does fetch a model list and caches it at `~/.codex/models_cache.json`
  with a per-model effort set and default; the catalog's codex rows are copied
  from an observed cache. Claude Code publishes one `--effort` list on its CLI
  without per-model qualification, so its rows repeat that list and are the
  weaker claim. The catalog must accommodate both without implying the two
  providers offer equivalent guarantees. Reading either provider's cache at
  runtime is out of scope; the file stays operator-editable instead.
- Model probing is out of scope here. It depends on a harness execution path that
  can spawn a provider process, which `crates/spire-adapters/src/harness.rs` does
  not yet do.
- The Linear `workflowStateCreate` contract requires verification against the
  authenticated target workspace before S16.6 is implemented, in the same way
  `projectCreate` did for S15.7.
- Whether the Linear label list needed by the Type labels section is returned by
  the existing team-configuration query, or requires a new one, is unverified.
