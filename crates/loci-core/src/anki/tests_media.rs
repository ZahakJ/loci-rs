//! Media-index tests (recall `tests/unit/anki/media.test.ts`).

use super::proto::{decode_media_entries, MediaEntry};
use super::read::{read_package, zstd};
use super::tests_fixtures::{fixture, read_fixture, rezip, LAYOUTS};
use crate::error::ErrorCode;
use crate::text::sha1_hex;
use crate::types::PackageLayout;
use regex::Regex;
use serde_json::Value;

/// Names of the fixture generator's media files (recall `scripts/fixture-lib/collection.ts` `palaceMedia`).
const GENERATED: [&str; 19] = [
    "io-heart.png",
    "io-skeleton.png",
    "io-brain.png",
    "io-eye.png",
    "io-arm.png",
    "lens.png",
    "circuit.png",
    "incline.png",
    "atom.png",
    "ar-kitab.wav",
    "ar-bayt.wav",
    "ar-shams.wav",
    "ar-qamar.wav",
    "ar-dhakira.wav",
    "ar-salam.wav",
    "verb-kataba.wav",
    "verb-qaraa.wav",
    "verb-dhahaba.wav",
    "verb-jalasa.wav",
];

// ── a tiny protobuf encoder for the tests (the library only decodes) ──

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}
fn ld(field: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(((field as u64) << 3) | 2);
    out.extend(varint(bytes.len() as u64));
    out.extend_from_slice(bytes);
    out
}
fn vi(field: u32, v: u64) -> Vec<u8> {
    let mut out = varint((field as u64) << 3);
    out.extend(varint(v));
    out
}
pub(super) fn encode_media_entries(entries: &[MediaEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        let mut body = ld(1, e.name.as_bytes());
        body.extend(vi(2, e.size));
        body.extend(ld(3, &e.sha1));
        if let Some(l) = e.legacy_zip_filename {
            body.extend(vi(255, l));
        }
        out.extend(ld(1, &body));
    }
    out
}
fn zstd_compress(bytes: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest)
}

#[test]
fn test_encoder_round_trips_through_the_decoder() {
    let entries = vec![MediaEntry { name: "a.png".into(), size: 300, sha1: vec![7; 20], legacy_zip_filename: Some(2) }];
    assert_eq!(decode_media_entries(&encode_media_entries(&entries)).unwrap(), entries);
    assert_eq!(zstd(&zstd_compress(b"hello zstd"), "x").unwrap(), b"hello zstd");
}

#[test]
fn names_sizes_and_lazy_bodies_match_the_generator() {
    let mut generated: Vec<&str> = GENERATED.to_vec();
    generated.sort();
    for (file, layout) in LAYOUTS {
        let image = read_fixture(file);
        assert_eq!(image.media.keys().map(String::as_str).collect::<Vec<_>>(), generated, "{file}");
        for (name, entry) in &image.media {
            let body = image.read_media(name).unwrap();
            assert_eq!(entry.size as usize, body.len(), "{file}: {name}");
            assert!(!body.is_empty());
            if layout == PackageLayout::Latest {
                assert_eq!(entry.sha1.as_deref(), Some(sha1_hex(&body).as_str()), "{file}: {name}");
            } else {
                assert_eq!(entry.sha1, None);
            }
        }
        assert_eq!(image.read_media("nope.png").unwrap_err().code, ErrorCode::NotFound);
    }
}

