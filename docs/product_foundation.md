# Loop Engineering — Implementation Plan v2

**Goal:** A reliable outer loop that discovers work in Linear and a strict inner pipeline that turns clear tickets into draft PRs — while architecture/ADR work stays interactive. This revision incorporates the concurrency, scheduling, CI, and failure-handling decisions from review.

**Core design principles (settled):**

1. **The outer loop is not an LLM.** Polling, filtering, and claiming are pure mechanics — a plain script against Linear's GraphQL API. Claude Code is only invoked per-ticket, so empty polls cost zero tokens.
2. **Concurrency control lives in exactly one place: the singleton poller.** Serialize the claiming, parallelize the runs. A systemd oneshot service can't overlap itself, so the claim race disappears by construction. Concurrent agent runs on different tickets are then safe.
3. **CI is the load-bearing gate.** GitHub Actions required checks are what make "CI green" a meaningful stop condition. The Reviewer stage supplements CI; it never replaces it.
4. **Start with 3 pipeline stages, not 6.** Split stages only when observed failure modes demand it.
5. **Artifacts are the contracts, and they're harness-agnostic.** `requirements.md`, `plan.md`, ADRs are just files in the repo. If Claude Code is ever swapped for pi (or anything else), the contracts survive.
6. **Draft PRs only. Agents never merge.** The bot token cannot merge; a human merges after checks pass.

---

## Phase 0 — Foundations

### Linear (lean, one team)

**Statuses:**

- Triage
- Backlog
- Specs Needed
- Ready for Agent
- In Progress
- In Review
- **Blocked** ← added: explicit terminal state for failed runs, so the poller never re-picks a failing ticket
- Done
- Canceled

**Labels** (type group): `bug`, `feature`, `chore`, `refactor`, `architecture`, `adr`, `spike`

**Views:** "Agent Queue" = status is `Ready for Agent` AND has a type label.

**Access:** Create a dedicated bot API key (personal API key on a bot workspace member if possible). All agent status changes and comments come from this identity — this is also how the poller counts *its own* active claims for the concurrency cap.

**Integration:** Enable the GitHub integration so PR open/merge can auto-move status (verify it doesn't fight the poller's own status writes; prefer one writer per transition — poller owns `Ready → In Progress`, pipeline owns `In Progress → In Review/Blocked`, GitHub integration owns `In Review → Done` on merge).

### Repository

- `docs/adr/` for ADRs (committed markdown is the source of truth; Linear tickets for ADRs are optional tracking only).
- Root `AGENTS.md` (with `CLAUDE.md` referencing it):
  - Always check `docs/adr/` before structural changes.
  - Coding conventions, test commands, definition of done.
  - Environment isolation rules for concurrent runs (see Phase 2).
- Branch naming convention: `agent/<ticket-id>-<slug>`. Predictable names make the reaper and ledger trivial.
- Worktree layout: `~/worktrees/<ticket-id>/` — one per ticket, cleaned up on Done/Canceled.

### VM

- Claude Code installed and authenticated; Linear MCP (`https://mcp.linear.app/mcp`) connected and tested interactively.
- A `loop/` directory in a small ops repo containing: poll script, pipeline launcher, reaper script, systemd units, run ledger.

---

## Phase 1 — CI gate first (before any agent writes code)

GitHub Actions, minimal and boring:

1. **`ci.yml`** — on `pull_request`: install, lint, typecheck, test. Nothing clever. If the suite needs services, use service containers so CI is self-contained.
2. **Branch protection on `main`:** required status checks = the CI workflow; require branches up to date before merge — or enable **merge queue** once >1 agent PR is routinely open. This is what catches two individually-green PRs that conflict semantically at merge.
3. **Bot GitHub token scoped to:** create branches, push, open draft PRs, comment. **No merge permission.**
4. Confirm the test suite is parallel-safe (no fixed ports, no shared mutable test DB). If it already runs in CI containers, this is likely done; if it assumes a fixed local environment, fix that now — it is the real concurrency bug, not git.

**Exit criteria:** a manually opened PR shows required checks; an out-of-date branch cannot merge.

---

## Phase 2 — Inner pipeline, run manually (3 stages)

