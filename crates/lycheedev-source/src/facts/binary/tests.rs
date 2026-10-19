use super::*;
use crate::facts::{Declaration, HeaderValue, LoadReference, Relationship, SyntaxNote};

// Frozen independent wire DTO: this deliberate test-only declaration detects
// field/order/width drift in the production shared contract macro.
#[derive(Serialize)]
struct V13<'a> {
    declarations: Vec<(&'a str, &'a str, &'a str, u64, u64, &'a str)>,
    relationships: Vec<(&'a str, &'a str, &'a str, &'a str, u64)>,
    loads: Vec<(&'a str, u64, &'a str)>,
    headers: Vec<(&'a str, &'a str, u64)>,
    diagnostics: Vec<(&'a str, u64)>,
}

#[test]
fn private_wire_matches_frozen_postcard_dto_and_complete_public_types() {
    let mut facts = Facts::fresh();
    facts.declarations.push(Declaration {
        name: "雪".into(),
        category: "function".into(),
        scope: String::new(),
        line: 127,
        end_line: 128,
        signature: String::new(),
    });
    facts.relationships.push(Relationship {
        from: "é".into(),
        to: "Target".into(),
        category: "call".into(),
        confidence: "literal".into(),
        line: u32::MAX as usize,
    });
    facts.loads.push(LoadReference {
        path: "目录/A.lua".into(),
        line: 0,
        kind: "script".into(),
    });
    facts.headers.push(HeaderValue {
        key: "Title".into(),
        value: "\n\0\\\"".into(),
        line: 1,
    });
    facts.diagnostics.push(SyntaxNote {
        message: "failure".into(),
        line: usize::MAX,
    });
    let expected = V13 {
        declarations: vec![("雪", "function", "", 127, 128, "")],
        relationships: vec![("é", "Target", "call", "literal", u32::MAX as u64)],
        loads: vec![("目录/A.lua", 0, "script")],
        headers: vec![("Title", "\n\0\\\"", 1)],
        diagnostics: vec![("failure", usize::MAX as u64)],
    };
    let input = lycheedev_core::storage::sha256(b"wire input");
    let record = encode(&facts, &input).unwrap();
    assert_eq!(&record[HEADER..], postcard::to_allocvec(&expected).unwrap());
    assert_eq!(
        record.len(),
        HEADER + postcard::experimental::serialized_size(&expected).unwrap()
    );
    let actual = decode(&record, &input, &OperationControl::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&facts).unwrap()
    );
    assert!(actual.raw_relationship_targets.is_none());
    let public = serde_json::to_value(actual).unwrap();
    assert!(public["declarations"][0].get("scope").is_none());
    assert!(public["declarations"][0].get("signature").is_none());
    assert_eq!(public["declarations"][0]["endLine"], 128);
}

#[test]
fn private_wire_empty_golden_has_five_explicit_sequence_counts() {
    let input = lycheedev_core::storage::sha256(b"empty");
    let record = encode(&Facts::default(), &input).unwrap();
    assert_eq!(&record[HEADER..], &[0, 0, 0, 0, 0]);
    assert_eq!(&record[8..12], &5u32.to_le_bytes());
    assert_eq!(&record[12..44], &digest(&input).unwrap());
    assert_eq!(&record[44..HEADER], Sha256::digest([0; 5]).as_slice());
}

#[test]
fn sequence_counts_and_string_sizes_are_admitted_before_allocation() {
    let control = OperationControl::default();
    for payload in [postcard::to_allocvec(&u64::MAX).unwrap(), vec![100, 0]] {
        let mut context = DecodeContext {
            control: &control,
            payload_bytes: payload.len(),
            remaining: MAX_OWNED,
            failure: None,
        };
        let mut deserializer = postcard::Deserializer::from_bytes(&payload);
        assert!(Vec::<Declaration>::decode(&mut deserializer, &mut context).is_err());
        assert_eq!(context.remaining, MAX_OWNED, "invalid count cannot reserve");
        assert!(
            context.failure.is_none(),
            "invalid cache requests a rebuild"
        );
    }
    let payload = postcard::to_allocvec("too large for the admitted space").unwrap();
    let mut context = DecodeContext {
        control: &control,
        payload_bytes: payload.len(),
        remaining: 2,
        failure: None,
    };
    let mut deserializer = postcard::Deserializer::from_bytes(&payload);
    assert!(String::decode(&mut deserializer, &mut context).is_err());
    assert_eq!(context.remaining, 2);
    assert!(context.failure.is_none());
    let payload = postcard::to_allocvec(&vec![1u64]).unwrap();
    let mut context = DecodeContext {
        control: &control,
        payload_bytes: payload.len(),
        remaining: size_of::<usize>() - 1,
        failure: None,
    };
    let mut deserializer = postcard::Deserializer::from_bytes(&payload);
    assert!(Vec::<usize>::decode(&mut deserializer, &mut context).is_err());
    assert_eq!(context.remaining, size_of::<usize>() - 1);
}

