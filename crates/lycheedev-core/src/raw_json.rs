//! Byte-exact Go encoding/json RawMessage admission. Unlike ordinary typed JSON,
//! Go's scanner accepts arbitrary bytes >= 0x20 in a quoted string. Do not turn
//! native reports into UTF-8 strings or Values: that changes their stored bytes.
use serde::{Serialize, Serializer, ser::Error as _};
use std::fmt;

#[cfg(test)]
mod tests;

/// Go 1.27.1 encoding/json/scanner.go maxNestingDepth.
pub const MAX_DEPTH: usize = 10_000;
pub(crate) const MARKER: &str = "$lycheedev::public_json::RawJsonBytes";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidJson {
    pub offset: usize,
}
impl fmt::Display for InvalidJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Go raw JSON at byte {}", self.offset)
    }
}
impl std::error::Error for InvalidJson {}

/// Validated, immutable, borrowed raw JSON. The native report consumer imposes
/// its existing 512 KiB cap before construction. This validator adds no smaller
/// output limit, tree allocation, number conversion or report-sized copy.
///
/// Serialize this only with public_json's typed presentation writers. Ordinary
/// serde_json serialization fails closed rather than producing a byte array or
/// replacing invalid UTF-8. This is not a persistence format.
#[derive(Debug, Clone, Copy)]
pub struct RawJsonBytes<'a>(&'a [u8]);
impl<'a> RawJsonBytes<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, InvalidJson> {
        validate_at(bytes, 0)?;
        Ok(Self(bytes))
    }
    pub fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}
impl Serialize for RawJsonBytes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(MARKER, &Payload(self.0))
    }
}
struct Payload<'a>(&'a [u8]);
impl Serialize for Payload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Only the private bytes-only adapter declares this payload binary.
        // serde_json's ordinary human-readable serializer must reject it.
        if serializer.is_human_readable() {
            return Err(S::Error::custom(
                "raw JSON bytes require the typed public writer",
            ));
        }
        serializer.serialize_bytes(self.0)
    }
}