Each stage is a separate `claude -p` invocation with its own `--allowedTools` scope, its own model, and a file artifact as its output contract. The launcher script (`run-pipeline.sh <ticket-id>`) sequences them.

### Stage A — Spec-check (cheap/fast model)

- Read the ticket via Linear MCP. Confirm type label; extract acceptance criteria into `requirements.md` in the worktree.
- **If criteria are missing or ambiguous: do not die silently.** Move the ticket to `Specs Needed`, post a comment listing exactly what's missing, exit. This routing is the quality valve for the whole system — garbage stops here.
- Tools: Linear MCP read/comment/status, file write. No bash.

### Stage B — Implement + test (strong model, isolated worktree)

- Create worktree + branch from the naming convention.
- Write `plan.md` (short plan + file touch list + test plan), then implement with tests.
- **Environment isolation (mandatory for concurrency):** ports randomized or derived from ticket ID; Docker Compose project name = ticket ID; per-worktree `.env`. Encode these rules in `AGENTS.md` so the agent applies them without being told per-run.
- Tools: file ops, bash (scoped), git. No Linear writes, no push yet.
- Stop condition: `requirements.md` satisfied per its own checklist, local tests green, or turn cap hit.

### Stage C — Review + ship (strong model — review is where judgment pays)

- Fresh context. Verify diff against `requirements.md` and `plan.md`; check ADR compliance; run tests + lint.
- **Reject path:** on failure, write findings to `review.md`, loop back to Stage B once. On second failure → mark ticket `Blocked` with a comment, stop.
- **Ship path:** commit, push, open **draft PR** linking the ticket, move status to `In Review`, post a summary comment with the run ID.
- Tools: bash (read/test), git push, GitHub CLI (draft PR only), Linear MCP write.

> Model routing note: cheap for Stage A, strong for B and C. A weak reviewer rubber-stamping a strong implementer defeats maker-checker — if anything, economize on B before C.

**Validation:** run the launcher by hand on 2–3 well-specified tickets. Check the artifacts read sensibly, worktrees don't collide, and the Linear trail is clean. Do not proceed to Phase 3 until 2–3 consecutive manual runs need no intervention.

---

## Phase 3 — Outer loop (systemd timer + dumb poller)

### Poll script (no LLM)

Plain script (bash + curl, or a small TypeScript/Python script) against Linear GraphQL:

1. **Reap stale runs:** any ticket `In Progress` claimed by the bot for > N hours (start: 2h) with no PR → move to `Blocked`, comment, clean up worktree.
2. Query `Ready for Agent` + type label.
3. **Route:** `architecture`/`adr`/`spike` → notify (ntfy/Slack/email), do not launch. Everything else continues.
4. **Concurrency cap:** count tickets currently `In Progress` claimed by the bot. If ≥ `MAX_ACTIVE` (start: 2), stop. This bounds token burn and VM load — a runaway queue can't launch twelve agents.
5. **Claim:** set `In Progress` + comment with run ID, *then* launch the pipeline detached (`systemd-run --user` or `nohup`) so the poller exits quickly.
6. Append every decision to the run ledger.

### Scheduling

- **systemd timer** (not raw cron): `OnCalendar=*:0/30`, `Persistent=true`, driving a **`Type=oneshot`** service. systemd will not start the service while a previous invocation is still running — this *is* the singleton lock. Logs land in journald.
- Explicitly **not** Claude Code's embedded scheduler for this: `/loop` is session-scoped, fires only while the session is idle, has no missed-fire catch-up, and interval loops expire after 7 days. Desktop scheduled tasks are macOS/Windows-only; cloud Routines run off-box away from worktrees. All fine for babysitting a PR in a session; none are orchestration.

### Run ledger

Append-only JSONL (or SQLite) at `~/loop/ledger.jsonl`: `{ts, ticket, run_id, stage, event, detail}`. This — not Linear comments — is the loop's memory of what it already tried. (Claude Code sessions also persist locally and can be resumed by ID for post-mortems; record the session ID in the ledger.) Later, this ledger is a natural feed into the homelab shared-state MCP layer so agents on other machines can see run history.

