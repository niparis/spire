# Project: spire
A Go CLI that bootstraps and syncs an opinionated Spec-Driven Development (SDD) methodology into other repositories.

Built with: Go 1.25 (standard library only — no external dependencies). Ships a markdown methodology consumed by OpenCode (built-in modes + skills + subagents).

## North Star Preferences
- Two halves, kept separate: `methodology/` is the shipped markdown payload (agent prompts, skills, subagents, templates, root projections) — editing it changes what users receive via `spire init`/`update`; `cmd/spire/` + `internal/` is the Go CLI that distributes it. Most tasks touch one side, not both.
- `methodology/agents/SPIRE.md` is the authoritative workflow. When you change it, keep the other methodology files, `README.md`, and `docs/specs/PRODUCT.md` consistent.
- Go: standard library only — don't add dependencies without a strong, stated reason.
- The methodology runs on OpenCode's built-in `plan`/`build` modes plus skills and subagents — never add custom primary agents.
- Canonical SDD layout is `docs/`-rooted: `docs/specs`, `docs/architecture`, `docs/changes`, `docs/archive`.
- Subagent files (`methodology/subagents/<name>.md`) are thin (frontmatter + "read these files"); detailed prompts live in `methodology/agents/<NAME>.md` and are projected to `.opencode/agents/` by `methodology/project_root/manifest.json`.

## Documentation & Resources
- How the workflow operates: `methodology/agents/SPIRE.md`.
- This repo dogfoods its own SDD: product vision in `docs/specs/PRODUCT.md`; feature/design docs in `docs/features/`.
- There is no `REQUIREMENTS.md` — work is tracked as SDD artifacts (below), not a flat roadmap file.

## Requirements / Roadmap
spire uses the very methodology it ships. Track work as SDD artifacts:
- A feature begins as a spec `docs/specs/feature-NNN-<slug>.md` (Gate 0).
- Active work lives in `docs/changes/NNN-<slug>/` (`AUDIT.md`, `PLAN.md`, `SESSION.md`, `VERIFICATION_REPORT.md`).
- Completed work moves to `docs/archive/NNN-<slug>/`.
- Lifecycle state is inferred from which of these files exist — keep them current as you work.

## Git Workflow
- Work on feature branches: `feat/…`, `fix/…`, `refactor/…`. Never push directly to `main` (protected).
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`; `!` for breaking changes).
- One focused change per branch; open a PR for review.
- Releases: bump `CHANGELOG.md`; the binary version is injected from the git tag at release. Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yaml` — don't tag casually.

## Workflow Commands
- Build:   `go build ./...`
- Vet:     `go vet ./...`
- Tests:   `go test ./...`
- Run CLI: `go run ./cmd/spire <init|update|upgrade>`
- Always run `go vet ./... && go test ./...` before opening a PR.

## Commits & Communication
- Be concise and direct. No apologies.
- If uncertain, ask clarifying questions.
- Use available skills for complex tasks (git operations, testing, refactoring).
