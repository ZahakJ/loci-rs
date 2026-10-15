//! Device-to-device sync without a server (docs/SYNC.md).
//!
//! Devices exchange bundles: every synced object with its stamp, and what the sender has seen.
//! Merging is per-object last-writer-wins, reviews are a union, deletions are tombstones, so any
//! set of bundles merged in any order gives the same collection. Nothing here touches files or
//! the network: shells move the bytes (a shared folder, or a file the user carries over).

pub mod bundle;
pub mod clock;
pub mod merge;
pub mod state;
pub mod tables;
#[cfg(test)]
mod tests;

use crate::backup::{backup_now, rotation::BackupFileKind};
use crate::collection::internals::meta;
use crate::collection::media::MEDIA_PREFIX;
use crate::collection::Collection;
use crate::error::{LociError, Result};
use crate::text::sha1_hex;
use bundle::{Bundle, Header, WireRow, FORMAT};
use clock::{covers, meet, Stamp, VersionVector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use tables::{Kind, Payload};

pub use state::schema_sql;

/// `syncStatus` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Sync has been used on this collection (it has a device id).
    pub enabled: bool,
    pub device_id: Option<String>,
    pub collection_id: String,
    /// The sync folder this device uses, if any (a path, or a label for a browser folder).
    pub folder: Option<String>,
    /// Seconds of the last merge or folder sync.
    pub last_sync: Option<i64>,
    pub peers: Vec<SyncPeer>,
    /// Ids this collection had before a restore; their files in a sync folder are stale.
    pub former_devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPeer {
    pub device_id: String,
    /// Seconds when this device last merged a bundle from it.
    pub last_seen: i64,
}

/// `exportSyncBundle` options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncExportOptions {
    /// For a sync folder: leave out what every listed device already has, and media files
    /// (they travel as separate files).
    pub folder: bool,
    /// Devices with a file in the folder.
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMediaRef {
    pub name: String,
    pub sha1: String,
}

/// `mergeSyncBundle` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub from_device: String,
    /// Objects the bundle changed here.
    pub rows_in: i64,
    /// Objects both devices had changed; the later change was kept.
    pub conflicts_resolved: i64,
    pub reviews_added: i64,
    /// Media files written from the bundle.
    pub media_added: i64,
    /// Media this collection has rows for but no file.
    pub media_needed: Vec<SyncMediaRef>,
    /// False when the sender left out things this device has not seen yet; the next round
    /// fills them in.
    pub complete: bool,
}

/// `syncMediaState` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncMediaState {
    pub have: Vec<SyncMediaRef>,
    pub need: Vec<SyncMediaRef>,
}

fn media_path(name: &str) -> String {
    format!("{MEDIA_PREFIX}{name}")
}

/// What a peer last told us: its vector, the media it lacks, when (seconds).
type PeerInfo = (VersionVector, Vec<String>, i64);

fn peers(db: &crate::db::Db) -> Result<HashMap<String, PeerInfo>> {
    let rows: Vec<(String, String, String, i64)> =
        db.all("SELECT dev, vector, wants, seen FROM sync_peers", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows
        .into_iter()
        .map(|(d, v, w, s)| (d, (serde_json::from_str(&v).unwrap_or_default(), serde_json::from_str(&w).unwrap_or_default(), s)))
        .collect())
}

impl Collection {
    /// Stamps what changed since the last call. The engine calls it after every write; errors
    /// are left for the next sync to surface (the change itself is safe either way).
    pub fn sync_after_write(&self) {
        if let Err(e) = state::stamp_pending(&self.core) {
            log::warn!("sync stamping failed: {e}");
        }
    }

