//! Validate a known Value member without building or recursively dropping a DOM.
//! This follows the pinned serde_json 1.0.151 Value accepted set, including its
//! first-key private Number/RawValue conventions. Unknown envelope members must
//! keep their separate IgnoredAny behavior; this validator is not for those.
//! Grammar errors come from the pinned scanner. Supplemental failures retain
//! input.invalid; malformed-subtree wording/precedence is not a serde emulator.
use lycheedev_core::{Result, ToolkitError};
use serde::{Deserialize, de::{DeserializeOwned, Visitor}};
use std::{borrow::Cow, fmt};

const DEPTH_LIMIT: usize = 128;
const NUMBER_KEY: &str = "$serde_json::private::Number";
const RAW_KEY: &str = "$serde_json::private::RawValue";

#[derive(Clone, Copy)]
enum Key { Ordinary, Number, Raw }
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("a string key") }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Key, E> {
                Ok(match value { NUMBER_KEY => Key::Number, RAW_KEY => Key::Raw, _ => Key::Ordinary })
            }
        }
        de.deserialize_str(KeyVisitor)
    }
}

// String validation uses serde's actual UTF-8 and surrogate handling, but an
// ordinary value/key does not need an owned copy. Only private scalar payloads
// do; no source-sized collection of sibling slices or decoded values is kept.
struct Text;
impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct TextVisitor;
        impl Visitor<'_> for TextVisitor {
            type Value = Text;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("a string") }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Text, E> { Ok(Text) }
        }
        de.deserialize_str(TextVisitor)
    }
}

#[derive(Clone, Copy)]
enum Container { Array, Object { first: bool, key: bool, start: usize } }
struct Document<'a> {
    raw: Cow<'a, [u8]>,
    position: usize,
    base_depth: usize,
    containers: Vec<Container>,
}
enum Progress { Continue, Embedded { text: String, range: std::ops::Range<usize> }, Finished }
impl<'a> Document<'a> {
    fn new(raw: Cow<'a, [u8]>, base_depth: usize) -> Result<Self> {
        // The pinned RawValue scanner checks the COMPLETE grammar iteratively,
        // including delimiters, number spelling, escapes and trailing bytes.
        // It does not impose Value's depth or Unicode/private-key semantics;
        // the small token walk below supplies only those missing checks.
        let _: &serde_json::value::RawValue = serde_json::from_slice(&raw)?;
        Ok(Self { raw, position: 0, base_depth, containers: Vec::new() })
    }
    fn white(&mut self) {
        while matches!(self.raw.get(self.position), Some(b' ' | b'\n' | b'\r' | b'\t')) { self.position += 1; }
    }
    fn peek(&self) -> Option<u8> { self.raw.get(self.position).copied() }
    fn scalar<T: DeserializeOwned>(&mut self) -> Result<T> {
        // Called only for a scalar token: neither serde nor its Drop can visit
        // an input container. Number's synthetic private map has fixed depth.
        let mut values = serde_json::Deserializer::from_slice(&self.raw[self.position..]).into_iter::<T>();
        let value = values.next().ok_or_else(|| ToolkitError::invalid("EOF while parsing a value"))??;
        self.position += values.byte_offset();
        Ok(value)
    }
    fn open(&mut self, container: Container) -> Result<()> {
        // serde decrements its 128 counter BEFORE entering a container and
        // rejects zero. Thus 127 total enclosing containers is the maximum.
        if self.base_depth + self.containers.len() >= DEPTH_LIMIT - 1 {
            return Err(ToolkitError::invalid("recursion limit exceeded"));
        }
        self.position += 1;
        self.containers.push(container);
        Ok(())
    }
    fn step(&mut self) -> Result<Progress> {
        self.white();
        match self.peek() {
            Some(b'[') => self.open(Container::Array)?,
            Some(b'{') => self.open(Container::Object { first: true, key: true, start: self.position })?,
            Some(b']' | b'}') => { self.containers.pop(); self.position += 1; }
            Some(b',') => {
                if let Some(Container::Object { first, key, .. }) = self.containers.last_mut() {
                    *first = false; *key = true;
                }
                self.position += 1;
            }
            Some(b':') => self.position += 1,
            Some(b'"') => {
                if let Some(Container::Object { first, key: true, start }) = self.containers.last().copied() {
                    let key = self.scalar::<Key>()?;
                    if let Some(Container::Object { key, .. }) = self.containers.last_mut() { *key = false; }
                    if first && matches!(key, Key::Number | Key::Raw) {
                        self.white();
                        self.position += 1; // the already validated colon
                        self.white();
                        if self.peek() != Some(b'"') {
                            return Err(ToolkitError::invalid("expected a string private value"));
                        }
                        let text = self.scalar::<String>()?;
                        self.white();
                        // Value's private-key visitor returns after one member.
                        if self.peek() != Some(b'}') { return Err(ToolkitError::invalid("trailing comma")); }
                        match key {
                            Key::Number => { let _: serde_json::Number = text.parse()?; }
                            Key::Raw => return Ok(Progress::Embedded { text, range: start..self.position+1 }),
                            Key::Ordinary => unreachable!(),
                        }
                    }
                } else {
                    self.scalar::<Text>()?;
                }
            }
            Some(b'-' | b'0'..=b'9') => { self.scalar::<serde_json::Number>()?; }
            Some(b't' | b'f') => { self.scalar::<bool>()?; }
            Some(b'n') => { self.scalar::<()>()?; }
            None => return Ok(Progress::Finished),
            _ => return Err(ToolkitError::invalid("expected value")),
        }
        Ok(Progress::Continue)
    }
}

