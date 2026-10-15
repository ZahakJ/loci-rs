//! The sync bundle byte format (docs/SYNC.md "Bundle"): a zip holding `sync.json` (header and
//! rows, deflated) and `media/<sha1>` entries (stored).

use super::clock::{Stamp, VersionVector};
use super::tables::Payload;
use crate::anki::zip::{create_zip, looks_like_zip, ZipArchive, ZipInput};
use crate::error::{LociError, Result};
use serde::{Deserialize, Serialize};

/// Bundle format version this build writes and reads.
pub const FORMAT: u32 = 1;
const DOC: &str = "sync.json";
const MEDIA_DIR: &str = "media/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    /// Collection id of the sender.
    pub collection: String,
    /// Device id of the sender.
    pub device: String,
    /// When it was written (ms).
    pub created: i64,
    /// What the sender had seen when it wrote the bundle.
    pub vector: VersionVector,
    /// Rows whose stamps this vector covers were left out; a reader that has not seen at least
    /// this much gets an incomplete picture.
    pub base: VersionVector,
    /// SHA-1s of media the sender has a row for but no file.
    #[serde(default)]
    pub wants: Vec<String>,
}

/// One object: `[kind, key, hlc, device, deleted, payload|null]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRow(pub String, pub String, pub i64, pub String, pub u8, pub Option<Payload>);

impl WireRow {
    pub fn stamp(&self) -> Stamp {
        Stamp::new(self.2, self.3.clone())
    }
    pub fn deleted(&self) -> bool {
        self.4 != 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Doc {
    #[serde(flatten)]
    header: Header,
    rows: Vec<WireRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    pub header: Header,
    pub rows: Vec<WireRow>,
    /// `(sha1, bytes)` of media files carried along.
    pub media: Vec<(String, Vec<u8>)>,
}

pub fn encode(b: &Bundle) -> Result<Vec<u8>> {
    let doc = Doc { header: b.header.clone(), rows: b.rows.clone() };
    let json = serde_json::to_vec(&doc)?;
    let mut inputs = vec![ZipInput::new(DOC, json, true)];
    for (sha1, bytes) in &b.media {
        inputs.push(ZipInput::new(format!("{MEDIA_DIR}{sha1}"), bytes.as_slice(), false));
    }
    create_zip(inputs)
}

pub fn decode(bytes: &[u8]) -> Result<Bundle> {
    let not_sync = || LociError::invalid("This is not a Loci sync file");
    if !looks_like_zip(bytes) {
        return Err(not_sync());
    }
    let zip = ZipArchive::open(bytes.to_vec()).map_err(|_| not_sync())?;
    if !zip.has(DOC) {
        return Err(not_sync());
    }
    let json = zip.read(DOC).map_err(|e| LociError::corrupt(format!("The sync file is damaged: {e}")))?;
    let format = serde_json::from_slice::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v.get("format").and_then(|f| f.as_u64()))
        .ok_or_else(|| LociError::corrupt("The sync file is damaged"))?;
    if format > FORMAT as u64 {
        return Err(LociError::invalid("This sync file was written by a newer version of Loci"));
    }
    let doc: Doc = serde_json::from_slice(&json).map_err(|e| LociError::corrupt(format!("The sync file is damaged: {e}")))?;
    let mut media = Vec::new();
    for name in zip.names() {
        if let Some(sha1) = name.strip_prefix(MEDIA_DIR) {
            if sha1.is_empty() {
                continue;
            }
            let data = zip.read(&name).map_err(|e| LociError::corrupt(format!("The sync file is damaged: {e}")))?;
            media.push((sha1.to_string(), data));
        }
    }
    Ok(Bundle { header: doc.header, rows: doc.rows, media })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_header_rows_and_media() {
        let b = Bundle {
            header: Header {
                format: FORMAT,
                collection: "c".into(),
                device: "d1".into(),
                created: 42,
                vector: [("d1".to_string(), 7i64 << 16)].into_iter().collect(),
                base: Default::default(),
                wants: vec!["ab".into()],
            },
            rows: vec![
                WireRow("notes".into(), "1".into(), 9 << 16, "d1".into(), 0, Some(vec![json!(1), json!("g"), json!("x\u{1f}y")])),
                WireRow("cards".into(), "2".into(), (9 << 16) + 1, "d1".into(), 1, None),
            ],
            media: vec![("deadbeef".into(), vec![1, 2, 3])],
        };
        let bytes = encode(&b).unwrap();
        assert_eq!(decode(&bytes).unwrap(), b);
        assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
    }

    #[test]
    fn rejects_other_files_and_newer_formats() {
        assert_eq!(decode(b"hello").unwrap_err().code, crate::error::ErrorCode::Invalid);
        let other = create_zip([ZipInput::new("x.txt", b"x".as_slice(), true)]).unwrap();
        assert_eq!(decode(&other).unwrap_err().code, crate::error::ErrorCode::Invalid);
        let newer = create_zip([ZipInput::new(DOC, br#"{"format":99}"#.as_slice(), true)]).unwrap();
        assert!(decode(&newer).unwrap_err().message.contains("newer"));
    }
}
