//! Go strings carry bytes. Code remains exact in memory and Lua publication;
//! JSON serialization replaces each invalid UTF-8 byte, as encoding/json does.
#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Only executable source fields use this type. Protocol identity strings keep
/// their existing UTF-8/token contracts. This is still a JSON string on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeBytes(Vec<u8>);

impl CodeBytes {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn push_byte(&mut self, byte: u8) {
        self.0.push(byte);
    }
}
impl From<String> for CodeBytes {
    fn from(value: String) -> Self {
        Self(value.into_bytes())
    }
}
impl From<&str> for CodeBytes {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}
impl From<Vec<u8>> for CodeBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}
impl From<&[u8]> for CodeBytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}
impl AsRef<[u8]> for CodeBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}
impl Serialize for CodeBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Ok(text) = std::str::from_utf8(&self.0) {
            return serializer.serialize_str(text);
        }
        // Go's utf8.DecodeRuneInString consumes ONE byte for every invalid
        // encoding, including a truncated multibyte prefix. from_utf8_lossy
        // groups some prefixes and therefore cannot implement this boundary.
        let mut rest = self.0.as_slice();
        let mut json = String::with_capacity(rest.len() + 2);
        json.push('"');
        while !rest.is_empty() {
            let (valid, invalid) = match std::str::from_utf8(rest) {
                Ok(_) => (rest.len(), false),
                Err(error) => (error.valid_up_to(), true),
            };
            if valid != 0 {
                let prefix =
                    std::str::from_utf8(&rest[..valid]).map_err(serde::ser::Error::custom)?;
                let escaped = serde_json::to_string(prefix).map_err(serde::ser::Error::custom)?;
                json.push_str(&escaped[1..escaped.len() - 1]);
            }
            rest = &rest[valid..];
            if invalid {
                json.push_str("\\ufffd");
                rest = &rest[1..];
            }
        }
        json.push('"');
        let raw =
            serde_json::value::RawValue::from_string(json).map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for CodeBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}
