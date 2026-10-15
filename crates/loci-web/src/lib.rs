//! Loci in the browser: the `loci-core` engine compiled to WebAssembly, driven from a dedicated
//! Web Worker (`apps/loci/src/bridge/wasm.worker.ts`).
//!
//! ```text
//! const engine = await WebEngine.open(true);   // installs the OPFS VFS, opens /loci
//! engine.persistent                             // false when OPFS was unavailable (memory only)
//! engine.call(method, argsJson, bytes, onProgress)
//!   -> string (JSON result) for JSON methods, Uint8Array | undefined for byte methods
//!   throws a JSON string {code, message} on failure
//! ```
//!
//! Storage: SQLite (sqlite-wasm-rs) with the OPFS "SyncAccessHandle pool" VFS installed as the
//! default VFS, so `loci-core` opens `/loci/collection.sqlite` and its blob store
//! `/loci/blobs.sqlite` (media and backups, see `loci_core::blobs`) exactly as on native, and every
//! write is durable when the call returns. Without OPFS (or in Node, for tests) the default
//! in-memory VFS is used instead.
#![cfg(target_arch = "wasm32")]

use loci_core::rpc::{Engine, Payload};
use loci_core::LociError;
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// Where the collection lives inside the VFS.
const COLLECTION_DIR: &str = "/loci";
/// OPFS directory of the SAH pool (distinct from the TypeScript build's `.loci`, same origin).
const POOL_DIRECTORY: &str = ".loci-rs";
const VFS_NAME: &str = "loci-opfs";
/// Files the pool must hold: two databases, their rollback journals, and headroom.
const POOL_CAPACITY: u32 = 8;

fn js_error(e: &LociError) -> JsValue {
    let json = serde_json::to_string(e).unwrap_or_else(|_| format!("{{\"code\":\"internal\",\"message\":{:?}}}", e.message));
    JsValue::from_str(&json)
}

fn internal(message: impl std::fmt::Display) -> JsValue {
    js_error(&LociError::internal(message.to_string()))
}

/// Why the OPFS VFS could not be installed.
enum InstallError {
    /// No OPFS here (not a dedicated worker, private mode, old browser): run in memory.
    Unsupported,
    /// OPFS exists but its files are held, usually by the worker of a tab that is still closing.
    Busy(String),
}

async fn install_opfs() -> Result<(), InstallError> {
    use sqlite_wasm_vfs::sahpool::{install, OpfsSAHError, OpfsSAHPoolCfgBuilder};
    let cfg = OpfsSAHPoolCfgBuilder::new().vfs_name(VFS_NAME).directory(POOL_DIRECTORY).initial_capacity(POOL_CAPACITY).build();
    let classify = |e: OpfsSAHError| match e {
        OpfsSAHError::NotSupported | OpfsSAHError::GetDirHandle(_) => InstallError::Unsupported,
        other => InstallError::Busy(other.to_string()),
    };
    let util = install::<rusqlite::ffi::WasmOsCallback>(&cfg, true).await.map_err(classify)?;
    util.reserve_minimum_capacity(POOL_CAPACITY).await.map_err(classify)?;
    Ok(())
}

/// One collection, owned by the worker.
#[wasm_bindgen]
pub struct WebEngine {
    engine: Engine,
    persistent: bool,
}

#[wasm_bindgen]
impl WebEngine {
    /// Opens the collection. With `persistent`, installs the OPFS VFS first (dedicated workers
    /// only); where OPFS is unavailable the collection runs in memory and `persistent` reads
    /// false. When OPFS files are still held elsewhere it fails with code `busy`, detail
    /// `storage-busy`: the caller retries in a fresh worker (a failed attempt keeps its handles).
    pub async fn open(persistent: bool) -> Result<WebEngine, JsValue> {
        let persistent = if persistent {
            match install_opfs().await {
                Ok(()) => true,
                Err(InstallError::Unsupported) => false,
                Err(InstallError::Busy(why)) => {
                    let e = LociError::busy(format!("The collection storage is in use: {why}"));
                    return Err(js_error(&e.with_detail("storage-busy")));
                }
            }
        } else {
            false
        };
        let engine = Engine::open(COLLECTION_DIR).map_err(|e| js_error(&e))?;
        Ok(WebEngine { engine, persistent })
    }

    /// True when the collection is stored in OPFS (survives reloads).
    #[wasm_bindgen(getter)]
    pub fn persistent(&self) -> bool {
        self.persistent
    }

    /// Runs one engine method (`loci_core::rpc` calling convention). `args_json` is a JSON array;
    /// `bytes` the byte payload of `previewImport`/`importPackage`/`addMedia`; `on_progress` is
    /// called with `{phase, done, total}` objects.
    pub fn call(
        &mut self,
        method: &str,
        args_json: &str,
        bytes: Option<Vec<u8>>,
        on_progress: Option<js_sys::Function>,
    ) -> Result<JsValue, JsValue> {
        let args: Vec<Value> = serde_json::from_str(args_json).map_err(|e| internal(format!("Bad arguments: {e}")))?;
        let mut sink = |p: loci_core::types::Progress| {
            if let Some(f) = &on_progress {
                let obj = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&obj, &"phase".into(), &JsValue::from_str(&p.phase));
                let _ = js_sys::Reflect::set(&obj, &"done".into(), &JsValue::from_f64(p.done as f64));
                let _ = js_sys::Reflect::set(&obj, &"total".into(), &JsValue::from_f64(p.total as f64));
                let _ = f.call1(&JsValue::NULL, &obj);
            }
        };
        match self.engine.dispatch(method, &args, bytes.as_deref(), &mut sink).map_err(|e| js_error(&e))? {
            Payload::Json(v) => Ok(JsValue::from_str(&serde_json::to_string(&v).map_err(internal)?)),
            Payload::Bytes(Some(b)) => Ok(js_sys::Uint8Array::from(&b[..]).into()),
            Payload::Bytes(None) => Ok(JsValue::UNDEFINED),
        }
    }
}
