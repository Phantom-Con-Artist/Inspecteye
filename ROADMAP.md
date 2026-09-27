# Inspectye Roadmap

Status legend: `[x]` done · `[~]` in progress · `[ ]` not started

Each step is completed, verified (tests + playground run), marked done here, and then the next step starts.

---

## Step 0 — Foundation hardening ✅ DONE

- [x] Scan performance: resolve Git HEAD once per scan, wrap scan writes in one transaction
- [x] Non-ASCII paths: `git ls-files -z` so tracked state is correct
- [x] Scan summary counts A→B→A edits as `Restored`, matching the stored transition
- [x] Persist real `added_lines` / `removed_lines` for stored transitions
- [x] Delete→restore diffs against the last state that existed on disk
- [x] `changes <path>` resolves relative paths against the current directory
- [x] State/transition write failures are no longer silently discarded

## Step 1 — Code state / change intelligence ✅ DONE

- [x] File state representation, previous vs current
- [x] Line-level added/removed lines and hunks
- [x] Transitions A → B → C, restoration to a known earlier state
- [x] Working-tree comparison state persisted; Git stays the source of Git history
- [x] `inspecteye changes <path>` distinguishes pending (unscanned) vs last recorded change
- Out of scope: global changes command, rename/move detection

## Step 2 — Resource discovery ✅ DONE

- [x] Classification: source, config, documentation, asset, generated, dependency, binary, unknown
- [x] Single classification table (remove the duplicated one in `metadata.rs`)
- [x] Fewer false positives (`packages/`, `bin/`, `Build/` only at conventional locations; Makefile, Dockerfile, `.env*`)
- [x] Track modified time
- [x] `.inspecteyeignore` with gitignore-like semantics (`*` stays in one segment, `**`, `!negation`, anchored patterns)
- [x] Ignored resources still indexed; `.git` / `.inspectye` stay structural exclusions

## Step 3 — Language analysis ✅ DONE

- [x] Parser abstraction + language-specific analyzer abstraction
- [x] Tree-sitter integration (initial languages)
- [x] Common language-independent analysis result
- [x] Parse once, reuse the tree across analyses

## Step 4 — Syntax diagnostics (`inspecteye inspect syntax-errors`) ✅ DONE

- [x] Parse requested file(s), report severity, message, line, column, range, excerpt
- [x] Confirmed errors vs warnings
- [x] Persist diagnostics; re-analyze only changed files

## Step 5 — Symbol analysis ✅ DONE

- [x] Classes, interfaces, structs, enums, functions, methods, constructors, properties, fields, constants, modules, namespaces, type aliases, traits, impl blocks, partial classes
- [x] Name, kind, range, parent, stable identity
- [x] Changed lines mapped to symbols; symbol history

## Step 6 — Relationships / dependency intelligence ✅ DONE

- [x] Imports, references, calls, depends_on, inherits, implements, generated_from
- [x] File↔file and symbol↔symbol relationships
- [x] In/out/total degree, hubs
- [x] `inspecteye inspect dependencies [path]`
- Note: resolution re-runs project-wide after each deep-scan (≈0.8s for 1,500 files / 37k references in release). Make it incremental if large repos need it.

## Step 7 — Logic checks (`inspecteye check-logic`) ✅ DONE

- [x] Deterministic low-level checks with line/range, excerpt, evidence, confidence
- [x] Rules: constant condition, assignment in condition, empty branch/catch, identical branches, duplicate else-if condition, unreachable code, overwritten-before-read, self-comparison, self-assignment, loop without exit, discarded result
- [x] Potentially affected symbols/files shown from relationship data
- Not yet: possible uninitialized use, missing cleanup paths, suspicious null handling (need data-flow analysis)

## Step 8 — Impact / affected-file analysis ✅ DONE

- [x] Dependents of changed files/symbols, with explanation; never claim breakage without evidence
- [x] `inspecteye impact [paths] [--symbol S] [--depth N]`; with no paths, the working-tree changes since the last scan
- [x] Symbol-level seeds from the symbol diff; references to removed declarations flagged; weakest-link certainty

