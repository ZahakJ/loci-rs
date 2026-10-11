//! Byte-level fixes applied to an Anki collection file before it is handed to SQLite
//! (research/anki-format.md §2.4). Port of recall `src/core/anki/sqlite-bytes.ts`.
//!
//! `read_package` only needs `fix_wal_header` (via `Db::open_from_bytes`): our connections register
//! Anki's `unicase` collation, so `strip_unicase_collation` is kept for engines that cannot
//! (the Tauri webview's sqlite-wasm, external tools) and for parity with the TypeScript build.

use crate::db::{is_sqlite_image, without_wal_header};
use crate::error::{LociError, Result};
use std::borrow::Cow;
use std::collections::HashSet;

/// True when `bytes` starts with the SQLite 3 file header.
pub fn is_sqlite_file(bytes: &[u8]) -> bool {
    is_sqlite_image(bytes)
}

/// Anki closes schema-18 files without switching back from WAL, so header bytes 18/19 (file format
/// write/read version) may be 2. An in-memory image with the WAL flag cannot be opened by
/// deserialize-based engines; set both to 1 (legacy rollback journal). Returns a patched copy, or
/// the input unchanged when no fix is needed.
pub fn fix_wal_header(bytes: &[u8]) -> Cow<'_, [u8]> {
    if !is_sqlite_file(bytes) {
        return Cow::Borrowed(bytes);
    }
    without_wal_header(bytes)
}

/// SQLite varint (big-endian, 7 bits per byte, 9th byte carries 8 bits). Returns (value, length).
fn sqlite_varint(b: &[u8], pos: usize) -> Result<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..8 {
        let x = *b.get(pos + i).ok_or_else(|| bad("varint past end of file"))?;
        v = (v << 7) | (x & 0x7f) as u64;
        if x & 0x80 == 0 {
            return Ok((v, i + 1));
        }
    }
    let last = *b.get(pos + 8).ok_or_else(|| bad("varint past end of file"))?;
    Ok(((v << 8) | last as u64, 9))
}

fn bad(what: &str) -> LociError {
    LociError::corrupt(format!("The collection database is damaged (invalid schema pages): {what}"))
}

