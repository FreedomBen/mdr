# Editor Support Plan

## Overview

Add an in-browser markdown editor to mdr's serve mode. A fixed edit icon
appears on every served page. Clicking it enters a split-pane edit mode:
CodeMirror 6 editor on the left, live Pandoc-rendered preview on the right.
Changes auto-save to disk. External file changes trigger a conflict warning.

---

## Architecture

### Current State

- Rust CLI using Axum (HTTP) + Tokio (async) + notify (file watching)
- Pandoc converts markdown server-side; output is served from `Arc<RwLock<String>>`
- WebSocket sends `"reload"` to clients when file changes
- All assets (template, CSS, Lua filter) are embedded via `include_str!()`
- Routes: `GET /` (HTML), `GET /live.js` (reload script), `GET /ws` (WebSocket)

### Design Decisions

| Question | Decision |
|----------|----------|
| Preview rendering | Server-side via Pandoc (exact fidelity) |
| Editor component | CodeMirror 6 (from CDN) |
| When available | Server mode (default, no `-o`); no `-w` flag needed |
| External conflicts | Warn user, let them choose to keep or reload |

---

## Implementation Plan

### 1. Server-Side: New Endpoints and State

#### 1a. Extend `AppState` (`src/main.rs`)

```rust
#[derive(Clone)]
struct AppState {
    html: SharedHtml,
    reload_tx: broadcast::Sender<()>,
    input_path: PathBuf,      // NEW
    assets: Assets,            // NEW
    editing: Arc<RwLock<bool>>, // NEW: tracks if a client is in edit mode
}
```

The `input_path` and `assets` are needed so the editor endpoints can read
the source file, save changes, and invoke Pandoc for rendering.

The `editing` flag lets the file watcher know not to trigger a full-page
reload when the save came from the editor itself.

#### 1b. `GET /source` - Return Raw Markdown

New handler that reads `input_path` and returns the raw markdown as
`text/plain; charset=utf-8`. Used by the editor to populate CodeMirror
when entering edit mode.

```rust
async fn serve_source(State(state): State<AppState>) -> impl IntoResponse {
    match tokio::fs::read_to_string(&state.input_path).await {
        Ok(content) => (StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            content).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
```

#### 1c. `GET /editor.js` - Editor Client Script

Serve the editor JavaScript (logic for entering/exiting edit mode,
CodeMirror integration, WebSocket messaging, auto-save). This script is
embedded via `include_str!()` from a new `assets/editor.js` file.

#### 1d. `GET /editor.css` - Editor Styles

Serve the editor-specific CSS (split pane layout, edit/exit icons,
conflict notification). Embedded via `include_str!()` from
`assets/css/editor.css`.

#### 1e. Build-from-String Function

Currently `make_pandoc_command()` takes a file path. Add a new function
that renders markdown from a string by piping to Pandoc's stdin:

```rust
fn build_string_to_html(markdown: &str, assets: &Assets) -> Result<String, i32> {
    let mut cmd = Command::new("pandoc");
    // Same args as make_pandoc_command but without input file path
    // and without --embed-resources (preview doesn't need embedded CSS)
    cmd.arg("--from").arg("markdown+tex_math_single_backslash")
       .arg("--lua-filter").arg(&assets.lua_path)
       .arg("--to").arg("html5+smart")
       .arg("--toc")
       .arg("--wrap=none")
       .stdin(Stdio::piped());

    let child = cmd.spawn()?;
    // Write markdown to stdin, read HTML from stdout
    // Return the body content (not full standalone doc)
}
```

Note: For the preview pane, we render a fragment (not `--standalone`)
since the preview pane already has its own container. We still apply the
Lua sidenote filter and KaTeX for fidelity. We also skip
`--embed-resources` since the preview doesn't need self-contained CSS.

Alternatively, we could render the full standalone doc and display it in
an iframe on the right side. This would give exact visual fidelity
including all CSS. This is the simpler approach and likely the better one.

**Recommended: Render full standalone HTML and display in an iframe.**

#### 1f. Extend WebSocket Protocol

Currently the WebSocket is unidirectional (server → client: `"reload"`).
Extend it to be bidirectional with JSON messages:

**Client → Server:**
```json
{"type": "update", "markdown": "# Hello\n\nWorld..."}
```

**Server → Client:**
```json
{"type": "rendered", "html": "<full standalone HTML>"}
{"type": "external_change", "markdown": "...new file content..."}
{"type": "saved"}
```

**Backward compatibility:** The server still sends plain `"reload"` text
messages for non-editor clients (reading mode). The editor client
intercepts messages before the live-reload handler.

#### 1g. WebSocket Handler Changes

The `handle_ws` function needs to become bidirectional:

