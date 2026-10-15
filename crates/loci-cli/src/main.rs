//! `loci`: the Loci engine from the command line, plus `serve`,
//! which exposes the engine's RPC over local HTTP for the web frontend in development and for the
//! Playwright suite.
//!
//! ```text
//! loci import <file.apkg|.colpkg|.zip> [--replace] [--no-scheduling]
//! loci export <out.apkg> [--deck <name>] [--no-scheduling] [--no-media]
//! loci due
//! loci study --auto <N> [--deck <name>]
//! loci stats [--deck <name>]
//! loci backup
//! loci integrity
//! loci serve [--port 5741] [--multi-session]
//! Options: --dir <collection dir> (default ./.loci-dev), --json
//! ```

mod serve;

use loci_core::rpc::{Engine, Payload};
use loci_core::types::Progress;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "Usage: loci <command> [options]
  import <file> [--replace] [--no-scheduling]   import an .apkg/.colpkg or a Loci full-backup .zip
  export <out.apkg> [--deck <name>] [--no-scheduling] [--no-media]
  due                                           deck tree with today's counts
  study --auto <N> [--deck <name>]              answer N cards with Good
  stats [--deck <name>]                         statistics and insights
  backup                                        write a manual backup
  integrity                                     check and repair the collection
  serve [--port 5741] [--multi-session]         RPC over local HTTP for the web frontend
Options: --dir <dir> (default ./.loci-dev), --json";

struct Args {
    command: String,
    positional: Vec<String>,
    flags: HashMap<String, Option<String>>,
}

impl Args {
    fn has(&self, k: &str) -> bool {
        self.flags.contains_key(k)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.get(k).and_then(|v| v.as_deref())
    }
}

fn parse_args(argv: &[String]) -> Args {
    let valued = ["dir", "deck", "auto", "port"];
    let mut positional = Vec::new();
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(rest) = a.strip_prefix("--") {
            if let Some((k, v)) = rest.split_once('=') {
                flags.insert(k.to_string(), Some(v.to_string()));
            } else if valued.contains(&rest) && i + 1 < argv.len() {
                flags.insert(rest.to_string(), Some(argv[i + 1].clone()));
                i += 1;
            } else {
                flags.insert(rest.to_string(), None);
            }
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }
    let command = if positional.is_empty() { "help".to_string() } else { positional.remove(0) };
    Args { command, positional, flags }
}

fn call(
    engine: &mut Engine,
    method: &str,
    args: &[Value],
    bytes: Option<&[u8]>,
    show_progress: bool,
) -> Result<Payload, loci_core::LociError> {
    let mut last = String::new();
    let mut sink = |p: Progress| {
        if show_progress {
            let line = format!("\r{} {}/{}      ", p.phase, p.done, p.total);
            if line != last {
                let _ = std::io::stderr().write_all(line.as_bytes());
                last = line;
            }
        }
    };
    let out = engine.dispatch(method, args, bytes, &mut sink);
    if show_progress && !last.is_empty() {
        eprintln!();
    }
    out
}

fn json_call(engine: &mut Engine, method: &str, args: &[Value], bytes: Option<&[u8]>, progress: bool) -> Result<Value, String> {
    call(engine, method, args, bytes, progress).and_then(Payload::into_json).map_err(|e| e.to_string())
}

fn deck_id_by_name(engine: &mut Engine, name: Option<&str>) -> Result<Value, String> {
    let Some(name) = name else { return Ok(Value::Null) };
    let tree = json_call(engine, "deckTree", &[], None, false)?;
    fn find(node: &Value, name: &str) -> Option<i64> {
        if node["deck"]["name"].as_str() == Some(name) {
            return node["deck"]["id"].as_i64();
        }
        node["children"].as_array()?.iter().find_map(|c| find(c, name))
    }
    find(&tree, name).map(Value::from).ok_or_else(|| format!("No deck named \"{name}\""))
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv);
    std::process::exit(match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("loci: {e}");
            1
        }
    });
}

