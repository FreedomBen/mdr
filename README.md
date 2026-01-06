# mdr

`mdr` is a small Rust wrapper around `pandoc` that ships an embedded HTML
template, CSS theme, and sidenote Lua filter. It lets you turn any Markdown file
into a standalone HTML page with one command:

```bash
mdr [-w|--watch] [-P|--public] [--port <port>] [--host <host>] [-o|--output <file>] [-n|--no-clobber] input.md
```

If you pass `-o/--output`, `mdr` writes the converted HTML there and exits (or
keeps watching with `-w`). If you omit `-o`, `mdr` renders to memory and serves
the HTML over HTTP (no disk writes). Passing `-o` with no path uses the derived
output path (input filename with `.html`). Override the KaTeX
CDN base with `MDR_KATEX=https://…/`.

- `-w`/`--watch` rebuilds the HTML whenever the input file changes (Linux first,
  cross‑platform via `notify`). With `-o`, it rewrites the output file; without
  `-o`, it pairs with the default HTTP server.
- Default server (when `-o` is omitted) serves the generated HTML from memory
  with live reload on `127.0.0.1:8080`. Use `--port` to choose a different port. Use
  `--host 0.0.0.0` or `-P`/`--public` to bind on all interfaces.
- `-o`/`--output` choose an explicit output file; skips HTTP server unless
  combined with `--watch`.
- `-n`/`--no-clobber` prompts before overwriting an existing output file; by
  default `mdr` overwrites without asking.

## Developing

- `make` / `make build` – build debug binary at `target/debug/mdr`
- `make dist` – build a statically linked musl release binary (default target
  `x86_64-unknown-linux-musl`) into `dist/mdr`; set `TARGET` to override
- `make install` – build release binary via `make dist` and copy it to
  `~/bin/mdr` (creates `~/bin` if missing)
- `make fmt` / `make lint` – format and run clippy (`-D warnings`)
- `make test` – run all tests (Rust + Ruby e2e)
- `make test-integration` – run Rust integration tests only
- `make test-e2e` – run Ruby end-to-end tests
- `make watch-cli` – `cargo watch -x check -x test` (requires `cargo-watch`)

## Assets

- `assets/template.html5`
- `assets/css/theme.css`
- `assets/css/skylighting-solarized-theme.css`
- `assets/pandoc-sidenote.lua`

These are embedded into the binary; edits trigger rebuilds automatically.

## License

HTML, CSS, and JavaScript code is licensed under the Blue Oak Model License. See
`LICENSE.md`. Text and images are licensed under CC-BY-SA 4.0. Fonts are **not**
licensed by this project; obtain your own licenses where required.
