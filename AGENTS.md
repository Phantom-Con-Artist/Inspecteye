# AGENTS.md

# Inspectye Development Guide

This document defines the engineering rules, architectural boundaries, development workflow, and product principles for building Inspectye.

Inspectye is a local-first developer tool that gives developers and AI agents a continuously updated understanding of what is happening inside a software project, how it changed, what is connected to it, what may be broken, and how the project arrived at its current state.

This file is intended to guide human developers and AI coding agents working on the Inspectye codebase.

---

# 1. Product Identity

## Name

Inspectye

## Tagline

> See inside your codebase.

## Core purpose

Inspectye is a local-first codebase intelligence and observability application.

Its job is not merely to display repository statistics.

It should help answer:

- What is happening inside this project?
- What changed?
- What is connected to the thing that changed?
- What may be broken?
- What could be affected?
- What happened historically?
- What evidence supports the finding?
- What should a developer or AI agent understand before changing the project?

Inspectye should maintain a continuously updated representation of project intelligence.

---

# 2. Core Product Principle

Inspectye should be thought of as an **intelligence layer around a software project**.

```text
Filesystem ──┐
             │
Git ─────────┼──> Inspectye Intelligence ──> Dashboard
             │                              ├── CLI
External ────┘                              └── AI Agents
Resources
```

Inspectye does not replace Git.

Inspectye does not replace the filesystem.

Inspectye builds derived intelligence from them.

---

# 3. Architectural Philosophy

Follow these principles throughout development:

1. Build the smallest useful thing.
2. Keep the architecture modular.
3. Prefer deterministic analysis before AI.
4. Prefer local processing before cloud services.
5. Preserve explainable evidence.
6. Never claim causality from correlation.
7. Never classify uncertain analysis as fact.
8. Detect problems as early as practical.
9. Maintain current project state continuously.
10. Preserve historical context where it is useful.
11. Do not force every feature into a graph.
12. Do not duplicate responsibilities already handled well by Git.
13. Do not scan unrelated parts of the user's machine without a clear reason.
14. Design for incremental analysis from the beginning.
15. Keep the core independent of UI, CLI, and AI interfaces.
16. Prefer simple modular architecture over premature abstraction.
17. Make future feature addition possible without making the current system unnecessarily complicated.

---

# 4. Technology Stack

## Core

Rust

Rust owns:

- application core
- CLI
- filesystem interaction
- repository scanning
- analysis/indexing
- Git integration
- SQLite persistence
- local API
- watcher
- AI/agent interface

## Frontend

TypeScript + React + Vite

TypeScript/React owns:

- dashboard
- visualization
- user interaction
- presentation of project intelligence

The frontend should not become the source of truth for project intelligence.

## Database

SQLite

SQLite is the local persistent intelligence store.

There is no requirement for a PostgreSQL/MySQL server for the local-first architecture.

## Parsing

Tree-sitter is the initial direction for source-code parsing.

Language support should be extensible rather than hardcoded into one giant analyzer.

---

# 5. Core Architecture

The intended high-level architecture is:

```text
                    ┌───────────────┐
                    │   Filesystem  │
                    └───────┬───────┘
                            │
                            ▼
                      ┌───────────┐
                      │  Scanner  │
                      └─────┬─────┘
                            │
                            ▼
                      ┌───────────┐
                      │  Analysis │
                      └─────┬─────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │ SQLite          │
                   │                 │
                   │ Project         │
                   │ Files           │
                   │ Symbols         │
                   │ Relationships   │
                   │ External Refs   │
                   │ Issues          │
                   │ Analysis State  │
                   └───────┬─────────┘
                           │
                 ┌─────────┼─────────┐
                 ▼         ▼         ▼
             Dashboard    CLI     AI Agent
```

Git is an additional source of information:

```text
                    ┌───────────┐
                    │    Git    │
                    └─────┬─────┘
                          │
             history / diff / status / content
                          │
                          ▼
                      Inspectye
```

---

# 6. Modularity Rules

Do not create a giant application module.

The codebase should be organized around responsibilities.

A reasonable starting structure is:

```text
src/
├── main.rs
├── cli/
├── project/
├── scanner/
├── analysis/
├── database/
├── git/
├── watcher/
├── issues/
└── intelligence/
```

The exact structure may evolve.

Do not treat this layout as immutable.

The important requirement is **clear responsibility boundaries**.

---

# 7. Core Independence

The core intelligence layer must not depend on:

- React
- browser APIs
- CLI formatting
- terminal output
- a particular AI agent
- HTTP transport
- presentation-specific data structures

The architecture should allow:

```text
                    Inspectye Core
                         │
             ┌───────────┼───────────┐
             ▼           ▼           ▼
           CLI       Dashboard     AI Agent
```

All interfaces should consume the same underlying project-intelligence model.

---

# 8. Database Boundary

SQLite should not leak throughout the entire codebase.

Avoid arbitrary SQL queries scattered across business logic.

Prefer a boundary such as:

```text
Analysis / Core
      ↓
Repositories / Stores
      ↓
SQLite
```

The core should work with domain concepts rather than raw database rows wherever practical.

Example domain concepts:

```rust
Project
File
Symbol
Relationship
ExternalReference
Issue
AnalysisResult
```

---

# 9. Initial Database Model

The initial conceptual database contains approximately these areas:

```text
projects
files
symbols
relationships
external_references
issues
analysis_runs
file_analysis
```

This is a starting model, not a permanent schema contract.

Do not add large numbers of tables without first establishing the domain concept they represent.

---

# 10. Git Boundary

Git is the source of truth for Git history.

Git owns:

- commits
- branches
- commit metadata
- diffs
- historical file contents
- authors
- Git objects

Inspectye should query Git rather than automatically duplicating all of this information in SQLite.

Useful operations include:

```text
git status
git diff
git log
git show
git blame
```

Inspectye can combine Git results with its own intelligence.

For example:

```text
Git:
    Database.cs changed in commit 101

Inspectye:
    Database.cs has 31 dependents
    4 external references
    2 active warnings
    8 affected relationships
```

Do not build a second Git.

If later performance requirements justify caching specific Git information, add that deliberately.

---

# 11. Filesystem Is the Current-State Source

The filesystem represents the current working tree.

Inspectye must account for:

- modified files
- untracked files
- new files
- deleted files
- renamed/moved files
- ignored files where relevant
- generated files where detectable
- external resources referenced by the project

Git alone is insufficient because important changes may be uncommitted.

---

# 12. Incremental Analysis

Inspectye must be designed for incremental analysis from the beginning.

Do not assume every operation requires a full repository scan.

The desired model is:

```text
File changes
     ↓
Watcher detects change
     ↓
Calculate content hash
     ↓
Compare against stored analysis
     ↓
Changed?
 ┌───┴───┐
 No      Yes
 │        │
Stop   Re-analyze
          │
          ▼
   Update symbols
          │
          ▼
   Update relationships
          │
          ▼
   Update issues
          │
          ▼
   Recalculate affected intelligence
```

The first implementation may perform full scans.

However, APIs and modules should not be designed in a way that makes incremental analysis impossible later.

Prefer concepts such as:

```rust
analyze_file(...)
analyze_files(...)
analyze_project(...)
```

rather than designing everything around a single monolithic:

```rust
analyze_entire_repository(...)
```

---

# 13. Content Hashing

File content hashes are important for determining whether analysis can be reused.

Conceptually:

```text
Current file hash
        ↓
Stored analysis hash
        ↓
Same ──> reuse
Different ──> re-analyze
```

Analysis state should also track relevant parser/analyzer versions.

For example:

```text
file_id
content_hash
parser_version
analyzer_version
analyzed_at
status
error
```

If the parser or analyzer changes incompatibly, affected analysis can be invalidated.

---

# 14. Project Intelligence Model

The database schema must implement the intelligence model rather than accidentally define it.

Important domain concepts include:

## Project

The repository/project being analyzed.

## File

A source, configuration, asset, generated file, or other project resource known to Inspectye.

## Symbol

A meaningful code structure such as:

- class
- interface
- struct
- function
- method
- module
- language-specific equivalent

## Relationship

