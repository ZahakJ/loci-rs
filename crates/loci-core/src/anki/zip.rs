//! Thin wrapper over the `zip` crate for Anki packages: list entries, read one entry, write a zip.
//!
//! Errors are plain [`ZipFormatError`]s; `read.rs` maps them to `LociError` (the `truncated` flag
//! decides between `corrupt` and `unsupported-package`).

use crate::error::{LociError, Result};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::sync::Mutex;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime};

/// Bytes are not a readable zip archive. `truncated` = it starts like a zip but cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ZipFormatError {
    pub message: String,
    pub truncated: bool,
}

impl ZipFormatError {
    fn new(message: impl Into<String>, truncated: bool) -> Self {
        ZipFormatError { message: message.into(), truncated }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntryInfo {
    pub name: String,
    /// Bytes after decompression (as recorded in the central directory).
    pub uncompressed_size: u64,
    /// Bytes stored in the archive.
    pub compressed_size: u64,
}

/// Local file header signature `PK\x03\x04` (or the empty-archive EOCD `PK\x05\x06`).
pub fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && bytes[0] == 0x50
        && bytes[1] == 0x4b
        && ((bytes[2] == 0x03 && bytes[3] == 0x04) || (bytes[2] == 0x05 && bytes[3] == 0x06))
}

/// A zip archive opened from memory. Entry bodies are inflated on demand by `read`.
pub struct ZipArchive {
    inner: Mutex<zip::ZipArchive<Cursor<Vec<u8>>>>,
    /// Entry names in central-directory order (directories skipped, first duplicate kept).
    names: Vec<String>,
    index: HashMap<String, (usize, ZipEntryInfo)>,
}

impl std::fmt::Debug for ZipArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipArchive").field("entries", &self.names.len()).finish()
    }
}

impl ZipArchive {
    pub fn open(bytes: Vec<u8>) -> std::result::Result<ZipArchive, ZipFormatError> {
        let truncated = looks_like_zip(&bytes);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ZipFormatError::new(format!("not a readable zip archive: {e}"), truncated))?;
        let mut names = Vec::new();
        let mut index = HashMap::new();
        for i in 0..archive.len() {
            let entry = match archive.by_index_raw(i) {
                Ok(e) => e,
                Err(e) => return Err(ZipFormatError::new(format!("not a readable zip archive: {e}"), truncated)),
            };
            if entry.is_dir() {
                continue;
            }
            let Ok(name) = entry.name() else { continue };
            let name = name.into_owned();
            if index.contains_key(&name) {
                continue; // keeps the first entry of a duplicated name
            }
            let info =
                ZipEntryInfo { name: name.clone(), uncompressed_size: entry.size(), compressed_size: entry.compressed_size() };
            names.push(name.clone());
            index.insert(name, (i, info));
        }
        Ok(ZipArchive { inner: Mutex::new(archive), names, index })
    }

    pub fn names(&self) -> Vec<String> {
        self.names.clone()
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    pub fn info(&self, name: &str) -> Option<&ZipEntryInfo> {
        self.index.get(name).map(|(_, info)| info)
    }

    /// Reads and inflates one entry (CRC checked). Fails with `truncated` when the body is damaged.
    pub fn read(&self, name: &str) -> std::result::Result<Vec<u8>, ZipFormatError> {
        let Some((i, info)) = self.index.get(name) else {
            return Err(ZipFormatError::new(format!("zip entry not found: {name}"), false));
        };
        let mut archive = self.inner.lock().map_err(|_| ZipFormatError::new("zip archive lock poisoned", true))?;
        let mut file =
            archive.by_index(*i).map_err(|e| ZipFormatError::new(format!("cannot read zip entry {name}: {e}"), true))?;
        let mut out = Vec::with_capacity(usize::try_from(info.uncompressed_size).unwrap_or(0));
        file.read_to_end(&mut out).map_err(|e| ZipFormatError::new(format!("cannot read zip entry {name}: {e}"), true))?;
        Ok(out)
    }
}

/// Entry body: bytes, or a thunk resolved just before the entry is written (keeps media out of
/// memory until needed).
pub enum ZipData<'a> {
    Bytes(Vec<u8>),
    Lazy(Box<dyn FnOnce() -> Result<Vec<u8>> + 'a>),
}

impl From<Vec<u8>> for ZipData<'_> {
    fn from(v: Vec<u8>) -> Self {
        ZipData::Bytes(v)
    }
}

