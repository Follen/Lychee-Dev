//! Public result encoding, matching Go's Encoder.SetEscapeHTML(false).
//!
//! A serde_json Number no longer records whether it came from binary64,
//! json.Number or json.RawMessage. Unknown numbers keep their stored tokens.
//! This does not undo normalization already performed by an upstream parser.
//! Producers may supply a structural layout only while they still know the
//! numeric type. This encoder is not used for archives, cache keys or journals.
mod typed;
pub use typed::{write_pretty_serializable_line, write_serializable_line};

use crate::{Result, ToolkitError};
use serde::{
    Serialize,
    ser::{Error as _, SerializeMap, SerializeSeq},
};
use serde_json::{
    Value,
    ser::{CompactFormatter, Formatter, PrettyFormatter},
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

/// Private-to-the-host numeric provenance, never part of a public JSON schema.
/// Object keys are supplied by the producer's typed schema, not inferred from
/// a field name or number spelling. Array applies the same layout to each row.
#[derive(Clone, Debug, Default)]
pub enum NumberLayout {
    #[default]
    Verbatim,
    Binary64,
    /// Original json.Number token supplied before any Value conversion.
    Exact(JsonNumber),
    /// Every declared field is required unless explicitly Optional.
    Object(BTreeMap<String, NumberLayout>),
    /// Only a typed producer may declare a legitimately absent object field.
    Optional(Box<NumberLayout>),
    Array(Box<NumberLayout>),
    /// Sparse per-position provenance for heterogeneous tuples and rows.
    Indexed(BTreeMap<usize, NumberLayout>),
    /// Same sparse indexed contract, with immutable producer-owned sharing.
    /// Unlike Array, this retains the maximum marked index validation.
    SharedIndexed(std::sync::Arc<BTreeMap<usize, NumberLayout>>),
}
/// An explicitly supplied original JSON-number token. This cannot hold an
/// object, string, raw document or non-JSON NaN/Infinity spelling. The parsed
/// Number is retained only to reject stale provenance for a different value.
#[derive(Clone, Debug)]
pub struct JsonNumber {
    raw: Box<serde_json::value::RawValue>,
    parsed: serde_json::Number,
}
impl JsonNumber {
    /// Bound each explicit token by the existing maximum SQL request size.
    /// The caller must also account for both owned token copies and its layout
    /// nodes in the operation's aggregate budget. This is not a limit applied
    /// to unknown numbers by the public encoder.
    pub const MAX_BYTES: usize = 1 << 20;
    pub fn new(token: &str) -> Result<Self> {
        if token.len() > Self::MAX_BYTES {
            return Err(ToolkitError::invalid(
                "original JSON number exceeds token budget",
            ));
        }
        if token.trim() != token
            || !token
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(byte, b'-' | b'0'..=b'9'))
        {
            return Err(ToolkitError::invalid(
                "original JSON number token is invalid",
            ));
        }
        let parsed: serde_json::Number = serde_json::from_str(token)?;
        let raw = serde_json::value::RawValue::from_string(token.to_owned())?;
        Ok(Self { raw, parsed })
    }
    pub fn as_str(&self) -> &str {
        self.raw.get()
    }
}
impl NumberLayout {
    pub fn field(name: impl Into<String>, layout: Self) -> Self {
        Self::Object(BTreeMap::from([(name.into(), layout)]))
    }
    pub fn optional_field(name: impl Into<String>, layout: Self) -> Self {
        Self::field(name, Self::Optional(Box::new(layout)))
    }
    pub fn array(layout: Self) -> Self {
        Self::Array(Box::new(layout))
    }
}

