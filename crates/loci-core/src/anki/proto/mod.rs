//! Minimal protobuf wire-format decoder plus typed decoders for the Anki messages Loci reads.
//!
//! Anki's `.proto` files are AGPL-3.0, so they are NOT vendored here. Messages are decoded by
//! field number; the numbers below are facts about the file format taken from
//! research/anki-format.md §1.1 and §2.3, which cite Anki at commit a13d8a6e:
//!   https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/import_export.proto
//!   https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/notetypes.proto
//!   https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/decks.proto
//!   https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/deck_config.proto
//!
//! Proto3 semantics: absent scalar = default (0 / false / ''), unknown fields are skipped, the last
//! occurrence of a non-repeated scalar wins, repeated scalars may be packed or unpacked.

use crate::error::LociError;

mod anki;
pub use anki::*;

// ───────────────────────── wire format ─────────────────────────

/// Wire types (0 varint, 1 fixed64, 2 length-delimited, 5 fixed32; 3/4 groups are unsupported).
pub const WIRE_VARINT: u8 = 0;
pub const WIRE_FIXED64: u8 = 1;
pub const WIRE_LENGTH_DELIMITED: u8 = 2;
pub const WIRE_FIXED32: u8 = 5;

/// One occurrence of a field on the wire. `Varint` holds the raw unsigned 64-bit value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireField<'a> {
    Varint { field: u32, value: u64 },
    Fixed64 { field: u32, bytes: &'a [u8] },
    LengthDelimited { field: u32, bytes: &'a [u8] },
    Fixed32 { field: u32, bytes: &'a [u8] },
}

impl WireField<'_> {
    pub fn field(&self) -> u32 {
        match self {
            WireField::Varint { field, .. }
            | WireField::Fixed64 { field, .. }
            | WireField::LengthDelimited { field, .. }
            | WireField::Fixed32 { field, .. } => *field,
        }
    }
    pub fn wire(&self) -> u8 {
        match self {
            WireField::Varint { .. } => WIRE_VARINT,
            WireField::Fixed64 { .. } => WIRE_FIXED64,
            WireField::LengthDelimited { .. } => WIRE_LENGTH_DELIMITED,
            WireField::Fixed32 { .. } => WIRE_FIXED32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ProtoDecodeError(pub String);

impl From<ProtoDecodeError> for LociError {
    fn from(e: ProtoDecodeError) -> Self {
        LociError::corrupt(format!("The collection database is damaged: {e}"))
    }
}

type PResult<T> = std::result::Result<T, ProtoDecodeError>;

/// Reads an unsigned LEB128 varint (up to 10 bytes) at `pos`. Returns (value, next position).
pub fn read_varint(buf: &[u8], mut pos: usize) -> PResult<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    for _ in 0..10 {
        let Some(&b) = buf.get(pos) else { return Err(ProtoDecodeError("truncated varint".into())) };
        pos += 1;
        if shift < 64 {
            result |= ((b & 0x7f) as u64).checked_shl(shift).unwrap_or(0);
        }
        if b & 0x80 == 0 {
            return Ok((result, pos));
        }
        shift += 7;
    }
    Err(ProtoDecodeError("varint longer than 10 bytes".into()))
}

/// Splits a message into its fields, in wire order. Groups (deprecated) are rejected.
pub fn decode_message(buf: &[u8]) -> PResult<Vec<WireField<'_>>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let (key, p1) = read_varint(buf, pos)?;
        pos = p1;
        let field = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        if field == 0 {
            return Err(ProtoDecodeError("field number 0".into()));
        }
        match wire {
            WIRE_VARINT => {
                let (v, p2) = read_varint(buf, pos)?;
                pos = p2;
                out.push(WireField::Varint { field, value: v });
            }
            WIRE_FIXED64 => {
                if pos + 8 > buf.len() {
                    return Err(ProtoDecodeError("truncated fixed64".into()));
                }
                out.push(WireField::Fixed64 { field, bytes: &buf[pos..pos + 8] });
                pos += 8;
            }
            WIRE_LENGTH_DELIMITED => {
                let (len, p2) = read_varint(buf, pos)?;
                let n = usize::try_from(len).unwrap_or(usize::MAX);
                if n > buf.len() || p2 + n > buf.len() {
                    return Err(ProtoDecodeError("truncated length-delimited field".into()));
                }
                out.push(WireField::LengthDelimited { field, bytes: &buf[p2..p2 + n] });
                pos = p2 + n;
            }
            WIRE_FIXED32 => {
                if pos + 4 > buf.len() {
                    return Err(ProtoDecodeError("truncated fixed32".into()));
                }
                out.push(WireField::Fixed32 { field, bytes: &buf[pos..pos + 4] });
                pos += 4;
            }
            _ => return Err(ProtoDecodeError(format!("unsupported wire type {wire}"))),
        }
    }
    Ok(out)
}

fn f32_at(b: &[u8]) -> f64 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64
}
fn f64_at(b: &[u8]) -> f64 {
    f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Field accessors over a decoded message, with proto3 defaults.
#[derive(Debug)]
pub struct ProtoMessage<'a> {
    pub fields: Vec<WireField<'a>>,
}

