//! Canonicalize only the text carried by serde's private RawValue escape hatch.
//! The owner otherwise retains the original frame. This local pass preserves
//! its former Value decode/encode without constructing or dropping a Value DOM.
use super::{response_validation, RESPONSE_BYTES};
use lycheedev_core::{Result, ToolkitError};
use serde::{de::{DeserializeSeed, MapAccess, Visitor}, Serialize};
use serde_json::value::RawValue;
use std::{collections::{btree_map, BTreeMap}, fmt, io::{self, Write}, ops::Range, sync::Arc};

const NUMBER_KEY: &str = "$serde_json::private::Number";
const RAW_KEY: &str = "$serde_json::private::RawValue";

#[derive(Clone, Copy)]
enum FirstKey { Ordinary, Number, Raw }

struct Fields {
    first: Option<FirstKey>,
    values: BTreeMap<String, Range<usize>>,
}
struct FieldsSeed { source_start: usize }
impl<'de> DeserializeSeed<'de> for FieldsSeed {
    type Value = Fields;
    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> std::result::Result<Fields, D::Error> {
        de.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for FieldsSeed {
    type Value = Fields;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("an object") }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Fields, M::Error> {
        let mut fields = Fields { first: None, values: BTreeMap::new() };
        // Only this one container is deserialized. Every child is a borrowed
        // RawValue, whose pinned scanner walks its grammar without recursion.
        while let Some((key, value)) = map.next_entry::<String, &'de RawValue>()? {
            if fields.first.is_none() {
                fields.first = Some(match key.as_str() {
                    NUMBER_KEY => FirstKey::Number,
                    RAW_KEY => FirstKey::Raw,
                    _ => FirstKey::Ordinary,
                });
            }
            let start = value.get().as_ptr() as usize - self.source_start;
            fields.values.insert(key, start..start + value.get().len());
        }
        Ok(fields)
    }
}

// None of these tasks owns another task. Embedded private strings get their
// own source buffer; byte ranges avoid self-references, and Arc drops are flat.
enum Task {
    Value { source: Arc<String>, range: Range<usize> },
    Array { source: Arc<String>, position: usize, end: usize, first: bool },
    Object { source: Arc<String>, fields: btree_map::IntoIter<String, Range<usize>>, first: bool },
}

fn white(source: &str, position: &mut usize) {
    while matches!(source.as_bytes().get(*position), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *position += 1;
    }
}
fn next_range(source: &str, position: &mut usize) -> Result<Range<usize>> {
    let mut values = serde_json::Deserializer::from_str(&source[*position..]).into_iter::<&RawValue>();
    let raw = values.next().ok_or_else(|| ToolkitError::invalid("EOF while parsing a value"))??;
    let start = raw.get().as_ptr() as usize - source.as_ptr() as usize;
    *position += values.byte_offset();
    Ok(start..start + raw.get().len())
}