impl From<&[u8]> for ZipData<'_> {
    fn from(v: &[u8]) -> Self {
        ZipData::Bytes(v.to_vec())
    }
}

pub struct ZipInput<'a> {
    pub name: String,
    pub data: ZipData<'a>,
    /// true = deflate; false = stored.
    pub compress: bool,
}

impl<'a> ZipInput<'a> {
    pub fn new(name: impl Into<String>, data: impl Into<ZipData<'a>>, compress: bool) -> ZipInput<'a> {
        ZipInput { name: name.into(), data: data.into(), compress }
    }
}

/// Writes entries in order into a new zip and returns its bytes. The `zip` crate switches to
/// Zip64 by itself when there are more than 65 535 entries or an entry exceeds 4 GiB. Timestamps
/// are fixed (2020-01-01) so the bytes are reproducible.
pub fn create_zip<'a>(inputs: impl IntoIterator<Item = ZipInput<'a>>) -> Result<Vec<u8>> {
    let stamp =
        DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0).map_err(|e| LociError::internal(format!("zip timestamp: {e:?}")))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for input in inputs {
        let bytes = match input.data {
            ZipData::Bytes(b) => b,
            ZipData::Lazy(f) => f()?,
        };
        let options = SimpleFileOptions::default()
            .compression_method(if input.compress { CompressionMethod::Deflated } else { CompressionMethod::Stored })
            .compression_level(if input.compress { Some(6) } else { None })
            .large_file(bytes.len() as u64 >= u32::MAX as u64)
            .last_modified_time(stamp);
        writer.start_file(input.name.as_str(), options).map_err(zip_io)?;
        writer.write_all(&bytes).map_err(|e| LociError::io(e.to_string()))?;
    }
    let cursor = writer.finish().map_err(zip_io)?;
    Ok(cursor.into_inner())
}

fn zip_io(e: zip::result::ZipError) -> LociError {
    LociError::io(format!("cannot write zip: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back_in_order() {
        let bytes = create_zip([
            ZipInput::new("meta", vec![8u8, 2], false),
            ZipInput::new("big", vec![7u8; 10_000], true),
            ZipInput::new("lazy", ZipData::Lazy(Box::new(|| Ok(b"later".to_vec()))), false),
        ])
        .unwrap();
        assert!(looks_like_zip(&bytes));
        let zip = ZipArchive::open(bytes.clone()).unwrap();
        assert_eq!(zip.names(), vec!["meta", "big", "lazy"]);
        assert_eq!(zip.read("meta").unwrap(), vec![8, 2]);
        assert_eq!(zip.read("big").unwrap().len(), 10_000);
        assert_eq!(zip.read("lazy").unwrap(), b"later");
        let info = zip.info("big").unwrap();
        assert_eq!(info.uncompressed_size, 10_000);
        assert!(info.compressed_size < 1_000);
        assert!(zip.info("lazy").unwrap().compressed_size == 5);
        assert!(!zip.has("nope"));
        assert!(!zip.read("nope").unwrap_err().truncated);
        // reproducible bytes
        let again = create_zip([ZipInput::new("meta", vec![8u8, 2], false)]).unwrap();
        let again2 = create_zip([ZipInput::new("meta", vec![8u8, 2], false)]).unwrap();
        assert_eq!(again, again2);
    }

    #[test]
    fn distinguishes_truncated_from_not_a_zip() {
        let not_zip = ZipArchive::open(b"hello world, not a zip at all".to_vec()).unwrap_err();
        assert!(!not_zip.truncated);
        assert!(!ZipArchive::open(Vec::new()).unwrap_err().truncated);
        let bytes = create_zip([ZipInput::new("a", vec![1u8; 5000], true)]).unwrap();
        let cut = bytes[..bytes.len() / 2].to_vec();
        let truncated = ZipArchive::open(cut).unwrap_err();
        assert!(truncated.truncated);
    }

    #[test]
    #[ignore = "writes 70 000 entries; run by hand to confirm Zip64 central directory"]
    fn more_than_65535_entries_use_zip64() {
        let inputs = (0..70_000).map(|i| ZipInput::new(i.to_string(), vec![1u8], false));
        let bytes = create_zip(inputs).unwrap();
        let zip = ZipArchive::open(bytes).unwrap();
        assert_eq!(zip.names().len(), 70_000);
        assert_eq!(zip.read("69999").unwrap(), vec![1]);
    }
}