/// `base_depth` counts the already-open envelope containers: Response.value is
/// one; ToolkitError.details inside Response.error is two. Private RawValue
/// strings start a fresh deserializer, exactly as pinned Value::deserialize does.
pub(super) fn validate_value(raw: &[u8], base_depth: usize) -> Result<Vec<std::ops::Range<usize>>> {
    if base_depth >= DEPTH_LIMIT { return Err(ToolkitError::invalid("recursion limit exceeded")); }
    let mut documents = vec![Document::new(Cow::Borrowed(raw), base_depth)?];
    let mut replacements = Vec::new();
    while let Some(document) = documents.last_mut() {
        match document.step()? {
            Progress::Continue => {}
            Progress::Embedded { text, range } => {
                // Only outer-document ranges are replaced. The local canonical
                // serializer handles any markers inside the decoded string.
                if documents.len() == 1 { replacements.push(range); }
                documents.push(Document::new(Cow::Owned(text.into_bytes()), 0)?);
            },
            Progress::Finished => { documents.pop(); }
        }
    }
    Ok(replacements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn agrees(raw: &[u8]) {
        let expected = serde_json::from_slice::<Value>(raw).is_ok();
        assert_eq!(validate_value(raw, 0).is_ok(), expected, "{raw:?}");
    }

    #[test]
    fn validation_matches_value_scalars_containers_unicode_and_duplicates() {
        for raw in [
            "null", "true", "false", "-0.0", "1e999999", "18446744073709551616000", "01", "1.", "1e", "NaN",
            "[]", "{}", "[1,true,null,{},[]]", "[1,]", "[1 2]", "{\"a\":1,}", "{\"a\" 1}", "{x:1}",
            "{\"a\":1,\"a\":2}", "{\"a\":\"\\uD800\",\"a\":2}", "{\"a\":2,\"a\":\"\\uD800\"}",
            "\"界😀\\r\\n\"", "\"\\uD83D\\uDE00\"", "\"\\uD800\"", "\"\\uDC00\"", "\"\\uD800x\"",
            "{\"\\uD800\":0}", "\"\\q\"", "\"unclosed", "[", "{", "", "null false", "null\u{000b}",
        ] { agrees(raw.as_bytes()); }
        agrees(b"\"\xff\""); agrees(b"{\"\xff\":null}"); agrees(b"[\"\xff\"]");
    }

    #[test]
    fn validation_preserves_first_private_number_and_raw_value_rules() {
        for raw in [
            r#"{"$serde_json::private::Number":"123456789012345678901234567890"}"#,
            r#"{"$serde_json::private::Number":"-0.0"}"#,
            r#"{"$serde_json::private::Number":"1e999999"}"#,
            r#"{"$serde_json::private::Number":" 1"}"#,
            r#"{"$serde_json::private::Number":"1 "}"#,
            r#"{"$serde_json::private::Number":1}"#,
            r#"{"$serde_json::private::Number":"1","extra":0}"#,
            r#"{"ordinary":0,"$serde_json::private::Number":1}"#,
            r#"{"$serde_json::private::RawValue":"[1,2]"}"#,
            r#"{"$serde_json::private::RawValue":"[1,]"}"#,
            r#"{"$serde_json::private::RawValue":"null false"}"#,
            r#"{"$serde_json::private::RawValue":null}"#,
            r#"{"$serde_json::private::RawValue":"0","extra":0}"#,
            r#"{"ordinary":0,"$serde_json::private::RawValue":false}"#,
            r#"{"\u0024serde_json::private::Number":"42"}"#,
        ] { agrees(raw.as_bytes()); }
        let mut embedded = "[1,{\"key\":true}]".to_owned();
        for _ in 0..8 {
            embedded = serde_json::to_string(&json!({RAW_KEY: embedded})).unwrap();
            agrees(embedded.as_bytes());
        }
    }

    #[test]
    fn validation_matches_nested_adversarial_value_forms_on_default_stack() {
        std::thread::spawn(|| {
            let forms = [
                "null", "-0.0", "1e999999", "\"\\uD83D\\uDE00\"", "\"\\uD800\"",
                "[]", "{}", "[1,]", "{\"x\":1,\"x\":2}",
                r#"{"$serde_json::private::Number":"123"}"#,
                r#"{"$serde_json::private::Number":"01"}"#,
                r#"{"$serde_json::private::RawValue":"[1,true]"}"#,
                r#"{"$serde_json::private::RawValue":"\"\\uD800\""}"#,
            ];
            for first in forms {
                for second in forms {
                    agrees(format!("[{first},{second}]").as_bytes());
                    agrees(format!("{{\"same\":{first},\"same\":{second}}}").as_bytes());
                    agrees(format!("{{\"ordinary\":{first},\"$serde_json::private::Number\":{second}}}").as_bytes());
                }
                let raw = serde_json::to_string(&json!({RAW_KEY:first})).unwrap();
                agrees(raw.as_bytes());
            }
        }).join().unwrap();
    }

    #[test]
    fn validation_keeps_original_depth_on_default_stack_without_recursive_oracle() {
        // The regression itself must not call the vulnerable recursive Value
        // decoder/drop at the maximum depth on this default-sized test thread.
        std::thread::spawn(|| {
            for base in [0usize, 1, 2] {
                for depth in [0usize, 125, 126, 127, 128, 4096] {
                    let raw = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
                    assert_eq!(validate_value(raw.as_bytes(), base).is_ok(), base + depth < DEPTH_LIMIT);
                    let objects = format!("{}null{}", "{\"child\":".repeat(depth), "}".repeat(depth));
                    assert_eq!(validate_value(objects.as_bytes(), base).is_ok(), base + depth < DEPTH_LIMIT);
                }
            }
            let nested = format!("{}null{}", "[".repeat(127), "]".repeat(127));
            let raw = serde_json::to_vec(&json!({RAW_KEY: nested})).unwrap();
            assert!(validate_value(&raw, 2).is_ok(), "embedded text starts a fresh depth counter");
            let rejected = format!("{}null{}", "[".repeat(128), "]".repeat(128));
            let raw = serde_json::to_vec(&json!({RAW_KEY: rejected})).unwrap();
            assert!(validate_value(&raw, 0).is_err());
        }).join().unwrap();
    }
}
