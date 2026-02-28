# Spire

`spire` bootstraps and maintains a Spec-Driven Development workflow in your repository.

It operationalizes two reliability layers from the product spec:
- Spec Quality (gate spec clarity before planning/implementation)
- Session Continuity (feature-scoped session state, no context drift)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/niparis/spire/main/scripts/install.sh | bash
```

Supported installer targets:
- macOS Apple Silicon (`darwin/arm64`)
- Windows Intel x64 (`windows/amd64`, manual binary usage)

## Workflow in 60 Seconds

1. Initialize SDD scaffolding in a project:
   ```bash
   spire init
   ```
2. Create the next feature scaffold:
   ```bash
   spire new
   ```
3. Run your audit/planning flow from the generated spec.
4. Implement and verify.
5. Check pipeline state at any time:
   ```bash
   spire status
   ```

## Command Reference

| Command | Behavior |
|---|---|
| `spire init` | Downloads methodology from the canonical Spire GitHub source, syncs it into `.methodology/`, applies root projections via manifest (for example, `AGENTS.md`), and avoids overwriting existing root files |
| `spire update` | Detects local edits in `.methodology/`, prompts in interactive mode, safely aborts in non-interactive mode, refreshes payload using `.methodology/.spire-source.json` (with canonical fallback), and reports protected-file notices |
| `spire new` | Creates the next numbered feature spec (`max+1`) and `changes/<feature>/SESSION.md` from templates |
| `spire status` | Scans feature artifacts and prints inferred lifecycle state (`Spec only` -> `Awaiting PR` -> `Complete`) |

## File Model

- `.methodology/` is the synced methodology payload managed by `spire`.
- `.methodology/project_root/manifest.json` controls which files are projected to repository root.
- `.methodology/.spire-source.json` stores where methodology was fetched from for deterministic updates.
- Canonical session continuity file is always `changes/[feature]/SESSION.md`.

`spire init` and `spire update` do not require `SPIRE_METHODOLOGY_SOURCE`.

## Versioning and Distribution

- Tags follow `vX.Y.Z` and trigger release builds.
- Release assets are published to GitHub Releases for supported targets.
- Installer defaults to latest release unless overridden via installer env vars.

## Troubleshooting

- `Run spire init first.`: initialize the repository before `update`/feature flows.
- Installer succeeded but `spire` not found: add install directory to your `PATH`.
- `spire update` blocked by local edits: stash or revert local `.methodology/` changes first.

## Related Docs

- `specs/PRODUCT.md`
