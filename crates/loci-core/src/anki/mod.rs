//! Anki package reading and writing (recall `src/core/anki/`).
//!
//! `read_package` turns `.apkg` / `.colpkg` bytes into an [`AnkiImage`] (Loci-typed notes, cards,
//! revlog, note types, decks, presets, media index); `write_legacy_package` turns an image back
//! into a LEGACY_2 `.apkg` that every Anki since 2.1.x imports. The `collection` module's importer
//! and exporter sit on top of these two functions.

pub mod deck_config;
pub mod image;
pub mod media_names;
pub mod nfc;
pub mod proto;
pub mod read;
pub mod sample_deck;
pub mod schema11;
pub mod sqlite_bytes;
pub mod write;
pub mod zip;

pub use deck_config::{dconf_to_deck_config, deck_config_proto_to_json, deck_config_to_dconf, FSRS6_DEFAULT_PARAMS};
pub use image::{AnkiCollectionConfig, AnkiDeck, AnkiDeckConfig, AnkiImage, AnkiMediaEntry, Json, MediaReader, EMPTY_DECK_TODAY};
pub use media_names::{
    add_hash_suffix, extract_media_references, extract_template_media_references, filename_is_safe, is_normalized_media_filename,
    is_remote_reference, normalize_media_filename, percent_decode, split_extension, truncate_filename, MAX_MEDIA_FILENAME_BYTES,
};
pub use read::{parse_card_data, read_package};
pub use sample_deck::{sample_card_count, sample_deck, sample_palace_image, SampleDeck, SampleNote, SampleNoteKind};
pub use schema11::SCHEMA11_DDL;
pub use sqlite_bytes::{fix_wal_header, is_sqlite_file, strip_unicase_collation};
pub use write::{
    build_dummy_collection, build_schema11_collection, notetype_to_json, serialize_card_data, write_legacy_package,
    WriteLegacyOptions, DUMMY_NOTE_TEXT, LEGACY2_META,
};
pub use zip::{create_zip, looks_like_zip, ZipArchive, ZipData, ZipEntryInfo, ZipFormatError, ZipInput};

#[cfg(test)]
mod tests_fixtures;
#[cfg(test)]
mod tests_media;
#[cfg(test)]
mod tests_write;
