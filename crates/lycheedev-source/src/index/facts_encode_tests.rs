use super::*;
use crate::facts::{Declaration, HeaderValue, LoadReference, Relationship, SyntaxNote};
const FACTS_HASH_BUFFER_BYTES: usize = 8 << 10;

// Keep the complete previous producer independent of the move and buffered
// hash. It intentionally retains the old temporary Vec and json!(f) cloning.
pub(super) fn historical_encode(parsed: &Facts, sha256: &str) -> Result<Vec<u8>> {
    let f = serde_json::to_value(parsed)?;
    let envelope = serde_json::to_vec(
        &json!({"schema":"lycheedev.source-facts.rust.v11","inputSHA256":sha256,"factsSHA256":storage::sha256(&serde_json::to_vec(&f)?),"facts":f}),
    )?;
    if envelope.len() > 32 << 20 {
        return Err(ToolkitError::new(
            "codebase.fact_budget",
            "Source fact cache exceeds the 32 MiB limit",
            3,
        ));
    }
    Ok(envelope)
}

fn same_public_bytes(facts: &Facts, input: &str) -> Value {
    let before = serde_json::to_vec(facts).unwrap();
    let raw_targets = facts.raw_relationship_targets.clone();
    let expected = historical_encode(facts, input).unwrap();
    let actual = encode_facts(facts, input, None).unwrap();
    let decoded = decode_facts(
        &actual,
        input,
        &lycheedev_core::control::OperationControl::default(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&decoded).unwrap(),
        before,
        "complete public bytes and order"
    );
    assert!(decoded.raw_relationship_targets.is_none());
    assert_eq!(serde_json::to_vec(facts).unwrap(), before);
    assert_eq!(facts.raw_relationship_targets, raw_targets);
    // Full historical producer remains an independent canonical JSON oracle;
    // cache bytes themselves deliberately use a new private version.
    let envelope: Value = serde_json::from_slice(&expected).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), envelope["facts"]);
    assert_eq!(envelope["schema"], "lycheedev.source-facts.rust.v11");
    assert_eq!(envelope["inputSHA256"], input);
    assert_eq!(envelope["facts"], serde_json::to_value(facts).unwrap());
    assert_eq!(
        envelope["factsSHA256"],
        storage::sha256(&serde_json::to_vec(&envelope["facts"]).unwrap())
    );
    assert_eq!(envelope.as_object().unwrap().len(), 4);
    envelope
}

#[test]
fn binary_facts_preserves_empty_arrays_and_skipped_raw_identity() {
    let input = storage::sha256(b"empty source");
    let empty = same_public_bytes(&Facts::default(), &input);
    let mut fresh = Facts::fresh();
    fresh
        .raw_relationship_targets
        .as_mut()
        .unwrap()
        .insert(usize::MAX, vec![0xff, 0, b'"']);
    assert_eq!(same_public_bytes(&fresh, &input), empty);
    assert_eq!(
        empty["facts"],
        json!({
            "declarations":[], "relationships":[], "loads":[], "headers":[], "diagnostics":[]
        })
    );
    assert_eq!(
        fresh.raw_relationship_targets.as_ref().unwrap()[&usize::MAX],
        [0xff, 0, b'"']
    );
}

#[test]
fn binary_facts_matches_all_typed_fields_unicode_order_and_duplicates() {
    // These are actual producer types: strings, usize coordinates and five
    // arrays. Unknown Value fields, floats and nested JSON are not Facts input.
    let text = "雪é\n\r\t\0\\\"";
    let mut facts = Facts::fresh();
    for mask in 0..4 {
        facts.declarations.push(Declaration {
            name: format!("{text}-{mask}"),
            category: "function".into(),
            scope: if mask & 1 == 0 {
                String::new()
            } else {
                text.into()
            },
            line: 0,
            end_line: usize::MAX,
            signature: if mask & 2 == 0 {
                String::new()
            } else {
                format!("F({text})")
            },
        });
    }
    facts.declarations.push(facts.declarations[0].clone());
    for ordinal in [2, 0, 2] {
        facts.relationships.push(Relationship {
            from: text.into(),
            to: format!("{text}-{ordinal}"),
            category: "call".into(),
            confidence: "literal".into(),
            line: usize::MAX,
        });
        facts.loads.push(LoadReference {
            path: format!("目录/{ordinal}.lua"),
            line: ordinal,
            kind: text.into(),
        });
        facts.headers.push(HeaderValue {
            key: format!("Title-{ordinal}"),
            value: text.into(),
            line: ordinal,
        });
        facts.diagnostics.push(SyntaxNote {
            message: format!("{text}-{ordinal}"),
            line: ordinal,
        });
    }
    facts
        .raw_relationship_targets
        .as_mut()
        .unwrap()
        .insert(0, vec![0xff, b'\n']);
    let envelope = same_public_bytes(&facts, &storage::sha256(text.as_bytes()));
    let rows = &envelope["facts"];
    assert_eq!(rows["declarations"].as_array().unwrap().len(), 5);
    for field in ["relationships", "loads", "headers", "diagnostics"] {
        assert_eq!(rows[field].as_array().unwrap().len(), 3);
        assert_eq!(rows[field][0], rows[field][2]);
        assert_ne!(rows[field][0], rows[field][1]);
    }
    assert_eq!(rows["declarations"][0], rows["declarations"][4]);
    for (mask, row) in rows["declarations"].as_array().unwrap()[..4]
        .iter()
        .enumerate()
    {
        assert_eq!(row.get("scope").is_some(), mask & 1 != 0);
        assert_eq!(row.get("signature").is_some(), mask & 2 != 0);
        assert_eq!(row["line"], 0);
        assert_eq!(row["endLine"], usize::MAX);
    }
    assert!(rows.get("rawRelationshipTargets").is_none());
}