#[test]
fn valid_wire_charges_owned_capacities_and_stops_inside_the_seed() {
    let control = OperationControl::default();
    let payload = postcard::to_allocvec(&vec!["snow 雪", "", "again"]).unwrap();
    let mut context = DecodeContext {
        control: &control,
        payload_bytes: payload.len(),
        remaining: MAX_OWNED,
        failure: None,
    };
    let mut deserializer = postcard::Deserializer::from_bytes(&payload);
    let rows = Vec::<String>::decode(&mut deserializer, &mut context).unwrap();
    assert_eq!(rows, ["snow 雪", "", "again"]);
    assert_eq!(
        MAX_OWNED - context.remaining,
        rows.capacity() * size_of::<String>() + rows.iter().map(String::capacity).sum::<usize>()
    );
    assert_eq!(deserializer.finalize().unwrap(), &[]);
    control.tracked_cancel_handle().unwrap().cancel();
    let mut deserializer = postcard::Deserializer::from_bytes(&payload);
    assert!(Vec::<String>::decode(&mut deserializer, &mut context).is_err());
    assert_eq!(context.failure.unwrap().code, "command.cancelled");
}

#[test]
fn coordinate_width_is_u64_with_checked_host_conversion() {
    let control = OperationControl::default();
    for coordinate in [0, 127, 128, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX] {
        let payload = postcard::to_allocvec(&coordinate).unwrap();
        let mut context = DecodeContext {
            control: &control,
            payload_bytes: payload.len(),
            remaining: MAX_OWNED,
            failure: None,
        };
        let mut deserializer = postcard::Deserializer::from_bytes(&payload);
        match usize::try_from(coordinate) {
            Ok(expected) => {
                assert_eq!(
                    usize::decode(&mut deserializer, &mut context).unwrap(),
                    expected
                );
                assert_eq!(postcard::to_allocvec(&WireRef(&expected)).unwrap(), payload);
            }
            Err(_) => assert!(usize::decode(&mut deserializer, &mut context).is_err()),
        }
        assert_eq!(context.remaining, MAX_OWNED);
    }
}

#[test]
fn bounded_output_refuses_growth_before_copy_and_keeps_complete_prefix() {
    use postcard::ser_flavors::Flavor;
    let mut output = Output(vec![7; MAXIMUM - 1]);
    output.try_push(8).unwrap();
    let before_capacity = output.0.capacity();
    assert!(output.try_extend(&[9, 10]).is_err());
    assert_eq!(output.0.len(), MAXIMUM);
    assert_eq!(output.0.capacity(), before_capacity);
    assert_eq!(output.0[MAXIMUM - 1], 8);
}

#[test]
fn authenticated_compact_wire_keeps_exact_old_json_limit_for_nul_and_unicode_expansion() {
    let input = lycheedev_core::storage::sha256(b"forged compact Facts");
    let prefix = "雪é\t";
    let mut facts = Facts {
        diagnostics: vec![SyntaxNote {
            message: prefix.into(),
            line: usize::MAX,
        }],
        ..Facts::default()
    };
    let original_length = |facts: &Facts| {
        // Independent old-style full Value envelope: JSON escaping and all
        // envelope fields determine admission, not binary encoded byte length.
        serde_json::to_vec(&serde_json::json!({
            "schema":"lycheedev.source-facts.rust.v11", "inputSHA256":input,
            "factsSHA256":"0".repeat(64), "facts":facts,
        }))
        .unwrap()
        .len()
    };
    let fixed = original_length(&facts);
    for limit in [MAXIMUM - 1, MAXIMUM, MAXIMUM + 1] {
        let content = limit - fixed;
        facts.diagnostics[0].message = format!(
            "{prefix}{}{}",
            "\0".repeat(content / 6),
            "x".repeat(content % 6)
        );
        assert_eq!(original_length(&facts), limit);
        // Bypass only the producer's logical JSON admission in this adversarial
        // test. The record has a valid current schema/input/length/payload SHA.
        let payload = postcard::to_allocvec(&WireRef(&facts)).unwrap();
        let mut raw = vec![0; HEADER];
        raw[..8].copy_from_slice(MAGIC);
        raw[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        raw[12..44].copy_from_slice(&digest(&input).unwrap());
        raw[44..HEADER].copy_from_slice(&Sha256::digest(&payload));
        raw.extend_from_slice(&payload);
        assert!(raw.len() < MAXIMUM / 2);
        let decoded = decode(&raw, &input, &OperationControl::default()).unwrap();
        assert_eq!(decoded.is_some(), limit <= MAXIMUM);
        if let Some(decoded) = decoded {
            assert_eq!(
                serde_json::to_value(decoded).unwrap(),
                serde_json::to_value(&facts).unwrap()
            );
            assert_eq!(encode(&facts, &input).unwrap(), raw);
        } else {
            assert_eq!(
                encode(&facts, &input).unwrap_err().code,
                "codebase.fact_budget"
            );
        }
    }
}

#[test]
fn controlled_json_admission_preserves_cancelled_and_expired_errors() {
    use std::io::Write;
    for expired in [false, true] {
        let control = if expired {
            OperationControl::uncancelable(std::time::Instant::now())
        } else {
            let control = OperationControl::default();
            control.tracked_cancel_handle().unwrap().cancel();
            control
        };
        let error = admit_public_facts(&Facts::default(), Some(&control)).unwrap_err();
        assert_eq!(
            error.code,
            if expired {
                "command.deadline"
            } else {
                "command.cancelled"
            }
        );
    }
    let control = OperationControl::default();
    let mut counter = JsonLength {
        bytes: 0,
        next_check: 0,
        control: Some(&control),
        stopped: None,
    };
    counter.write_all(b"first").unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    assert!(counter.write_all(&[0; 64 << 10]).is_err());
    assert_eq!(counter.stopped.unwrap().code, "command.cancelled");
    // Original producer counting has no new operation check or error order.
    assert!(admit_public_facts(&Facts::default(), None).is_ok());
}
