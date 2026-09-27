# Inspectye Database Schema

## Purpose

Inspectye uses SQLite as its **persistent project-intelligence store**.

The database is not a replacement for Git and is not the source of truth for the repository's code or Git history.

Instead:

- **Filesystem** is the source of truth for the current working tree.
- **Git** is the source of truth for Git history, commits, diffs, branches, and historical file states.
- **SQLite** stores Inspectye's derived intelligence so it can understand, track, and update the project efficiently.

The core idea is:

```text
Filesystem ──┐
             ├──> Inspectye Analyzer ──> SQLite Intelligence Store
Git ─────────┘                                      │
                                                    ├── Dashboard
                                                    ├── CLI
                                                    └── AI Agents
```

---

# Why Inspectye Needs a Database

Git already tracks changes, so Inspectye should **not duplicate Git's job**.

Git can answer:

- What changed?
- When did it change?
- Who changed it?
- What did a commit contain?
- What did a file look like at a particular commit?
- What is the diff between two commits?

Inspectye needs to answer a different class of questions:

- What files exist right now?
- What symbols are inside them?
- What files depend on this file?
- Which functions/classes reference this symbol?
- What external filesystem resources does the code reference?
- Which warnings currently apply?
- What analysis has already been performed?
- What needs to be re-analyzed after a change?
- What parts of the project may be affected by a modification?

These are **derived project-intelligence questions**.

SQLite gives Inspectye persistent, queryable local state instead of requiring the entire repository to be re-analyzed every time.

---

# Database Responsibilities

The initial database should store:

1. Project information
2. File information
3. Symbols
4. Relationships
5. External filesystem references
6. Detected issues
7. Analysis runs
8. Per-file analysis state

The first version should remain intentionally small.

Do not create dozens of tables before the underlying intelligence model is defined.

---

# 1. `projects`

Represents the repository/project being indexed by Inspectye.

```sql
projects
--------
id
root_path
name
created_at
updated_at
```

Example:

```text
1 | D:/MyProject | MyProject
```

A single Inspectye database is associated with a project/repository.

---

# 2. `files`

The central representation of files currently known to Inspectye.

```sql
files
-----
id
project_id
path
kind
language
size_bytes
line_count
content_hash
git_tracked
exists_on_disk
is_ignored
last_seen_at
last_analyzed_at
```

Resource kinds currently classified:
- `source`: Code files eligible for deep parsing/analysis (`.rs`, `.ts`, `.js`, `.py`, `.go`, etc.)
- `config`: Configuration/manifests (`.json`, `.toml`, `.yaml`, `.ini`, `.inspecteyeignore`, etc.)
- `documentation`: Human documentation (`.md`, `.txt`, `.rst`, `.adoc`, etc.)
- `asset`: Static media/assets (`.png`, `.svg`, `.ico`, `.mp4`, etc.)
- `dependency`: Vendored packages/modules (`node_modules/`, `vendor/`, etc.)
- `generated`: Build outputs/artifacts (`dist/`, `target/`, `build/`, `out/`, etc.)
- `binary`: Compiled binaries / executables (`.exe`, `.dll`, `.so`, `.bin`, etc.)
- `unknown`: Unrecognized files without known patterns

### `is_ignored`

Boolean flag set if the file path matches patterns in `.inspecteyeignore`.
*Important:* `.inspecteyeignore` controls **analysis depth**, not discovery. Ignored files are still discovered and indexed in SQLite with `is_ignored = 1`, but are skipped from deep analysis.


Example:

```text
42 | src/Auth.cs | csharp | 18342 | 512 | abc123...
```

## Important fields

### `content_hash`

Used to determine whether the contents of a file have changed since the last analysis.

Conceptually:

```text
Current file hash
       ↓
Compare with analyzed hash
       ↓
Same ──> reuse existing analysis
Different ──> re-analyze
```

This is important for large repositories.

If a repository contains 3,000 files and only one changes, Inspectye should not blindly re-analyze all 3,000 files.

---

# 3. `symbols`

Represents code structures such as:

- classes
- interfaces
- structs
- functions
- methods
- other language-specific symbols

```sql
symbols
-------
id
file_id
parent_id
kind
name
signature
start_line
end_line
start_byte
end_byte
```

Example:

```text
Auth.cs
└── class AuthService
    ├── Authenticate()
    ├── Logout()
    └── RefreshToken()
```

