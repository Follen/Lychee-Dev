//! New fixed-Go pin reader mappings: every test below is NOT_RUN. These are
//! staged pure decoder fixtures, not disk integration or Go differential runs.
use super::*;

fn source() -> selection::SourcePin {
    selection::SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: String::new(),
        exact_commit: "a".repeat(40),
        parser_revision: "fixture-v1".into(),
    }
}
fn data() -> selection::DataPin {
    selection::DataPin {
        product: "retail".into(),
        region: "us".into(),
        full_build: "12.1.0.12345".into(),
        build_config: "b".repeat(32),
        cdn_config: "c".repeat(32),
        language: "enUS".into(),
        definition_commit: "d".repeat(40),
    }
}
fn changes() -> ChangeWire {
    ChangeWire {
        provider: "fixture".into(),
        product: "retail".into(),
        full_build: "12.1.0.12345".into(),
        region: "us".into(),
        scope: "account".into(),
        fetched_at: super::super::parse_timestamp("2026-10-15T02:00:00.12Z").unwrap(),
        sha256: "e".repeat(64),
    }
}
fn fixture() -> PinWire {
    PinWire {
        schema: "lycheedev.selection.v1".into(),
        source: Some(source()),
        ..PinWire::default()
    }
}
fn seal(pin: &mut PinWire) -> Vec<u8> {
    pin.id = identity(pin).unwrap();
    storage::identity_json(pin).unwrap()
}
fn read(pin: &mut PinWire) -> Result<selection::PinnedSet> {
    let raw = seal(pin);
    read_pin(&raw, &pin.id, &OperationControl::default())
}
fn unsealed_reason(pin: &PinWire, id: &str) -> String {
    read_pin(
        &storage::identity_json(pin).unwrap(),
        id,
        &OperationControl::default(),
    )
    .unwrap_err()
    .message
}

#[test]
fn history_pin_typed_hash_schema_id_parent_and_default_fields() {
    let mut pin = fixture();
    pin.parent = "opaque parent is hashed but not resolved on read".into();
    let result = read(&mut pin).unwrap();
    assert_eq!(result.id, pin.id);
    assert_eq!(result.parent, pin.parent);
    assert_eq!(result.source.as_ref().unwrap().requested_ref, "");
    let unsigned = fixture();
    assert_eq!(
        String::from_utf8(storage::identity_json(&unsigned).unwrap()).unwrap(),
        format!(
            "{{\"schema\":\"lycheedev.selection.v1\",\"id\":\"\",\"source\":{{\"repository\":\"wow-ui-source\",\"product\":\"retail\",\"requestedRef\":\"\",\"exactCommit\":\"{}\",\"parserRevision\":\"fixture-v1\"}}}}",
            "a".repeat(40)
        )
    );
    pin.source.as_mut().unwrap().requested_ref = "modified".into();
    assert_eq!(unsealed_reason(&pin, &pin.id), "selection.pin_integrity");
    pin.schema = "future".into();
    assert_eq!(unsealed_reason(&pin, &pin.id), "selection.invalid_pin");
    pin.schema = "lycheedev.selection.v1".into();
    assert_eq!(unsealed_reason(&pin, "PIN-other"), "selection.invalid_pin");
}

#[test]
fn history_pin_invalid_id_precedes_empty_unresolved_and_hash_errors() {
    for id in [
        "",
        "PIN-x",
        "pin-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "PIN-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        let mut pin = PinWire {
            schema: "lycheedev.selection.v1".into(),
            id: id.into(),
            ..PinWire::default()
        };
        assert_eq!(unsealed_reason(&pin, id), "selection.invalid_pin");
        pin.source = Some(selection::SourcePin::default());
        assert_eq!(unsealed_reason(&pin, id), "selection.invalid_pin");
    }
    let mut pin = PinWire {
        schema: "lycheedev.selection.v1".into(),
        id: format!("PIN-{}", "f".repeat(64)),
        ..PinWire::default()
    };
    assert_eq!(unsealed_reason(&pin, &pin.id), "selection.empty");
    pin.source = Some(selection::SourcePin::default());
    assert_eq!(
        unsealed_reason(&pin, &pin.id),
        "selection.unresolved_source"
    );
}

