//! The Loci shell: a Tauri 2 window (or Android activity) around the Rust engine.
//!
//! The webview talks to the engine through four commands (see `apps/loci/src/bridge/tauri.ts`):
//! `stage_bytes` (raw body in, token out), `rpc` (JSON in/out, optional staged bytes, progress
//! channel), `rpc_bytes` (JSON in, raw bytes out) and `save_bytes` (raw body in, saved through the
//! native dialog on desktop or into the app's downloads folder on Android).
//!
//! The collection lives in the platform app-data directory (`app_data_dir()`, which is the app's
//! internal storage on Android): `collection.sqlite`, `media/`, `backups/`. Closing the window
//! writes an automatic backup if anything changed. The `sync_folder_*` commands give the webview's
//! folder sync (`src/ui/sync/`) access to `<sync folder>/loci-sync/` and nothing else.

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

/// Sync folder access (docs/SYNC.md). `root` is the folder the user picked; `path` is a list of
/// names that must start with `loci-sync`, so nothing outside `<root>/loci-sync/` is touched.
fn sync_path(root: &str, path: &[String]) -> Result<PathBuf, LociError> {
    let mut p = PathBuf::from(root);
    if !p.is_absolute() {
        return Err(LociError::invalid("The sync folder must be an absolute path"));
    }
    if path.first().map(String::as_str) != Some("loci-sync") {
        return Err(LociError::invalid("Sync files live in the loci-sync folder"));
    }
    for seg in path {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains(['/', '\\', '\0']) {
            return Err(LociError::invalid(format!("Unusable name in the sync folder: {seg:?}")));
        }
        p.push(seg);
    }
    Ok(p)
}

#[derive(serde::Serialize)]
struct FolderEntry {
    name: String,
    dir: bool,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, LociError> + Send + 'static) -> Result<T, LociError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| LociError::internal(format!("The file task failed: {e}")))?
}

/// Entries of a directory in the sync folder (empty when it does not exist).
#[tauri::command]
async fn sync_folder_list(root: String, path: Vec<String>) -> Result<Vec<FolderEntry>, LociError> {
    let dir = sync_path(&root, &path)?;
    blocking(move || {
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let Ok(name) = e.file_name().into_string() else { continue };
            let dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            out.push(FolderEntry { name, dir });
        }
        Ok(out)
    })
    .await
}

/// A file in the sync folder as raw bytes (empty when missing).
#[tauri::command]
async fn sync_folder_read(root: String, path: Vec<String>) -> Result<Response, LociError> {
    let file = sync_path(&root, &path)?;
    let bytes = blocking(move || Ok(std::fs::read(&file).unwrap_or_default())).await?;
    Ok(Response::new(bytes))
}

/// Writes a file in the sync folder (temp file + rename, so readers never see half of it).
/// Body: the bytes; headers: `x-loci-root`, `x-loci-path` (JSON list), both URI-encoded.
#[tauri::command]
async fn sync_folder_write(request: Request<'_>) -> Result<(), LociError> {
    let bytes = raw_body(&request)?;
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(percent_decode);
    let root = header("x-loci-root").ok_or_else(|| LociError::invalid("No sync folder given"))?;
    let path: Vec<String> = serde_json::from_str(&header("x-loci-path").unwrap_or_default())
        .map_err(|_| LociError::invalid("No sync file path given"))?;
    let file = sync_path(&root, &path)?;
    blocking(move || {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tmp = file.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        std::fs::write(&tmp, &bytes)?;
        if let Err(e) = std::fs::rename(&tmp, &file) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    })
    .await
}

/// Removes a file from the sync folder (no-op when missing).
#[tauri::command]
async fn sync_folder_remove(root: String, path: Vec<String>) -> Result<(), LociError> {
    let file = sync_path(&root, &path)?;
    blocking(move || {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        Ok(())
    })
    .await
}

/// Folder picker for the sync folder (desktop); None when cancelled.
#[tauri::command]
async fn pick_sync_folder(app: tauri::AppHandle) -> Result<Option<String>, LociError> {
    blocking(move || pick_folder(&app)).await
}

#[cfg(not(target_os = "android"))]
fn pick_folder(app: &tauri::AppHandle) -> Result<Option<String>, LociError> {
    use tauri_plugin_dialog::DialogExt;
    let Some(picked) = app.dialog().file().set_title("Sync folder").blocking_pick_folder() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| LociError::io(format!("Unusable folder: {e}")))?;
    Ok(Some(path.display().to_string()))
}