    pub fn sync_status(&self) -> Result<SyncStatus> {
        let db = self.core.db();
        let device = state::device(db)?;
        let mut peers: Vec<SyncPeer> =
            peers(db)?.into_iter().map(|(device_id, (_, _, last_seen))| SyncPeer { device_id, last_seen }).collect();
        peers.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.device_id.cmp(&b.device_id)));
        Ok(SyncStatus {
            enabled: device.is_some(),
            device_id: device,
            collection_id: self.core.get_meta(meta::COLLECTION_ID)?.unwrap_or_default(),
            folder: state::get(db, state::key::FOLDER)?,
            last_sync: state::get(db, state::key::LAST_SYNC)?.and_then(|v| v.parse().ok()),
            peers,
            former_devices: state::get(db, state::key::FORMER)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default(),
        })
    }

    /// Remembers (or with None forgets) this device's sync folder; turns sync on.
    pub fn set_sync_folder(&self, folder: Option<&str>) -> Result<SyncStatus> {
        let db = self.core.db();
        state::enable(db)?;
        state::set(db, state::key::FOLDER, folder.filter(|f| !f.trim().is_empty()))?;
        self.sync_status()
    }

    /// Writes this device's bundle.
    pub fn export_sync_bundle(&self, options: &SyncExportOptions) -> Result<Vec<u8>> {
        let core = &self.core;
        let db = core.db();
        let device = state::enable(db)?;
        state::stamp_pending(core)?;
        let ctx = state::ctx(core)?;
        let known = peers(db)?;
        let base: VersionVector = if options.folder {
            let listed: Vec<&String> = options.peers.iter().filter(|p| **p != device).collect();
            if !listed.is_empty() && listed.iter().all(|p| known.contains_key(*p)) {
                meet(listed.iter().map(|p| &known[*p].0))
            } else {
                VersionVector::new()
            }
        } else {
            VersionVector::new()
        };
        let stamped: Vec<(String, String, i64, String, i64)> =
            db.all("SELECT tbl, key, hlc, dev, deleted FROM sync_rows ORDER BY tbl, key", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
        let mut rows = Vec::new();
        let mut cache: HashMap<Kind, HashMap<String, Payload>> = HashMap::new();
        for (t, key, hlc, dev, deleted) in stamped {
            let Some(kind) = Kind::from_name(&t) else { continue };
            let stamp = Stamp::new(hlc, dev);
            if covers(&base, &stamp) {
                continue;
            }
            let payload = if deleted != 0 {
                None
            } else {
                let p = if base.is_empty() {
                    if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(kind) {
                        e.insert(tables::read_all(db, kind, &ctx)?.into_iter().collect());
                    }
                    cache[&kind].get(&key).cloned()
                } else {
                    tables::read_one(db, kind, &key, &ctx)?
                };
                match p {
                    Some(p) => Some(p),
                    None => continue,
                }
            };
            rows.push(WireRow(t, key, stamp.hlc, stamp.dev, (deleted != 0) as u8, payload));
        }
        let media_rows: Vec<(String, String, i64, String)> = db.all(
            "SELECT m.name, m.sha1, s.hlc, s.dev FROM media m JOIN sync_rows s ON s.tbl = 'media' AND s.key = m.name ORDER BY m.name",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let mut wants = BTreeSet::new();
        let mut media = Vec::new();
        if options.folder {
            for (name, sha1, _, _) in &media_rows {
                if !core.blobs.exists(&media_path(name)) {
                    wants.insert(sha1.clone());
                }
            }
        } else {
            let all_peers = meet(known.values().map(|p| &p.0));
            let asked: BTreeSet<&String> = known.values().flat_map(|p| p.1.iter()).collect();
            let mut sent = BTreeSet::new();
            for (name, sha1, hlc, dev) in &media_rows {
                let path = media_path(name);
                if !core.blobs.exists(&path) {
                    wants.insert(sha1.clone());
                    continue;
                }
                let needed = known.is_empty() || !covers(&all_peers, &Stamp::new(*hlc, dev.clone())) || asked.contains(sha1);
                if needed && !sent.contains(sha1) {
                    if let Some(bytes) = core.blobs.get(&path)? {
                        sent.insert(sha1.clone());
                        media.push((sha1.clone(), bytes));
                    }
                }
            }
        }
        let header = Header {
            format: FORMAT,
            collection: core.get_meta(meta::COLLECTION_ID)?.unwrap_or_default(),
            device,
            created: core.now(),
            vector: state::vector(db)?,
            base,
            wants: wants.into_iter().collect(),
        };
        if options.folder {
            state::set(db, state::key::LAST_SYNC, Some(&ctx.now_s.to_string()))?;
        }
        bundle::encode(&Bundle { header, rows, media })
    }

    /// Merges another device's bundle. Writes an automatic backup first when anything will change.
    pub fn merge_sync_bundle(&mut self, bytes: &[u8]) -> Result<SyncReport> {
        let b = bundle::decode(bytes)?;
        let core = &self.core;
        state::enable(core.db())?;
        state::stamp_pending(core)?;
        let (steps, conflicts, complete) = merge::plan(core, &b)?;
        if !steps.is_empty() {
            backup_now(core, BackupFileKind::Auto)?;
        }
        let out = merge::apply(core, &b, &steps, complete)?;
        let media_added = self.store_bundle_media(&b.media)?;
        if out.rows_in > 0 {
            self.session_after_undo()?;
        }
        Ok(SyncReport {
            from_device: b.header.device.clone(),
            rows_in: out.rows_in,
            conflicts_resolved: conflicts,
            reviews_added: out.reviews_added,
            media_added,
            media_needed: self.sync_media_state()?.need,
            complete: out.complete,
        })
    }

    fn store_bundle_media(&self, media: &[(String, Vec<u8>)]) -> Result<i64> {
        let mut n = 0;
        for (sha1, bytes) in media {
            if sha1_hex(bytes) == *sha1 {
                n += self.add_sync_media(sha1, bytes)?;
            }
        }
        Ok(n)
    }

    /// Media rows with and without a file here.
    pub fn sync_media_state(&self) -> Result<SyncMediaState> {
        let rows: Vec<(String, String)> =
            self.core.db().all("SELECT name, sha1 FROM media ORDER BY name", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = SyncMediaState::default();
        for (name, sha1) in rows {
            let r = SyncMediaRef { name: name.clone(), sha1 };
            if self.core.blobs.exists(&media_path(&name)) {
                out.have.push(r);
            } else {
                out.need.push(r);
            }
        }
        Ok(out)
    }

    /// Stores a media file fetched from another device for every row that names its SHA-1 and
    /// has no file yet. Returns how many files were written.
    pub fn add_sync_media(&self, sha1: &str, bytes: &[u8]) -> Result<i64> {
        if sha1_hex(bytes) != sha1.to_ascii_lowercase() {
            return Err(LociError::media("The media file does not match its checksum"));
        }
        let names: Vec<String> = self.core.db().all("SELECT name FROM media WHERE sha1 = ?", [sha1], |r| r.get(0))?;
        let mut n = 0;
        for name in names {
            let path = media_path(&name);
            if !self.core.blobs.exists(&path) {
                self.core.blobs.put(&path, bytes)?;
                n += 1;
            }
        }
        Ok(n)
    }
}
