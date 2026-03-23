# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

mdr is a Rust CLI that wraps Pandoc to convert Markdown to standalone HTML. It embeds its own HTML template, CSS themes, and a Lua sidenote filter directly in the binary via `include_str!()`. It can optionally serve the output over HTTP with live reload via WebSocket.

External runtime dependency: Pandoc (≥2) must be installed.

## Commands

| Task | Command |
|------|---------|
| Build (debug) | `make` or `make build` |
| Build (release, static musl) | `make dist` |
| Install to ~/bin | `make install` |
| Format | `make fmt` |
| Lint | `make lint` (clippy with `-D warnings`) |
| All tests | `make test` (Rust + Ruby e2e) |
| Rust integration tests only | `make test-integration` |
| E2E tests only | `make test-e2e` (requires Ruby, builds debug binary first) |
| Single Rust test | `cargo test <test_name>` |
| Watch mode | `make watch-cli` (requires `cargo-watch`) |

## Architecture

**Single-file Rust binary** — all logic lives in `src/main.rs` (~500 lines). The flow is:

```
main() → run() → parse_args() → ensure_pandoc()
  → materialize_assets() to temp dir
  → run_build_once() / run_watch_mode() / run_serve_mode()
```

**Modes of operation:**
- **Default (serve):** Builds HTML in memory, serves via Axum HTTP server on port 8080 (auto-increments if busy). Includes WebSocket endpoint (`/ws`) for live reload.
- **Watch (`-w`):** Monitors input file with `notify` crate, debounces 250ms, rebuilds on change.
- **Output (`-o`):** Writes HTML to a file instead of serving.

**Embedded assets** (`assets/` dir): `template.html5`, `css/theme.css`, `css/skylighting-solarized-theme.css`, `pandoc-sidenote.lua` — all compiled into the binary, then materialized to a temp dir at runtime for Pandoc to consume.

**Key async patterns:** Tokio runtime, `broadcast::channel` for signaling reload to all WebSocket clients, `tokio::task::spawn_blocking` for Pandoc subprocess calls.

## Testing

- **Integration tests** (`tests/integration/integration.rs`): Use `assert_cmd` to invoke the CLI as a subprocess. Use a fake pandoc script to isolate from real Pandoc.
- **E2E tests** (`tests/e2e/test_mdr_e2e.rb`): Ruby/Minitest. Invoke the real compiled binary with real Pandoc. Test file conversion, HTTP serving, WebSocket reload, and port fallback. Require the debug binary to be built first.
- **Fixtures** in `tests/fixtures/` — `full.md` and `full.html` are the reference input/output pair.

## Conventions

- Never modify the `TODO` file.
- Rust: `cargo fmt` + `cargo clippy -- -D warnings` before commits.
- Commit style: short, sentence-case summaries under ~72 chars.
- File naming: lowercase kebab-case or snake_case.
- Keep `src/main.rs` focused; embed assets in binary rather than shipping external files.
- `MDR_KATEX` env var overrides the KaTeX CDN base URL (used in tests with local fixtures).
