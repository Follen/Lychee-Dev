use super::*;
#[test]
fn wire_byte_grammar_accepts_raw_non_utf8_and_surrogate_escapes() {
    for byte in 0x20..=0xff {
        if matches!(byte, b'"' | b'\\') {
            continue;
        }
        assert!(is_valid(&[b'"', byte, b'"']), "{byte:x}");
        assert!(
            is_valid(&[b'{', b'"', byte, b'"', b':', b'0', b'}']),
            "key {byte:x}"
        );
    }
    for bytes in [
        &b"\t\r\n { \"s\":\"\xff\xc0\x80\xed\xa0\x80\xf4\x90\x80\x80\" } "[..],
        br#"{"same":1,"same":1E+400,"surrogates":"\uD800\uDEAD\uFFFF\u0000"}"#,
        br#"[null,true,false,-0,1e400,9007199254740993123456789.0000,-1.25E-003]"#,
        br#""\"\\\/\b\f\n\r\t""#,
    ] {
        assert!(is_valid(bytes), "{bytes:?}");
    }
}
#[test]
fn wire_byte_grammar_rejects_controls_bad_structure_escapes_and_numbers() {
    for byte in 0..0x20 {
        assert!(!is_valid(&[b'"', byte, b'"']));
    }
    for bytes in [
        &b""[..],
        b" ",
        b"\xff",
        b"\xef\xbb\xbf{}",
        b"{}\xff",
        b"{}{}",
        b"true false",
        b"[",
        b"{",
        b"[1,]",
        b"[,1]",
        b"{,}",
        b"{\"a\",1}",
        b"{\"a\":}",
        b"{\"a\":1,}",
        b"{1:2}",
        b"[1}",
        b"{\"a\":[1]",
        b"[truefalse]",
        b"\"",
        b"\"\\\"",
        b"\"\\x20\"",
        b"\"\\u12\"",
        b"\"\\u123g\"",
        b"\"\\\xff\"",
        b"01",
        b"-01",
        b"+1",
        b"-",
        b"1.",
        b".1",
        b"1e",
        b"1e+",
        b"1e-",
        b"1E+-1",
        b"NaN",
        b"Infinity",
        b"true\x0c",
        b"null\x0b",
    ] {
        assert!(!is_valid(bytes), "{bytes:?}");
    }
}
#[test]
fn wire_byte_grammar_depth_is_iterative_and_exactly_10000() {
    let deep = [vec![b'['; MAX_DEPTH], b"0".to_vec(), vec![b']'; MAX_DEPTH]].concat();
    assert!(is_valid(&deep));
    assert!(validate_at(&deep, 1).is_err());
    assert!(!is_valid(&[b"[", deep.as_slice(), b"]"].concat()));
    let objects = [
        b"{\"x\":".repeat(MAX_DEPTH),
        b"null".to_vec(),
        vec![b'}'; MAX_DEPTH],
    ]
    .concat();
    assert!(is_valid(&objects));
    assert!(is_valid(b"\"[[[{{{\""));
}
#[test]
fn raw_byte_wrapper_is_borrowed_and_ordinary_json_fails_closed() {
    let bytes = b"{\"s\":\"\xff\"}";
    let raw = RawJsonBytes::new(bytes).unwrap();
    assert_eq!(raw.as_bytes().as_ptr(), bytes.as_ptr());
    assert!(serde_json::to_vec(&raw).is_err());
    assert!(RawJsonBytes::new(b"[] trailing").is_err());
}
