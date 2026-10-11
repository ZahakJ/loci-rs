//! The Loci shell: a Tauri 2 window (or Android activity) around the Rust engine.
//!
//! The webview talks to the engine through four commands (see `apps/loci/src/bridge/tauri.ts`):
//! `stage_bytes` (raw body in, token out), `rpc` (JSON in/out, optional staged bytes, progress
//! channel), `rpc_bytes` (JSON in, raw bytes out) and `save_bytes` (raw body in, saved through the
//! native dialog on desktop or into the app's downloads folder on Android).
//!
//! The collection lives in the platform app-data directory (`app_data_dir()`, which is the app's
//! internal storage on Android): `collection.sqlite`, `media/`, `backups/`. Closing the window
//! writes an automatic backup if anything changed.

use loci_core::rpc::Engine;
use loci_core::types::Progress;
use loci_core::LociError;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

/// Process start, for the cold-start measurement (`app_ready`).
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);
use tauri::ipc::{Channel, InvokeBody, Request, Response};
use tauri::{Manager, State};

pub struct AppState {
    engine: Mutex<Option<Engine>>,
    staged: Mutex<HashMap<String, Vec<u8>>>,
    data_dir: PathBuf,
}

type Shared = Arc<AppState>;

fn with_engine<T>(state: &AppState, f: impl FnOnce(&mut Engine) -> Result<T, LociError>) -> Result<T, LociError> {
    let mut guard = state.engine.lock().map_err(|_| LociError::internal("The engine lock is poisoned"))?;
    match guard.as_mut() {
        Some(e) => f(e),
        None => Err(LociError::internal("The collection is not open")),
    }
}

fn take_staged(state: &AppState, token: Option<&str>) -> Result<Option<Vec<u8>>, LociError> {
    let Some(token) = token else { return Ok(None) };
    let mut staged = state.staged.lock().map_err(|_| LociError::internal("The staging lock is poisoned"))?;
    staged.remove(token).map(Some).ok_or_else(|| LociError::invalid("Unknown byte token"))
}

fn raw_body(request: &Request<'_>) -> Result<Vec<u8>, LociError> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(Value::Array(items)) => Ok(items.iter().filter_map(|v| v.as_u64()).map(|v| v as u8).collect()),
        InvokeBody::Json(_) => Err(LociError::invalid("Expected a raw byte body")),
    }
}

/// Stores a byte payload for the next `rpc` call; returns its token.
#[tauri::command]
fn stage_bytes(state: State<'_, Shared>, request: Request<'_>) -> Result<String, LociError> {
    let bytes = raw_body(&request)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    state.staged.lock().map_err(|_| LociError::internal("The staging lock is poisoned"))?.insert(token.clone(), bytes);
    Ok(token)
}

/// One JSON method call; `token` names staged bytes for the byte-taking methods.
#[tauri::command]
async fn rpc(
    state: State<'_, Shared>,
    method: String,
    args: Vec<Value>,
    token: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<Value, LociError> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = take_staged(&state, token.as_deref())?;
        with_engine(&state, |e| {
            let mut sink = |p: Progress| {
                let _ = on_progress.send(p);
            };
            e.dispatch(&method, &args, bytes.as_deref(), &mut sink)?.into_json()
        })
    })
    .await
    .map_err(|e| LociError::internal(format!("The engine task failed: {e}")))?
}

/// One method call returning raw bytes (empty body = the engine returned null).
#[tauri::command]
async fn rpc_bytes(
    state: State<'_, Shared>,
    method: String,
    args: Vec<Value>,
    on_progress: Channel<Progress>,
) -> Result<Response, LociError> {
    let state = Arc::clone(&state);
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        with_engine(&state, |e| {
            let mut sink = |p: Progress| {
                let _ = on_progress.send(p);
            };
            e.dispatch(&method, &args, None, &mut sink)?.into_bytes()
        })
    })
    .await
    .map_err(|e| LociError::internal(format!("The engine task failed: {e}")))??;
    Ok(Response::new(bytes.unwrap_or_default()))
}

/// Saves a raw body as a file. Desktop: native save dialog (None when cancelled). Android: the
/// app's downloads folder. Returns where the file went, for a toast.
#[tauri::command]
async fn save_bytes(app: tauri::AppHandle, request: Request<'_>) -> Result<Option<String>, LociError> {
    let bytes = raw_body(&request)?;
    let filename = request
        .headers()
        .get("x-loci-filename")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "loci-export.bin".to_string());
    let safe = filename.replace(['/', '\\', '\0'], "_");
    tauri::async_runtime::spawn_blocking(move || save_file(&app, &safe, &bytes))
        .await
        .map_err(|e| LociError::internal(format!("The save task failed: {e}")))?
}

#[cfg(not(target_os = "android"))]
fn save_file(app: &tauri::AppHandle, filename: &str, bytes: &[u8]) -> Result<Option<String>, LociError> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().set_file_name(filename).blocking_save_file();
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|e| LociError::io(format!("Unusable save location: {e}")))?;
    std::fs::write(&path, bytes)?;
    Ok(Some(format!("Saved {}", path.display())))
}

#[cfg(target_os = "android")]
fn save_file(app: &tauri::AppHandle, filename: &str, bytes: &[u8]) -> Result<Option<String>, LociError> {
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir().map(|p| p.join("exports")))
        .map_err(|e| LociError::io(format!("No downloads folder: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    let mut path = dir.join(filename);
    let mut n = 1;
    while path.exists() {
        let stem = std::path::Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("export");
        let ext =
            std::path::Path::new(filename).extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes)?;
    Ok(Some(format!("Saved to {}", path.display())))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Called by the frontend once Home (or Welcome) is interactive; logs the cold-start time and
/// returns it in milliseconds.
#[tauri::command]
fn app_ready() -> u64 {
    let ms = STARTED.elapsed().as_millis() as u64;
    log::info!("Loci ready in {ms} ms");
    ms
}

/// Where the collection lives (for the About/Data screens).
#[tauri::command]
fn data_dir(state: State<'_, Shared>) -> String {
    state.data_dir.display().to_string()
}

fn backup_on_leave(state: &AppState) {
    let _ = with_engine(state, |e| e.dispatch("backupOnLeave", &[], None, &mut |_| {}));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    LazyLock::force(&STARTED);
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().map_err(|e| format!("No app data directory: {e}"))?;
            std::fs::create_dir_all(&data_dir)?;
            let engine = Engine::open(&data_dir).map_err(|e| format!("Cannot open the collection: {e}"))?;
            log::info!("Loci collection at {}", data_dir.display());
            app.manage(Arc::new(AppState { engine: Mutex::new(Some(engine)), staged: Mutex::new(HashMap::new()), data_dir }));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(state) = window.try_state::<Shared>() {
                    backup_on_leave(&state);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![stage_bytes, rpc, rpc_bytes, save_bytes, data_dir, app_ready])
        .run(tauri::generate_context!())
        .expect("error while running Loci");
}
