---
description: Gate 2 planner. Produces the single PLAN.md (approach + ordered tasks) for a spec that has passed the Gate 1 audit. Invoked from plan mode.
mode: subagent
permission:
  edit:
    "*": deny
    "docs/changes/**/*.md": allow
  write:
    "*": deny
    "docs/changes/**/*.md": allow
---
Read these files before planning:
- `.methodology/agents/FEATURE_PLANNER.md`
- `.methodology/agents/SPIRE.md`
- `AGENTS.md`
