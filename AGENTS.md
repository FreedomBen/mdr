# Repository Guidelines

## Agent instructions
- Never modify TODO.  Don't even read it because it doesn't contain any instructions or help for you.

## Project Structure & Module Organization
- `src/` Rust sources for the `mdr` CLI; `assets/` embeds the template, CSS, and sidenote filter.
- `template.html5` now lives in `assets/`; tweak `assets/css/` for theme changes.
- Release binaries land in `dist/`; Cargo artifacts live in `target/`.

## Build, Test, and Development Commands
- `make` / `make build` — build the debug `mdr` binary.
- `make dist` — build a release binary into `dist/mdr` (set `TARGET` to cross-compile).
- `make fmt`, `make lint`, `make test` — format, clippy (`-D warnings`), and tests.
- `make watch-cli` — run `cargo watch -x check -x test` for the Rust crate (requires `cargo-watch`).

## Coding Style & Naming Conventions
- Markdown: prefer H1 title at top, consistent heading hierarchy, fenced code blocks; keep metadata minimal because the wrapper adds a title when missing.
- Shell: follow existing pattern (`set -euo pipefail`, lowercase variables, `"$(...)"` quoting).
- Rust: run `cargo fmt` and `cargo clippy -- -D warnings` before commits; keep functions small and panic-free in CLI paths.
- Files and outputs: use lowercase kebab or snake case (`foo-bar.md`, `docs/foo-bar.html`).

## Testing Guidelines
- Wrapper: `make test` covers Rust logic; add focused unit tests for new flags or argument parsing.
- Site output: after template/CSS changes, run `make site` then spot-check `docs/index.html` and a couple of pages in the browser for layout/regressions.

## Commit & Pull Request Guidelines
- Recent history uses short, sentence-case summaries (e.g., `Rename wrapper to mdr`, `Switch to lua filter`); mirror that style and keep subject lines under ~72 chars.
- In PRs, include: scope of change, affected commands, manual test notes; attach before/after screenshots when altering rendered HTML or CSS.
- Note external requirements when relevant (Pandoc ≥2, `watchman-make`, Rust target installs). If changing defaults or CLI flags, document new usage in `README.md`.