The symbol model is important for future structural and historical intelligence.

For example, instead of only asking:

> What changed in `Auth.cs`?

Inspectye can eventually answer:

> How did `Authenticate()` change between two revisions?

---

# 4. `relationships`

Represents meaningful relationships between project entities.

```sql
relationships
-------------
id
source_type
source_id
target_type
target_id
kind
confidence
created_at
updated_at
```

Possible relationship kinds include:

```text
IMPORTS
CALLS
REFERENCES
DEPENDS_ON
INHERITS
IMPLEMENTS
GENERATED_FROM
```

Example:

```text
Auth.cs → User.cs
AuthService → UserService
API.cs → AuthService
```

## Important design principle

The database stores relationships, but the Inspectye UI does **not** need to represent everything as a graph.

The same relationship data can power:

- tables
- dependency lists
- impact views
- file detail pages
- timelines
- relationship explorers
- graph visualizations when a graph is actually useful

Inspectye should not become "a graph application."

---

# 5. `external_references`

Tracks references to resources outside the repository.

This supports the external filesystem dependency detection discussed for Inspectye.

Example code:

```text
C:/assets/bear_butt.png
```

Possible schema:

```sql
external_references
-------------------
id
file_id
source_line
reference_type
raw_value
resolved_path
exists
inside_project
last_checked_at
```

Inspectye can then report:

```text
config.cs:18

External filesystem dependency

C:/assets/bear_butt.png

Exists:          Yes
Inside repository: No

Warning:
This resource exists on the current machine but is
outside the repository and may not exist elsewhere.
```

If the referenced resource does not exist:

```text
Error:
Referenced external resource does not exist.
```

Inspectye should not scan an entire computer looking for dependencies.

It should inspect external resources that are actually referenced by the project or explicitly configured.

---

# 6. `issues`

Stores problems and warnings discovered by Inspectye.

```sql
issues
------
id
project_id
file_id
symbol_id
kind
severity
message
status
first_seen_at
last_seen_at
evidence
```

Possible severity levels:

```text
INFO
WARNING
ERROR
```

Examples:

```text
EXTERNAL_REFERENCE
WARNING
C:/assets/bear_butt.png exists outside repository
```

```text
MISSING_REFERENCE
ERROR
src/Auth.cs references missing src/User.cs
```

## Evidence matters

Inspectye should preserve the evidence behind an issue.

It should avoid turning inference into fact.

For example, if Inspectye knows:

```text
Auth.cs references User.cs
User.cs does not exist
```

it can report that directly.

It should not automatically claim:

```text
User.cs is definitely the reason the build fails
```

unless it has actual evidence establishing that relationship.

---

# 7. `analysis_runs`

Tracks Inspectye's analysis activity.

```sql
analysis_runs
-------------
id
project_id
started_at
finished_at
status
trigger
files_scanned
files_changed
files_failed
```

Example:

```text
Run 42
Trigger: watcher

Files scanned: 1,234
Files changed: 7
Files failed: 0
Status: completed
```

This supports the continuous local watcher and incremental analysis model.

---

# 8. `file_analysis`

Tracks whether a file's current contents have already been analyzed.

```sql
file_analysis
-------------
file_id
content_hash
parser_version
analyzer_version
analyzed_at
status
error
```

This lets Inspectye determine whether existing analysis can be reused.

Example:

```text
File hash changed
        ↓
Previous analysis hash no longer matches
        ↓
Re-analyze file
        ↓
Update symbols / relationships / issues
```

The parser and analyzer versions are important because analysis results may become invalid when Inspectye itself changes.

---

# 9. `file_states`

Represents discrete snapshots/versions of a file's state tracked across scans.

```sql
file_states
-----------
id
file_id
content_hash
git_commit
recorded_at
line_count
size_bytes
snapshot
```

### Purpose & Constraints:
- `snapshot`: Stores textual snapshot content for fast diffing if the file is text-based and within a reasonable size limit (<= 2MB).
- `git_commit`: Associates the state snapshot with Git HEAD commit if inside a Git repository.
- Does not replace Git history; preserves local state transitions between scans.

---

# 10. `state_transitions`

Represents explicit transitions from one recorded file state to another.

```sql
state_transitions
-----------------
id
file_id
from_state_id
to_state_id
transition_type
restored_to_state_id
analysis_run_id
lines_added
lines_removed
created_at
```