## Step 9 — Partial class intelligence (C#) ✅ DONE

- [x] Group partial declarations; aggregate LOC, changes, diagnostics, symbols, dependencies
- [x] `inspecteye inspect partial-classes`; references to a partial type resolve across its parts

## Step 10 — Historical intelligence ✅ DONE

- [x] Git history integration (`git log --follow`, `git show`), rename-aware
- [x] `inspecteye history <file> [--symbol S]`: commits + recorded working-tree states; symbol history by structural identity (moves are not changes)
- [x] `inspecteye compare <file> <from> [<to>] [--symbol S]`: any two revisions or the working tree, whole file or one symbol
- [x] Restoration to earlier content tracked (Step 1)

## Step 11 — Project insights (`inspecteye insights`) ✅ DONE

- [x] Explicit, evidence-based ranked lists (no opaque scores); each ranking states its metric
- [x] Changed (Git churn and commit counts, recorded working-tree changes), diagnostics, dependencies, symbols (most changed, most diagnostics), size, complexity (cyclomatic, per function), partial classes
- [x] `inspecteye insights [category...] [--limit N]`
- [x] Call resolution tightened: receiver calls (`x.m()`) only link through local or imported evidence

## Step 12 — Machine interface (`--json`) ✅ DONE

- [x] Global `--json` flag on every command: one envelope per run,
      `{schema_version, command, ok, data | error}`; progress lines suppressed
- [x] Result types serialize as-is (`serde`); durations as `duration_ms`;
      impact output uses paths and symbol names instead of database ids
- [x] `inspecteye inspect file <path>`: one-file summary for agents (record,
      analysis state + `analysis_current`, symbols, diagnostics, dependencies,
      dependents, recent changes) — reads the index only
- [x] JSON errors with a stable `kind` (`not_initialized`, `file_not_found`,
      `invalid_argument`, ...); argument errors too (exit code 2)
- [x] Tests: `tests/json_tests.rs` runs the real binary
- Later: local HTTP API, MCP, `failures` (needs build/test integration)

## Step 13 — Continuous watcher — deferred

## Step 14 — Preflight (`inspecteye check`) 🚧 PARTLY DONE (via Step 16)

- [x] `inspecteye check`: refresh, then syntax + logic + dependency + external checks
      over the project; exit code 1 on any ERROR
- [x] `--strict`: warnings fail too; `--no-refresh`
- [ ] Configurable exceptions (accepted findings, per-rule severity overrides)
- [ ] Machine-readable CI summary formats (e.g. SARIF)

## Step 15 — External filesystem references ✅ DONE

- [x] Detection (`intelligence/external.rs`): Windows drive paths (`C:\x`, `C:/x`),
      UNC (`\\server\share`), well-known Unix roots (`/etc`, `/var`, `/home/u/x`, ...),
      `~/` (not in JS/TS/CSS/HTML, where it is a module alias), `file://` URLs
- [x] Code files: string literals only (tree-sitter); comments and docstrings are
      skipped; escaping follows the literal kind (raw/verbatim vs escaped, so
      `"x:\n"` is not a path). Config files and unparsed languages: line scan,
      comment lines skipped, `*ignore` pattern files skipped
- [x] Not mistaken for paths: URL routes (`/api/users`, `/home/settings`), URLs,
      regexes (`"\\d+\\s"`), module aliases
- [x] Evaluation touches only the referenced path and its parent:
      - missing, parent missing too → ERROR / fact
      - missing, directory exists → WARNING / inference (may be created at runtime)
      - exists outside the repo → WARNING / fact
      - absolute path into the repo → WARNING / fact
      - templated, network (UNC, never probed) or other-OS path → WARNING / possibility