/// Validate the complete immutable value/layout before touching stdout, then
/// serialize that same pair with the same formatter configuration directly to
/// the writer. Go Encode buffers to achieve the same no-partial-frame guarantee
/// for serialization errors; two deterministic passes avoid an additional
/// frame-sized allocation without inventing a smaller output limit. Output IO
/// errors may still leave a partial frame and retain external.io / exit 5.
/// The two-pass CPU cost remains subject to actual performance verification.
pub fn write_line(out: &mut impl Write, value: &Value, numbers: &NumberLayout) -> Result<()> {
    write_line_at(out, value, &[], numbers)
}
/// Apply an existing layout beneath required envelope fields by borrowing it.
/// No layout, Exact token or prefix key is copied. Prefixes are supplied by
/// the envelope assembler, never inferred from result keys or numeric tokens.
pub fn write_line_at(
    out: &mut impl Write,
    value: &Value,
    prefix: &[&str],
    numbers: &NumberLayout,
) -> Result<()> {
    encode_line(out, value, prefix, numbers, CompactFormatter)
}
pub fn write_pretty_line(
    out: &mut impl Write,
    value: &Value,
    numbers: &NumberLayout,
) -> Result<()> {
    encode_line(
        out,
        value,
        &[],
        numbers,
        PrettyFormatter::with_indent(b"  "),
    )
}
fn encode_line(
    out: &mut impl Write,
    value: &Value,
    prefix: &[&str],
    numbers: &NumberLayout,
    structure: impl Formatter + Clone,
) -> Result<()> {
    let value = PublicValue {
        value,
        prefix,
        numbers,
    };
    value.serialize(&mut serde_json::Serializer::with_formatter(
        io::sink(),
        PublicFormatter(structure.clone()),
    ))?;
    value
        .serialize(&mut serde_json::Serializer::with_formatter(
            &mut *out,
            PublicFormatter(structure),
        ))
        .map_err(|error| {
            if let Some(kind) = error.io_error_kind() {
                ToolkitError::io(io::Error::new(kind, error))
            } else {
                ToolkitError::from(error)
            }
        })?;
    out.write_all(b"\n")?;
    Ok(())
}

struct PublicValue<'a> {
    value: &'a Value,
    prefix: &'a [&'a str],
    numbers: &'a NumberLayout,
}
impl Serialize for PublicValue<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        if let Some((required, remainder)) = self.prefix.split_first() {
            let object = self.value.as_object().ok_or_else(|| {
                S::Error::custom("public number prefix differs from result shape")
            })?;
            if !object.contains_key(*required) {
                return Err(S::Error::custom("required public number prefix is absent"));
            }
            let mut map = serializer.serialize_map(Some(object.len()))?;
            for (key, value) in object {
                let selected = key.as_str() == *required;
                map.serialize_entry(
                    key,
                    &PublicValue {
                        value,
                        prefix: if selected { remainder } else { &[] },
                        numbers: if selected {
                            self.numbers
                        } else {
                            &NumberLayout::Verbatim
                        },
                    },
                )?;
            }
            return map.end();
        }
        match (self.numbers, self.value) {
            (NumberLayout::Verbatim, value) => value.serialize(serializer),
            (NumberLayout::Optional(inner), value) => PublicValue {
                value,
                prefix: &[],
                numbers: inner,
            }
            .serialize(serializer),
            (NumberLayout::Exact(original), Value::Number(number)) => {
                if number != &original.parsed {
                    return Err(S::Error::custom(
                        "original JSON number differs from result value",
                    ));
                }
                original.raw.serialize(serializer)
            }
            (NumberLayout::Binary64, Value::Number(number)) => {
                let token = number.as_str();
                // Integer tokens always keep their exact digits, including
                // u64::MAX and 2^53+1. Never round them via an f64 intermediary.
                if !token.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
                    return number.serialize(serializer);
                }
                let value = token
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| S::Error::custom("public binary64 number is not finite"))?;
                value.serialize(serializer)
            }
            (NumberLayout::Object(fields), Value::Object(object)) => {
                if fields.iter().any(|(key, layout)| {
                    !object.contains_key(key) && !matches!(layout, NumberLayout::Optional(_))
                }) {
                    return Err(S::Error::custom("required public number field is absent"));
                }
                let mut map = serializer.serialize_map(Some(object.len()))?;
                for (key, value) in object {
                    map.serialize_entry(
                        key,
                        &PublicValue {
                            value,
                            prefix: &[],
                            numbers: fields.get(key).unwrap_or(&NumberLayout::Verbatim),
                        },
                    )?;
                }
                map.end()
            }
            (NumberLayout::Indexed(elements), Value::Array(values)) => {
                serialize_indexed(elements, values, serializer)
            }
            (NumberLayout::SharedIndexed(elements), Value::Array(values)) => {
                serialize_indexed(elements, values, serializer)
            }
            (NumberLayout::Array(element), Value::Array(values)) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(&PublicValue {
                        value,
                        prefix: &[],
                        numbers: element,
                    })?;
                }
                seq.end()
            }
            _ => Err(S::Error::custom(
                "public number layout differs from result shape",
            )),
        }
    }
}

