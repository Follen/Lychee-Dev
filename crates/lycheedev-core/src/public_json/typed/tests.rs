use super::*;
use serde::ser::{Error as _, SerializeMap, SerializeSeq, SerializeStruct};
use serde_json::value::RawValue;
use std::{cell::Cell, collections::BTreeMap};

fn render<T: ?Sized + Serialize>(value: &T, pretty: bool) -> String {
    let mut bytes = Vec::new();
    if pretty {
        write_pretty_serializable_line(&mut bytes, value).unwrap();
    } else {
        write_serializable_line(&mut bytes, value).unwrap();
    }
    String::from_utf8(bytes).unwrap()
}
fn raw(text: &str) -> &RawValue {
    serde_json::from_str(text).unwrap()
}
#[derive(Serialize)]
struct Envelope<'a, T> {
    schema: &'a str,
    result: T,
}
#[derive(Serialize)]
struct Report<'a> {
    report: &'a RawValue,
    typed: &'a str,
    after: bool,
}
fn report() -> Report<'static> {
    Report {
        report: raw(
            " { \"z\": 9007199254740993, \"z\": 1E+00, \"n\": -0, \"list\": [ { }, [ ], { \"x\": 1e400 } ], \"s\": \"<>&\u{2028}\u{2029}\\u003C\\/\" } \r\n",
        ),
        typed: "<>&\u{2028}\u{2029}",
        after: true,
    }
}
#[test]
fn typed_compact_keeps_duplicate_raw_fields_and_lexemes() {
    let value = Envelope {
        schema: "lycheedev.result.v1",
        result: report(),
    };
    assert_eq!(
        render(&value, false),
        concat!(
            "{\"schema\":\"lycheedev.result.v1\",\"result\":{\"report\":{",
            "\"z\":9007199254740993,\"z\":1E+00,\"n\":-0,",
            "\"list\":[{},[],{\"x\":1e400}],\"s\":\"<>&\u{2028}\u{2029}\\u003C\\/\"},",
            "\"typed\":\"<>&\\u2028\\u2029\",\"after\":true}}\n"
        )
    );
}
#[test]
fn typed_pretty_formats_raw_at_the_envelope_depth() {
    let value = Envelope {
        schema: "lycheedev.result.v1",
        result: report(),
    };
    assert_eq!(
        render(&value, true),
        concat!(
            "{\n  \"schema\": \"lycheedev.result.v1\",\n  \"result\": {\n",
            "    \"report\": {\n      \"z\": 9007199254740993,\n      \"z\": 1E+00,\n",
            "      \"n\": -0,\n      \"list\": [\n        {},\n        [],\n        {\n",
            "          \"x\": 1e400\n        }\n      ],\n",
            "      \"s\": \"<>&\u{2028}\u{2029}\\u003C\\/\"\n    },\n",
            "    \"typed\": \"<>&\\u2028\\u2029\",\n    \"after\": true\n  }\n}\n"
        )
    );
}
#[test]
fn raw_scalar_whitespace_and_string_escapes_are_not_reencoded() {
    for token in [
        "null",
        "true",
        "false",
        "-0",
        "1E-07",
        "1e400",
        "9007199254740993.125",
        r#""中文😀\\\"/,{}[]: \n\t\u0000\u003c\u2028\uD800""#,
    ] {
        let text = format!(" \r\n\t{token}\r\n ");
        for pretty in [false, true] {
            assert_eq!(render(raw(&text), pretty), format!("{token}\n"));
        }
    }
}
#[test]
fn typed_strings_keep_html_and_escape_separators_and_controls() {
    let text = "中文😀<>&\u{2028}\u{2029}\"\\\0\u{8}\u{c}\n\r\t";
    assert_eq!(
        render(text, false),
        "\"中文😀<>&\\u2028\\u2029\\\"\\\\\\u0000\\b\\f\\n\\r\\t\"\n"
    );
    let map = BTreeMap::from([("<>&\u{2028}", text)]);
    assert!(render(&map, false).starts_with("{\"<>&\\u2028\":"));
}
#[test]
fn typed_float_width_does_not_coerce_raw_or_unknown_number_tokens() {
    #[derive(Serialize)]
    struct Numbers<'a> {
        narrow: f32,
        wide: f64,
        integer: u64,
        raw: &'a RawValue,
        unknown: serde_json::Number,
    }
    let value = Numbers {
        narrow: 0.1f32,
        wide: f64::from(0.1f32),
        integer: u64::MAX,
        raw: raw("9007199254740993.125"),
        unknown: serde_json::from_str("1.00").unwrap(),
    };
    assert_eq!(
        render(&value, false),
        "{\"narrow\":0.1,\"wide\":0.10000000149011612,\"integer\":18446744073709551615,\"raw\":9007199254740993.125,\"unknown\":1.00}\n"
    );
}
#[test]
fn typed_float32_go_thresholds_and_negative_zero() {
    for (value, text) in [
        (0f32, "0"),
        (-0f32, "-0"),
        (1f32, "1"),
        (1e-6f32, "0.000001"),
        (1e-7f32, "1e-7"),
        (1e20f32, "100000000000000000000"),
        (1e21f32, "1e+21"),
        (f32::from_bits(1), "1e-45"),
        (f32::MAX, "3.4028235e+38"),
    ] {
        assert_eq!(render(&value, false), format!("{text}\n"));
    }
}
#[test]
fn typed_float64_go_thresholds_and_negative_zero() {
    for (value, text) in [
        (0f64, "0"),
        (-0f64, "-0"),
        (1f64, "1"),
        (1e-6f64, "0.000001"),
        (1e-7f64, "1e-7"),
        (1e20f64, "100000000000000000000"),
        (1e21f64, "1e+21"),
        (f64::from_bits(1), "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
    ] {
        assert_eq!(render(&value, false), format!("{text}\n"));
    }
}
#[test]
fn raw_arrays_objects_and_multiple_reports_share_formatter_state() {
    let rows = [
        raw(r#" [ [], {}, [false], {"a":[],"b":{}} ] "#),
        raw(r#" {"x":1,"x":2} "#),
    ];
    assert_eq!(
        render(&rows, true),
        concat!(
            "[\n  [\n    [],\n    {},\n    [\n      false\n    ],\n    {\n      \"a\": [],\n      \"b\": {}\n    }\n  ],\n",
            "  {\n    \"x\": 1,\n    \"x\": 2\n  }\n]\n"
        )
    );
}
#[test]
fn raw_depth_uses_go_limit_not_value_recursion_limit() {
    let deep = format!("{}0{}", "[".repeat(GO_MAX_DEPTH), "]".repeat(GO_MAX_DEPTH));
    assert_eq!(render(raw(&deep), false), format!("{deep}\n"));
    let deeper = format!("[{deep}]");
    let mut out = b"previous\n".to_vec();
    assert!(write_serializable_line(&mut out, raw(&deeper)).is_err());
    assert_eq!(out, b"previous\n");
    // Compact checks RawMessage independently, whereas Go's final pretty
    // appendIndent also counts the enclosing typed object/array depth.
    let nested = Envelope {
        schema: "s",
        result: raw(&deep),
    };
    assert!(write_serializable_line(&mut io::sink(), &nested).is_ok());
    assert!(write_pretty_serializable_line(&mut out, &nested).is_err());
    assert_eq!(out, b"previous\n");
    let valid = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    assert!(write_pretty_serializable_line(&mut io::sink(), raw(&valid)).is_ok());
}

struct NoWrite;
impl Write for NoWrite {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("preflight must fail before writing")
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn rejected<T: ?Sized + Serialize>(value: &T) {
    for pretty in [false, true] {
        let error = if pretty {
            write_pretty_serializable_line(&mut NoWrite, value).unwrap_err()
        } else {
            write_serializable_line(&mut NoWrite, value).unwrap_err()
        };
        assert_eq!(error.code, "input.invalid");
        assert_eq!(error.exit_code, 4);
    }
}
#[test]
fn nonfinite_floats_fail_preflight_at_all_nested_serde_shapes() {
    #[derive(Serialize)]
    struct Newtype(f64);
    #[derive(Serialize)]
    struct Tuple(f64, bool);
    #[derive(Serialize)]
    enum Variant {
        New(f64),
        Tuple(bool, f64),
        Struct { value: f64 },
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        rejected(&value);
        rejected(&Some(value));
        rejected(&Newtype(value));
        rejected(&(true, value));
        rejected(&Tuple(value, true));
        rejected(&vec![value]);
        rejected(&BTreeMap::from([("value", value)]));
        rejected(&Envelope {
            schema: "s",
            result: value,
        });
        rejected(&Variant::New(value));
        rejected(&Variant::Tuple(true, value));
        rejected(&Variant::Struct { value });
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        rejected(&value);
        rejected(&Some(vec![value]));
    }
    struct FloatKey;
    impl Serialize for FloatKey {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry(&f64::NAN, &true)?;
            map.end()
        }
    }
    rejected(&FloatKey);
}
#[test]
fn late_custom_serialization_and_invalid_map_keys_emit_no_record() {
    struct Fail;
    impl Serialize for Fail {
        fn serialize<S: serde::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            Err(S::Error::custom("late fixture failure"))
        }
    }
    rejected(&("x".repeat(2 << 20), Fail));
    rejected(&BTreeMap::from([((1, 2), true)]));
}
// An ordinary RawValue cannot contain malformed JSON, but a custom Serialize
// implementation can use the same marker. Do not trust that unchecked escape.
struct UncheckedRaw<'a>(&'a str);
impl Serialize for UncheckedRaw<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut raw = serializer.serialize_struct("$serde_json::private::RawValue", 1)?;
        raw.serialize_field("$serde_json::private::RawValue", self.0)?;
        raw.end()
    }
}
#[test]
fn malformed_raw_fragments_fail_preflight_without_any_output() {
    for text in [
        "",
        " ",
        "{",
        "[1,]",
        r#"{"a":1,}"#,
        "[NaN]",
        "1 2",
        "01",
        "1e",
        "false true",
        "\"unterminated",
        "\"line\nfeed\"",
        r#""\q""#,
        r#""\u00xz""#,
        "[}",
    ] {
        rejected(&Envelope {
            schema: "s",
            result: UncheckedRaw(text),
        });
    }
}
#[test]
fn borrowed_report_bytes_fail_preflight_when_invalid() {
    struct ReportBytes<'a>(&'a [u8]);
    impl Serialize for ReportBytes<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let raw: &RawValue = serde_json::from_slice(self.0).map_err(S::Error::custom)?;
            raw.serialize(serializer)
        }
    }
    rejected(&Envelope {
        schema: "s",
        result: ReportBytes(b"{\"broken\":}"),
    });
    let value = Envelope {
        schema: "s",
        result: ReportBytes(br#" {"x":9007199254740993,"x":1e0} "#),
    };
    assert_eq!(
        render(&value, false),
        "{\"schema\":\"s\",\"result\":{\"x\":9007199254740993,\"x\":1e0}}\n"
    );
}
#[test]
fn stable_serializers_are_called_twice_without_frame_size_cap() {
    struct Stable<'a> {
        calls: &'a Cell<usize>,
        raw: &'a RawValue,
    }
    impl Serialize for Stable<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            self.calls.set(self.calls.get() + 1);
            self.raw.serialize(serializer)
        }
    }
    struct Count {
        bytes: usize,
        writes: usize,
    }
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes += bytes.len();
            self.writes += 1;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    // Generic public output is not restricted to the Live 512KiB report or
    // journal 8MiB checkpoint limit. There is no second frame-sized buffer.
    let text = format!("{{\"text\":\"{}\",\"tail\":true}}", "x".repeat(9 << 20));
    let calls = Cell::new(0);
    let value = Stable {
        calls: &calls,
        raw: raw(&text),
    };
    let mut count = Count {
        bytes: 0,
        writes: 0,
    };
    write_serializable_line(&mut count, &value).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(count.bytes, text.len() + 1);
    assert!(count.writes > 4);
}
#[test]
fn actual_output_failures_preserve_partial_bytes_and_io_classification() {
    struct FailAfter {
        prefix: Vec<u8>,
        remaining: usize,
    }
    impl Write for FailAfter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
            }
            let n = bytes.len().min(self.remaining);
            self.prefix.extend_from_slice(&bytes[..n]);
            self.remaining -= n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for pretty in [false, true] {
        let value = report();
        let expected = render(&value, pretty);
        // Includes failure in a raw token, structure indentation, typed escape
        // and the final newline, not just a single synthetic first-write fault.
        for remaining in 0..expected.len() {
            let mut writer = FailAfter {
                prefix: Vec::new(),
                remaining,
            };
            let error = if pretty {
                write_pretty_serializable_line(&mut writer, &value).unwrap_err()
            } else {
                write_serializable_line(&mut writer, &value).unwrap_err()
            };
            assert_eq!(error.code, "external.io");
            assert_eq!(error.exit_code, 5);
            assert_eq!(writer.prefix, expected.as_bytes()[..remaining]);
        }
    }
}
#[test]
fn write_zero_is_io_and_interrupted_writes_are_retried() {
    struct Zero;
    impl Write for Zero {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        write_serializable_line(&mut Zero, &true).unwrap_err().code,
        "external.io"
    );
    struct Interrupted {
        first: bool,
        bytes: Vec<u8>,
    }
    impl Write for Interrupted {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Interrupted {
        first: true,
        bytes: Vec::new(),
    };
    write_serializable_line(&mut writer, &true).unwrap();
    assert_eq!(writer.bytes, b"true\n");
}
#[test]
fn unstable_second_pass_failure_is_not_misclassified_as_writer_io() {
    struct Changes(Cell<bool>);
    impl Serialize for Changes {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let changed = self.0.replace(true);
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&true)?;
            seq.serialize_element(&UncheckedRaw(if changed { "invalid" } else { "null" }))?;
            seq.end()
        }
    }
    let mut output = Vec::new();
    let error = write_serializable_line(&mut output, &Changes(Cell::new(false))).unwrap_err();
    assert_eq!(error.code, "input.invalid");
    assert_eq!(output, b"[true,"); // Documented stable-Serialize precondition.
}

#[test]
fn arbitrary_precision_marker_cannot_inject_non_number_json() {
    struct NumberToken<'a>(&'a str);
    impl Serialize for NumberToken<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut token = serializer.serialize_struct("$serde_json::private::Number", 1)?;
            token.serialize_field("$serde_json::private::Number", self.0)?;
            token.end()
        }
    }
    for token in ["", " 1", "1 ", "1,2", "{}", "null", "NaN", "01", "1e"] {
        rejected(&NumberToken(token));
    }
    for token in ["-0", "1E-07", "9007199254740993.125", "1e400"] {
        assert_eq!(render(&NumberToken(token), false), format!("{token}\n"));
    }
}
