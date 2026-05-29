# SPIRE.md — Global Agent Governance

This is a multi agent implementation of SDD (Specs Driven Development)

1. An architecture agent (Productengineer) produces 

- docs/specs/PRODUCT.md : contains the product vision, user personas and key user stories. it's our product north star. It will contain a list of features with expected outcomes.
- docs/architecture/ARCHITECTURE.md : contains the technology architecture of the project

If those files do not exist you MUST ask the human to select the Productengineer agent to create them

2. Then a Feature Planner subagent (featureplanner) will first detail and then audit the plan for a feature.

The human will select the feature either directly, or you will propose it form the list we have in docs/specs/PRODUCT.md. That feature has a shorthand, we will refer to it as [feature] in this document. 
Each feature needs to have an incremental ID so that we can easily remember in what order we have built the software.

We have the following files:
- docs/specs/[incremental-id]-[feature]/PLAN.md : our approach
- docs/specs/[incremental-id]-[feature]/TASKS.md. : detailed tasks
- docs/specs/[incremental-id]-[feature]/SESSION.md : live memory of the session
- docs/specs/[incremental-id]-[feature]/VERIFICATION_REPORT.md : audit report, post implementation

A feature could create a need for an infra change which should then be recorded at docs/architecture/adr-[incremental-id]-[feature].md

If the user asks you to implement a change or a fix and you dont know where to map it ask. Don't forget to adjust the documentation artifacts if the user asks for hotfixes or post planning changes.

## Skills
- .methodology/skills/spec-auditor/SKILL.md      (always loaded in plan agent)
- .methodology/skills/product-definition.md      (load for product work)
- .methodology/skills/architecture-definition.md (load for architecture work)
- .methodology/skills/verification.md            (load for verification work)

## Subagents (When to Invoke)

- `verifier` (MUST): before PR or merge decision; output `changes/[feature]/VERIFICATION_REPORT.md`; if verdict is NEEDS WORK, stop.
- `reviewer` (MUST): after major module completion or SC-3 failure; output `changes/[feature]/REVIEW_REPORT.md`; unresolved HIGH issues block progress.
- `docs-writer` (SHOULD): when API/behavior/docs-facing changes occur; output doc updates + note in `SESSION.md`.
- `investigator` (SHOULD): when blocked by unknowns or external tradeoffs; output recommendation + sources in `SESSION.md`.

Dispatch rule: pick the first matching MUST; if none, pick highest-value SHOULD if it matches the situation described.
Log every delegation in `changes/[feature]/SESSION.md` (agent, reason, inputs, output, verdict).