A meaningful connection between entities.

Examples:

```text
IMPORTS
CALLS
REFERENCES
DEPENDS_ON
INHERITS
IMPLEMENTS
GENERATED_FROM
```

## External Reference

A project reference to a resource outside the repository.

## Issue

A finding requiring attention or representing useful project intelligence.

## Evidence

The concrete information supporting an issue or inference.

---

# 15. Relationships Are Not Automatically Graph UI

Inspectye may internally model relationships in a graph-like way.

The UI should not become graph-oriented by default.

Relationship data may be displayed as:

- tables
- lists
- detail pages
- impact trees
- timelines
- dependency panels
- graph views when useful

The data model should preserve relationships.

The presentation should choose the most useful representation for the question.

---

# 16. External Filesystem Dependencies

Inspectye should detect references such as:

```text
C:/assets/bear_butt.png
C:\assets\bear_butt.png
```

when those paths appear in code/configuration/resources that Inspectye understands.

Inspectye may check:

- whether the referenced path exists
- whether it is inside the repository
- whether it is outside the repository
- whether it is likely machine-specific

Example:

```text
External filesystem dependency

C:/assets/bear_butt.png

Exists: Yes
Inside repository: No

Severity: Warning
```

If the referenced path does not exist:

```text
Referenced external resource does not exist.
```

Do not scan the entire computer.

Only inspect resources actually referenced by the project or explicitly configured.

---

# 17. Severity Model

Use explicit severity concepts.

Initial levels:

```text
INFO
WARNING
ERROR
```

## INFO

Useful intelligence that does not necessarily represent a problem.

Examples:

- frequently modified file
- high dependency count
- historical association
- stale TODO

## WARNING

Potentially problematic but may be legitimate.

Examples:

- external filesystem dependency
- potentially isolated file
- machine-specific reference
- generated-file ambiguity

## ERROR

Strong evidence of actual breakage.

Examples:

- referenced file is missing
- confirmed broken internal reference
- confirmed required dependency is missing
- confirmed required check failed

Do not promote uncertain findings to ERROR merely because they look suspicious.

---

# 18. Evidence and Uncertainty

Inspectye must distinguish:

```text
FACT
INFERENCE
POSSIBILITY
```

Do not report inferred causality as established fact.

Bad:

```text
Database.cs caused the build failure.
```

when the evidence only shows:

```text
Database.cs changed before a build failure.
```

Better:

```text
Database.cs changed before the failure and is connected
to the affected module.

Evidence:
- changed in commit X
- referenced by Y
- failure occurred afterward
```

Inspectye should prefer transparent evidence over opaque scores.

---

# 19. Potentially Isolated Files

Static analysis must not automatically classify a file as:

```text
DEAD
UNUSED
DEFINITELY UNUSED
```

unless the evidence genuinely supports that conclusion.

Prefer:

```text
Potentially isolated file
```

and show why.

Possible reasons:

- no detected references
- no imports
- no reverse dependencies
- not referenced by known entry points

Unknown relationships should remain unknown.

---

# 20. Historical Intelligence

Git should be used for historical source-of-truth operations.

Inspectye can eventually support questions such as:

```text
What did Auth.cs look like at commit 88?
```

or:

```text
How did Authenticate() change between commits 88 and 101?
```

Important:

Line numbers are not stable identities across history.

This:

```text
commit 88
lines 3-54
```

is a valid request for an exact historical region.

But identifying the same logical code across commits should eventually use:

- Git diffs
- symbols
- functions
- classes
- structural identity

Do not assume line numbers alone identify logical code.

---

# 21. CLI Design

The CLI is both a developer interface and a potential machine-readable interface for AI agents.

Initial workflow:

```bash
cd repo
inspecteye init
inspecteye scan
inspecteye status
```

Dashboard launcher:

```bash
inspecteye inspect
```

Potential future commands:

```bash
inspecteye check
inspecteye doctor
inspecteye scan
```

Machine-readable mode may support commands such as:

```bash
inspecteye --json status
inspecteye --json file Auth.cs
inspecteye --json impact Database.cs
inspecteye --json failures
```

