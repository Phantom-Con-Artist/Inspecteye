# Inspectye --- Product & Architecture Decisions

## Status

This document records the decisions made during the initial design
discussion for Inspectye.

It should be treated as a working source of truth for the current
product direction. It does not replace future technical design
documents; unresolved implementation details should remain explicitly
unresolved until we decide them.

------------------------------------------------------------------------

# 1. Product Identity

**Project name:** Inspectye

**Tagline:** *See inside your codebase.*

Inspectye is a **local-first codebase intelligence and observability
layer for developers**.

Its purpose is not simply to display repository statistics or Git
history. It continuously builds and maintains an understandable model of
what is happening inside a software project.

The core developer questions are:

-   What is happening in my project?
-   What changed?
-   What is connected?
-   What is broken?
-   What might be affected?
-   Why might something be broken?
-   What happened historically?
-   What does an AI agent need to know about the project?

Inspectye should help developers understand a codebase without
repeatedly having to manually inspect the entire repository.

------------------------------------------------------------------------

# 2. Core Product Concept

The central concept is:

> **Inspectye is a local watcher and intelligence layer that
> continuously maintains a structured, historical understanding of a
> software project.**

It tracks relevant project state, changes, relationships, failures,
history, and other useful signals.

The goal is for the developer to be able to open Inspectye and
immediately understand the current state of the project.

The goal for AI agents is similar: an agent should be able to query
Inspectye for focused project intelligence rather than repeatedly
ingesting thousands of files to reconstruct the state of the repository.

------------------------------------------------------------------------

# 3. Local-First

Inspectye is primarily a **local application**.

The core system should run on the developer's machine.

Source code should not need to be uploaded to a cloud service.

Core local components include:

-   Repository scanning
-   Code analysis
-   Project indexing
-   Project intelligence
-   Historical information
-   Local database
-   Local API
-   CLI
-   Dashboard

Cloud services may be added later, but they must remain optional and
clearly separated from the local core.

Privacy is a major product advantage.

------------------------------------------------------------------------

# 4. Developer Workflow

The primary workflow should be extremely simple.

From a new or existing repository:

``` bash
inspecteye init
```

Inspectye initializes itself for the repository.

After initialization:

``` bash
inspect
```

opens the local Inspectye dashboard.

The expected conceptual workflow is:

``` text
Repository
    |
    v
inspecteye init
    |
    v
Inspectye begins understanding the project
    |
    v
Local project index
    |
    v
inspect
    |
    v
Dashboard
```

The CLI should also eventually expose useful machine-readable operations
for developers and AI agents.

Examples:

``` bash
inspecteye status
inspecteye scan
inspecteye check

inspect --json status
inspect --json file src/AuthService.ts
inspect --json impact src/Database.ts
inspect --json failures
```

Exact command names can be refined during implementation.

------------------------------------------------------------------------

# 5. Inspectye Is a Local Watcher

Inspectye should not require the developer to continuously remember to
run analysis.

It should act as a **local watcher**.

Conceptually:

``` text
Developer changes code
        |
        v
Inspectye detects change
        |
        v
Incremental analysis
        |
        v
Project state updated
        |
        v
Warnings / intelligence updated
```

Inspectye should maintain a dynamically updated representation of the
project's current state.

It should avoid rescanning the entire repository for every small change
when incremental analysis can safely update only the affected portions.

------------------------------------------------------------------------

# 6. Current State and Historical State

Inspectye should understand two major dimensions of a project.

## Current State

What does the project look like right now?

This can include:

-   Current files
-   Current symbols
-   Current dependencies
-   Working-tree changes
-   Uncommitted changes
-   Current warnings
-   Current build/test information
-   Current external references
-   Current project statistics

## Historical State

What did the project look like at a previous point in time?

This can include:

-   Git commits
-   Historical file contents
-   Historical symbols
-   Historical relationships
-   Historical changes
-   Historical failures
-   Historical project state

The system should make it possible to investigate how the project
arrived at its current state.

Conceptually:

``` text
Historical State
      |
      | compare
      v
Current State
```

------------------------------------------------------------------------