```rust
async fn handle_ws(mut socket: WebSocket, state: AppState,
                   rx: &mut broadcast::Receiver<()>) {
    loop {
        select! {
            // Existing: forward rebuild notifications
            Ok(_) = rx.recv() => {
                // If editing flag is set, send external_change instead of reload
                if *state.editing.read().await {
                    let md = fs::read_to_string(&state.input_path)?;
                    let msg = json!({"type": "external_change", "markdown": md});
                    socket.send(Message::Text(msg.to_string())).await;
                } else {
                    socket.send(Message::Text("reload".into())).await;
                }
            }
            // NEW: receive editor updates
            Some(Ok(Message::Text(text))) = socket.recv() => {
                if let Ok(msg) = serde_json::from_str::<EditorMessage>(&text) {
                    match msg.msg_type.as_str() {
                        "update" => {
                            // 1. Save markdown to disk
                            fs::write(&state.input_path, &msg.markdown)?;
                            // 2. Render with Pandoc
                            let html = build_string_to_html(&msg.markdown, &state.assets)?;
                            // 3. Update shared HTML state
                            *state.html.write().await = html.clone();
                            // 4. Send rendered HTML back
                            let resp = json!({"type": "rendered", "html": html});
                            socket.send(Message::Text(resp.to_string())).await;
                        }
                        "enter_edit" => {
                            *state.editing.write().await = true;
                        }
                        "exit_edit" => {
                            *state.editing.write().await = false;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
```

#### 1h. Register New Routes

```rust
let app = Router::new()
    .route("/", get(serve_output))
    .route("/live.js", get(live_js))
    .route("/editor.js", get(editor_js))     // NEW
    .route("/editor.css", get(editor_css))   // NEW
    .route("/source", get(serve_source))     // NEW
    .route("/ws", get(ws_handler))
    .with_state(state);
```

#### 1i. Debouncing

The editor client should debounce `update` messages (300-500ms after last
keystroke). The server should also debounce saves to avoid excessive disk
writes. The file watcher's existing 250ms debounce will prevent
self-triggered rebuilds, but the `editing` flag provides additional
protection against feedback loops.

#### 1j. Suppressing Self-Triggered Reloads

When the editor saves a file, the file watcher will detect the change.
We need to prevent this from triggering a reload/conflict warning. Options:

1. **Editing flag** (chosen): When `editing` is true, the watcher sends
   `external_change` instead of `reload`. But we also need to distinguish
   editor-initiated saves from truly external changes.

2. **Better approach**: Track a "last save timestamp" or "last save hash"
   in AppState. When the watcher fires, compare the file content hash
   against the last editor-saved hash. If they match, it was our save;
   ignore it. If they differ, it's an external change; warn the user.

### 2. Client-Side: Editor JavaScript (`assets/editor.js`)

#### 2a. Edit Mode Toggle

On page load, inject a floating edit button (pencil icon, SVG) in the
top-right corner. It uses `position: fixed` so it stays visible during
scroll.

```javascript
// Injected into the page via <script src="/editor.js">
const editBtn = document.createElement('button');
editBtn.id = 'mdr-edit-btn';
editBtn.innerHTML = '<svg>...</svg>'; // Pencil icon
editBtn.title = 'Edit';
document.body.appendChild(editBtn);
```

#### 2b. Entering Edit Mode

When the edit button is clicked:

1. Fetch raw markdown from `GET /source`
2. Send `{"type": "enter_edit"}` over WebSocket
3. Load CodeMirror 6 from CDN (lazy-load on first edit):
   - `@codemirror/view`, `@codemirror/state`, `@codemirror/lang-markdown`
   - Use a bundled ESM from esm.sh or jsdelivr
4. Transform the page layout:
   - Hide the current page content
   - Create a split-pane container (CSS grid: `grid-template-columns: 1fr 1fr`)
   - Left pane: CodeMirror editor instance
   - Right pane: iframe showing rendered preview
5. Replace pencil icon with an "X" (exit) icon
6. Initialize the iframe with the current rendered HTML

#### 2c. Live Preview Updates

Register a CodeMirror update listener. On document changes (debounced
300ms), send the full markdown content to the server via WebSocket:

```javascript
const debounce = (fn, ms) => {
    let timer;
    return (...args) => { clearTimeout(timer); timer = setTimeout(() => fn(...args), ms); };
};

const sendUpdate = debounce((markdown) => {
    ws.send(JSON.stringify({ type: 'update', markdown }));
}, 300);

editor.dispatch // on update → sendUpdate(editor.state.doc.toString())
```

When the server responds with `{"type": "rendered", "html": "..."}`,
update the iframe's content:

```javascript
ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    if (msg.type === 'rendered') {
        const iframe = document.getElementById('mdr-preview');
        iframe.srcdoc = msg.html;
    } else if (msg.type === 'external_change') {
        showConflictNotification(msg.markdown);
    }
};
```

