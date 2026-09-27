# Inspectye

> See inside your codebase.

Inspectye is a local-first codebase intelligence tool. It keeps an
incrementally updated picture of a project and answers questions like:

- What changed, and what is connected to it?
- What may be broken, and what evidence supports that?
- What could be affected if this file or symbol changes?
- How did this code get to its current state?

Everything runs locally: a Rust core, a SQLite index in `.inspectye/`, and Git
as the source of history. The CLI, the desktop dashboard and AI agents (via
`--json`) all read the same intelligence.

## Install

Requires a Rust toolchain.

```sh
cargo install --path .
```

## Quick start

```sh
cd your-project
inspecteye init        # create .inspectye/ and the local database
inspecteye scan        # index files and record what changed
inspecteye deep-scan   # symbols, relationships, diagnostics, external paths
inspecteye status      # what Inspectye currently knows
inspecteye check       # preflight: all checks, exit code 1 on errors
```

## Commands

| Family | Commands | What it does |
|---|---|---|
| Fundamental | `init`, `scan`, `deep-scan`, `status` | Manage Inspectye's knowledge of the project |
| Inspect | `inspect symbols / symbol / dependencies / dependents / impact / history / diagnostics / references / file / changes / compare` | Read stored intelligence (never re-analyzes) |
| Check | `check [syntax / logic / dependencies] [targets] [--strict]` | Analyze now; exit code 1 when the check fails |
| Insights | `insights [changed / diagnostics / dependencies / symbols / size / complexity / partial-classes] [target]` | Project-wide ranked metrics |
| Tree | `tree [target] [--depth N] [--files] [--directories] [--tracked] [--ignored]` | Project structure |
| Dashboard | `dashboard` | Open the desktop dashboard window |

Run `inspecteye --help` for examples and `inspecteye <command> --help` for details.

Languages with full analysis: Rust, TypeScript, JavaScript, Python, C#.
Configuration files and other languages are scanned for external filesystem
references.

### Findings

Every finding has a **severity** (`INFO`, `WARNING`, `ERROR`) and a
**certainty** (`fact`, `inference`, `possibility`) with the evidence behind it.
Uncertain findings are never reported as errors.

### Machine-readable output

Every command accepts `--json` and prints one document:

```json
{ "schema_version": 1, "command": "inspect file", "ok": true, "data": { ... } }
{ "schema_version": 1, "command": "status", "ok": false,
  "error": { "kind": "not_initialized", "message": "..." } }
```

`inspecteye --json inspect file <path>` gives an AI agent everything known
about one file in one call.

## Dashboard

A Tauri desktop app in [`dashboard/`](dashboard/) that links the same core.
To build it you need Node.js 20+, a Rust toolchain and, on Windows, the WebView2
runtime:

```sh
cd dashboard
npm install
npm run build        # produces src-tauri/target/release/inspecteye-dashboard(.exe)
```

`inspecteye dashboard` looks for the executable next to `inspecteye`, in
`INSPECTEYE_DASHBOARD`, or in the build output above.

## Development

```sh
cargo test
```

- [`AGENTS.md`](AGENTS.md): engineering rules and architecture boundaries
- [`ROADMAP.md`](ROADMAP.md): what is done and what is next
- [`Inspectye_Product_and_Architecture_Decisions.md`](Inspectye_Product_and_Architecture_Decisions.md)
- [`Inspectye_Database_Schema.md`](Inspectye_Database_Schema.md)