#[test]
fn every_img_src_and_sound_reference_in_notes_resolves_to_a_media_entry() {
    let image = read_fixture("palace-legacy2.apkg");
    let img = Regex::new(r#"<img src="([^"]+)""#).unwrap();
    let sound = Regex::new(r"\[sound:([^\]]+)\]").unwrap();
    let mut refs = std::collections::HashSet::new();
    for n in &image.notes {
        for f in &n.fields {
            for m in img.captures_iter(f) {
                refs.insert(m[1].to_string());
            }
            for m in sound.captures_iter(f) {
                refs.insert(m[1].to_string());
            }
        }
    }
    assert_eq!(refs.len(), GENERATED.len());
    for r in &refs {
        assert!(image.media.contains_key(r), "{r}");
    }
}

fn edit_latest_media(edit: impl Fn(&mut Vec<MediaEntry>)) -> Vec<u8> {
    rezip(fixture("palace-latest.apkg"), |name, data| {
        if name != "media" {
            return Some(data);
        }
        let mut entries = decode_media_entries(&zstd(&data, "media").unwrap()).unwrap();
        edit(&mut entries);
        Some(zstd_compress(&encode_media_entries(&entries)))
    })
}

#[test]
fn latest_a_sha1_mismatch_is_detected_when_the_body_is_read() {
    let bytes = edit_latest_media(|entries| entries[0].sha1 = vec![0; 20]);
    let image = read_package(bytes).unwrap();
    let first = image.media.iter().find(|(_, e)| e.zip_entry == "0").map(|(n, _)| n.clone()).unwrap();
    assert_eq!(image.read_media(&first).unwrap_err().code, ErrorCode::Corrupt);
    let second = image.media.iter().find(|(_, e)| e.zip_entry == "1").map(|(n, _)| n.clone()).unwrap();
    assert!(!image.read_media(&second).unwrap().is_empty());
}

#[test]
fn latest_a_non_normalised_entry_name_makes_the_package_corrupt_like_anki() {
    let bytes = edit_latest_media(|entries| entries[0].name = "bad:name.png".into());
    assert_eq!(read_package(bytes).unwrap_err().code, ErrorCode::Corrupt);
}

fn edit_legacy_media_map(edit: impl Fn(&mut serde_json::Map<String, Value>)) -> Vec<u8> {
    rezip(fixture("palace-legacy2.apkg"), |name, data| {
        if name != "media" {
            return Some(data);
        }
        let Value::Object(mut map) = serde_json::from_slice::<Value>(&data).unwrap() else { panic!("media map") };
        edit(&mut map);
        Some(serde_json::to_vec(&Value::Object(map)).unwrap())
    })
}

#[test]
fn legacy_map_names_are_normalised_missing_bodies_and_duplicates_are_warned_about() {
    let bytes = edit_legacy_media_map(|map| {
        map.insert("0".into(), Value::from("cafe\u{301}:<1>.png")); // NFD + forbidden characters
        map.insert("999".into(), Value::from("missing.png"));
        let two = map["2"].clone();
        map.insert("1".into(), two); // duplicate name
    });
    let image = read_package(bytes).unwrap();
    assert!(image.media.contains_key("café1.png"));
    assert_eq!(image.media["café1.png"].zip_entry, "0");
    assert!(!image.media.contains_key("missing.png"));
    assert!(
        image.warnings.iter().any(|w| w.contains("1 media file(s) listed in the package are missing")),
        "{:?}",
        image.warnings
    );
    assert!(image.warnings.iter().any(|w| w.contains("duplicate names")));
}

#[test]
fn legacy_path_separators_are_removed_a_name_that_normalises_to_nothing_is_corrupt() {
    let traversal = rezip(fixture("palace-legacy2.apkg"), |name, data| {
        if name == "media" {
            Some(br#"{"0":"../../evil.png","1":".."}"#.to_vec())
        } else {
            Some(data)
        }
    });
    let image = read_package(traversal).unwrap();
    assert_eq!(image.media.keys().map(String::as_str).collect::<Vec<_>>(), ["....evil.png", ".._"]);
    let empty = rezip(
        fixture("palace-legacy2.apkg"),
        |name, data| if name == "media" { Some(br#"{"0":"/"}"#.to_vec()) } else { Some(data) },
    );
    assert_eq!(read_package(empty).unwrap_err().code, ErrorCode::Corrupt);
}

#[test]
fn a_package_without_a_media_entry_has_no_media() {
    let bytes = rezip(fixture("palace-legacy2.apkg"), |name, data| if name == "media" { None } else { Some(data) });
    let image = read_package(bytes).unwrap();
    assert!(image.media.is_empty());
    assert!(!image.notes.is_empty());
}