# 7. Inspectye Is Not Just a Graph Application

Inspectye may represent relationships internally, but **the entire
product does not need to be graph-oriented**.

Graphs are one useful representation for questions involving
relationships and impact.

Different questions should use different UI representations.

Examples:

  Question                        Useful representation
  ------------------------------- ---------------------------
  What is the project doing?      Overview
  What changed?                   Timeline / change list
  What is this file?              File detail
  What failed?                    Failure investigation
  What depends on this file?      Relationship view / graph
  What could be affected?         Impact/path view
  What TODOs exist?               List
  What happened historically?     Timeline / diff
  What should an AI agent know?   Structured JSON/text

The underlying data model should capture relationships without forcing
every user interaction into a graph visualization.

------------------------------------------------------------------------

# 8. Project vs Repository

A repository is the core unit Inspectye initializes around.

However, a real software project can depend on resources outside the
repository.

Example:

``` text
D:\Very Important File\
    src\
        renderer.ts

C:\assets\
    bear_butt.png
```

If the code contains:

``` text
C:/assets/bear_butt.png
```

the asset is outside the repository but may still be part of the
software's real operating environment.

Inspectye should therefore distinguish between:

-   Repository resources
-   External resources/dependencies

Inspectye must **not automatically scan the entire filesystem**.

External resources should be discovered from relevant code/configuration
or explicitly configured by the developer.

------------------------------------------------------------------------

# 9. External Filesystem References

Absolute filesystem references are an important example of a problem
Inspectye can detect before compilation or committing.

Example:

``` text
C:/assets/bear_butt.png
```

Inspectye can detect:

``` text
Absolute path
    |
    v
Does the file exist?
    |
    v
Is it inside the repository?
```

Possible result:

``` text
WARNING

External filesystem dependency

src/renderer.ts references:

C:/assets/bear_butt.png

The file exists on this machine but is outside
the repository and may not exist on another machine.
```

If the file does not exist:

``` text
ERROR

Broken external reference

src/renderer.ts references:

C:/assets/bear_butt.png

The referenced file does not exist.
```

Inspectye should distinguish between:

-   A reference that is valid but external
-   A reference that is broken
-   A reference that is uncertain

An external reference should **not automatically be considered an
error**.

------------------------------------------------------------------------

# 10. Preflight / Early Detection

A major product principle is:

> **Detect likely project breakage as early as possible.**

Inspectye should not only explain failures after compilation or testing.

It should detect relevant problems while the developer is working,
before they become larger problems.

A future preflight command can look conceptually like:

``` bash
inspecteye check
```

Possible output:

``` text
Inspectye Preflight

✓ Repository structure
✓ Internal references
✓ Dependency references
⚠ External filesystem references
✓ Test relationships

1 warning
```

Eventually, an appropriate strict/CI mode could make certain confirmed
problems fail automation.

However, Inspectye should not become an unnecessarily aggressive
gatekeeper.

Warnings should be distinguishable from confirmed errors.

------------------------------------------------------------------------

# 11. Severity Model

Inspectye should distinguish evidence levels.

## Informational

Useful project intelligence.

Examples:

-   Frequently modified file
-   Historical failure association
-   High number of dependents
-   Stale TODO

## Warning

Potentially problematic, but legitimate use is possible.

Examples:

-   Absolute external path
-   External dependency
-   Potentially isolated file
-   Generated-file ambiguity
-   Machine-specific reference

## Error

Inspectye has strong evidence that something is broken.

Examples:

-   Referenced file does not exist
-   Broken internal reference
-   Missing required dependency
-   Confirmed failed required check

Inspectye should avoid presenting inferred problems as confirmed facts.

------------------------------------------------------------------------

# 12. Continuous Codebase Intelligence

Inspectye should understand more than the current file tree.

Relevant project intelligence includes:

## Codebase structure

-   Files
-   Directories
-   Languages
-   Classes
-   Functions/methods
-   Variables
-   Symbols
-   Imports
-   References
-   Calls

## Dependencies

-   Direct dependencies
-   Reverse dependencies
-   Transitive relationships where reliable
-   Module relationships
-   Circular relationships
-   Dependency hotspots
-   Isolated candidates