The exact CLI syntax may evolve.

Do not couple internal APIs directly to human-readable terminal output.

---

# 22. AI Agent Interface

AI agents should consume Inspectye's derived intelligence instead of repeatedly reading the entire repository.

Possible future interfaces:

- CLI
- local REST API
- MCP
- structured JSON

Conceptually:

```text
AI Agent
    ↓
Inspectye interface
    ↓
Project Intelligence
    ↓
Files / Symbols / Relationships / Issues / History
```

Inspectye is the intelligence layer.

The AI agent remains responsible for reasoning and taking actions.

Do not turn Inspectye into a generic AI assistant.

---

# 23. Dashboard

The dashboard is a presentation layer over Inspectye's core intelligence.

It should consume the same model exposed through other interfaces.

The dashboard should eventually help developers inspect:

- project overview
- files
- dependencies
- symbols
- changes
- warnings/errors
- historical activity
- impact
- external references
- analysis state

Do not implement intelligence separately in React.

---

# 24. Architecture for Feature Addition

When adding a new feature, ask:

1. What new domain concept does this introduce?
2. Does it belong in the core intelligence model?
3. Is it deterministic or inferred?
4. What evidence supports it?
5. What existing component should own it?
6. Can it operate incrementally?
7. Does it require a new persistence concept?
8. Can the CLI, dashboard, and AI interface all consume the same result?
9. Does it duplicate functionality already provided by Git?
10. Does it make an existing boundary harder to change?

A feature should normally be added as a new capability rather than by expanding an unrelated module.

---

# 25. Traits and Abstractions

Use traits when there is a meaningful boundary or realistic replacement.

Good candidates include:

```text
SourceControl
Parser
Analyzer
FileSystem
Watcher
Persistence
```

Do not create traits merely to make code look "architectural."

Avoid unnecessary abstraction.

The goal is:

> Modular enough to change. Simple enough to understand.

---

# 26. Error Handling

Rust errors should preserve useful context.

Avoid silently swallowing errors.

Prefer errors that answer:

- What operation failed?
- What resource was involved?
- Why did it fail?
- Can the operation continue?
- Is the failure expected or unexpected?

For analysis of individual files, one broken file should not necessarily prevent the entire repository from being scanned.

Distinguish:

```text
project-level failure
file-level analysis failure
recoverable warning
```

---

# 27. Logging

Logging should help developers understand what Inspectye is doing without becoming noise.

Useful events include:

- scan started
- scan completed
- file changed
- file analyzed
- analysis invalidated
- Git operation failed
- database migration occurred
- issue detected

Avoid logging every internal function call.

---

# 28. Database Migrations

The SQLite schema will evolve.

Do not assume the first schema is permanent.

Use an explicit migration strategy once the database layer is implemented.

Every schema change should be:

- versioned
- reproducible
- safe to apply
- understandable

Development databases may be recreated freely during early development, but the architecture should leave room for real migrations.

---

# 29. Testing Strategy

Test the intelligence engine independently from the CLI and UI.

Priorities:

## Unit tests

For:

- hashing
- path handling
- parsing
- symbol extraction
- relationship detection
- severity logic
- issue creation
- Git operations
- database repositories

## Integration tests

For workflows such as:

```text
init
→ scan
→ database populated
→ modify file
→ rescan
→ only necessary analysis updated
```

## Fixture repositories

Maintain small artificial repositories for testing:

```text
simple-project
dependency-project
broken-reference-project
external-reference-project
git-history-project
```

These should be deterministic and easy to inspect.

---

# 30. First Development Milestone

The first milestone should establish a working, extensible foundation.

Implement:

```bash
inspecteye init
inspecteye scan
inspecteye status
```

The flow should be:

```text
inspecteye init
        ↓
create .inspectye/
        ↓
initialize SQLite
        ↓
register project
        ↓
inspecteye scan
        ↓
discover files
        ↓
calculate metadata/hashes
        ↓
persist file information
        ↓
inspecteye status
        ↓
show current project state
```

The first milestone does NOT need:

- full dependency analysis
- complete symbol intelligence
- AI
- dashboard
- runtime analysis
- sophisticated failure propagation

