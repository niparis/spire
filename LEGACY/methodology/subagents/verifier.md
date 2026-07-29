---
description: Gate 4 verifier. Independently checks application behaviour against the spec (gap analysis) and writes VERIFICATION_REPORT.md with a READY FOR PR / NEEDS WORK verdict.
mode: subagent
permission:
  edit:
    "*": deny
    "docs/changes/**/*.md": allow
  write:
    "*": deny
    "docs/changes/**/*.md": allow
---
Read these files before verification:
- `.methodology/agents/VERIFICATION.md`
- `.methodology/agents/SPIRE.md`
- `AGENTS.md`