## Git

-   Commits
-   File history
-   Recent changes
-   Frequently modified files
-   Contributors
-   Change hotspots
-   Branch information where useful

## Development state

-   Uncommitted changes
-   TODOs
-   FIXMEs
-   Pending work signals
-   Build state
-   Test state
-   Failure history

## External resources

-   External filesystem references
-   Other relevant external dependencies
-   Environment/configuration references where technically detectable

## Historical intelligence

-   Changes over time
-   Files associated with failures
-   Historical relationships
-   Changes preceding failures
-   Historical project states

The product should derive useful context from these signals rather than
merely displaying raw counts.

------------------------------------------------------------------------

# 13. The Project Intelligence Model

The most important part of Inspectye is not the dashboard.

It is not the CLI.

It is not the graph visualization.

It is the **project intelligence model** underneath them.

Conceptually:

``` text
                 Project Intelligence
                         |
        +----------------+----------------+
        |                |                |
      Files             Git           Failures
        |                |                |
     Symbols          Changes         Tests
        |                |                |
        +----------------+----------------+
                         |
                  Relationships
                         |
        +----------------+----------------+
        |                |                |
    Dashboard          CLI           AI Agent
```

The dashboard, CLI, and AI interface should consume the same underlying
intelligence.

------------------------------------------------------------------------

# 14. Typed Relationships

Inspectye should understand that different relationships mean different
things.

Examples:

``` text
renderer.ts
    |
    +-- imports --> Renderer.tsx
    |
    +-- references --> C:/assets/bear_butt.png
    |
    +-- changed by --> commit abc123
    |
    +-- potentially affects --> APIController.ts
```

These should not be collapsed into a generic relationship type.

The system should preserve the meaning and evidence behind
relationships.

------------------------------------------------------------------------

# 15. Historical Code and Exact Previous State

Inspectye should support historical investigation at the file and
eventually code-region/symbol level.

Example:

``` text
Commit 88
   |
   v
Commit 89
   |
   ...
   |
Commit 101  <-- bug introduced
   |
   v
Current state
```

A developer may want to investigate:

> Replace a specific region of a file with the exact state that region
> had at commit 88.

This is technically feasible because Git stores historical file states.

For example:

``` text
Current file
L1
L2
L3  <-- problematic region
...
L54
L55
```

Inspectye can retrieve the historical file at commit 88 and obtain the
corresponding historical lines.

It can then help produce a patch representing:

``` diff
- current region
+ historical region from commit 88
```

However, an important technical principle is:

> **Line numbers are not stable identities.**

If code moves between commits, "the same lines" and "the same logical
code" are different concepts.

Therefore, future historical intelligence should consider:

-   Git diffs
-   Symbols
-   Functions
-   Classes
-   Structural identity
-   Historical code regions

The goal is eventually to answer questions such as:

> How did `authenticate()` change between commit 88 and commit 101?

rather than treating source code only as numbered text.

------------------------------------------------------------------------

# 16. Incremental Analysis

Inspectye should avoid unnecessarily analyzing thousands of files after
every small change.

Example:

``` text
Developer edits:

src/auth/AuthService.ts
```

Inspectye should determine what needs to be recalculated.

Conceptually:

``` text
AuthService.ts changed
        |
        v
Re-analyze affected source
        |
        +--> Symbols changed?
        |
        +--> Dependencies changed?
        |
        +--> References changed?
        |
        +--> Dependents potentially affected?
        |
        +--> Warnings changed?
        |
        +--> Historical/current state updated
```

The exact invalidation and dependency-propagation strategy is an
implementation concern to be designed later.

------------------------------------------------------------------------

# 17. AI Agent Interface

AI agents should be able to use Inspectye through the CLI and eventually
other interfaces such as MCP.

The agent should not need to ingest the entire repository simply to
understand project state.

Example:

``` bash
inspect --json status
```

``` bash
inspect --json file src/AuthService.ts
```

``` bash
inspect --json impact src/Database.ts
```

``` bash
inspect --json failures
```

The output should be structured and concise.

Inspectye provides project intelligence.