fn be32(b: &[u8], at: usize) -> Result<usize> {
    let s = b.get(at..at + 4).ok_or_else(|| bad("pointer past end of file"))?;
    Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

const fn is_space(c: u8) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c | 0x0b)
}
const fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Removes `COLLATE unicase` from every schema statement stored in `sqlite_master`, by overwriting
/// the clause with spaces in place. Only pages of the sqlite_master b-tree (and their overflow
/// chains) are touched, so note content that happens to contain the words is never altered.
/// Returns a patched copy, or the input when nothing matched. Fails on a structurally invalid
/// b-tree.
pub fn strip_unicase_collation(bytes: &[u8]) -> Result<Cow<'_, [u8]>> {
    if !is_sqlite_file(bytes) {
        return Ok(Cow::Borrowed(bytes));
    }
    let raw_page_size = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
    let page_size = if raw_page_size == 1 { 65536 } else { raw_page_size };
    if page_size < 512 {
        return Err(bad("page size"));
    }
    let usable = page_size.checked_sub(bytes[20] as usize).ok_or_else(|| bad("reserved bytes"))?;
    let page_count = bytes.len() / page_size;
    let page_start = |n: usize| (n - 1) * page_size;
    // one entry per record: absolute byte offsets of its payload
    let mut positions: Vec<Vec<usize>> = Vec::new();

    let mut visited = HashSet::new();
    let mut stack = vec![1usize];
    while let Some(page) = stack.pop() {
        if page < 1 || page > page_count || !visited.insert(page) {
            return Err(bad(&format!("invalid sqlite_master page {page}")));
        }
        let base = page_start(page);
        let hdr = base + if page == 1 { 100 } else { 0 };
        let ptype = *bytes.get(hdr).ok_or_else(|| bad("page header past end"))?;
        let cells = u16::from_be_bytes([bytes[hdr + 3], bytes[hdr + 4]]) as usize;
        let is_leaf = ptype == 0x0d;
        if !is_leaf && ptype != 0x05 {
            return Err(bad(&format!("unexpected b-tree page type {ptype}")));
        }
        let ptrs = hdr + if is_leaf { 8 } else { 12 };
        for i in 0..cells {
            let off = ptrs + 2 * i;
            let cell = base + u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
            if !is_leaf {
                stack.push(be32(bytes, cell)?);
                continue;
            }
            let (payload, n1) = sqlite_varint(bytes, cell)?;
            let (_, n2) = sqlite_varint(bytes, cell + n1)?;
            let payload = payload as usize;
            let start = cell + n1 + n2;
            let max_local = usable - 35;
            let mut local = payload;
            if payload > max_local {
                let min_local = ((usable - 12) * 32) / 255 - 23;
                let k = min_local + (payload - min_local) % (usable - 4);
                local = if k <= max_local { k } else { min_local };
            }
            if start + local > bytes.len() {
                return Err(bad("cell payload past end of file"));
            }
            let mut pos: Vec<usize> = (start..start + local).collect();
            let mut remaining = payload - local;
            let mut ovf = if remaining > 0 { be32(bytes, start + local)? } else { 0 };
            let mut seen = HashSet::new();
            while remaining > 0 {
                if ovf < 1 || ovf > page_count || !seen.insert(ovf) {
                    return Err(bad("invalid overflow chain"));
                }
                let ob = page_start(ovf);
                let take = remaining.min(usable - 4);
                pos.extend(ob + 4..ob + 4 + take);
                remaining -= take;
                ovf = be32(bytes, ob)?;
            }
            positions.push(pos);
        }
        if !is_leaf {
            stack.push(be32(bytes, hdr + 8)?);
        }
    }

    let mut out: Option<Vec<u8>> = None;
    for pos in &positions {
        let n = pos.len();
        let mut i = 0;
        while i + 1 < n {
            let at = |j: usize, out: &Option<Vec<u8>>| out.as_ref().map_or(bytes, |v| v.as_slice())[pos[j]];
            if !is_space(at(i, &out)) {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < n && is_space(at(j, &out)) {
                j += 1;
            }
            let word = |from: usize, w: &[u8], out: &Option<Vec<u8>>| {
                from + w.len() <= n && w.iter().enumerate().all(|(k, c)| at(from + k, out).to_ascii_lowercase() == *c)
            };
            if !word(j, b"collate", &out) {
                i += 1;
                continue;
            }
            let mut k = j + 7;
            if k >= n || !is_space(at(k, &out)) {
                i += 1;
                continue;
            }
            while k < n && is_space(at(k, &out)) {
                k += 1;
            }
            if !word(k, b"unicase", &out) {
                i += 1;
                continue;
            }
            let end = k + 7;
            if end < n && is_ident_char(at(end, &out)) {
                i += 1;
                continue;
            }
            let buf = out.get_or_insert_with(|| bytes.to_vec());
            for &p in &pos[j..end] {
                buf[p] = 0x20;
            }
            i = end;
        }
    }
    Ok(match out {
        Some(v) => Cow::Owned(v),
        None => Cow::Borrowed(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn open_raw(bytes: &[u8]) -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.deserialize_read_exact(rusqlite::MAIN_DB, std::io::Cursor::new(bytes), bytes.len(), false).unwrap();
        conn
    }

    fn with_unicase_schema(page_size: u32, extra_cols: usize) -> Vec<u8> {
        let db = Db::memory().unwrap();
        db.execute_batch(&format!("PRAGMA page_size = {page_size}")).unwrap();
        let cols: Vec<String> = (0..extra_cols).map(|i| format!("c{i} text NOT NULL")).collect();
        let sep = if cols.is_empty() { "" } else { ", " };
        db.execute_batch(&format!("CREATE TABLE wide ({}{sep}last_col text NOT NULL COLLATE unicase)", cols.join(", "))).unwrap();
        db.execute_batch("CREATE TABLE t2 (name text COLLATE unicase, x text COLLATE  UNICASE)").unwrap();
        db.run("INSERT INTO t2 VALUES ('x COLLATE unicase y', 'z')", []).unwrap();
        db.serialize().unwrap()
    }

    #[test]
    fn wal_header_fix_sets_bytes_18_19_and_leaves_the_rest() {
        let mut bytes = with_unicase_schema(4096, 0);
        bytes[18] = 2;
        bytes[19] = 2;
        let fixed = fix_wal_header(&bytes);
        assert_eq!((fixed[18], fixed[19]), (1, 1));
        assert_eq!(fixed.len(), bytes.len());
        assert_eq!(fixed.iter().zip(bytes.iter()).filter(|(a, b)| a != b).count(), 2);
        assert_eq!(bytes[18], 2); // input not mutated
        assert!(matches!(fix_wal_header(&fixed), Cow::Borrowed(_)));
        let junk = vec![2u8; 200];
        assert!(!is_sqlite_file(&junk));
        assert!(matches!(fix_wal_header(&junk), Cow::Borrowed(_)));
    }

    #[test]
    fn removes_every_declaration_in_place_and_tables_become_readable() {
        let bytes = with_unicase_schema(4096, 0);
        {
            let raw = open_raw(&bytes);
            let sql: String = raw.query_row("SELECT group_concat(sql, ';') FROM sqlite_master", [], |r| r.get(0)).unwrap();
            assert_eq!(sql.to_lowercase().matches("collate").count(), 3);
            assert!(raw.prepare("SELECT * FROM t2 ORDER BY name").and_then(|mut s| s.query([]).map(|_| ())).is_err());
        }
        let stripped = strip_unicase_collation(&bytes).unwrap();
        assert!(matches!(stripped, Cow::Owned(_)));
        assert_eq!(stripped.len(), bytes.len());
        let raw = open_raw(&stripped);
        let sql: String = raw.query_row("SELECT group_concat(sql, ';') FROM sqlite_master", [], |r| r.get(0)).unwrap();
        assert!(!sql.to_lowercase().contains("unicase"));
        let n: i64 = raw.query_row("SELECT count(*) FROM t2 WHERE name > ''", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let check: String = raw.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap();
        assert_eq!(check, "ok");
        // note content that mentions the words is untouched
        let v: String = raw.query_row("SELECT name FROM t2", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "x COLLATE unicase y");
    }

    #[test]
    fn never_touches_content_only_databases() {
        let db = Db::memory().unwrap();
        db.execute_batch("CREATE TABLE t (a text); INSERT INTO t VALUES ('x COLLATE unicase y')").unwrap();
        let bytes = db.serialize().unwrap();
        assert!(matches!(strip_unicase_collation(&bytes).unwrap(), Cow::Borrowed(_)));
        assert!(matches!(strip_unicase_collation(b"junk").unwrap(), Cow::Borrowed(_)));
    }

    #[test]
    fn handles_schema_text_that_spills_onto_overflow_pages() {
        let bytes = with_unicase_schema(512, 60);
        let stripped = strip_unicase_collation(&bytes).unwrap();
        let raw = open_raw(&stripped);
        let sql: String = raw.query_row("SELECT sql FROM sqlite_master WHERE name = 'wide'", [], |r| r.get(0)).unwrap();
        assert!(sql.len() > 512);
        assert!(!sql.contains("unicase"));
        assert!(regex::Regex::new(r"last_col text NOT NULL\s+\)$").unwrap().is_match(&sql), "{sql}");
    }
}