// Both ownership forms intentionally share every bound check and traversal.
fn serialize_indexed<S: serde::Serializer>(
    elements: &BTreeMap<usize, NumberLayout>,
    values: &[Value],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    if elements
        .keys()
        .next_back()
        .is_some_and(|index| *index >= values.len())
    {
        return Err(S::Error::custom(
            "public number layout index exceeds result length",
        ));
    }
    let mut seq = serializer.serialize_seq(Some(values.len()))?;
    for (index, value) in values.iter().enumerate() {
        seq.serialize_element(&PublicValue {
            value,
            prefix: &[],
            numbers: elements.get(&index).unwrap_or(&NumberLayout::Verbatim),
        })?;
    }
    seq.end()
}

// Keep serde's already-reviewed JSON structure/standard control escapes and
// override only the public Go contract. In particular, HTML remains literal;
// U+2028/U+2029 are escaped even when SetEscapeHTML is false.
struct PublicFormatter<F>(F);
impl<F: Formatter> Formatter for PublicFormatter<F> {
    fn write_f64<W: ?Sized + Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        if !value.is_finite() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "public binary64 number is not finite",
            ));
        }
        let absolute = value.abs();
        if absolute != 0.0 && !(1e-6..1e21).contains(&absolute) {
            let text = format!("{value:e}");
            let (mantissa, exponent) = text.split_once('e').expect("scientific float");
            writer.write_all(mantissa.as_bytes())?;
            writer.write_all(b"e")?;
            if !exponent.starts_with('-') {
                writer.write_all(b"+")?;
            }
            writer.write_all(exponent.as_bytes())
        } else {
            write!(writer, "{value}")
        }
    }
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (at, ch) in fragment.char_indices() {
            let escaped: &[u8] = match ch {
                '\u{2028}' => b"\\u2028",
                '\u{2029}' => b"\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..at])?;
            writer.write_all(escaped)?;
            start = at + ch.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn end_object_key<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_key(w)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_value(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn encoded(value: &Value, numbers: &NumberLayout) -> String {
        let mut out = Vec::new();
        write_line(&mut out, value, numbers).unwrap();
        String::from_utf8(out).unwrap()
    }
    #[test]
    fn typed_binary64_matches_go_digits_thresholds_and_negative_zero() {
        for (number, expected) in [
            (0.0, "0"),
            (1.0, "1"),
            (-0.0, "-0"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (f64::from(0.1f32), "0.10000000149011612"),
            (f64::from(1e-6f32), "9.999999974752427e-7"),
            (f64::from(1e20f32), "100000002004087730000"),
            (f64::from(f32::from_bits(1)), "1.401298464324817e-45"),
            (f64::from(f32::MAX), "3.4028234663852886e+38"),
            (f64::from_bits(1), "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
        ] {
            assert_eq!(
                encoded(&Value::from(number), &NumberLayout::Binary64),
                format!("{expected}\n")
            );
            // Parse the emitted token directly; serde's typed float parser has
            // no float_roundtrip feature and is not the oracle for these bits.
            assert_eq!(expected.parse::<f64>().unwrap().to_bits(), number.to_bits());
        }
    }
    #[test]
    fn integer_tokens_never_round_even_under_binary64_layout() {
        for token in [
            "18446744073709551615",
            "9007199254740993",
            "-9223372036854775808",
            "18446744073709551616",
        ] {
            let value: Value = serde_json::from_str(token).unwrap();
            for layout in [NumberLayout::Verbatim, NumberLayout::Binary64] {
                assert_eq!(encoded(&value, &layout), format!("{token}\n"));
            }
        }
    }
    #[test]
    fn unknown_number_lexemes_are_never_guessed_as_binary64() {
        for token in [
            "1.00",
            "1e-07",
            "1e+21",
            "9007199254740993.125",
            "1e+400",
            "-0.0",
        ] {
            let value: Value = serde_json::from_str(token).unwrap();
            assert_eq!(
                encoded(&value, &NumberLayout::Verbatim),
                format!("{token}\n")
            );
        }
    }
    #[test]
    fn nested_layout_changes_only_explicit_schema_fields() {
        let value: Value = serde_json::from_str(r#"{"rows":[{"f":[-0.0,1.0],"raw":9007199254740993.125},{"raw":1e400}],"elsewhere":-0.0}"#).unwrap();
        let layout = NumberLayout::field(
            "rows",
            NumberLayout::array(NumberLayout::optional_field(
                "f",
                NumberLayout::array(NumberLayout::Binary64),
            )),
        );
        assert_eq!(
            encoded(&value, &layout),
            "{\"elsewhere\":-0.0,\"rows\":[{\"f\":[-0,1],\"raw\":9007199254740993.125},{\"raw\":1e+400}]}\n"
        );
    }
    #[test]
    fn explicit_original_json_numbers_preserve_parser_normalized_spellings() {
        for token in ["-0", "1E-07", "1e7", "9007199254740993.125", "1e400"] {
            let original = JsonNumber::new(token).unwrap();
            assert_eq!(original.as_str(), token);
            let value: Value = serde_json::from_str(token).unwrap();
            assert_eq!(
                encoded(&value, &NumberLayout::Exact(original)),
                format!("{token}\n")
            );
        }
        for token in [
            "",
            " 1",
            "1 ",
            "01",
            "1x",
            "1 2",
            "1,null",
            "NaN",
            "Infinity",
            "null",
            "true",
            "{}",
            "[]",
            "\"1\"",
            r#"{"$serde_json::private::Number":"1"}"#,
        ] {
            assert!(JsonNumber::new(token).is_err(), "{token}");
        }
        let layout = NumberLayout::Exact(JsonNumber::new("-0").unwrap());
        let mut out = Vec::new();
        assert!(write_line(&mut out, &json!(1), &layout).is_err());
        assert!(out.is_empty());
    }
    #[test]
    fn explicit_original_number_budget_is_checked_before_copying() {
        let token = "1".repeat(JsonNumber::MAX_BYTES);
        assert_eq!(
            JsonNumber::new(&token).unwrap().as_str().len(),
            JsonNumber::MAX_BYTES
        );
        assert!(JsonNumber::new(&(token + "1")).is_err());
    }
    #[test]
    fn indexed_layout_preserves_heterogeneous_raw_cells() {
        let value: Value =
            serde_json::from_str(r#"[[9007199254740993.125,-0.0],[1.0,18446744073709551615]]"#)
                .unwrap();
        let numbers = NumberLayout::Indexed(BTreeMap::from([
            (
                0,
                NumberLayout::Indexed(BTreeMap::from([(1, NumberLayout::Binary64)])),
            ),
            (
                1,
                NumberLayout::Indexed(BTreeMap::from([(0, NumberLayout::Binary64)])),
            ),
        ]));
        assert_eq!(
            encoded(&value, &numbers),
            "[[9007199254740993.125,-0],[1,18446744073709551615]]\n"
        );
        let invalid = NumberLayout::Indexed(BTreeMap::from([(2, NumberLayout::Binary64)]));
        let mut out = Vec::new();
        assert!(write_line(&mut out, &value, &invalid).is_err());
        assert!(out.is_empty());
    }
    #[test]
    fn public_strings_keep_html_and_escape_javascript_separators() {
        let value = json!({"<>&\u{2028}":"中文😀<>&\u{2028}\u{2029}\"\\\0\u{8}\u{c}\n\r\t"});
        assert_eq!(
            encoded(&value, &NumberLayout::Verbatim),
            "{\"<>&\\u2028\":\"中文😀<>&\\u2028\\u2029\\\"\\\\\\u0000\\b\\f\\n\\r\\t\"}\n"
        );
    }
    #[test]
    fn pretty_structure_uses_same_numeric_and_escape_contract() {
        let value = json!({"empty":{},"f":[-0.0,1e21],"html":"<>&\u{2028}"});
        let numbers = NumberLayout::field("f", NumberLayout::array(NumberLayout::Binary64));
        let mut out = Vec::new();
        write_pretty_line(&mut out, &value, &numbers).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\n  \"empty\": {},\n  \"f\": [\n    -0,\n    1e+21\n  ],\n  \"html\": \"<>&\\u2028\"\n}\n"
        );
    }
    #[test]
    fn invalid_typed_numbers_or_stale_layout_emit_no_partial_frame() {
        for value in [
            serde_json::from_str::<Value>("1e400").unwrap(),
            json!("NaN"),
            Value::Null,
            json!([]),
        ] {
            let mut out = b"previous\n".to_vec();
            assert!(write_line(&mut out, &value, &NumberLayout::Binary64).is_err());
            assert_eq!(out, b"previous\n");
        }
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = Vec::new();
            assert!(
                PublicFormatter(CompactFormatter)
                    .write_f64(&mut bytes, number)
                    .is_err()
            );
            assert!(bytes.is_empty());
        }
    }
    #[test]
    fn required_object_fields_and_borrowed_prefixes_cannot_disappear() {
        let exact = NumberLayout::field("n", NumberLayout::Exact(JsonNumber::new("-0").unwrap()));
        for value in [json!({}), json!({"renamed":0})] {
            let mut out = Vec::new();
            assert!(write_line(&mut out, &value, &exact).is_err());
            assert!(out.is_empty());
        }
        for value in [
            json!({}),
            json!({"envelope":{}}),
            json!({"envelope":{"result":{"renamed":0}}}),
        ] {
            let mut out = Vec::new();
            assert!(write_line_at(&mut out, &value, &["envelope", "result"], &exact).is_err());
            assert!(out.is_empty());
        }
        let value = json!({"envelope":{"result":{"n":0}}, "other":-0.0});
        let mut out = Vec::new();
        write_line_at(&mut out, &value, &["envelope", "result"], &exact).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"envelope\":{\"result\":{\"n\":-0}},\"other\":-0.0}\n"
        );
    }
    #[test]
    fn optional_object_fields_allow_absence_but_validate_present_shape() {
        let numbers = NumberLayout::optional_field(
            "fields",
            NumberLayout::field("f", NumberLayout::Binary64),
        );
        assert_eq!(
            encoded(&json!({"removed":true}), &numbers),
            "{\"removed\":true}\n"
        );
        assert_eq!(
            encoded(&json!({"fields":{"f":-0.0}}), &numbers),
            "{\"fields\":{\"f\":-0}}\n"
        );
        for value in [
            json!({"fields":{}}),
            json!({"fields":null}),
            json!({"fields":{"f":null}}),
        ] {
            let mut out = Vec::new();
            assert!(write_line(&mut out, &value, &numbers).is_err());
            assert!(out.is_empty());
        }
    }
    #[test]
    fn late_validation_failure_never_touches_output_writer() {
        struct NoWrite;
        impl Write for NoWrite {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                panic!("validation must precede every output byte")
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let value = json!({"a": "x".repeat(2 << 20), "z": null});
        let numbers = NumberLayout::field("z", NumberLayout::Binary64);
        assert!(write_line(&mut NoWrite, &value, &numbers).is_err());
        assert!(write_pretty_line(&mut NoWrite, &value, &numbers).is_err());
    }
    #[test]
    fn direct_output_has_no_frame_cap_and_preserves_partial_io_failure() {
        struct Count {
            bytes: usize,
            calls: usize,
        }
        impl Write for Count {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes += bytes.len();
                self.calls += 1;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let value = json!({"text": "x".repeat(2 << 20)});
        let mut count = Count { bytes: 0, calls: 0 };
        write_line(&mut count, &value, &NumberLayout::Verbatim).unwrap();
        assert_eq!(count.bytes, (2 << 20) + "{\"text\":\"\"}\n".len());
        assert!(
            count.calls > 1,
            "do not buffer a full output frame in one allocation"
        );
        struct FailsAfter {
            prefix: Vec<u8>,
            left: usize,
        }
        impl Write for FailsAfter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.left == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed consumer"));
                }
                let n = bytes.len().min(self.left);
                self.prefix.extend_from_slice(&bytes[..n]);
                self.left -= n;
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let value = json!({"f":-0.0, "raw": "unchanged"});
        let numbers = NumberLayout::field("f", NumberLayout::Binary64);
        let expected = encoded(&value, &numbers);
        let mut writer = FailsAfter {
            prefix: Vec::new(),
            left: 17,
        };
        let error = write_line(&mut writer, &value, &numbers).unwrap_err();
        assert_eq!(error.code, "external.io");
        assert_eq!(error.exit_code, 5);
        assert_eq!(writer.prefix, expected.as_bytes()[..17]);
    }
    #[test]
    fn metadata_is_not_serialized_with_outcome_or_defaulted_into_unknown_numbers() {
        let mut outcome = crate::Outcome::new(json!({"f":-0.0}));
        outcome.public_numbers = NumberLayout::field("f", NumberLayout::Binary64);
        let value = serde_json::to_value(&outcome).unwrap();
        assert!(value.get("public_numbers").is_none());
        assert!(value.get("publicNumbers").is_none());
        let restored: crate::Outcome = serde_json::from_value(value).unwrap();
        assert!(matches!(restored.public_numbers, NumberLayout::Verbatim));
    }
}

#[cfg(test)]
#[path = "public_json/shared_indexed_tests.rs"]
mod shared_indexed_tests;