The AI agent remains responsible for reasoning and performing coding
tasks.

Inspectye should **not become a general AI coding agent**.

------------------------------------------------------------------------

# 18. Rust + TypeScript Stack

The agreed initial stack is:

## Rust

Rust owns the core application:

-   CLI
-   Repository scanning
-   Filesystem I/O
-   Incremental indexing
-   Code analysis
-   Git integration
-   Build/test integration
-   Project intelligence
-   SQLite integration
-   Local API
-   Machine-readable agent interface

## TypeScript

TypeScript owns the dashboard/frontend.

Likely frontend direction:

-   TypeScript
-   React
-   Vite

The frontend should consume the Rust intelligence/API rather than
independently understanding the repository.

The basic architectural boundary is:

``` text
Rust = understands the project

TypeScript = presents the project
```

------------------------------------------------------------------------

# 19. SQLite

Inspectye should use **SQLite** as the initial local project
database/index.

There is no need for a PostgreSQL/MySQL-style server.

Conceptually:

``` text
Repository
    |
    v
Inspectye
    |
    v
.inspecteye/
    |
    +-- local index/database
```

SQLite is storage.

The Inspectye intelligence model is the conceptual layer above it.

Relationships can be stored relationally and represented as graphs only
when useful.

The SQLite CLI does not need to be a runtime dependency of the
application.

------------------------------------------------------------------------

# 20. Parsing

Tree-sitter is the initial parsing direction.

The architecture should separate:

``` text
Source code
    |
    v
Language parser
    |
    v
Inspectye normalized project model
```

Inspectye should not make its entire product model identical to any
particular parser's AST representation.

Language-specific parsing should feed a normalized Inspectye
representation.

The initial language set should be deliberately limited rather than
attempting to support every language immediately.

------------------------------------------------------------------------

# 21. What Inspectye Is Not

Inspectye is not:

-   A Git replacement
-   A GitHub replacement
-   A Jira replacement
-   A project-management platform
-   A generic code editor
-   A cloud repository
-   A generic AI coding assistant
-   A general-purpose observability platform
-   An autonomous coding agent

Inspectye is:

> **A local codebase intelligence and observability layer.**

------------------------------------------------------------------------

# 22. V1 Direction

V1 should remain disciplined.

The first meaningful version should focus on:

1.  Repository initialization
2.  Repository scanning
3.  Code/file structure analysis
4.  Project intelligence model
5.  Incremental local indexing
6.  Git history/activity
7.  File-level intelligence
8.  Dependency/reference intelligence
9.  Basic preflight/error detection
10. Local dashboard
11. Local API
12. CLI access for developers and AI agents

Later capabilities can include:

-   Deeper failure propagation
-   More sophisticated historical code identity
-   Runtime intelligence
-   More languages
-   IDE integrations
-   MCP
-   More external-resource analysis
-   Optional cloud functionality

------------------------------------------------------------------------

# 23. Product Principles

Inspectye should follow these principles:

### Build the smallest useful thing.

### Prefer deterministic analysis before AI.

### Prefer local computation before cloud services.

### Prefer explainable evidence before opaque scores.

### Prefer relationships and context over isolated statistics.

### Never claim causality when only correlation is known.

### Never classify uncertain analysis as fact.

### Detect problems as early as practical.

### Maintain current state continuously.

### Preserve historical state and context.

### Do not force graph visualizations where another representation is better.

### Do not scan unrelated parts of the user's filesystem without explicit reason/permission.

### Every feature should answer a useful developer question.

------------------------------------------------------------------------

# 24. North Star

The current North Star is:

> **Give developers and AI agents a continuously updated understanding
> of what is happening inside a software project, how it changed, what
> is connected to it, what may be broken, and how the project arrived at
> its current state.**

The desired developer experience is:

``` text
Developer changes code
        |
        v
Inspectye notices
        |
        v
Inspectye updates its understanding
        |
        v
Potential problem detected
        |
        v
Developer gets useful context
        |
        v
Developer fixes or investigates
        |
        v
Inspectye updates again
```

The desired AI-agent experience is:

``` text
Agent receives task
        |
        v
Queries Inspectye
        |
        v
Gets focused project context
        |
        v
Inspects only relevant code
        |
        v
Makes change
        |
        v
Queries Inspectye again
        |
        v
Understands the resulting project state
```

Inspectye's purpose is therefore not simply to "analyze a repository."

It is to maintain a **living, local, historical understanding of the
software project** so that both developers and AI agents can reason
about the project without repeatedly reconstructing that understanding
from scratch.

------------------------------------------------------------------------

# 38. Scan vs. Deep-Scan Architecture

To keep normal scanning fast and predictable while supporting rich code analysis, Inspectye decouples lightweight resource discovery from semantic deep analysis.

```text
Filesystem
    │
    ▼
inspecteye scan (Fast / Default)
    ├── Discovers all repository resources
    ├── Classifies resources (source, config, documentation, asset, dependency, generated, binary, unknown)
    ├── Calculates content hashes & collects lightweight metadata
    ├── Detects Git tracked/untracked state
    ├── Checks .inspecteyeignore rules (marks is_ignored in SQLite)
    └── Tracks lifecycle changes (New, Changed, Unchanged, Restored, Restored and changed, Removed)
    │
    ▼
inspecteye deep-scan (Semantic / In-depth)
    ├── Reads indexed resources from SQLite
    ├── Filters eligible resources: kind == "source" && exists_on_disk && !is_ignored
    ├── Skips up-to-date resources (content_hash unchanged & completed in file_analysis)
    ├── Invokes modular DeepAnalyzer abstraction
    └── Updates file_analysis records and files.last_analyzed_at
```

### Resource Classification

Resource classification is conservative and deterministic:
- `source`: Source code eligible for deep analysis (`.rs`, `.ts`, `.js`, `.py`, `.go`, `.c`, etc.).
- `config`: Configuration and project definitions (`.json`, `.toml`, `.yaml`, `.ini`, `.inspecteyeignore`, etc.).
- `documentation`: Human-readable text and documentation (`.md`, `.txt`, `.rst`, etc.).
- `asset`: Static binary media and image assets (`.png`, `.svg`, `.ico`, `.mp4`, etc.).
- `dependency`: Vendored / third-party dependencies (`node_modules/`, `vendor/`, etc.).
- `generated`: Build outputs, distribution bundles, or compiler artifacts (`dist/`, `target/`, `build/`, `out/`, etc.).
- `binary`: Compiled binaries or system libraries (`.exe`, `.dll`, `.so`, `.bin`, etc.).
- `unknown`: Resources not matched by known extension or structural directory patterns.

### `.inspecteyeignore` Semantics

`.inspecteyeignore` controls **analysis depth**, not visibility.

- Files matching `.inspecteyeignore` are **discovered and indexed** in SQLite with `is_ignored = 1`.
- They are excluded from **deep code analysis**.
- Structural project boundaries (`.git/`, `.inspectye/`) remain excluded from discovery altogether.

------------------------------------------------------------------------

# 39. Code State Transitions & File-Specific Line-Level Changes

When files change, Inspectye tracks what changed inside that specific file rather than only reporting hash mismatches:

```text
File State A  ───>  File State B  ───>  State Transition
```

### File-Specific CLI (`inspecteye changes <path>`)
- To prevent noise across large codebases, changes are requested strictly per file:
  `inspecteye changes <path>` (e.g. `inspecteye changes src/main.rs`).
- Inspectye resolves both disk/working-tree modifications (uncommitted changes) and recorded historical transitions.

### Line-Level Deterministic Diffing
- Diffs are computed deterministically preserving line numbers for both previous and current file states.
- Hunks report added lines, removed lines, and line offsets without requiring external Git diff commands.
- Lightweight textual snapshots are preserved in `file_states` (capped at 2MB per text file).

### Historical Restoration Semantics
- If a file transitions `A -> B -> C` where `C.content_hash == A.content_hash`, Inspectye detects that state C is an exact restoration of earlier state A.
- Transition is explicitly classified as `Restored`, noting the historical state ID.
- Unchanged scans impose zero additional state insertion overhead on the SQLite database.
