//! `loci serve`: the engine's RPC over local HTTP, for the web frontend in development
//! (`pnpm dev` without Tauri) and for the Playwright suite.
//!
//! ```text
//! POST /rpc              {"method","args"}            → JSON result (or {"error":{code,message}})
//! POST /rpc-bytes        {"method","args"}            → application/octet-stream (404 when null)
//! POST /rpc-with-bytes?method=…&args=…  raw body     → JSON result (previewImport, importPackage, addMedia)
//! GET  /health                                        → {"ok":true}
//! ```
//!
//! `--multi-session`: every distinct `x-loci-session` header gets its own fresh collection in a
//! temporary directory (what Playwright needs: each browser context starts empty). Without it, one
//! engine serves `--dir`. Requests are handled one at a time; the engine is single-threaded by design.

use loci_core::rpc::{Engine, Payload};
use loci_core::LociError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use tiny_http::{Header, Method, Request, Response, Server};

struct Sessions {
    multi: bool,
    dir: PathBuf,
    engines: HashMap<String, Engine>,
}

impl Sessions {
    fn engine(&mut self, session: Option<&str>) -> Result<&mut Engine, LociError> {
        let key = if self.multi { session.unwrap_or("default").to_string() } else { "default".to_string() };
        if !self.engines.contains_key(&key) {
            let dir = if self.multi && key != "default" {
                let d = std::env::temp_dir().join(format!("loci-serve-{}-{}", std::process::id(), sanitize(&key)));
                std::fs::create_dir_all(&d)?;
                d
            } else {
                self.dir.clone()
            };
            let mut engine = Engine::open(&dir)?;
            engine.dispatch("open", &[], None, &mut |_| {})?;
            self.engines.insert(key.clone(), engine);
        }
        Ok(self.engines.get_mut(&key).expect("inserted"))
    }
}

fn sanitize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn cors(mut r: Response<std::io::Cursor<Vec<u8>>>) -> Response<std::io::Cursor<Vec<u8>>> {
    r.add_header(header("Access-Control-Allow-Origin", "*"));
    r.add_header(header("Access-Control-Allow-Headers", "content-type, x-loci-session"));
    r.add_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
    r
}

fn json_response(status: u16, v: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    cors(Response::from_data(body).with_status_code(status).with_header(header("Content-Type", "application/json")))
}

fn error_response(e: &LociError) -> Response<std::io::Cursor<Vec<u8>>> {
    let status = match e.code {
        loci_core::ErrorCode::NotFound => 404,
        loci_core::ErrorCode::Invalid
        | loci_core::ErrorCode::UnsupportedPackage
        | loci_core::ErrorCode::Corrupt
        | loci_core::ErrorCode::Template
        | loci_core::ErrorCode::Media => 400,
        loci_core::ErrorCode::Conflict | loci_core::ErrorCode::Busy => 409,
        loci_core::ErrorCode::Io | loci_core::ErrorCode::Internal => 500,
    };
    json_response(status, &json!({ "error": e }))
}

fn read_body(req: &mut Request) -> Result<Vec<u8>, LociError> {
    let mut buf = Vec::with_capacity(req.body_length().unwrap_or(0));
    req.as_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

fn query_params(url: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(q) = url.split_once('?').map(|(_, q)| q) {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                out.insert(percent_decode(k), percent_decode(v));
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let s = s.replace('+', " ");
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
    String::from_utf8(out).unwrap_or_default()
}

fn handle(sessions: &mut Sessions, req: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let session = req.headers().iter().find(|h| h.field.equiv("x-loci-session")).map(|h| h.value.as_str().to_string());
    if *req.method() == Method::Options {
        return cors(Response::from_data(Vec::new()).with_status_code(204));
    }
    if path == "/health" {
        return json_response(200, &json!({ "ok": true }));
    }
    if *req.method() != Method::Post {
        return json_response(405, &json!({ "error": { "code": "invalid", "message": "POST only" } }));
    }
    let result = (|| -> Result<Response<std::io::Cursor<Vec<u8>>>, LociError> {
        let (method, args, bytes) = if path == "/rpc-with-bytes" {
            let q = query_params(&url);
            let method = q.get("method").cloned().ok_or_else(|| LociError::invalid("method missing"))?;
            let args: Vec<Value> = serde_json::from_str(q.get("args").map(String::as_str).unwrap_or("[]"))?;
            (method, args, Some(read_body(req)?))
        } else {
            let body = read_body(req)?;
            let v: Value = serde_json::from_slice(&body)?;
            let method = v["method"].as_str().ok_or_else(|| LociError::invalid("method missing"))?.to_string();
            let args: Vec<Value> = v["args"].as_array().cloned().unwrap_or_default();
            (method, args, None)
        };
        let engine = sessions.engine(session.as_deref())?;
        let payload = engine.dispatch(&method, &args, bytes.as_deref(), &mut |_| {})?;
        Ok(match (path.as_str(), payload) {
            ("/rpc-bytes", Payload::Bytes(Some(b))) => {
                cors(Response::from_data(b).with_status_code(200).with_header(header("Content-Type", "application/octet-stream")))
            }
            ("/rpc-bytes", Payload::Bytes(None)) => cors(Response::from_data(Vec::new()).with_status_code(404)),
            (_, Payload::Json(v)) => json_response(200, &v),
            (_, Payload::Bytes(_)) => json_response(
                500,
                &json!({ "error": { "code": "internal", "message": "bytes result on a JSON route; use /rpc-bytes" } }),
            ),
        })
    })();
    match result {
        Ok(r) => r,
        Err(e) => error_response(&e),
    }
}

pub fn serve(dir: PathBuf, port: u16, multi: bool) -> Result<(), String> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| format!("serve: cannot listen on 127.0.0.1:{port}: {e}"))?;
    let mut sessions = Sessions { multi, dir: dir.clone(), engines: HashMap::new() };
    if !multi {
        sessions.engine(None).map_err(|e| e.to_string())?;
    }
    eprintln!(
        "loci serve: http://127.0.0.1:{port} ({})",
        if multi { "fresh collection per x-loci-session".to_string() } else { dir.display().to_string() }
    );
    for mut req in server.incoming_requests() {
        let resp = handle(&mut sessions, &mut req);
        let _ = req.respond(resp);
    }
    Ok(())
}
