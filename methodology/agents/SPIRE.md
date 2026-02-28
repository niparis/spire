# SPIRE.md — Global Agent Governance

This is a multi agent implementation of SDD (Specs Driven Development)

1. An architecture agent (Productengineer) produces 
- specs/PRODUCT.md : contains the product vision, user personas and key user stories. it's our product north star. It will contain a list of features with expected outcomes.
- architecture/ARCHITECTURE.md : contains the technology architecture of the project

If those files do not exist you MUST ask the human to select the Productengineer agent to create them

2. Then a Feature Planner subagent (featureplanner) will first detail and then audit the plan for a feature.
The human will select the feature either directly, or you will propose it form the list we have in specs/PRODUCT.md. That feature has a shorthand, we will refer to it as [feature] in this document. We have the following files:
- specs/[feature]/PLAN.md : our approach
- specs/[feature]/TASKS.md. : detailed tasks
- specs/[feature]/SESSION.md : live memory of the session
- specs/[feature]/VERIFICATION_REPORT.md : audit report, post implementation

A feature could create a need for an infra change which should then be recorded at architecture/adr-[incremental_id]-[feature].md




## Skills
- skills/spec-auditor/SKILL.md      (always loaded in plan agent)
- skills/product-definition.md      (load for product work)
- skills/architecture-definition.md (load for architecture work)
- skills/verification.md            (load for verification work)

## Subagents (When to Invoke)

- `verifier` (MUST): before PR or merge decision; output `changes/[feature]/VERIFICATION_REPORT.md`; if verdict is NEEDS WORK, stop.
- `reviewer` (MUST): after major module completion or SC-3 failure; output `changes/[feature]/REVIEW_REPORT.md`; unresolved HIGH issues block progress.
- `docs-writer` (SHOULD): when API/behavior/docs-facing changes occur; output doc updates + note in `SESSION.md`.
- `investigator` (SHOULD): when blocked by unknowns or external tradeoffs; output recommendation + sources in `SESSION.md`.

Dispatch rule: pick the first matching MUST; if none, pick highest-value SHOULD if it matches the situation described.
Log every delegation in `changes/[feature]/SESSION.md` (agent, reason, inputs, output, verdict).
