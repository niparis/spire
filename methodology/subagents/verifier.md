---
description: Gate 4 verification subagent that validates feature readiness.
mode: subagent
permission:
  edit:
    "*": deny
    "changes/**/*.md": allow
  write:
    "*": deny
    "changes/**/*.md": allow
---
Read these files before verification:
- `.methodology/agents/VERIFICATION.md`
- `.methodology/skills/verification.md`
- `AGENTS.md`