- [x] Machine-specific evidence (path under a user's home directory)
- [x] Test code (test dirs, `*.test.*`, `test_*`, `mod tests`, `*Tests`) is INFO:
      fixture paths are often deliberately fake
- [x] Persistence: `external_references` table (migrations 11–12); extraction is
      incremental by content hash; existence is re-checked on every deep scan
      (outside files change without the project changing)
- [x] Config files are now deep-scan eligible (text-only analysis)
- [x] Surfaces: deep-scan summary, `inspect references [paths]`, `inspect file`,
      `--json`; `check_external_references` re-checks on demand
- [x] Tests: unit (detection, evaluation) + `tests/external_reference_tests.rs`
      (outside file disappears → re-checked without re-analysis; file deleted →
      references removed). Dogfooded on Inspecteye's own source: 0 non-test findings

## Step 16 — CLI nomenclature overhaul ✅ DONE

Command families (see `inspecteye --help`):

| Family | Commands | Behavior |
|---|---|---|
| Fundamental | `init`, `scan`, `deep-scan`, `status` | lifecycle; `status` reads stored state only |
| Inspect | `inspect symbols / symbol / dependencies / dependents / impact / history / diagnostics / references` (+ `file`, `changes`, `compare`) | read-only; never re-analyzes |
| Check | `check [syntax / logic / dependencies] [targets] [--strict] [--no-refresh]` | refreshes, analyzes, exit code 1 on failure |
| Insights | `insights [changed / diagnostics / dependencies / symbols / size / complexity / partial-classes] [target]` | read-only rankings; `target` filters rows by path |
| Tree | `tree [target] [--depth N] [--files] [--directories] [--tracked] [--ignored]` | structure from the index |
| Dashboard | `dashboard [path]` | opens the desktop window (Step 17) |

- [x] Old → new: `inspect syntax-errors` → `check syntax`; `check-logic` → `check logic`;
      `impact` → `inspect impact`; `history` → `inspect history`; `compare` → `inspect compare`;
      `changes` → `inspect changes`; `inspect partial-classes` → `insights partial-classes`;
      `inspect symbols --history` → `inspect history <file>` (now includes recorded symbol changes)
- [x] New: `inspect symbol` (definition, members, uses/used-by incl. members, dependent files,
      diagnostics, external refs, recorded changes), `inspect dependents`, `inspect diagnostics
      --category`, `inspect references` (project files, packages, unresolved relative imports,
      external paths), `tree`
- [x] `check dependencies`: broken project-relative imports (JS/TS `./x`, Python `.x`, Rust `mod x;`)
      = ERROR/fact only when no candidate file exists on disk (ignored files count as existing);
      import cycles (Python/JS/TS) = INFO; external references re-checked live
- [x] Uniform `CheckReport {scope, strict, passed, counts, findings[]}` for every check
- [x] Rank rows carry a structured `path`; `NotFound` error kind (`not_found`)
- [x] No `git` family: Git stays an internal data source (per the nomenclature's own rule)
- [x] Tests: `tests/json_tests.rs` covers every command, check exit codes, tree and symbol shapes

## Step 17 — Desktop dashboard (Tauri) ✅ DONE

- [x] `dashboard/`: Tauri 2 app. The Rust side links the `inspecteye` crate and serves one IPC
      command, `query(view, target)`, backed by the same `ProjectManager` as the CLI
      (no HTTP server, no ports, no subprocess). Queries are serialized (one writer: refresh)
- [x] TypeScript front end (no framework), built with `tsc` via npm scripts
- [x] Views: Overview (state, errors/warnings, external paths, pending changes, composition,
      hubs, churn), Files (tree + per-file findings, connections, symbols, external paths,
      recorded changes, Git history, "what could this affect"), Findings (filterable),
      Insights (all rankings), Changes (pending changes and their potential impact)
- [x] Actions: Refresh analysis (scan + incremental deep scan), Run checks
- [x] Security: capability `core:default` only; strict CSP; no filesystem/shell plugins
- [x] `inspecteye dashboard` launches `inspecteye-dashboard --root <project>` (found next to the
      CLI, via `INSPECTEYE_DASHBOARD`, or in `dashboard/src-tauri/target/{release,debug}`)
- [x] Core: Git processes spawn without console windows (no flashing in the GUI)
- Build: `cd dashboard && npm install && npm run build`