#[cfg(target_os = "android")]
fn pick_folder(_app: &tauri::AppHandle) -> Result<Option<String>, LociError> {
    Ok(None)
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

#[cfg(desktop)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu {
    //! The macOS menu bar. Built on every desktop target so it is type-checked everywhere; only
    //! macOS installs it. Items that are app actions emit a `menu` event with their id, which the
    //! webview turns into the same command the palette runs.
    use tauri::menu::{AboutMetadataBuilder, Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::{AppHandle, Emitter, Runtime};

    /// Ids forwarded to the webview; each is a command id in `commands.ts`. The palette has no
    /// accelerator here: Cmd+K belongs to the editor (insert link) while a field has focus, so the
    /// webview handles it.
    pub const FORWARDED: &[&str] = &[
        "settings",
        "add",
        "import",
        "home",
        "browse",
        "stats",
        "palette",
        "theme.system",
        "theme.light",
        "theme.dark",
        "manual",
        "shortcuts",
    ];

    fn item<R: Runtime>(app: &AppHandle<R>, id: &str, label: &str, accel: Option<&str>) -> tauri::Result<MenuItem<R>> {
        MenuItem::with_id(app, id, label, true, accel)
    }

    pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
        let about = AboutMetadataBuilder::new()
            .name(Some("Loci"))
            .version(Some(env!("CARGO_PKG_VERSION")))
            .comments(Some("A memory palace for everything you learn."))
            .website(Some("https://zahakj.github.io/loci-rs/"))
            .license(Some("MIT"))
            .build();
        let app_menu = Submenu::with_items(
            app,
            "Loci",
            true,
            &[
                &PredefinedMenuItem::about(app, Some("About Loci"), Some(about))?,
                &PredefinedMenuItem::separator(app)?,
                &item(app, "settings", "Settings…", Some("CmdOrCtrl+,"))?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::services(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        let file = Submenu::with_items(
            app,
            "File",
            true,
            &[
                &item(app, "add", "Add Card…", Some("CmdOrCtrl+N"))?,
                &PredefinedMenuItem::separator(app)?,
                &item(app, "import", "Import…", Some("CmdOrCtrl+Shift+I"))?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::close_window(app, None)?,
            ],
        )?;
        let edit = Submenu::with_items(
            app,
            "Edit",
            true,
            &[
                &PredefinedMenuItem::undo(app, None)?,
                &PredefinedMenuItem::redo(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, None)?,
                &PredefinedMenuItem::copy(app, None)?,
                &PredefinedMenuItem::paste(app, None)?,
                &PredefinedMenuItem::select_all(app, None)?,
            ],
        )?;
        let theme = Submenu::with_items(
            app,
            "Appearance",
            true,
            &[
                &item(app, "theme.system", "Match System", None)?,
                &item(app, "theme.light", "Light", None)?,
                &item(app, "theme.dark", "Dark", None)?,
            ],
        )?;
        let view = Submenu::with_items(
            app,
            "View",
            true,
            &[
                &item(app, "home", "Decks", Some("CmdOrCtrl+1"))?,
                &item(app, "browse", "Browse", Some("CmdOrCtrl+2"))?,
                &item(app, "stats", "Statistics", Some("CmdOrCtrl+3"))?,
                &PredefinedMenuItem::separator(app)?,
                &item(app, "palette", "Command Palette…", None)?,
                &theme,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::fullscreen(app, None)?,
            ],
        )?;
        let window = Submenu::with_items(
            app,
            "Window",
            true,
            &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, Some("Zoom"))?],
        )?;
        let help = Submenu::with_items(
            app,
            "Help",
            true,
            &[&item(app, "manual", "Loci Manual", None)?, &item(app, "shortcuts", "Keyboard Shortcuts", Some("CmdOrCtrl+/"))?],
        )?;
        Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
    }

    pub fn forward<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
        let id = event.id().as_ref();
        if FORWARDED.contains(&id) {
            let _ = app.emit("menu", id);
        }
    }
}

fn backup_on_leave(state: &AppState) {
    let _ = with_engine(state, |e| e.dispatch("backupOnLeave", &[], None, &mut |_| {}));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    LazyLock::force(&STARTED);
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    // Desktop: the window comes back at the size and place it was closed at.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    // macOS gets a native menu bar; Linux and Windows keep the app's own chrome (sidebar and
    // command palette), see docs/internals.md "Window and menus".
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::forward);
    builder
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
        .invoke_handler(tauri::generate_handler![
            stage_bytes,
            rpc,
            rpc_bytes,
            save_bytes,
            data_dir,
            app_ready,
            sync_folder_list,
            sync_folder_read,
            sync_folder_write,
            sync_folder_remove,
            pick_sync_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running Loci");
}