### Failure policy (explicit)

| Condition | Action |
|---|---|
| Spec incomplete (Stage A) | → `Specs Needed` + comment |
| Review rejects twice (Stage C) | → `Blocked` + `review.md` findings in comment |
| Run crashes / stale > 2h | Reaper → `Blocked` + comment |
| Same ticket `Blocked` → manually re-readied and fails again | Stays `Blocked`; ledger shows retry count; human investigates |

`Blocked` is only exited by a human moving the ticket back to `Ready for Agent` (or `Specs Needed`).

### Rollout of the loop itself

1. **Dry-run mode first:** poller logs and notifies what it *would* claim, changes nothing. Run for a few days.
2. Enable claiming + pipeline for `chore` only.
3. Expand to `bug`, then `feature`/`refactor` as trust builds.

---

## Phase 4 — Hardening and optional upgrades

- **Merge queue** on `main` once agent PR volume makes "up to date before merge" annoying.
- **Worktree hygiene:** reaper deletes worktrees for tickets in `Done`/`Canceled`/`Blocked` > 7 days; disk check in the poller.
- **Budget controls:** turn caps per stage; per-day run cap in the poller; ledger makes spend visible per ticket.
- **Event-driven upgrade (optional):** replace polling with Linear webhook → Cloudflare Tunnel from the homelab → `repository_dispatch` → `anthropics/claude-code-action@v1` on a **self-hosted runner** on the VM. You gain GH concurrency groups, secrets management, and per-run logs on your own hardware, and kill polling latency. Do this only after the pipeline is boring — at that point orchestration reliability is the bottleneck, not agent behavior.
- **Harness optionality:** the stage contracts are files; the launcher is a script. The trigger to evaluate pi's SDK: the launcher accumulating ugly glue around the `claude -p` boundary (stdout parsing, flag-fighting, wanting mid-run control). Until then, Claude Code's appliance properties (permissions, MCP, worktrees, GH Action) win for unattended runs.

---

## Phase 5 — Architecture & ADR path (interactive only, unchanged)

- Interactive session, strongest model, ADR skill/template → commit `docs/adr/NNN-title.md`.
- Linear ticket only when prioritization/linkage is needed; the committed markdown is the source of truth.
- `AGENTS.md` instructs coding agents to read `docs/adr/` before structural changes; Stage C verifies compliance.
- The poller's routing (Phase 3, step 3) guarantees these labels never enter the unattended pipeline.

---

## Supporting skills (SKILL.md), written as needed

1. **Verification / definition-of-done** (Stage C always runs this) — build first.
2. **Requirements writer** (Stage A).
3. **ADR creator/reviewer** (interactive path).
4. Linear update helpers only if MCP calls prove fiddly.

---

## Build order (checklist)

- [ ] Linear: statuses incl. `Blocked`, type labels, Agent Queue view, bot API key
- [ ] Repo: `AGENTS.md`, `docs/adr/`, branch/worktree conventions
- [ ] **CI:** `ci.yml`, required checks, up-to-date-before-merge, scoped bot token (no merge)
- [ ] Verify test suite is parallel-safe (ports, DBs, containers)
- [ ] Verification skill + minimal `AGENTS.md` isolation rules
- [ ] `run-pipeline.sh` — Stage A → B → C, manual invocation
- [ ] 2–3 clean manual pipeline runs
- [ ] Poll script with reaper, routing, cap, claim, ledger — **dry-run mode**
- [ ] systemd timer + oneshot service
- [ ] Enable real claims for `chore`; expand by label as trust builds
- [ ] Merge queue when PR volume warrants
- [ ] (Optional) webhook → Actions → self-hosted runner migration

## Success criteria

- Labeled tickets in `Ready for Agent` become draft PRs with green required checks, without babysitting.
- No ticket is ever claimed twice; no run ever left silently stuck (`Blocked` + comment, always).
- Two concurrent runs never interfere (worktrees + env isolation + singleton claiming).
- Architecture/ADR work never enters the unattended path; agents never contradict committed ADRs.
- You intervene only on `In Review`, `Specs Needed`, and `Blocked` — and the ledger tells you why each ticket is where it is.
