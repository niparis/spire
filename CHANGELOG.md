# Changelog

## 0.5.0

- Add `spire update --force` to overwrite protected project-root projections such as `opencode.json`.
- Show a `--force` hint when upstream protected files change but local files are kept.

## 0.1.0

- Initial Go CLI scaffold for `spire`.
- `spire init` now resolves methodology source automatically from canonical GitHub distribution.
- `spire update` uses `.methodology/.spire-source.json` metadata for deterministic refresh (with canonical fallback).
- No required runtime `SPIRE_METHODOLOGY_SOURCE` environment variable.