### Transition Types:
- `New`: File first observed and recorded.
- `Modified`: File content changed between states.
- `Restored`: File content restored exactly to an earlier historical state (`from_state -> to_state` where `to_state.hash == historical_state.hash`).
- `RestoredModified`: File reappeared at the same path with modified content.
- `Removed`: File removed from the working tree.
- `Unchanged`: No change detected.

---

# Git Is Not Duplicated

Inspectye should **not initially create its own complete Git database**.

Git already owns:

```text
commits
branches
commit metadata
diffs
historical file contents
authors
Git object history
```

Inspectye can query Git when it needs those things.

For example:

```text
git log
git diff
git show
git status
git blame
```

Inspectye can then combine Git information with its own intelligence.

Conceptually:

```text
Inspectye file
│
├── Current analysis
├── Symbols
├── Relationships
├── Warnings
│
└── Git history
    ├── Commit 101
    ├── Commit 88
    └── Commit 42
```

If performance later requires caching or indexing specific Git information, that can be introduced deliberately.

It should not be duplicated by default.

---

# Database vs Git

| Concern | Git | Inspectye SQLite |
|---|---|---|
| Commit history | Yes | No, initially |
| Branches | Yes | No, initially |
| Commit diffs | Yes | No, initially |
| Historical file contents | Yes | No, initially |
| Current working tree | Reads filesystem | Tracks/indexes it |
| File metadata | Partial | Yes |
| Symbols | No | Yes |
| Code relationships | No | Yes |
| Dependency intelligence | No | Yes |
| External filesystem references | No | Yes |
| Current warnings | No | Yes |
| Analysis state | No | Yes |
| Incremental indexing state | No | Yes |
| Evidence for Inspectye findings | No | Yes |

The key principle:

> **Do not build a second Git. Build the intelligence layer around Git and the filesystem.**

---

# Incremental Analysis

The database is particularly important for the local watcher.

Example:

```text
Developer edits Database.cs
          ↓
Filesystem watcher notices change
          ↓
Inspectye calculates new file hash
          ↓
Hash differs from stored analysis
          ↓
Re-analyze Database.cs
          ↓
Update symbols
          ↓
Update relationships
          ↓
Update external references
          ↓
Recalculate affected intelligence
          ↓
Update issues
          ↓
Dashboard / CLI / AI agent see new state
```

This is how Inspectye can maintain a continuously updated project state without repeatedly scanning the entire repository.

---

# Proposed Initial Schema

The initial conceptual schema is therefore:

```text
projects
   │
   ├── files
   │     │
   │     ├── symbols
   │     ├── external_references
   │     ├── file_analysis
   │     └── issues
   │
   ├── relationships
   │
   └── analysis_runs
```

Relationships can connect entities across files and symbols.

Git remains an external source of historical information:

```text
                  ┌───────────────┐
                  │ Filesystem    │
                  └───────┬───────┘
                          │
                          ▼
                    ┌───────────┐
                    │  Scanner  │
                    └─────┬─────┘
                          │
                          ▼
                    ┌───────────┐
                    │ Analyzer  │
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
           Dashboard     CLI     AI Agent


                    ┌───────────┐
                    │    Git    │
                    └─────┬─────┘
                          │
                History / Diff / State
                          │
                          ▼
                     Inspectye
```

---

# What We Should NOT Lock Down Yet

Before writing actual `CREATE TABLE` statements, we should define Inspectye's **data model / ontology**.

Specifically:

- What exactly is a `File`?
- What exactly is a `Symbol`?
- What kinds of `Relationship` exist?
- What is a `Reference`?
- What counts as an `Issue`?
- How is `Evidence` represented?
- What does `confidence` mean?
- How are stale analysis results invalidated?
- Which entities are project-wide versus file-specific?
- How should generated files be represented?
- How should deleted files be represented?
- How should renamed/moved files be tracked?
- How should Git identity connect to Inspectye's structural identity?

These decisions should come **before** the final SQLite schema.

The schema should implement the intelligence model, not define it accidentally.

---

# Architectural Principle

The database should be treated as:

> **Inspectye's persistent memory of the project's derived intelligence.**

It is not:

- a replacement for Git
- a copy of the repository
- a second source of truth for source code
- a generic event log
- a reason to force every feature into a graph

The goal is to make Inspectye capable of answering:

> **What is happening in this project right now, what changed, what is connected to it, what may be broken, and what evidence do we have?**
