use super::*;
use flate2::{Compression, write::DeflateEncoder};
use std::io::Write;
const MODERN: &str = include_str!("fixtures/modern.txt");
const MODERN_V2: &str = include_str!("fixtures/modern-serialization-v2.txt");
const LEGACY: &str = include_str!("fixtures/legacy.txt");
const LEGACY_EXPLICIT: &str = include_str!("fixtures/legacy-explicit.txt");
fn expected() -> WaIdentity {
    WaIdentity {
        parse_wa_uid: "SYNTHETIC-uid-001".into(),
        parse_wa_id: "Synthetic WA 中文 ^ ~ 😀".into(),
    }
}
fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut output = DeflateEncoder::new(Vec::new(), Compression::default());
    output.write_all(bytes).unwrap();
    output.finish().unwrap()
}
fn encoded(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789()";
    let (mut cache, mut count) = (0u32, 0u32);
    let mut output = String::new();
    for byte in bytes {
        cache |= u32::from(*byte) << count;
        count += 8;
        while count >= 6 {
            output.push(ALPHABET[(cache & 63) as usize] as char);
            cache >>= 6;
            count -= 6;
        }
    }
    if count != 0 {
        output.push(ALPHABET[cache as usize] as char);
    }
    output
}
#[test]
fn dd_shipped_serializers_independently_generated_both_formats_and_all_selected_identity_bytes_match()
 {
    let control = OperationControl::default();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/provenance.json")).unwrap();
    for (index, fixture) in [MODERN, MODERN_V2, LEGACY, LEGACY_EXPLICIT]
        .iter()
        .enumerate()
    {
        assert_eq!(
            manifest["fixtures"][index]["sha256"],
            lycheedev_core::storage::sha256(fixture.as_bytes())
        );
        assert_eq!(parse(fixture, &control).unwrap(), expected());
        assert_eq!(
            parse(&format!("\r\n  {fixture}\t\r\n"), &control).unwrap(),
            expected()
        );
    }
    // An explicitly versioned old export and its original one-! form select
    // the same data codec; the original DD decoder also accepts version zero.
    assert_eq!(
        parse(&LEGACY_EXPLICIT.replacen("!WA:1!", "!WA:0!", 1), &control).unwrap(),
        expected()
    );
}
#[test]
fn modern_shared_and_recursive_tables_large_arrays_maps_and_opaque_actions_are_data_only() {
    let control = OperationControl::default();
    let compressed = decode_print(MODERN.strip_prefix("!WA:2!").unwrap(), &control).unwrap();
    let plain = inflate(&compressed, &control).unwrap();
    assert!(plain.len() > 131072);
    assert!(
        plain
            .windows(b"NEVER_EXECUTE_SYNTHETIC_WA_ACTION".len())
            .any(|bytes| bytes == b"NEVER_EXECUTE_SYNTHETIC_WA_ACTION")
    );
    // The d table is referenced from an earlier table and refers to itself;
    // identity extraction must not recursively traverse that cycle.
    assert_eq!(Serialized::parse(&plain, &control).unwrap(), expected());
    let mut trailing = compressed.clone();
    trailing.extend_from_slice(b"ignored-trailing-compressed-bytes");
    assert_eq!(
        parse(&format!("!WA:2!{}", encoded(&trailing)), &control).unwrap(),
        expected()
    );
    assert_eq!(
        parse(
            &format!("!WA:2!{}", encoded(&compressed[..compressed.len() - 2])),
            &control
        )
        .unwrap_err()
        .details["stage"],
        "deflate"
    );
}
#[test]
fn legacy_string_escapes_float_pairs_and_terminator_follow_the_data_format() {
    let control = OperationControl::default();
    assert_eq!(
        ace::parse(b"^1^T^Sd^T^Suid^S~@~z~{~|~}~a~^Sid^Slabel^t^t^^", &control)
            .unwrap()
            .parse_wa_uid
            .as_bytes(),
        &[0, 30, 127, 126, 94, 33, 126]
    );
    let plain = b" ^1extension ^T ^Sd ^T ^Suid ^SUID~}~| ^Sid ^Slabel~`~a ^t ^Snumber ^F4503599627370496 ^f-52 ^t ^^ return os.execute('NEVER_EXECUTE')";
    assert_eq!(
        ace::parse(plain, &control).unwrap(),
        WaIdentity {
            parse_wa_uid: "UID^~".into(),
            parse_wa_id: "label !".into()
        }
    );
    for bad in [
        b"^1^T^Sd^T^Suid^Z^t^t^^".as_slice(),
        b"^1^T^Sd^T^Suid^Su^Sid^Sl^t^t",
        b"^1^F1^Sbad^^",
        b"^2^^",
        b"^1^T^Sbad^S~!^t^^",
    ] {
        assert!(ace::parse(bad, &control).is_err());
    }
}
#[test]
fn malformed_references_lengths_nan_keys_and_deep_tables_fail_without_panicking_or_echoing_data() {
    let control = OperationControl::default();
    let mut cases = vec![
        vec![],
        vec![3, 0],
        vec![1, 208, 0],
        vec![1, 232, 0],
        vec![1, 112, 10, b'x'],
        vec![1, 22, 0, 0],
        vec![1, 176, 15, 66, 65],
    ];
    let mut nan = vec![1, 22, 72];
    nan.extend_from_slice(&f64::NAN.to_be_bytes());
    nan.push(0);
    cases.push(nan);
    let mut deep = vec![1];
    deep.extend(std::iter::repeat_n(26, 130));
    deep.push(0);
    cases.push(deep);
    for bytes in cases {
        let error = Serialized::parse(&bytes, &control).unwrap_err();
        assert_eq!(error.exit_code, 4);
        assert_eq!(error.details["stage"], "serialization");
    }
    // Missing d/uid/id and arbitrary code-looking strings remain ordinary
    // invalid data, not a reason to evaluate them in an interpreter.
    for bytes in [
        b"^1^Sreturn~`os.execute('PRIVATE_PAYLOAD')^^".as_slice(),
        b"^1^T^Sd^T^Suid^Ssecret^t^t^^",
    ] {
        let error = ace::parse(bytes, &control).unwrap_err();
        assert_eq!(error.details["stage"], "identity");
        assert!(!error.to_string().contains("PRIVATE_PAYLOAD"));
        assert!(!error.to_string().contains("secret"));
    }
}
#[test]
fn invalid_export_prefix_print_alphabet_and_compression_are_reported_at_the_actual_boundary() {
    let control = OperationControl::default();
    for value in [
        "",
        "ordinary text",
        "!WA:no!private",
        "!WA:3!private",
        "!WA:9999999999999999999999!private",
    ] {
        assert_eq!(
            parse(value, &control).unwrap_err().details["stage"],
            "prefix"
        );
    }
    for value in ["!a", "!WA:2!a=b", "!WA:2!abcd\nefgh", "!WA:2!秘密"] {
        let error = parse(value, &control).unwrap_err();
        assert_eq!(error.details["stage"], "encoding");
        assert!(!error.to_string().contains("秘密"));
    }
    assert_eq!(
        parse("!WA:2!aaaaaa", &control).unwrap_err().details["stage"],
        "deflate"
    );
}
#[test]
fn inflated_size_and_original_cancellation_deadline_bound_untrusted_exports() {
    let control = OperationControl::default();
    let bytes = compressed(&vec![b'x'; MAX_BYTES + 1]);
    assert!(bytes.len() < 65536);
    let error = parse(&format!("!WA:2!{}", encoded(&bytes)), &control).unwrap_err();
    assert_eq!(error.details["stage"], "deflate");
    assert!(error.message.contains("32 MiB"));
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(parse(MODERN, &cancelled).unwrap_err().exit_code, 7);
    let expired = OperationControl::uncancelable(std::time::Instant::now());
    assert_eq!(
        parse(MODERN, &expired).unwrap_err().code,
        "command.deadline"
    );
}