Build the foundation first.

---

# 31. Initial Project Layout

A reasonable starting point:

```text
Inspectye/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs
│   ├── cli/
│   ├── project/
│   ├── scanner/
│   ├── analysis/
│   ├── database/
│   ├── git/
│   ├── watcher/
│   ├── issues/
│   └── intelligence/
├── tests/
└── .gitignore
```

This structure is allowed to evolve.

Do not reorganize code merely for aesthetics.

---

# 32. Development Sequence

Build in vertical slices.

Recommended sequence:

## Phase 1 — Foundation

- Cargo project
- CLI skeleton
- project discovery
- `.inspectye/`
- SQLite initialization
- project record

## Phase 2 — File Index

- filesystem scanner
- file metadata
- language detection
- line counts
- content hashes
- Git-tracked state

## Phase 3 — Incremental State

- analysis state
- changed-file detection
- rescan behavior
- basic watcher

## Phase 4 — Code Intelligence

- parser integration
- symbols
- references
- relationships

## Phase 5 — Issues

- external references
- missing references
- warnings/errors
- evidence

## Phase 6 — Git Intelligence

- status
- diffs
- history
- historical file retrieval

## Phase 7 — Interfaces

- local API
- JSON CLI
- dashboard
- AI-agent interface

This sequence is flexible. Do not follow it blindly if implementation evidence suggests a better order.

---

# 33. What Not to Build Yet

Do not prematurely build:

- cloud backend
- PostgreSQL
- authentication
- multi-user infrastructure
- microservices
- distributed workers
- plugin marketplace
- huge graph visualization system
- generic AI chatbot
- elaborate scoring system
- full runtime observability platform

Inspectye is initially a local developer intelligence tool.

Keep it that way until there is a concrete reason to expand the boundary.

---

# 34. Code Review Rules for AI Agents

Before changing code:

1. Read the relevant existing code.
2. Understand the current boundary.
3. Avoid unnecessary rewrites.
4. Reuse existing domain concepts where appropriate.
5. Check whether the change affects persistence.
6. Check whether incremental analysis remains possible.
7. Check whether CLI/API/UI coupling has been introduced.
8. Add tests for behavior that matters.
9. Keep the change focused.

Do not rewrite large portions of the project simply because another architecture looks cleaner.

---

# 35. AI Agent Working Style

AI coding agents should behave as engineering contributors, not autonomous architects changing the entire project.

When uncertain:

- inspect the existing implementation
- identify the smallest compatible change
- preserve existing behavior
- explain architectural consequences
- ask before making a major irreversible decision

Do not invent requirements.

Do not silently introduce cloud services.

Do not silently introduce new frameworks.

Do not silently change the project's core architecture.

---

# 36. Feature Evaluation

Every proposed feature should answer at least one useful developer question.

Examples:

```text
What changed?
What is connected?
What is broken?
What may be affected?
Why might it be broken?
What needs attention?
What happened historically?
What should an AI agent know?
```

If a feature does not improve a meaningful answer, question whether it belongs in Inspectye.

---

# 37. Product Boundaries

Inspectye is NOT:

- a project-management application
- an issue tracker
- a replacement for Git
- a cloud hosting platform
- a generic AI assistant
- a generic filesystem search tool
- a graph visualization product

Inspectye may integrate with these systems or use their information.

Its core purpose remains **project intelligence**.

---

# 38. North Star

The North Star for Inspectye is:

> Give developers and AI agents a continuously updated understanding of what is happening inside a software project, how it changed, what is connected to it, what may be broken, and how the project arrived at its current state.

When making architectural decisions, prefer the design that moves Inspectye toward this goal while keeping the implementation understandable and extensible.

---

# 39. Final Engineering Rule

When choosing between two designs, prefer:

```text
simple
+
modular
+
observable
+
evidence-based
+
incremental
+
replaceable at real boundaries
```

over:

```text
clever
+
prematurely abstract
+
highly coupled
+
opaque
+
difficult to change
```

Inspectye should grow by **adding capabilities to a stable core**, not by repeatedly rebuilding its foundation.