#[test]
fn binary_facts_matches_canonical_hash_across_buffer_boundaries() {
    let input = storage::sha256(b"buffered source");
    let mut facts = Facts {
        headers: vec![HeaderValue {
            key: "Title".into(),
            value: String::new(),
            line: 1,
        }],
        ..Facts::default()
    };
    let fixed = serde_json::to_vec(&serde_json::to_value(&facts).unwrap())
        .unwrap()
        .len();
    for length in [
        FACTS_HASH_BUFFER_BYTES - 1,
        FACTS_HASH_BUFFER_BYTES,
        FACTS_HASH_BUFFER_BYTES + 1,
        2 * FACTS_HASH_BUFFER_BYTES - 1,
        2 * FACTS_HASH_BUFFER_BYTES,
        2 * FACTS_HASH_BUFFER_BYTES + 1,
    ] {
        facts.headers[0].value = "x".repeat(length - fixed);
        assert_eq!(
            serde_json::to_vec(&serde_json::to_value(&facts).unwrap())
                .unwrap()
                .len(),
            length
        );
        let envelope = same_public_bytes(&facts, &input);
        assert_eq!(
            envelope["facts"]["headers"][0]["value"],
            facts.headers[0].value
        );
    }
}

#[test]
fn binary_facts_keeps_exact_binary_32_mib_gate_and_profile_on_both_exits() {
    use crate::facts::binary::{HEADER, MAGIC};
    use sha2::{Digest, Sha256};
    const LIMIT: usize = 32 << 20;
    let input = storage::sha256(b"budget source");
    let mut facts = Facts {
        diagnostics: vec![SyntaxNote {
            message: String::new(),
            line: 0,
        }],
        ..Facts::default()
    };
    // Frozen v13 positional DTO, independent of the production Wire adapter.
    // Five one-byte array lengths, a four-byte string length and a zero line.
    let overhead = HEADER + 5 + 4 + 1;
    for length in [LIMIT - 1, LIMIT, LIMIT + 1] {
        facts.diagnostics[0].message = "x".repeat(length - overhead);
        let payload = postcard::to_allocvec(&(
            Vec::<()>::new(),
            Vec::<()>::new(),
            Vec::<()>::new(),
            Vec::<()>::new(),
            vec![(facts.diagnostics[0].message.as_str(), 0u64)],
        ))
        .unwrap();
        let mut expected = vec![0; HEADER];
        expected[..8].copy_from_slice(MAGIC);
        expected[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        expected[12..44].copy_from_slice(&Sha256::digest(b"budget source"));
        expected[44..HEADER].copy_from_slice(&Sha256::digest(&payload));
        expected.extend_from_slice(&payload);
        assert_eq!(expected.len(), length);
        drop(payload);
        let profile = Profile::enabled(true);
        let actual = encode_facts(&facts, &input, Some(&profile));
        assert_eq!(
            profile.calls(Phase::FactsSerialize),
            1,
            "both exits close the original phase"
        );
        let control = lycheedev_core::control::OperationControl::default();
        if length <= LIMIT {
            assert_eq!(actual.unwrap(), expected, "complete frozen binary frame");
            let decoded = decode_facts(&expected, &input, &control).unwrap().unwrap();
            assert_eq!(
                serde_json::to_vec(&decoded).unwrap(),
                serde_json::to_vec(&facts).unwrap()
            );
        } else {
            let error = actual.unwrap_err();
            assert_eq!(error.code, "codebase.fact_budget");
            assert_eq!(error.message, "Source fact cache exceeds the 32 MiB limit");
            assert_eq!(error.exit_code, 3);
            assert!(error.details.is_null());
            assert!(decode_facts(&expected, &input, &control).unwrap().is_none());
        }
        assert_eq!(facts.diagnostics[0].message.len(), length - overhead);
        assert!(
            facts.diagnostics[0]
                .message
                .bytes()
                .all(|byte| byte == b'x')
        );
    }
}