fn frame_limit() -> ToolkitError {
    ToolkitError::new("source.native_frame_limit", "Source frame exceeds budget", 3)
}
#[derive(Default)]
struct Output { bytes: Vec<u8>, exceeded: bool }
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > RESPONSE_BYTES {
            self.exceeded = true;
            return Err(io::Error::other("Source frame exceeds budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
impl Output {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_all(bytes).map_err(|_| frame_limit())
    }
    fn scalar<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        match serde_json::to_writer(&mut *self, value) {
            Ok(()) => Ok(()),
            Err(_) if self.exceeded => Err(frame_limit()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Equal to serializing a freshly decoded Value, subject to the unchanged
/// response-byte cap. Value's accepted set (including overwritten duplicates,
/// private markers and each fresh embedded-text depth budget) is checked first.
/// This is deliberately not a generic replacement for the response serializer.
pub(super) fn canonical_value(text: String) -> Result<Vec<u8>> {
    response_validation::validate_value(text.as_bytes(), 0)?;
    let source = Arc::new(text);
    let mut position = 0;
    let range = next_range(&source, &mut position)?;
    let mut tasks = vec![Task::Value { source, range }];
    let mut output = Output::default();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Value { source, range } => match source.as_bytes()[range.start] {
                b'[' => {
                    output.append(b"[")?;
                    tasks.push(Task::Array { source, position: range.start + 1, end: range.end - 1, first: true });
                }
                b'{' => {
                    let mut de = serde_json::Deserializer::from_str(&source[range]);
                    let fields = FieldsSeed { source_start: source.as_ptr() as usize }.deserialize(&mut de)?;
                    de.end()?;
                    drop(de);
                    match fields.first {
                        Some(FirstKey::Raw) => {
                            let text: String = serde_json::from_str(&source[fields.values[RAW_KEY].clone()])?;
                            let source = Arc::new(text);
                            let mut position = 0;
                            let range = next_range(&source, &mut position)?;
                            tasks.push(Task::Value { source, range });
                        }
                        Some(FirstKey::Number) => {
                            let text: String = serde_json::from_str(&source[fields.values[NUMBER_KEY].clone()])?;
                            let number: serde_json::Number = text.parse()?;
                            output.scalar(&number)?;
                        }
                        Some(FirstKey::Ordinary) | None => {
                            output.append(b"{")?;
                            tasks.push(Task::Object { source, fields: fields.values.into_iter(), first: true });
                        }
                    }
                }
                b'"' => {
                    let value: String = serde_json::from_str(&source[range])?;
                    output.scalar(&value)?;
                }
                b'-' | b'0'..=b'9' => {
                    let value: serde_json::Number = serde_json::from_str(&source[range])?;
                    output.scalar(&value)?;
                }
                _ => output.append(source[range].as_bytes())?, // validated true, false or null
            },
            Task::Array { source, mut position, end, first } => {
                white(&source, &mut position);
                if position == end {
                    output.append(b"]")?;
                    continue;
                }
                if !first { output.append(b",")?; }
                let range = next_range(&source, &mut position)?;
                white(&source, &mut position);
                if source.as_bytes().get(position) == Some(&b',') { position += 1; }
                // One sibling at a time: an array never queues all its ranges.
                tasks.push(Task::Array { source: Arc::clone(&source), position, end, first: false });
                tasks.push(Task::Value { source, range });
            }
            Task::Object { source, mut fields, first } => {
                if let Some((key, range)) = fields.next() {
                    if !first { output.append(b",")?; }
                    output.scalar(&key)?;
                    output.append(b":")?;
                    tasks.push(Task::Object { source: Arc::clone(&source), fields, first: false });
                    tasks.push(Task::Value { source, range });
                } else {
                    output.append(b"}")?;
                }
            }
        }
    }
    Ok(output.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn agrees(text: &str) {
        let expected = serde_json::from_str::<Value>(text).map(|value| serde_json::to_vec(&value).unwrap());
        let actual = canonical_value(text.to_owned());
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual, expected, "{text}"),
            (Err(_), Err(actual)) => assert_eq!(actual.code, "input.invalid", "{text}"),
            (expected, actual) => panic!("canonical mismatch for {text}: {expected:?} != {actual:?}"),
        }
    }

    #[test]
    fn canonical_bytes_match_value_on_scalars_maps_duplicates_and_private_forms() {
        for text in [
            " null ", "true", "false", "0", "-0", "-0.0", "1.00", "1E+02", "1e999999",
            "184467440737095516160000", r#""界😀\n\t\uD83D\uDE00\/""#,
            "[]", "{}", r#" {"z":[{},true,null],"a":{"b":2,"a":1},"z":false} "#,
            r#"{"z":1,"\u007a":2,"a":3}"#,
            r#"{"$serde_json::private::Number":"123456789012345678901234567890"}"#,
            r#"{"$serde_json::private::Number":"-0.0"}"#,
            r#"{"\u0024serde_json::private::Number":"1E+02"}"#,
            r#"{"$serde_json::private::RawValue":"{\"z\":0,\"a\":1}"}"#,
            r#"{"$serde_json::private::RawValue":"{\"$serde_json::private::Number\":\"1e999999\"}"}"#,
            r#"{"a":[{"z":0,"a":1},{"z":2,"a":3}],"b":[[],[],{}]}"#,
            r#"{"ordinary":0,"$serde_json::private::Number":"1"}"#,
            r#"{"ordinary":0,"$serde_json::private::RawValue":"[1,]"}"#,
            r#"{"$serde_json::private::Number":"01"}"#,
            r#"{"$serde_json::private::RawValue":"[1,]"}"#,
            r#"{"$serde_json::private::RawValue":"null","extra":0}"#,
            r#"{"same":"\uD800","same":2}"#,
            r#"{"same":{"$serde_json::private::Number":"01"},"same":2}"#,
            r#"{"same":{"$serde_json::private::RawValue":"[1,]"},"same":2}"#,
            "[1,]", "null false", r#""\uDC00""#,
        ] { agrees(text); }
        let mut embedded = r#"{"z":0,"a":[1,{"same":1,"same":2}]}"#.to_owned();
        for _ in 0..8 {
            embedded = serde_json::to_string(&json!({ RAW_KEY: embedded })).unwrap();
            agrees(&embedded);
            agrees(&format!(r#"{{"z":{embedded},"a":[{embedded}]}}"#));
        }
    }

    #[test]
    fn raw_marker_counterexample_keeps_the_old_second_parse_failure() {
        let text = r#"{"z":0,"$serde_json::private::Number":"1"}"#;
        let canonical = canonical_value(text.to_owned()).unwrap();
        assert_eq!(canonical, br#"{"$serde_json::private::Number":"1","z":0}"#);
        assert!(serde_json::from_slice::<Value>(&canonical).is_err());
        let wrapped = serde_json::to_string(&json!({ RAW_KEY: text })).unwrap();
        agrees(&wrapped);
        assert_eq!(canonical_value(wrapped).unwrap(), canonical);
    }

    #[test]
    fn maximum_depth_and_wide_arrays_are_stack_safe_without_a_recursive_oracle() {
        std::thread::spawn(|| {
            for depth in [0usize, 1, 126, 127, 128, 4096] {
                for (open, close) in [("[", "]"), ("{\"child\":", "}")] {
                    let text = format!("{}null{}", open.repeat(depth), close.repeat(depth));
                    let actual = canonical_value(text.clone());
                    if depth < 128 { assert_eq!(actual.unwrap(), text.as_bytes()); }
                    else { assert_eq!(actual.unwrap_err().code, "input.invalid"); }
                }
            }
            let deep = format!("{}null{}", "[".repeat(127), "]".repeat(127));
            let wrapped = serde_json::to_string(&json!({ RAW_KEY: deep })).unwrap();
            let nested = format!("{}{}{}", "[".repeat(126), wrapped, "]".repeat(126));
            let expected = format!("{}null{}", "[".repeat(253), "]".repeat(253));
            assert_eq!(canonical_value(nested).unwrap(), expected.as_bytes());
            let wide = format!("[{}0]", "0,".repeat(100_000));
            assert_eq!(canonical_value(wide.clone()).unwrap(), wide.as_bytes());
        }).join().unwrap();
    }

    #[test]
    fn output_never_crosses_the_original_response_limit() {
        std::thread::spawn(|| {
            let exact = format!("\"{}\"", "x".repeat(RESPONSE_BYTES - 2));
            assert_eq!(canonical_value(exact).unwrap().len(), RESPONSE_BYTES);
            let oversized = format!("\"{}\"", "x".repeat(RESPONSE_BYTES - 1));
            assert_eq!(canonical_value(oversized).unwrap_err().code, "source.native_frame_limit");
        }).join().unwrap();
    }
}
