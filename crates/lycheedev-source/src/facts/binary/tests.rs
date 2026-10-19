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
    assert!(deserializer.finalize().unwrap().is_empty());
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
                assert_eq!(
                    postcard::to_allocvec(&WireRef(&expected, &EncodeOwnership::new())).unwrap(),
                    payload
                );
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
fn compact_facts_beyond_retired_json_cache_limit_keep_complete_public_bytes() {
    let input = lycheedev_core::storage::sha256(b"compact Facts");
    let facts = Facts {
        diagnostics: vec![SyntaxNote {
            message: format!("雪é\t{}", "\0".repeat(MAXIMUM / 6)),
            line: usize::MAX,
        }],
        ..Facts::default()
    };
    // JSON is a result oracle, not the private cache admission representation.
    let public = serde_json::to_vec(&facts).unwrap();
    assert!(public.len() > MAXIMUM);
    let raw = encode(&facts, &input).unwrap();
    assert!(raw.len() < MAXIMUM / 2);
    let decoded = decode(&raw, &input, &OperationControl::default())
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), public);
    assert_eq!(encode(&decoded, &input).unwrap(), raw);
    assert_eq!(decoded.diagnostics[0].line, usize::MAX);
    assert!(decoded.declarations.is_empty());
    assert!(decoded.relationships.is_empty());
    assert!(decoded.loads.is_empty());
    assert!(decoded.headers.is_empty());
}

#[test]
fn wire_ownership_admission_matches_decoded_shape_and_preserves_field_bytes() {
    let facts = Facts {
        declarations: vec![
            Declaration {
                name: "雪".into(),
                category: "function".into(),
                scope: String::new(),
                line: 127,
                end_line: 128,
                signature: "F()".into(),
            };
            3
        ],
        headers: vec![HeaderValue {
            key: "Title".into(),
            value: "\0\n\\\"".repeat(100),
            line: 1,
        }],
        ..Facts::default()
    };
    let needed = 3 * size_of::<Declaration>()
        + 3 * ("雪".len() + "function".len() + "F()".len())
        + size_of::<HeaderValue>()
        + "Title".len()
        + facts.headers[0].value.len();
    let baseline = postcard::to_allocvec(&V13 {
        declarations: vec![("雪", "function", "", 127, 128, "F()"); 3],
        relationships: vec![],
        loads: vec![],
        headers: vec![("Title", facts.headers[0].value.as_str(), 1)],
        diagnostics: vec![],
    })
    .unwrap();
    for limit in [needed - 1, needed, needed + 1] {
        let ownership = EncodeOwnership(Cell::new(limit));
        let actual = postcard::to_allocvec(&WireRef(&facts, &ownership));
        assert_eq!(actual.is_ok(), limit >= needed);
        if let Ok(actual) = actual {
            assert_eq!(actual, baseline);
            assert_eq!(ownership.0.get(), limit - needed);
        }
    }
    assert_eq!(EncodeOwnership::new().0.get(), 256 << 20);
}

#[test]
fn compact_row_and_string_ownership_reject_before_emitting_their_wire_bytes() {
    let rows = vec![
        Declaration {
            name: String::new(),
            category: String::new(),
            scope: String::new(),
            line: 0,
            end_line: 0,
            signature: String::new(),
        };
        3
    ];
    let ownership = EncodeOwnership(Cell::new(3 * size_of::<Declaration>() - 1));
    let mut output = [0xa5; 100];
    assert!(postcard::to_slice(&WireRef(&rows, &ownership), &mut output).is_err());
    assert_eq!(output, [0xa5; 100], "row admission precedes even the count");
    let value = "雪\0".repeat(1024);
    let ownership = EncodeOwnership(Cell::new(value.len() - 1));
    assert!(postcard::to_slice(&WireRef(&value, &ownership), &mut output).is_err());
    assert_eq!(output, [0xa5; 100], "string admission precedes its length");
    let ownership = EncodeOwnership(Cell::new(7));
    assert!(ownership.admit::<postcard::Error>(usize::MAX).is_err());
    assert!(
        ownership
            .rows::<Declaration, postcard::Error>(usize::MAX)
            .is_err()
    );
    assert_eq!(ownership.0.get(), 7, "overflow cannot wrap the allowance");
}

#[test]
fn authenticated_compact_rows_over_real_decoded_budget_reject_without_row_allocation() {
    // Construct only the compact wire, never millions of owned declarations.
    // Every field is zero/empty, so all six field encodings occupy one byte.
    let count = MAX_OWNED / size_of::<Declaration>() + 1;
    let mut payload = postcard::to_allocvec(&(count as u64)).unwrap();
    payload.resize(payload.len() + count * 6, 0);
    payload.extend_from_slice(&[0; 4]);
    let input = lycheedev_core::storage::sha256(b"compact row ownership");
    let mut raw = vec![0; HEADER];
    raw[..8].copy_from_slice(MAGIC);
    raw[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    raw[12..44].copy_from_slice(&digest(&input).unwrap());
    raw[44..HEADER].copy_from_slice(&Sha256::digest(&payload));
    raw.extend_from_slice(&payload);
    assert!(raw.len() < MAXIMUM);
    assert!(
        decode(&raw, &input, &OperationControl::default())
            .unwrap()
            .is_none()
    );
    let control = OperationControl::default();
    let mut context = DecodeContext {
        control: &control,
        payload_bytes: payload.len(),
        remaining: MAX_OWNED,
        failure: None,
    };
    let mut deserializer = postcard::Deserializer::from_bytes(&payload);
    assert!(Facts::decode(&mut deserializer, &mut context).is_err());
    assert_eq!(
        context.remaining, MAX_OWNED,
        "rejected before reserving the row vector"
    );
    assert!(
        context.failure.is_none(),
        "bad ownership requests a complete rebuild"
    );
}
