[한국어](README.ko.md) | **English**

# Polyglot Local Vault

A local desktop workspace that indexes the many kinds of files on your machine into a single Vault —
searching them instantly, and treating what is *inside* each file (functions, headings, config keys)
as addressable objects with their own `vault://` addresses.

Indexing, search, structure extraction, and relationship inference are all done by **deterministic
algorithms**. AI is an optional add-on that attaches over MCP.

![Relationship graph over this repository's own 203 files — colored by language, with import and doc-to-code links](docs/screenshots/graph-view.png)

**Status (2026-08-21): Phase 0–4 complete.** Work has passed the MVP boundary (P0–P3) through P4 (MCP).
Core `polyglot-vault/` is 6,850 lines, desktop `desktop/src-tauri/` is 1,146 lines; 142 + 21 tests pass.
There is no public release yet.

**[View the intro page →](https://claude.ai/code/artifact/6908e7d8-a0ac-4273-88cd-92c73bf968a0)**

## Progress

| Phase | Scope | Status |
|---|---|---|
| P0 | Scanning · `vault://` addressing · locking · integrity scan · watcher · git reader · parser adapter | Done (2026-08-18) |
| P1 | Fast search — path table · inverted index · incremental indexing · search box + viewer | Done (2026-08-19) |
| P2 | Parsers + symbol search — symbol extraction across 12 formats | Done (2026-08-19) |
| P3 | Workspace + graph + suggestions — R1 · R2 · import resolution, approval UI, D3 graph | Done (2026-08-21) |
| P4 | MCP — `search` / `read` / `neighbors` / `link` over a stdio server | Done (2026-08-21) |

The desktop framework is **locked to Tauri** (2026-08-18): a Windows release build measured keystroke
p95 of 8.4 ms against PySide6's 14.7 ms.

What comes next is undecided — candidates are fixing bugs found in daily use, measuring the
suggestion approval rate (the P3 exit condition), and preparing a release.

## Getting started

Opening this directory in Claude Code makes it read `CLAUDE.md` first.
For a human, the reading order is:

1. `CLAUDE.md` — current state, locked decisions, rejected designs, what to do next (**the source of truth**)
2. `docs/design/00_README.md` — overview of the design document package
3. `docs/design/18_DATA_FORMATS.md` — `vault://` address grammar and the `.vault/` file formats
4. `docs/design/17_MEASUREMENT_BASIS.md` — where every number comes from, and its limits

The 7 blockers / 7 majors / 3 minors listed in `TODO.md` are **all closed** (2026-08-17); the file
is kept as a record.

Design documents are written in Korean; this README is the English entry point.

### Build

```bash
cd polyglot-vault && cargo test     # core tests
cd desktop && npm install
npm run tauri dev                   # run the desktop app in dev mode
npm run tauri build                 # release build
```

The MCP server is a separate stdio binary.

```bash
cd polyglot-vault && cargo run --bin vault-mcp
```

## Layout

```
polyglot-vault/     Rust core — index · parsers · suggestion engine · MCP (src/mcp.rs, src/bin/vault-mcp.rs)
desktop/            Tauri desktop app (src-tauri/ Rust, src/ frontend)
docs/design/        19 v0.2 design documents
docs/design_v0.1/   original v0.1 (reference only, do not edit)
research/reports/   design reviews, benchmark reports, P4 MCP call-efficiency measurement
research/data/      raw measurement data
research/bench/     reproducible measurement code (4 Rust / 10 Python)
```

## 12 supported formats

Code `.py` `.go` `.rs` `.ts` (class/struct/trait/interface/function/method via Tree-sitter) ·
Docs `.md` `.rst` `.txt` (headings) ·
Data `.json` `.yaml` `.toml` (nested keys → JSON Pointer) `.csv` (header columns) ·
Notebooks `.ipynb` (code cells re-parsed with the Python parser)

## Measured numbers

Every number below was measured on 18 real public repositories (130K commits, 230K files).
No synthetic data. Conditions are documented in `docs/design/17_MEASUREMENT_BASIS.md`.

| Metric | Value |
|---|---|
| Filename fuzzy search p95 | 7.4 ms @ 100K files (2 cores) |
| Cold indexing | 18.5 s @ 100K files |
| Full-text index / source size | 5.5 % |
| Integrity scan | 281 ms @ 100K files |
| File link auto-recovery | 95.7 % |
| Symbol link recovery | 80.0 % automatic → 93.6 % including one click |
| Symbol survival while editing | 96.9 % with Tree-sitter vs 16.7 % with CPython `ast` |
| MCP call efficiency (20 natural-language queries) | 1.05 calls on average, 20/20 correct, 17.6× fewer tokens than grep |

CI gates are based on the automatic recovery rates — aggregate symbols > 78 %, files > 93 %;
per-repository symbols > 63 %, files > 47 %.
The one-click figure is a product goal, not a gate.

## License

[PolyForm Noncommercial 1.0.0](LICENSE) — free to use, modify, and redistribute for any
noncommercial purpose. Commercial use (selling, offering as a paid service, redistributing
for revenue) requires the copyright holder's permission — reach out at knox9014@gmail.com.
