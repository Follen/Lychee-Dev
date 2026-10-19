use super::*;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

fn budget(limits: Cost) -> Budget {
    Budget::new(limits, Instant::now() + Duration::from_secs(30), None)
}
#[test]
fn go_semantic_syntax_builds_conditions_and_exact_numbers() {
    let ledger = budget(Cost::defaults());
    let mappings = parse_mappings(b"FLAGS SpellMisc::Attributes[0] SpellAttributes0\nENUM SpellEffect::EffectMiscValue[0] SpellModOp SpellEffect::EffectAura=107\n", "SpellEffect", &ledger).unwrap();
    assert_eq!(
        mappings,
        [Mapping {
            field: "EffectMiscValue[0]".into(),
            kind: "ENUM".into(),
            name: "SpellModOp".into(),
            condition_field: "SpellEffect::EffectAura".into(),
            condition_value: "107".into(),
        }]
    );
    let raw = b"0x8000000000000000 HIGH // high bit\n(BUILD 3.0.1.8303-3.3.5.12340, 4.0.0.11792) 146 OLD\n(BUILD 12.1.0.69933) 146 CURRENT\n5\n-1 NONE\n";
    assert_eq!(
        parse_values(raw, "12.1.0.69933", &ledger).unwrap(),
        vec![
            vec![
                Scalar::UInt(1 << 63),
                Scalar::String("HIGH".into()),
                Scalar::String("high bit".into())
            ],
            vec![
                Scalar::UInt(146),
                Scalar::String("CURRENT".into()),
                Scalar::String("".into())
            ],
            vec![Scalar::UInt(5), Scalar::Null, Scalar::String("".into())],
            vec![
                Scalar::Int(-1),
                Scalar::String("NONE".into()),
                Scalar::String("".into())
            ],
        ]
    );
    for (token, want) in [
        ("18446744073709551615", Scalar::UInt(u64::MAX)),
        ("-9223372036854775808", Scalar::Int(i64::MIN)),
        ("0x-1", Scalar::Int(-1)),
        ("0009", Scalar::UInt(9)),
    ] {
        assert_eq!(number(token).unwrap(), want);
    }
    for token in [
        "+1",
        "0XFF",
        "18446744073709551616",
        "-9223372036854775809",
        "0x",
        "--1",
        "1.0",
        "-0x1",
    ] {
        assert_eq!(
            number(token).unwrap_err().code,
            "records.definition_identity",
            "{token}"
        );
    }
}
#[test]
fn go_semantic_irrelevant_mapping_empty_catalog_and_strict_selected_syntax() {
    let ledger = budget(Cost::defaults());
    assert!(parse_mappings(b"COLOR Sample::Color ignored invalid trailing tokens\nUNKNOWN Other::Value ../../escape\nFLAGS Other::Bad[ Def\n", "Sample", &ledger).unwrap().is_empty());
    for raw in [
        "FLAGS Sample::Attributes ../../escape",
        "FLAGS Sample::A[bad] Def",
        "FLAGS Sample::A Def extra",
        "FLAGS Sample::A Def Other::F=1 extra",
        "flags Sample::A Def",
        "ENUM Sample::A D-ef",
        "ENUM Sample::A Def Other::F[=1",
    ] {
        assert_eq!(
            parse_mappings(raw.as_bytes(), "sample", &ledger)
                .unwrap_err()
                .code,
            "records.definition_identity",
            "{raw}"
        );
    }
    for raw in [
        "(BOGUS 1) 2 UNKNOWN",
        "(BUILD 12.1.0.69933",
        "(BUILD 12.1.0.69933)",
        "not-a-number name extra",
    ] {
        assert!(
            parse_values(raw.as_bytes(), "12.1.0.69933", &ledger).is_err(),
            "{raw}"
        );
    }
    assert!(
        parse_values(
            b"(BUILD 1.0.0.1) invalid skipped tokens",
            "12.1.0.69933",
            &ledger
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        parse_mappings(&[0xff], "Sample", &ledger).unwrap_err().code,
        "records.definition_identity"
    );
    assert_eq!(
        parse_values(&[0xff], "12.1.0.69933", &ledger)
            .unwrap_err()
            .code,
        "records.definition_identity"
    );
}
#[test]
fn semantic_parser_reserves_before_rows_and_keeps_long_names() {
    let raw = "0\n".repeat(100_000);
    let ledger = budget(Cost {
        retained_bytes: raw.len() as u64 + 160,
        ..Cost::defaults()
    });
    assert_eq!(
        parse_values(raw.as_bytes(), "12.1.0.69933", &ledger)
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(ledger.snapshot().unwrap().decode_work, 1);
    assert_eq!(
        ledger.snapshot().unwrap().retained_bytes,
        raw.len() as u64 + 160
    );
    let raw = b"FLAGS SpellMisc::Attributes A\nFLAGS SpellMisc::Other B\n";
    let ledger = budget(Cost {
        retained_bytes: raw.len() as u64 + 256,
        ..Cost::defaults()
    });
    assert_eq!(
        parse_mappings(raw, "SpellMisc", &ledger).unwrap_err().code,
        "records.query_resource_budget"
    );
    assert_eq!(ledger.snapshot().unwrap().decode_work, 1);
    let ledger = budget(Cost::defaults());
    // Value is split once. The former oversized-token rejection was rejecting
    // a valid long display name, not protecting the strict mapping grammar.
    let values = parse_values("0 ".repeat(100_000).as_bytes(), "12.1.0.69933", &ledger).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0][0], Scalar::UInt(0));
    assert_eq!(
        values[0][1],
        Scalar::String("0 ".repeat(99_999).trim_end().into())
    );
}
#[test]
fn semantic_go_row_and_mapping_limits_and_control_recovery() {
    let ledger = budget(Cost::defaults());
    assert_eq!(
        parse_values("0\n".repeat(100_000).as_bytes(), "12.1.0.69933", &ledger)
            .unwrap()
            .len(),
        100_000
    );
    let ledger = budget(Cost::defaults());
    assert_eq!(
        parse_values("0\n".repeat(100_001).as_bytes(), "12.1.0.69933", &ledger)
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
    let raw = "FLAGS Sample::A Def\n".repeat(4096);
    let ledger = budget(Cost::defaults());
    assert_eq!(
        parse_mappings(raw.as_bytes(), "Sample", &ledger)
            .unwrap()
            .len(),
        4096
    );
    assert_eq!(
        parse_mappings(
            (raw + "FLAGS Sample::A Def\n").as_bytes(),
            "Sample",
            &ledger
        )
        .unwrap_err()
        .code,
        "records.metadata_limit"
    );
    let cancelled = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(Arc::new(AtomicBool::new(true))),
    );
    assert_eq!(
        parse_values(b"1 X", "12.1.0.69933", &cancelled)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(cancelled.snapshot().unwrap().metadata_bytes, 0);
    let expired = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    assert_eq!(
        parse_mappings(b"FLAGS Sample::A Def", "Sample", &expired)
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(
        parse_values(b"1 X", "12.1.0.69933", &budget(Cost::defaults()))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn semantic_cached_load_honors_cancellation_after_validation_without_replay() {
    use lycheedev_core::{control::TrackedCancellation, workspace};
    use serde_json::json;
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-semantic-load-control")
        .join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    workspace::initialize(&home).unwrap();
    let url = "https://raw.githubusercontent.com/wowdev/WoWDBDefs/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/meta/enums/Test.dbde";
    let raw = b"1 ONE\n";
    let blob = evidence::publish_blob(&home, raw, 4 << 20).unwrap();
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let mut metadata = Metadata::open(&home, true).unwrap();
    metadata
        .put(&key, &json!({"url":url,"blob":blob}), None)
        .unwrap();
    let generation = metadata.generation(&key).unwrap();
    drop(metadata);
    let cancelled = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), cancelled.clone());
    let ledger = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let mut validations = 0;
    let error = load(
        &home,
        url,
        true,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |bytes| {
            assert_eq!(bytes, raw);
            validations += 1;
            cancelled.cancel();
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(validations, 1);
    assert_eq!(
        Metadata::open(&home, false)
            .unwrap()
            .generation(&key)
            .unwrap(),
        generation
    );
    let control = OperationControl::default();
    let ledger = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let (reference, bytes) = load(
        &home,
        url,
        true,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |bytes| {
            assert_eq!(bytes, raw);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(reference, blob);
    assert_eq!(bytes, raw);
}

#[test]
fn go_semantic_table_filter_preserves_ascii_unicode_simple_fold_classes() {
    for (selected, canonical, expected) in [
        ("Sample", "sample", true),
        ("ſample", "Sample", true),
        ("Known", "Known", true),
        ("ſKILL", "Skill", true),
        ("_ſ_K09", "_S_K09", true),
        ("ſampleX", "Sample", false),
        ("ſampl", "Sample", false),
        ("İtem", "Item", false),
        ("ıtem", "Item", false),
        ("ßample", "SSample", false),
        ("ﬀlags", "fflags", false),
        ("Ｓample", "Sample", false),
    ] {
        assert_eq!(
            table_matches(selected, canonical),
            expected,
            "{selected} / {canonical}"
        );
    }
    let ledger = budget(Cost::defaults());
    let mappings = parse_mappings(
        "ENUM ſKILL::Value State\nENUM Skill::Other OtherState\nENUM SKıLL::Ignored MustNotLoad\n"
            .as_bytes(),
        "Skill",
        &ledger,
    )
    .unwrap();
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].field, "Value");
    assert_eq!(mappings[0].name, "State");
    assert_eq!(mappings[1].field, "Other");
    // Once a Unicode-folded table matches, malformed selected syntax must fail;
    // it cannot be silently treated as an irrelevant mapping.
    assert_eq!(
        parse_mappings(
            "ENUM ſKILL::Value ../../escape".as_bytes(),
            "Skill",
            &ledger
        )
        .unwrap_err()
        .code,
        "records.definition_identity"
    );
}

#[test]
fn semantic_parsers_keep_independent_invocation_checks_inside_rows() {
    let ledger = budget(Cost::defaults());
    let raw = b"ENUM Sample::Value Values\nENUM Sample::Other Values\n";
    let mut checks = 0;
    let result = parse_mappings_with_check(raw, "Sample", &ledger, || {
        checks += 1;
        if checks == 3 {
            Err(fault("fixture.invocation", 7, "invocation stopped"))
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(error) if error.code == "fixture.invocation"));
    assert_eq!(checks, 3);
    assert_eq!(ledger.logical_snapshot().unwrap().decode_work, 1);
    assert!(ledger.check().is_ok());
    let ledger = budget(Cost::defaults());
    let mut checks = 0;
    let result = parse_values_with_check(b"1 FIRST\n2 SECOND\n", "12.1.0.69933", &ledger, || {
        checks += 1;
        if checks == 3 {
            Err(fault("fixture.invocation", 7, "invocation stopped"))
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(error) if error.code == "fixture.invocation"));
    assert_eq!(checks, 3);
    assert_eq!(ledger.logical_snapshot().unwrap().decode_work, 1);
    assert!(ledger.check().is_ok());
}