#[test]
fn history_pin_case_fold_duplicate_object_merge_scalar_null_and_unknown() {
    let mut pin = fixture();
    let raw = String::from_utf8(seal(&mut pin)).unwrap();
    let noncanonical = format!(
        "{},\"SCHEMA\":null,\"SOURCE\":{{\"product\":null,\"requestedRef\":\"\"}},\"source\":{{}},\"future\":[{{\"n\":1}}]}}",
        raw.strip_suffix('}').unwrap()
    );
    let result = read_pin(
        noncanonical.as_bytes(),
        &pin.id,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(result.source.as_ref().unwrap().product, "retail");
    let folded = raw.replace("\"source\"", "\"ſource\"");
    assert!(read_pin(folded.as_bytes(), &pin.id, &OperationControl::default()).is_ok());
}

#[test]
fn history_pin_pointer_null_clears_and_later_object_allocates_fresh_defaults() {
    let mut pin = fixture();
    pin.data = Some(data());
    let raw = String::from_utf8(seal(&mut pin)).unwrap();
    let mut only_data = pin.clone();
    only_data.source = None;
    let _ = seal(&mut only_data);
    let cleared = format!(
        "{},\"source\":null,\"id\":\"{}\"}}",
        raw.strip_suffix('}').unwrap(),
        only_data.id
    );
    assert!(
        read_pin(
            cleared.as_bytes(),
            &only_data.id,
            &OperationControl::default()
        )
        .unwrap()
        .source
        .is_none()
    );
    let reset = format!(
        "{},\"source\":null,\"source\":{{\"product\":\"retail\"}}}}",
        raw.strip_suffix('}').unwrap()
    );
    assert_eq!(
        read_pin(reset.as_bytes(), &pin.id, &OperationControl::default())
            .unwrap_err()
            .message,
        "selection.unresolved_source"
    );
}

#[test]
fn history_pin_source_data_and_changes_require_exact_resolved_shapes() {
    for field in 0..4 {
        let mut pin = fixture();
        let s = pin.source.as_mut().unwrap();
        match field {
            0 => s.repository.clear(),
            1 => s.product.clear(),
            2 => s.parser_revision.clear(),
            _ => s.exact_commit = "A".repeat(40),
        }
        assert_eq!(
            read(&mut pin).unwrap_err().message,
            "selection.unresolved_source"
        );
    }
    for field in 0..7 {
        let mut pin = fixture();
        let mut d = data();
        match field {
            0 => d.product.clear(),
            1 => d.region.clear(),
            2 => d.language.clear(),
            3 => d.full_build = "12.1.0".into(),
            4 => d.build_config = "B".repeat(32),
            5 => d.cdn_config.clear(),
            _ => d.definition_commit = "x".repeat(40),
        }
        pin.data = Some(d);
        assert_eq!(
            read(&mut pin).unwrap_err().message,
            "selection.unresolved_data"
        );
    }
    for field in 0..7 {
        let mut pin = fixture();
        let mut c = changes();
        match field {
            0 => c.provider.clear(),
            1 => c.product.clear(),
            2 => c.region.clear(),
            3 => c.scope.clear(),
            4 => c.full_build = "12.1.0".into(),
            5 => c.fetched_at = Timestamp::default(),
            _ => c.sha256 = "E".repeat(64),
        }
        pin.changes = Some(c);
        assert_eq!(
            read(&mut pin).unwrap_err().message,
            "selection.unresolved_changes"
        );
    }
}

#[test]
fn history_pin_build_matches_go_ascii_uint32_parts_without_plus_sign() {
    for valid in ["0.0.0.0", "4294967295.00.000.1", "12.1.0.12345"] {
        assert!(build(valid), "{valid}");
        let mut pin = fixture();
        let mut d = data();
        d.full_build = valid.into();
        pin.data = Some(d);
        assert!(read(&mut pin).is_ok());
    }
    for invalid in [
        "+1.2.3.4",
        "1.+2.3.4",
        "1.2.3.+4",
        "1.2.3.-0",
        "1.2.3.4294967296",
        "1.2.3.4.5",
        "1..3.4",
        "１.2.3.4",
        "1.2.3. 4",
    ] {
        assert!(!build(invalid), "{invalid}");
        let mut pin = fixture();
        let mut d = data();
        d.full_build = invalid.into();
        pin.data = Some(d);
        assert_eq!(
            read(&mut pin).unwrap_err().message,
            "selection.unresolved_data"
        );
    }
}

#[test]
fn history_pin_timestamp_normalizes_go_fraction_offset_and_layout_before_hash() {
    let mut pin = fixture();
    pin.changes = Some(changes());
    let raw = String::from_utf8(seal(&mut pin)).unwrap();
    let legacy = raw.replace(
        "2026-10-15T02:00:00.12Z",
        "2026-10-15T2:00:00,120000000000+00:00",
    );
    let result = read_pin(legacy.as_bytes(), &pin.id, &OperationControl::default()).unwrap();
    assert_eq!(
        result.spec.changes.unwrap().fetched_at,
        "2026-10-15T02:00:00.12Z"
    );
    for malformed in [
        "2026-10-15 02:00:00Z",
        "2026-10-15T02:00:00z",
        "2026-10-15T02:00:00\\u005a",
        "2026-10-15T24:00:00Z",
    ] {
        let raw = raw.replace("2026-10-15T02:00:00.12Z", malformed);
        assert!(
            read_pin(raw.as_bytes(), &pin.id, &OperationControl::default()).is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn history_pin_timestamp_is_zero_uses_instant_including_large_offsets() {
    for zero in [
        "0001-01-01T00:00:00Z",
        "0001-01-01T01:00:00+01:00",
        "0000-12-31T23:00:00-01:00",
        "0001-01-02T00:00:00+24:00",
        "0001-01-02T01:00:00+24:60",
    ] {
        let timestamp = super::super::parse_timestamp(zero).unwrap();
        assert!(timestamp.is_zero(), "{zero}");
        let mut pin = fixture();
        let mut c = changes();
        c.fetched_at = timestamp;
        pin.changes = Some(c);
        // Do not marshal the +/-24h value before the predicate being tested:
        // Go validatePins rejects zero before attempting the identity marshal.
        assert_eq!(
            validate(&pin).unwrap_err().message,
            "selection.unresolved_changes"
        );
    }
    for nonzero in [
        "0000-12-31T23:00:00Z",
        "0001-01-01T00:00:00.000000001Z",
        "0001-01-01T00:00:00+01:00",
        "2026-10-15T00:00:00+24:00",
    ] {
        let timestamp = super::super::parse_timestamp(nonzero).unwrap();
        assert!(!timestamp.is_zero(), "{nonzero}");
    }
    let mut pin = fixture();
    pin.changes = Some(changes());
    let raw = String::from_utf8(seal(&mut pin)).unwrap();
    let large_offset = raw.replace("2026-10-15T02:00:00.12Z", "2026-10-15T02:00:00+24:00");
    assert!(
        read_pin(
            large_offset.as_bytes(),
            &pin.id,
            &OperationControl::default()
        )
        .unwrap_err()
        .message
        .contains("timezone hour outside of range")
    );
}

#[test]
fn history_pin_products_allow_only_exact_or_wow_source_family_compatibility() {
    let mut pin = fixture();
    pin.data = Some(data());
    pin.changes = Some(changes());
    pin.changes.as_mut().unwrap().product = "classic".into();
    assert_eq!(
        read(&mut pin).unwrap_err().message,
        "selection.product_mismatch: retail / classic"
    );
    pin.changes = None;
    pin.source.as_mut().unwrap().product = "classic".into();
    assert_eq!(
        read(&mut pin).unwrap_err().message,
        "selection.product_mismatch: classic / retail"
    );
    pin.source.as_mut().unwrap().product = "forever".into();
    pin.data.as_mut().unwrap().product = "forever-asia-beta".into();
    assert!(read(&mut pin).is_ok());
    pin.source.as_mut().unwrap().repository = "other-repository".into();
    assert_eq!(
        read(&mut pin).unwrap_err().message,
        "selection.product_mismatch: forever / forever-asia-beta"
    );
    pin.source.as_mut().unwrap().repository = "wow-ui-source".into();
    pin.source.as_mut().unwrap().product = "forever-cn-beta".into();
    assert_eq!(
        read(&mut pin).unwrap_err().message,
        "selection.product_mismatch: forever-cn-beta / forever-asia-beta"
    );
}

#[test]
fn history_pin_raw_invalid_utf8_and_unpaired_surrogates_match_go_hash() {
    let mut pin = fixture();
    pin.source.as_mut().unwrap().parser_revision = "\u{fffd}".into();
    let raw = String::from_utf8(seal(&mut pin)).unwrap();
    let prefix = raw.find('\u{fffd}').unwrap();
    let mut invalid_utf8 = raw.as_bytes()[..prefix].to_vec();
    invalid_utf8.push(0xff);
    invalid_utf8.extend_from_slice(&raw.as_bytes()[prefix + '\u{fffd}'.len_utf8()..]);
    assert_eq!(
        read_pin(&invalid_utf8, &pin.id, &OperationControl::default())
            .unwrap()
            .spec
            .source
            .unwrap()
            .parser_revision,
        "\u{fffd}"
    );
    let surrogate = raw.replace('\u{fffd}', "\\ud800");
    assert!(read_pin(surrogate.as_bytes(), &pin.id, &OperationControl::default()).is_ok());
}

#[test]
fn history_pin_cancel_and_deadline_preserve_original_control_cause() {
    let mut pin = fixture();
    let raw = seal(&mut pin);
    let cancel = lycheedev_core::control::TrackedCancellation::new();
    let control = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancel.clone(),
    );
    cancel.cancel();
    assert_eq!(
        read_pin(&raw, &pin.id, &control).unwrap_err().code,
        control.check().unwrap_err().code
    );
    let control = OperationControl::uncancelable(
        std::time::Instant::now() - std::time::Duration::from_secs(1),
    );
    assert_eq!(
        read_pin(&raw, &pin.id, &control).unwrap_err().code,
        control.check().unwrap_err().code
    );
}
