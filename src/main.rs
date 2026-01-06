use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use notify::event::ModifyKind;
use notify::{
    recommended_watcher, Config as NotifyConfig, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use tokio::net::TcpListener;
use tokio::select;
use tokio::sync::{broadcast, mpsc};

const TEMPLATE_HTML: &str = include_str!("../assets/template.html5");
const THEME_CSS: &str = include_str!("../assets/css/theme.css");
const SKYLIGHTING_CSS: &str = include_str!("../assets/css/skylighting-solarized-theme.css");
const SIDENOTE_LUA: &str = include_str!("../assets/pandoc-sidenote.lua");
const DEFAULT_KATEX: &str = "https://cdn.jsdelivr.net/npm/katex@0.15.1/dist/";

#[derive(Clone)]
struct Assets {
    template_path: PathBuf,
    lua_path: PathBuf,
    theme_path: PathBuf,
    skylighting_path: PathBuf,
}

struct Config {
    bin: String,
    watch: bool,
    serve: bool,
    port: u16,
    host: String,
    no_clobber: bool,
    input_path: PathBuf,
    output_path: PathBuf,
}

#[tokio::main]
async fn main() {
    if let Err(code) = run().await {
        process::exit(code);
    }
}

async fn run() -> Result<(), i32> {
    let config = parse_args()?;
    ensure_pandoc(&config.bin);

    if config.no_clobber {
        confirm_overwrite(&config.output_path, &config.bin);
    }

    let temp = match temp_root() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("mdr: failed to create temp dir: {err}");
            return Err(1);
        }
    };

    let assets = match materialize_assets(&temp) {
        Ok(a) => a,
        Err(code) => {
            cleanup(&temp);
            return Err(code);
        }
    };

    if let Err(code) = run_build_once(&config.input_path, &config.output_path, &assets).await {
        cleanup(&temp);
        return Err(code);
    }

    let result = if config.serve {
        run_serve_mode(&config, &assets).await
    } else if config.watch {
        run_watch_mode(&config, &assets).await
    } else {
        Ok(())
    };

    cleanup(&temp);
    result
}

fn usage(bin: &str) {
    eprintln!(
        "usage: {bin} [-w|--watch] [-s|--serve|-P|--public] [--port <port>] [--host <host>] [-n|--no-clobber] <input.md> [output.html]"
    );
}

fn parse_args() -> Result<Config, i32> {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "mdr".into());

    let mut watch = false;
    let mut serve = false;
    let mut port: u16 = 8080;
    let mut host: String = "127.0.0.1".into();
    let mut no_clobber = false;
    let mut positional: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-w" | "--watch" => watch = true,
            "-s" | "--serve" => serve = true,
            "-P" | "--public" => {
                serve = true;
                host = "0.0.0.0".into();
            }
            "-h" | "--help" => {
                usage(&bin);
                process::exit(0);
            }
            "--port" | "-p" => {
                let Some(val) = args.next() else {
                    eprintln!("{bin}: --port requires a value");
                    return Err(64);
                };
                port = match val.parse::<u16>() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("{bin}: invalid port: {val}");
                        return Err(64);
                    }
                };
            }
            "--host" => {
                let Some(val) = args.next() else {
                    eprintln!("{bin}: --host requires a value");
                    return Err(64);
                };
                host = val;
            }
            "-n" | "--no-clobber" => no_clobber = true,
            _ if arg.starts_with('-') => {
                eprintln!("{bin}: unknown option: {arg}");
                usage(&bin);
                return Err(64);
            }
            _ => positional.push(arg),
        }
    }

    if positional.is_empty() || positional.len() > 2 {
        usage(&bin);
        return Err(64);
    }

    let input_path = PathBuf::from(&positional[0]);
    let output_path = positional.get(1).map(PathBuf::from).unwrap_or_else(|| {
        let mut derived = input_path.clone();
        derived.set_extension("html");
        derived
    });

    if output_path.as_os_str().is_empty() {
        eprintln!("mdr: could not derive output path from input");
        return Err(64);
    }

    if serve {
        watch = true; // serving implies watching
    }

    Ok(Config {
        bin,
        watch,
        serve,
        port,
        host,
        no_clobber,
        input_path,
        output_path,
    })
}