fn run(args: Args) -> Result<i32, String> {
    if args.command == "help" || args.has("help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let json = args.has("json");
    let dir = PathBuf::from(args.get("dir").unwrap_or(".loci-dev"));
    if args.command == "serve" {
        let port: u16 = args.get("port").unwrap_or("5741").parse().map_err(|_| "serve: --port must be a number".to_string())?;
        return serve::serve(dir, port, args.has("multi-session")).map(|_| 0);
    }
    let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let mut engine = Engine::open(&dir).map_err(|e| e.to_string())?;
    let out = |human: String, data: &Value| {
        if json {
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        } else {
            println!("{human}");
        }
    };
    let report = json_call(&mut engine, "open", &[], None, false)?;
    if report["ok"] != Value::Bool(true) && args.command != "integrity" {
        let problems: Vec<String> = report["problems"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
            .unwrap_or_default();
        eprintln!(
            "The collection failed its integrity check:\n  {}\nRun \"integrity\" to repair, or restore a backup.",
            problems.join("\n  ")
        );
        return Ok(2);
    }
    let code = match args.command.as_str() {
        "import" => {
            let file = args.positional.first().ok_or("import: give the path of an .apkg, .colpkg or Loci backup .zip")?;
            let bytes = std::fs::read(file).map_err(|e| format!("import: cannot read {file}: {e}"))?;
            let options = json!({
                "mode": if args.has("replace") { "replace" } else { "merge" },
                "keepScheduling": !args.has("no-scheduling"),
                "conflict": "newer",
            });
            let r = json_call(&mut engine, "importPackage", &[options], Some(&bytes), !json && tty)?;
            let warnings: Vec<String> = r["warnings"]
                .as_array()
                .map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect())
                .unwrap_or_default();
            out(
                format!(
                    "Imported {} notes ({} updated, {} skipped), {} cards, {} reviews, {} media files, {} decks in {} ms.{}",
                    r["notesAdded"],
                    r["notesUpdated"],
                    r["notesSkipped"],
                    r["cardsAdded"],
                    r["reviewsAdded"],
                    r["mediaAdded"],
                    r["decksAdded"],
                    r["durationMs"],
                    if warnings.is_empty() { String::new() } else { format!("\nWarnings:\n  {}", warnings.join("\n  ")) }
                ),
                &r,
            );
            0
        }
        "export" => {
            let file = args.positional.first().ok_or("export: give the output path (.apkg)")?;
            let deck_id = deck_id_by_name(&mut engine, args.get("deck"))?;
            let options = json!({ "deckId": deck_id, "includeScheduling": !args.has("no-scheduling"), "includeMedia": !args.has("no-media") });
            let bytes = call(&mut engine, "exportPackage", &[options], None, !json && tty)
                .and_then(Payload::into_bytes)
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            std::fs::write(file, &bytes).map_err(|e| format!("export: cannot write {file}: {e}"))?;
            out(format!("Wrote {file} ({:.1} KB).", bytes.len() as f64 / 1024.0), &json!({ "file": file, "bytes": bytes.len() }));
            0
        }
        "due" => {
            let tree = json_call(&mut engine, "deckTree", &[], None, false)?;
            let mut lines = Vec::new();
            fn walk(n: &Value, depth: usize, lines: &mut Vec<String>) {
                for c in n["children"].as_array().into_iter().flatten() {
                    let name = c["deck"]["name"].as_str().unwrap_or("");
                    let leaf = name.rsplit("::").next().unwrap_or(name);
                    let label = format!("{}{}", "  ".repeat(depth), leaf);
                    lines.push(format!(
                        "{:<40} {:>5} {:>5} {:>5}  ~{} min",
                        label, c["newCount"], c["learnCount"], c["reviewCount"], c["estimatedMinutes"]
                    ));
                    walk(c, depth + 1, lines);
                }
            }
            walk(&tree, 0, &mut lines);
            out(
                format!(
                    "{:<40} {:>5} {:>5} {:>5}\n{}\n{:<40} {:>5} {:>5} {:>5}  ~{} min",
                    "Deck",
                    "New",
                    "Learn",
                    "Due",
                    lines.join("\n"),
                    "Total",
                    tree["newCount"],
                    tree["learnCount"],
                    tree["reviewCount"],
                    tree["estimatedMinutes"]
                ),
                &tree,
            );
            0
        }
        "study" => {
            let n: i64 = args
                .get("auto")
                .and_then(|v| v.parse().ok())
                .filter(|n| *n >= 1)
                .ok_or("study: use --auto <N> to answer N cards with Good")?;
            let deck_id = deck_id_by_name(&mut engine, args.get("deck"))?;
            let mut card = json_call(&mut engine, "startStudy", &[deck_id], None, false)?;
            let mut answered = 0;
            let mut log = Vec::new();
            while !card.is_null() && answered < n {
                let cid = card["card"]["id"].clone();
                let label = card["preview"]["labels"]["3"].as_str().unwrap_or("").to_string();
                json_call(&mut engine, "answerCard", &[cid.clone(), json!(3), json!(8000)], None, false)?;
                log.push(json!({ "card": cid, "next": label }));
                answered += 1;
                card = json_call(&mut engine, "nextCard", &[], None, false)?;
            }
            let summary = json_call(&mut engine, "endStudy", &[], None, false)?;
            let lines: Vec<String> =
                log.iter().map(|l| format!("  {} → {}", l["card"], l["next"].as_str().unwrap_or(""))).collect();
            out(
                format!(
                    "Answered {} card{} with Good{}.\n{}",
                    answered,
                    if answered == 1 { "" } else { "s" },
                    if answered < n { " (nothing else is due now)" } else { "" },
                    lines.join("\n")
                ),
                &json!({ "answered": answered, "log": log, "summary": summary }),
            );
            0
        }
        "stats" => {
            let deck_id = deck_id_by_name(&mut engine, args.get("deck"))?;
            let s = json_call(&mut engine, "stats", &[json!({ "deckId": deck_id })], None, false)?;
            let pct = |v: &Value| match v.as_f64() {
                Some(x) => format!("{:.1}%", x * 100.0),
                None => "—".to_string(),
            };
            let cc = &s["cardCounts"];
            let retention: Vec<String> = s["retention"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| format!("{} {} ({})", r["label"].as_str().unwrap_or(""), pct(&r["trueRetention"]), r["reviews"]))
                .collect();
            let forecast: Vec<String> =
                s["forecast"].as_array().into_iter().flatten().take(7).map(|f| f["due"].to_string()).collect();
            let mut lines = vec![
                format!(
                    "Today: {} answers in {} min, {} correct",
                    s["today"]["studied"],
                    (s["today"]["seconds"].as_f64().unwrap_or(0.0) / 60.0).round(),
                    pct(&s["today"]["correctPct"])
                ),
                format!(
                    "Cards: {} new, {} learning, {} young, {} mature, {} suspended, {} buried",
                    cc["new"], cc["learning"], cc["young"], cc["mature"], cc["suspended"], cc["buried"]
                ),
                format!("True retention: {}", retention.join(", ")),
                format!("Due in the next 7 days: {}", forecast.join(" ")),
                format!(
                    "Momentum: {}/30 days, current run {}, longest {}",
                    s["momentum"]["daysStudiedLast30"], s["momentum"]["currentRun"], s["momentum"]["longestRun"]
                ),
            ];
            for i in s["insights"].as_array().into_iter().flatten() {
                lines.push(format!("• {}", i["text"].as_str().unwrap_or("")));
            }
            out(lines.join("\n"), &s);
            0
        }
        "backup" => {
            let info = json_call(&mut engine, "backupNow", &[], None, false)?;
            out(
                format!(
                    "Backup {} ({:.1} KB, {} cards) in {}.",
                    info["name"].as_str().unwrap_or(""),
                    info["bytes"].as_f64().unwrap_or(0.0) / 1024.0,
                    info["cards"],
                    dir.join("backups").display()
                ),
                &info,
            );
            0
        }
        "integrity" => {
            let r = json_call(&mut engine, "checkIntegrity", &[], None, false)?;
            let list = |k: &str| -> Vec<String> {
                r[k].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default()
            };
            let ok = r["ok"] == Value::Bool(true);
            let repaired = list("repaired");
            out(
                if ok {
                    format!(
                        "OK{}",
                        if repaired.is_empty() { String::new() } else { format!("; repaired:\n  {}", repaired.join("\n  ")) }
                    )
                } else {
                    format!("Problems:\n  {}", list("problems").join("\n  "))
                },
                &r,
            );
            if ok {
                0
            } else {
                1
            }
        }
        other => {
            eprintln!("Unknown command \"{other}\".\n{USAGE}");
            64
        }
    };
    let _ = json_call(&mut engine, "close", &[], None, false);
    engine.close().map_err(|e| e.to_string())?;
    Ok(code)
}