impl<'a> ProtoMessage<'a> {
    pub fn new(bytes: &'a [u8]) -> PResult<ProtoMessage<'a>> {
        Ok(ProtoMessage { fields: decode_message(bytes)? })
    }

    pub fn has(&self, field: u32) -> bool {
        self.fields.iter().any(|f| f.field() == field)
    }
    fn last(&self, field: u32) -> Option<&WireField<'a>> {
        self.fields.iter().rev().find(|f| f.field() == field)
    }
    fn all(&self, field: u32) -> impl Iterator<Item = &WireField<'a>> {
        self.fields.iter().filter(move |f| f.field() == field)
    }

    /// uint32 / uint64.
    pub fn uint(&self, field: u32) -> u64 {
        match self.last(field) {
            Some(WireField::Varint { value, .. }) => *value,
            _ => 0,
        }
    }
    /// int32 / int64 (two's complement, negative values use 10-byte varints).
    pub fn int(&self, field: u32) -> i64 {
        self.uint(field) as i64
    }
    /// sint32 / sint64 (zigzag).
    pub fn sint(&self, field: u32) -> i64 {
        match self.last(field) {
            Some(WireField::Varint { value: v, .. }) => ((v >> 1) as i64) ^ -((v & 1) as i64),
            _ => 0,
        }
    }
    pub fn bool(&self, field: u32) -> bool {
        self.uint(field) != 0
    }
    /// Enum value (int32 on the wire).
    pub fn enum_value(&self, field: u32) -> i64 {
        self.int(field)
    }
    /// `float` (fixed32), widened to f64 without rounding.
    pub fn float(&self, field: u32) -> f64 {
        match self.last(field) {
            Some(WireField::Fixed32 { bytes, .. }) => f32_at(bytes),
            _ => 0.0,
        }
    }
    /// `double` (fixed64).
    pub fn double(&self, field: u32) -> f64 {
        match self.last(field) {
            Some(WireField::Fixed64 { bytes, .. }) => f64_at(bytes),
            _ => 0.0,
        }
    }
    pub fn bytes(&self, field: u32) -> &'a [u8] {
        match self.last(field) {
            Some(WireField::LengthDelimited { bytes, .. }) => bytes,
            _ => &[],
        }
    }
    pub fn string(&self, field: u32) -> String {
        String::from_utf8_lossy(self.bytes(field)).into_owned()
    }
    /// Sub-message, or None when absent.
    pub fn message(&self, field: u32) -> PResult<Option<ProtoMessage<'a>>> {
        match self.last(field) {
            Some(WireField::LengthDelimited { bytes, .. }) => Ok(Some(ProtoMessage::new(bytes)?)),
            _ => Ok(None),
        }
    }
    pub fn repeated_messages(&self, field: u32) -> PResult<Vec<ProtoMessage<'a>>> {
        let mut out = Vec::new();
        for f in self.all(field) {
            if let WireField::LengthDelimited { bytes, .. } = f {
                out.push(ProtoMessage::new(bytes)?);
            }
        }
        Ok(out)
    }
    /// repeated float, packed or unpacked.
    pub fn repeated_float(&self, field: u32) -> PResult<Vec<f64>> {
        let mut out = Vec::new();
        for f in self.all(field) {
            match f {
                WireField::Fixed32 { bytes, .. } => out.push(f32_at(bytes)),
                WireField::LengthDelimited { bytes, .. } => {
                    if bytes.len() % 4 != 0 {
                        return Err(ProtoDecodeError("packed float length not a multiple of 4".into()));
                    }
                    for chunk in bytes.as_chunks::<4>().0 {
                        out.push(f32_at(chunk));
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
    /// repeated uint32/uint64, packed or unpacked.
    pub fn repeated_uint(&self, field: u32) -> PResult<Vec<u64>> {
        let mut out = Vec::new();
        for f in self.all(field) {
            match f {
                WireField::Varint { value, .. } => out.push(*value),
                WireField::LengthDelimited { bytes, .. } => {
                    let mut pos = 0;
                    while pos < bytes.len() {
                        let (v, p) = read_varint(bytes, pos)?;
                        out.push(v);
                        pos = p;
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
    /// Optional scalar presence helpers (proto3 `optional`).
    pub fn optional_uint(&self, field: u32) -> Option<u64> {
        self.has(field).then(|| self.uint(field))
    }
    pub fn optional_int(&self, field: u32) -> Option<i64> {
        self.has(field).then(|| self.int(field))
    }
    pub fn optional_float(&self, field: u32) -> Option<f64> {
        self.has(field).then(|| self.float(field))
    }
}

/// f32 values come back with binary noise (0.9 → 0.8999999761581421); round to 7 significant
/// digits like `Number(Math.fround(x).toPrecision(7))`.
pub fn round_f32(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    format!("{:.6e}", x as f32 as f64).parse::<f64>().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32le(x: f32) -> Vec<u8> {
        x.to_le_bytes().to_vec()
    }
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn reads_varints() {
        assert_eq!(read_varint(&[0x96, 0x01], 0).unwrap(), (150, 2));
        assert_eq!(read_varint(&[0x00], 0).unwrap(), (0, 1));
        assert_eq!(read_varint(&[0xff, 0xff, 0xff, 0xff, 0x0f], 0).unwrap(), (4294967295, 5));
    }

    #[test]
    fn splits_a_message_into_fields() {
        // field 1 varint 150, field 2 string "testing", field 5 fixed32, field 9 fixed64
        let bytes =
            cat(&[&[0x08, 0x96, 0x01, 0x12, 0x07], b"testing", &[0x2d], &f32le(2.5), &[0x49, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f]]);
        let fields = decode_message(&bytes).unwrap();
        let shape: Vec<(u32, u8)> = fields.iter().map(|f| (f.field(), f.wire())).collect();
        assert_eq!(shape, vec![(1, 0), (2, 2), (5, 5), (9, 1)]);
        let m = ProtoMessage::new(&bytes).unwrap();
        assert_eq!(m.uint(1), 150);
        assert_eq!(m.string(2), "testing");
        assert_eq!(m.float(5), 2.5);
        assert_eq!(m.double(9), 1.0);
        assert_eq!(m.uint(3), 0); // absent → proto3 default
        assert_eq!(m.string(4), "");
    }

    #[test]
    fn decodes_negative_int64_and_zigzag_sint32() {
        let neg = ProtoMessage::new(&[0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]).unwrap();
        assert_eq!(neg.int(1), -1);
        assert_eq!(ProtoMessage::new(&[0x08, 0x03]).unwrap().sint(1), -2);
        assert_eq!(ProtoMessage::new(&[0x08, 0x04]).unwrap().sint(1), 2);
    }

    #[test]
    fn reads_packed_and_unpacked_repeated_scalars_last_scalar_wins() {
        let packed = cat(&[&[0x0a, 0x08], &f32le(1.0), &f32le(10.0)]);
        assert_eq!(ProtoMessage::new(&packed).unwrap().repeated_float(1).unwrap(), vec![1.0, 10.0]);
        let unpacked = cat(&[&[0x0d], &f32le(1.5), &[0x0d], &f32le(3.0)]);
        assert_eq!(ProtoMessage::new(&unpacked).unwrap().repeated_float(1).unwrap(), vec![1.5, 3.0]);
        let uints = ProtoMessage::new(&[0x1a, 0x03, 0x00, 0x96, 0x01, 0x18, 0x05]).unwrap();
        assert_eq!(uints.repeated_uint(3).unwrap(), vec![0, 150, 5]);
        assert_eq!(ProtoMessage::new(&[0x08, 0x01, 0x08, 0x02]).unwrap().uint(1), 2);
    }

    #[test]
    fn skips_unknown_fields_and_rejects_malformed_input() {
        assert_eq!(ProtoMessage::new(&[0xa0, 0x1f, 0x01, 0x08, 0x07]).unwrap().uint(1), 7); // field 500, then field 1
        assert!(decode_message(&[0x08]).is_err());
        assert!(decode_message(&[0x12, 0x05, 0x61]).is_err());
        assert!(decode_message(&[0x0b]).is_err()); // group
        assert!(decode_message(&[0x00]).is_err()); // field 0
    }

    #[test]
    fn rounds_f32_noise() {
        assert_eq!(round_f32(0.9f32 as f64), 0.9);
        assert_eq!(round_f32(0.1542f32 as f64), 0.1542);
        assert_eq!(round_f32(0.0), 0.0);
        assert_eq!(round_f32(36500.0), 36500.0);
    }
}