fn temp_root() -> io::Result<PathBuf> {
    let mut dir = env::temp_dir();
    dir.push(format!("mdr-{}", process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn katex_url() -> String {
    let mut url = env::var("MDR_KATEX").unwrap_or_else(|_| DEFAULT_KATEX.to_string());
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

fn has_title_metadata(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };

    // YAML metadata block
    if content.starts_with("---\n") {
        if let Some(end) = content[4..].find("\n---") {
            let header = &content[4..4 + end];
            if header
                .lines()
                .any(|l| l.trim_start().to_ascii_lowercase().starts_with("title:"))
            {
                return true;
            }
        }
    }

    // Pandoc %-style metadata
    if let Some(first_line) = content.lines().next() {
        if first_line.starts_with('%') && first_line.len() > 1 {
            return true;
        }
    }

    false
}

fn ensure_pandoc(bin: &str) {
    match Command::new("pandoc")
        .arg("--version")
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(_) | Err(_) => {
            eprintln!(
                "{bin}: pandoc not found. Please install pandoc and ensure it is on your PATH."
            );
            process::exit(127);
        }
    }
}

fn confirm_overwrite(path: &Path, bin: &str) {
    if path.exists() {
        eprintln!(
            "{bin}: warning: output file already exists: {}",
            path.display()
        );
        eprint!("Overwrite? [y/N]: ");
        let _ = io::stderr().flush();

        let mut response = String::new();
        match io::stdin().read_line(&mut response) {
            Ok(_) => {
                let normalized = response.trim().to_ascii_lowercase();
                if normalized != "y" && normalized != "yes" {
                    eprintln!("{bin}: aborting; not overwriting existing file");
                    process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("{bin}: failed to read confirmation: {err}");
                process::exit(1);
            }
        }
    }
}

fn materialize_assets(temp: &Path) -> Result<Assets, i32> {
    let template_path = temp.join("template.html5");
    let lua_path = temp.join("pandoc-sidenote.lua");
    let css_dir = temp.join("css");
    let theme_path = css_dir.join("theme.css");
    let skylighting_path = css_dir.join("skylighting-solarized-theme.css");

    let writes = [
        (template_path.as_path(), TEMPLATE_HTML),
        (lua_path.as_path(), SIDENOTE_LUA),
        (theme_path.as_path(), THEME_CSS),
        (skylighting_path.as_path(), SKYLIGHTING_CSS),
    ];

    for (path, contents) in writes {
        if let Err(err) = write_file(path, contents) {
            eprintln!("mdr: failed to write {path:?}: {err}");
            return Err(1);
        }
    }

    Ok(Assets {
        template_path,
        lua_path,
        theme_path,
        skylighting_path,
    })
}

async fn run_build_once(input_path: &Path, output_path: &Path, assets: &Assets) -> Result<(), i32> {
    let input = input_path.to_path_buf();
    let output = output_path.to_path_buf();
    let assets = assets.clone();

    tokio::task::spawn_blocking(move || build_once(&input, &output, &assets))
        .await
        .map_err(|_| {
            eprintln!("mdr: build task panicked");
            1
        })?
}

fn build_once(input_path: &Path, output_path: &Path, assets: &Assets) -> Result<(), i32> {
    let mut cmd = Command::new("pandoc");
    cmd.arg(format!("--katex={}", katex_url()))
        .arg("--from")
        .arg("markdown+tex_math_single_backslash")
        .arg("--embed-resources")
        .arg("--lua-filter")
        .arg(&assets.lua_path)
        .arg("--to")
        .arg("html5+smart")
        .arg("--standalone");

    if !has_title_metadata(input_path) {
        let fallback_title = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document");
        cmd.arg("--metadata").arg(format!("title={fallback_title}"));
    }

    cmd.arg("--template")
        .arg(&assets.template_path)
        .arg("--css")
        .arg(&assets.theme_path)
        .arg("--css")
        .arg(&assets.skylighting_path)
        .arg("--toc")
        .arg("--wrap=none")
        .arg("--output")
        .arg(output_path)
        .arg(input_path);

    let status = cmd.status();

    match status {
        Ok(code) if code.success() => Ok(()),
        Ok(code) => {
            let code = code.code().unwrap_or(-1);
            eprintln!("mdr: pandoc failed with exit code {code}");
            Err(code)
        }
        Err(err) => {
            eprintln!("mdr: failed to spawn pandoc: {err}");
            Err(127)
        }
    }
}

async fn run_watch_mode(config: &Config, assets: &Assets) -> Result<(), i32> {
    eprintln!(
        "{}: watching {} for changes (press Ctrl+C to stop)",
        config.bin,
        config.input_path.display()
    );

    watch_and_rebuild(
        config.bin.clone(),
        config.input_path.clone(),
        config.output_path.clone(),
        assets.clone(),
        None,
    )
    .await
}

async fn run_serve_mode(config: &Config, assets: &Assets) -> Result<(), i32> {
    let (reload_tx, _) = broadcast::channel(32);

    let mut watch_handle = tokio::spawn(watch_and_rebuild(
        config.bin.clone(),
        config.input_path.clone(),
        config.output_path.clone(),
        assets.clone(),
        Some(reload_tx.clone()),
    ));

    let mut server_handle = tokio::spawn(run_http_server(
        config.bin.clone(),
        config.output_path.clone(),
        config.port,
        config.host.clone(),
        reload_tx,
    ));

    let result = select! {
        res = &mut watch_handle => res.unwrap_or_else(|_| Err(1)),
        res = &mut server_handle => res.unwrap_or_else(|_| Err(1)),
    };

    watch_handle.abort();
    server_handle.abort();

    result
}

async fn watch_and_rebuild(
    bin: String,
    input_path: PathBuf,
    output_path: PathBuf,
    assets: Assets,
    reload_tx: Option<broadcast::Sender<()>>,
) -> Result<(), i32> {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let mut watcher: RecommendedWatcher = recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|err| {
        eprintln!("mdr: failed to start watcher: {err}");
        1
    })?;

    if let Err(err) = watcher.configure(NotifyConfig::default()) {
        eprintln!("mdr: watcher configuration failed: {err}");
        return Err(1);
    }

    let watch_target = input_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let canonical_input = match input_path.canonicalize() {
        Ok(path) => path,
        Err(err) => {
            eprintln!(
                "{bin}: warning: could not canonicalize input {}: {err}",
                input_path.display()
            );
            input_path.clone()
        }
    };

    if let Err(err) = watcher.watch(&watch_target, RecursiveMode::NonRecursive) {
        eprintln!("mdr: unable to watch {}: {err}", watch_target.display());
        return Err(1);
    }

    let debounce = Duration::from_millis(250);
    let mut last_build = Instant::now() - debounce;
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        select! {
            _ = &mut ctrl_c => {
                eprintln!("{bin}: stopping watch");
                break;
            }
            Some(res) = rx.recv() => {
                match res {
                    Ok(event) => {
                        if !relevant_event(&event)
                            || !event_targets_input(&event, &input_path, &watch_target, &canonical_input)
                        {
                            continue;
                        }

                        if last_build.elapsed() < debounce {
                            continue;
                        }

                        if !input_path.exists() {
                            eprintln!(
                                "{bin}: input file {} is missing; waiting for it to reappear",
                                input_path.display()
                            );
                            continue;
                        }

                        if let Err(code) = run_build_once(&input_path, &output_path, &assets).await {
                            if code == 127 {
                                return Err(code);
                            }
                            last_build = Instant::now();
                            continue;
                        }

                        last_build = Instant::now();

                        if let Some(ref tx) = reload_tx {
                            let _ = tx.send(());
                        }

                        eprintln!("{bin}: change detected; rebuild complete");
                    }
                    Err(err) => {
                        eprintln!("mdr: watch error: {err}");
                    }
                }
            }
            else => break,
        }
    }

    Ok(())
}

async fn run_http_server(
    bin: String,
    output_path: PathBuf,
    port: u16,
    host: String,
    reload_tx: broadcast::Sender<()>,
) -> Result<(), i32> {
    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .map_err(|err| {
            eprintln!("mdr: failed to bind HTTP server: {err}");
            1
        })?;

    let addr = listener.local_addr().map_err(|err| {
        eprintln!("mdr: failed to read server address: {err}");
        1
    })?;

    eprintln!(
        "{bin}: serving {} at http://{addr}/ (live reload enabled)",
        output_path.display()
    );

    let state = AppState {
        output_path,
        reload_tx,
    };

    let app = Router::new()
        .route("/", get(serve_output))
        .route("/live.js", get(live_js))
        .route("/ws", get(ws_handler))
        .with_state(state);

    axum::serve(listener, app).await.map_err(|err| {
        eprintln!("mdr: server error: {err}");
        1
    })
}

#[derive(Clone)]
struct AppState {
    output_path: PathBuf,
    reload_tx: broadcast::Sender<()>,
}

async fn serve_output(State(state): State<AppState>) -> impl IntoResponse {
    match tokio::fs::read_to_string(&state.output_path).await {
        Ok(mut html) => {
            if !html.contains("/live.js") {
                html.push_str("\n<script src=\"/live.js\"></script>\n");
            }
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response()
        }
        Err(err) => {
            eprintln!(
                "mdr: failed to read output {}: {err}",
                state.output_path.display()
            );
            (StatusCode::NOT_FOUND, "output not found").into_response()
        }
    }
}

async fn live_js() -> impl IntoResponse {
    const SCRIPT: &str = r#"(() => {
const proto = location.protocol === "https:" ? "wss://" : "ws://";
const ws = new WebSocket(proto + location.host + "/ws");
ws.onmessage = () => location.reload();
ws.onclose = () => setTimeout(() => location.reload(), 1000);
})();"#;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        SCRIPT,
    )
}

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    let mut rx = state.reload_tx.subscribe();
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = handle_ws(socket, &mut rx).await {
            eprintln!("mdr: websocket error: {err}");
        }
    })
}