#### 2d. External Conflict Notification

When the server sends `external_change`, show a non-modal notification
bar at the top of the editor pane:

```
"File changed externally.  [Keep my changes]  [Reload from disk]"
```

- **Keep my changes**: Dismiss notification, continue editing. The
  editor's version will be saved on next auto-save.
- **Reload from disk**: Replace editor content with the new markdown
  from the message payload.

#### 2e. Exiting Edit Mode

When the exit icon is clicked:

1. Send `{"type": "exit_edit"}` over WebSocket
2. Remove the split-pane layout
3. Restore normal page layout (reload the page or replace body with
   current rendered HTML)
4. Replace exit icon with pencil icon

Simplest approach: just `location.reload()` to return to reading mode
with the latest saved content.

#### 2f. Keyboard Shortcuts

- `Escape` exits edit mode (same as clicking the exit icon)
- Standard CodeMirror keybindings for the editor (undo, redo, etc.)

### 3. Client-Side: Editor CSS (`assets/css/editor.css`)

```css
/* Edit/Exit button - fixed top-right */
#mdr-edit-btn {
    position: fixed;
    top: 1rem;
    right: 1rem;
    z-index: 9999;
    background: rgba(255, 255, 255, 0.9);
    border: 1px solid #ccc;
    border-radius: 4px;
    padding: 8px;
    cursor: pointer;
    box-shadow: 0 1px 3px rgba(0,0,0,0.1);
}

/* Split pane container */
#mdr-editor-container {
    display: grid;
    grid-template-columns: 1fr 1fr;
    height: 100vh;
    width: 100vw;
    position: fixed;
    top: 0;
    left: 0;
    z-index: 9998;
    background: white;
}

/* Editor pane */
#mdr-editor-pane {
    overflow: auto;
    border-right: 1px solid #ccc;
}
#mdr-editor-pane .cm-editor {
    height: 100%;
}

/* Preview pane (iframe) */
#mdr-preview {
    width: 100%;
    height: 100%;
    border: none;
}

/* Conflict notification */
#mdr-conflict-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    background: #fff3cd;
    border-bottom: 1px solid #ffc107;
    padding: 8px 16px;
    z-index: 10000;
    display: flex;
    align-items: center;
    gap: 12px;
}
```

### 4. Asset Embedding Changes

#### 4a. New Asset Files

```
assets/
  editor.js                    # Editor logic (~200 lines)
  css/
    editor.css                 # Editor styles (~80 lines)
```

#### 4b. New Constants in `main.rs`

```rust
const EDITOR_JS: &str = include_str!("../assets/editor.js");
const EDITOR_CSS: &str = include_str!("../assets/css/editor.css");
```

#### 4c. CodeMirror from CDN

CodeMirror 6 is loaded dynamically from CDN when entering edit mode
(not embedded in the binary). This keeps the binary small and is
consistent with how KaTeX is already loaded from CDN.

```javascript
// Dynamic import from CDN
const CM_CDN = 'https://cdn.jsdelivr.net/npm/';
async function loadCodeMirror() {
    const { EditorView, basicSetup } = await import(CM_CDN + '@codemirror/basic-setup');
    const { markdown } = await import(CM_CDN + '@codemirror/lang-markdown');
    // ... initialize editor
}
```

Note: ESM dynamic imports from CDN may have CORS or bundling issues. If
so, we may need to use a pre-built UMD bundle. Alternatives:
- Use esm.sh which handles bundling: `https://esm.sh/@codemirror/view`
- Pre-bundle CodeMirror into a single JS file and embed it
- Use a simpler editor (textarea with monospace font) as fallback

**Recommendation**: Start with esm.sh CDN. If that proves unreliable,
fall back to embedding a pre-built bundle.

### 5. Dependency Changes

#### 5a. `Cargo.toml` - Add serde_json

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Needed for parsing/serializing WebSocket JSON messages.

#### 5b. Message Types

```rust
#[derive(Deserialize)]
struct EditorMessage {
    #[serde(rename = "type")]
    msg_type: String,
    markdown: Option<String>,
}

#[derive(Serialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown: Option<String>,
}
```

### 6. Template Changes

Inject `editor.js` and `editor.css` into the served HTML (same pattern
as `live.js` injection in `serve_output`):

```rust
async fn serve_output(State(state): State<AppState>) -> impl IntoResponse {
    let mut html = state.html.read().await.clone();
    if !html.contains("/live.js") {
        html.push_str("\n<script src=\"/live.js\"></script>\n");
    }
    // NEW: inject editor assets
    if !html.contains("/editor.js") {
        html.push_str("<link rel=\"stylesheet\" href=\"/editor.css\">\n");
        html.push_str("<script src=\"/editor.js\"></script>\n");
    }
    // ...
}
```

