# Architecture: [Product Name]
Version: 0.1 | Stability: EVOLVING | Last updated: YYYY-MM-DD

## 1. System Overview
One paragraph. What this system does technically, and its primary
architectural style (e.g. monolith, microservices, event-driven, serverless).
An agent reading this should immediately understand the shape of the system.

## 2. Component Map
List every major component with a one-line description of its responsibility.
Format:
  [component-name]: [single responsibility] | [tech] | [location in repo]

Example:
  api-server:     HTTP request handling and routing | Go/Chi  | /cmd/api
  auth-service:   Token issuance and validation     | Go      | /internal/auth
  web-client:     User-facing SPA                   | React   | /web
  job-runner:     Async background processing       | Go      | /internal/jobs
  database:       Primary data store                | Postgres | external

## 3. Data Flow
How does data move through the system for the primary use cases?
Not every flow — just the ones an agent must understand to avoid
breaking something when implementing a feature.
Prose or simple ASCII diagram. Avoid tools that require rendering.

## 4. Tech Stack
Format: [layer]: [technology] | [version or constraint] | [why]
Be specific about versions where they constrain implementation choices.

  Language:   Go 1.22+
  Web:        Chi router
  Database:   PostgreSQL 16 | migrations via golang-migrate
  Auth:       JWT | RS256 | 1hr access / 7d refresh
  Infra:      Railway (prod) | Docker Compose (local dev)
  CI/CD:      GitHub Actions

## 5. Key Conventions
Rules an agent must follow that are not obvious from the code itself.
Format: [area]: [rule]

  Error handling:  Always wrap errors with context using fmt.Errorf("...: %w", err)
  Logging:         Structured JSON via slog, no fmt.Println in production paths
  DB access:       Repository pattern only — no raw queries outside /internal/db
  API responses:   Envelope format {data: ..., error: ...} on all endpoints
  Testing:         Table-driven tests, no mocks for DB (use test containers)

## 6. External Dependencies
Services, APIs, and infrastructure this system depends on.
Format: [name]: [purpose] | [criticality: blocking/degrading] | [docs location]

## 7. Local Development
How to get a working environment.
  Prerequisites: [list]
  Start:         [exact command]
  Test:          [exact command]
  Common issues: [known gotchas]

## 8. Known Constraints and Non-Negotiables
Things that cannot be changed without a significant architectural decision.
These are the guard rails for every feature implementation.
If an agent's plan would violate one of these, it must flag it before proceeding.

## 9. Open Architectural Questions
Format: Q1: [question] | Owner: [person] | Due: [date]
Feature specs should not be written against areas covered by open questions here.

## ADR Index
Links to all Architecture Decision Records, newest first.
  adr-003-job-queue-design.md       | 2026-01-15 | ACCEPTED
  adr-002-auth-strategy.md          | 2025-12-01 | ACCEPTED
  adr-001-monorepo-structure.md     | 2025-11-20 | ACCEPTED