async fn handle_ws(
    mut socket: WebSocket,
    rx: &mut broadcast::Receiver<()>,
) -> Result<(), axum::Error> {
    loop {
        match rx.recv().await {
            Ok(_) => {
                if socket.send(Message::Text("reload".into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }

    let _ = socket.close().await;
    Ok(())
}

fn cleanup(temp: &Path) {
    if let Err(err) = fs::remove_dir_all(temp) {
        // Not fatal; leave directory behind for inspection.
        eprintln!("mdr: warning: unable to remove temp dir {temp:?}: {err}");
    }
}

fn relevant_event(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(
            ModifyKind::Name(_) | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Any
        ) | EventKind::Create(_)
            | EventKind::Remove(_)
    )
}

fn event_targets_input(
    event: &notify::Event,
    input: &Path,
    watch_dir: &Path,
    canonical_input: &Path,
) -> bool {
    for path in &event.paths {
        let candidate = if path.is_absolute() {
            path.clone()
        } else {
            watch_dir.join(path)
        };

        if let Ok(canon) = candidate.canonicalize() {
            if &canon == canonical_input {
                return true;
            }
        }

        if let (Some(ev), Some(inp)) = (candidate.file_name(), input.file_name()) {
            if ev == inp {
                return true;
            }
        }

        if candidate == *input {
            return true;
        }
    }

    false
}

fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(contents.as_bytes())
}