### 7. File Watcher Integration

The watcher in `watch_and_rebuild()` needs awareness of edit mode:

1. Add `editing: Arc<RwLock<bool>>` and a save-hash tracker to the
   watcher's scope (passed through from AppState).
2. When a file change is detected and `editing` is true:
   - Compute hash of current file content
   - Compare against last editor-save hash
   - If same → ignore (our own save)
   - If different → send `external_change` via broadcast channel
3. When `editing` is false: existing behavior (rebuild + send `"reload"`)

### 8. Serving Without Watch Mode

Currently, `serve` mode always implies `watch = true`. The editor needs
to work even if the user explicitly disables watch mode. Since serve mode
already sets `watch = true` (line 252-253 of main.rs), this is handled
automatically. No changes needed here.

---

## Sequence Diagrams

### Entering Edit Mode
```
User          Browser              Server
 |  click       |                    |
 |  pencil   -->|                    |
 |              | GET /source     -->|
 |              |<-- markdown text   |
 |              | WS: enter_edit  -->|
 |              |                    | (sets editing=true)
 |              | load CodeMirror    |
 |              | (from CDN)         |
 |  split-pane  |                    |
 |  visible  <--|                    |
```

### Live Editing Flow
```
User          Browser              Server
 | keystroke -->|                    |
 |              | (debounce 300ms)   |
 |              | WS: {update, md}-->|
 |              |                    | write file to disk
 |              |                    | run pandoc
 |              |<-- {rendered, html}|
 |  preview  <--|                    |
 |  updates     |                    |
```

### External Change Conflict
```
User          Browser              Server         External
 | (editing)    |                    |                |
 |              |                    |  file change <-|
 |              |                    | (watcher fires)|
 |              |                    | hash != last   |
 |              |<-- {external_change, md}            |
 |  warning  <--|                    |                |
 |  banner      |                    |                |
 | [Keep]    -->|  (dismiss)         |                |
 |   or         |                    |                |
 | [Reload]  -->|  (replace editor)  |                |
```

---

## Open Questions

1. **Pandoc rendering latency**: Full Pandoc renders on each debounced
   keystroke may be slow for large documents. Should we add a loading
   indicator in the preview pane? Should we render fragments instead of
   full documents for faster turnaround?

2. **Binary size impact**: If CDN loading proves unreliable and we need
   to embed CodeMirror, the binary grows by ~200-400KB. Is that
   acceptable?

3. **Multiple editor clients**: Should we support multiple browser tabs
   editing simultaneously, or lock to one editor at a time? (Recommend:
   lock to one, show "another session is editing" if a second client
   tries to enter edit mode.)

4. **Undo across saves**: Auto-save writes to disk on every debounced
   change. If the user wants to revert, they only have CodeMirror's
   in-memory undo. Should we keep a backup file or leave this to git?

---

## Implementation Order

1. **Phase 1 - Server foundation**
   - Add `serde`/`serde_json` dependencies
   - Extend `AppState` with `input_path`, `assets`, `editing`
   - Add `GET /source` endpoint
   - Add `build_string_to_html()` function (Pandoc from stdin)
   - Extend WebSocket to be bidirectional with JSON protocol

2. **Phase 2 - Editor client**
   - Create `assets/editor.js` with edit mode toggle
   - Create `assets/css/editor.css` with split-pane layout
   - Add `GET /editor.js` and `GET /editor.css` endpoints
   - Inject editor assets in `serve_output`
   - Implement CodeMirror loading from CDN

3. **Phase 3 - Live preview loop**
   - Wire up CodeMirror change → debounced WebSocket send
   - Server-side: receive markdown, save, render, respond
   - Client-side: update preview iframe with rendered HTML
   - Self-save detection (hash comparison) to prevent feedback loops

4. **Phase 4 - Conflict handling**
   - Detect external changes during edit mode
   - Send `external_change` message with new content
   - Client-side conflict notification bar
   - Keep/Reload user choice

5. **Phase 5 - Polish**
   - Keyboard shortcuts (Escape to exit)
   - Loading indicator during Pandoc renders
   - Scroll position preservation in preview
   - Error handling (Pandoc failures, WebSocket disconnects)
   - Dark mode support for editor (match existing theme)

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/main.rs` | Modify | Extend AppState, add endpoints, extend WebSocket, add stdin-based Pandoc rendering |
| `assets/editor.js` | Create | Editor client logic (~200 lines) |
| `assets/css/editor.css` | Create | Editor layout styles (~80 lines) |
| `Cargo.toml` | Modify | Add serde, serde_json dependencies |
| `tests/integration/integration.rs` | Modify | Add tests for new endpoints |
| `tests/e2e/test_mdr_e2e.rb` | Modify | Add e2e tests for editor flow |
