# mdr

`mdr` is a small Rust wrapper around `pandoc` that ships an embedded HTML
template, CSS theme, and sidenote Lua filter. It lets you turn any Markdown file
into a standalone HTML page with one command:

```bash
mdr input.md [output.html]
```

If you omit `output.html`, `mdr` swaps the extension of the input path. Override
the KaTeX CDN base with `MDR_KATEX=https://…/`.

## Developing

- `make` / `make build` – build debug binary at `target/debug/mdr`
- `make dist` – build a release binary into `dist/mdr` (set `TARGET` to cross‑compile)
- `make fmt` / `make lint` – format and run clippy (`-D warnings`)
- `make test` – run unit + integration tests
- `make watch-cli` – `cargo watch -x check -x test` (requires `cargo-watch`)

## Site/docs

The demo site lives in `site/`:

- Markdown sources: `site/src/`
- Static assets: `site/public/`
- Generated output: `docs/`

Build once with `make site`. Serve and auto‑rebuild with `make watch`
(`watchman-make` required). The site build dogfoods the `mdr` binary.

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