pub fn is_valid(bytes: &[u8]) -> bool {
    validate_at(bytes, 0).is_ok()
}
#[derive(Clone, Copy)]
enum Frame {
    ArrayFirst,
    ArrayValue,
    ArrayAfter,
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectAfter,
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}
/// Iterative grammar scanner; scratch is bounded by Go's depth limit. String
/// bytes and escaped surrogate code units are intentionally never decoded.
pub(crate) fn validate_at(bytes: &[u8], base: usize) -> Result<(), InvalidJson> {
    scan(bytes, base, false).map(|_| ())
}
/// The first complete JSON value, with Go byte/string/depth rules. Selection
/// files decode that value before classifying any following data. This shares
/// the validator and does not change full-document RawMessage admission.
pub(crate) fn first_value_end(bytes: &[u8]) -> Result<usize, InvalidJson> {
    scan(bytes, 0, true)
}
fn scan(bytes: &[u8], base: usize, first_only: bool) -> Result<usize, InvalidJson> {
    let mut at = 0;
    let mut frames = Vec::new();
    let mut root = false;
    loop {
        while bytes.get(at).is_some_and(|b| whitespace(*b)) {
            at += 1;
        }
        let frame = frames.last().copied();
        let byte = bytes.get(at).copied();
        match frame {
            None if root => {
                return if first_only || at == bytes.len() {
                    Ok(at)
                } else {
                    Err(InvalidJson { offset: at })
                };
            }
            Some(Frame::ArrayFirst) if byte == Some(b']') => {
                frames.pop();
                at += 1;
                continue;
            }
            Some(Frame::ObjectFirst) if byte == Some(b'}') => {
                frames.pop();
                at += 1;
                continue;
            }
            Some(Frame::ArrayAfter) => {
                match byte {
                    Some(b',') => *frames.last_mut().unwrap() = Frame::ArrayValue,
                    Some(b']') => {
                        frames.pop();
                    }
                    _ => return Err(InvalidJson { offset: at }),
                }
                at += 1;
                continue;
            }
            Some(Frame::ObjectAfter) => {
                match byte {
                    Some(b',') => *frames.last_mut().unwrap() = Frame::ObjectKey,
                    Some(b'}') => {
                        frames.pop();
                    }
                    _ => return Err(InvalidJson { offset: at }),
                }
                at += 1;
                continue;
            }
            Some(Frame::ObjectFirst | Frame::ObjectKey) => {
                string(bytes, &mut at)?;
                *frames.last_mut().unwrap() = Frame::ObjectColon;
                continue;
            }
            Some(Frame::ObjectColon) => {
                if byte != Some(b':') {
                    return Err(InvalidJson { offset: at });
                }
                at += 1;
                *frames.last_mut().unwrap() = Frame::ObjectValue;
                continue;
            }
            Some(Frame::ArrayFirst | Frame::ArrayValue) => {
                *frames.last_mut().unwrap() = Frame::ArrayAfter
            }
            Some(Frame::ObjectValue) => *frames.last_mut().unwrap() = Frame::ObjectAfter,
            None => root = true,
        }
        match byte {
            Some(b'[' | b'{') => {
                if frames.len() >= MAX_DEPTH.saturating_sub(base) {
                    return Err(InvalidJson { offset: at });
                }
                frames.push(if byte == Some(b'[') {
                    Frame::ArrayFirst
                } else {
                    Frame::ObjectFirst
                });
                at += 1;
            }
            Some(b'"') => string(bytes, &mut at)?,
            Some(b't') => literal(bytes, &mut at, b"true")?,
            Some(b'f') => literal(bytes, &mut at, b"false")?,
            Some(b'n') => literal(bytes, &mut at, b"null")?,
            Some(b'-' | b'0'..=b'9') => number(bytes, &mut at)?,
            _ => return Err(InvalidJson { offset: at }),
        }
    }
}
fn literal(bytes: &[u8], at: &mut usize, token: &[u8]) -> Result<(), InvalidJson> {
    if !bytes[*at..].starts_with(token) {
        return Err(InvalidJson { offset: *at });
    }
    *at += token.len();
    Ok(())
}
fn string(bytes: &[u8], at: &mut usize) -> Result<(), InvalidJson> {
    if bytes.get(*at) != Some(&b'"') {
        return Err(InvalidJson { offset: *at });
    }
    *at += 1;
    loop {
        match bytes.get(*at).copied() {
            Some(b'"') => {
                *at += 1;
                return Ok(());
            }
            Some(b'\\') => {
                *at += 1;
                match bytes.get(*at).copied() {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => *at += 1,
                    Some(b'u') => {
                        *at += 1;
                        for _ in 0..4 {
                            if !bytes.get(*at).is_some_and(u8::is_ascii_hexdigit) {
                                return Err(InvalidJson { offset: *at });
                            }
                            *at += 1;
                        }
                    }
                    _ => return Err(InvalidJson { offset: *at }),
                }
            }
            Some(0x20..=0xff) => *at += 1,
            _ => return Err(InvalidJson { offset: *at }),
        }
    }
}
fn number(bytes: &[u8], at: &mut usize) -> Result<(), InvalidJson> {
    if bytes.get(*at) == Some(&b'-') {
        *at += 1;
    }
    match bytes.get(*at) {
        Some(b'0') => *at += 1,
        Some(b'1'..=b'9') => digits(bytes, at),
        _ => return Err(InvalidJson { offset: *at }),
    }
    if bytes.get(*at) == Some(&b'.') {
        *at += 1;
        let first = *at;
        digits(bytes, at);
        if first == *at {
            return Err(InvalidJson { offset: *at });
        }
    }
    if matches!(bytes.get(*at), Some(b'e' | b'E')) {
        *at += 1;
        if matches!(bytes.get(*at), Some(b'+' | b'-')) {
            *at += 1;
        }
        let first = *at;
        digits(bytes, at);
        if first == *at {
            return Err(InvalidJson { offset: *at });
        }
    }
    Ok(())
}
fn digits(bytes: &[u8], at: &mut usize) {
    while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
        *at += 1;
    }
}
