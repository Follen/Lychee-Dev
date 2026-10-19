use super::*;
use crate::facts::{Declaration, HeaderValue, LoadReference, Relationship, SyntaxNote};

// Keep the complete previous producer independent of the move and buffered
// hash. It intentionally retains the old temporary Vec and json!(f) cloning.
fn historical_encode(parsed: &Facts, sha256: &str) -> Result<Vec<u8>> {
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

fn same_bytes(facts: &Facts, input: &str) -> Value {
    let before = serde_json::to_vec(facts).unwrap();
    let raw_targets = facts.raw_relationship_targets.clone();
    let expected = historical_encode(facts, input).unwrap();
    let actual = encode_facts(facts, input, None).unwrap();
    assert_eq!(
        actual, expected,
        "complete envelope bytes, including field order and SHA"
    );
    assert_eq!(serde_json::to_vec(facts).unwrap(), before);
    assert_eq!(facts.raw_relationship_targets, raw_targets);
    let envelope: Value = serde_json::from_slice(&actual).unwrap();
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
fn encode_facts_moved_envelope_preserves_empty_arrays_and_skipped_raw_identity() {
    let input = storage::sha256(b"empty source");
    let empty = same_bytes(&Facts::default(), &input);
    let mut fresh = Facts::fresh();
    fresh
        .raw_relationship_targets
        .as_mut()
        .unwrap()
        .insert(usize::MAX, vec![0xff, 0, b'"']);
    assert_eq!(same_bytes(&fresh, &input), empty);
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
fn encode_facts_moved_envelope_matches_all_typed_fields_unicode_order_and_duplicates() {
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
    let envelope = same_bytes(&facts, &storage::sha256(text.as_bytes()));
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
fn encode_facts_moved_envelope_matches_canonical_hash_across_buffer_boundaries() {
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
        let envelope = same_bytes(&facts, &input);
        assert_eq!(
            envelope["facts"]["headers"][0]["value"],
            facts.headers[0].value
        );
    }
}

#[test]
fn encode_facts_moved_envelope_keeps_exact_32_mib_gate_and_profile_on_both_exits() {
    const LIMIT: usize = 32 << 20;
    let input = storage::sha256(b"budget source");
    let mut facts = Facts {
        diagnostics: vec![SyntaxNote {
            message: String::new(),
            line: 0,
        }],
        ..Facts::default()
    };
    // The only varying bytes are unescaped ASCII. The two hashes always have
    // 64 characters, so this small old envelope gives the exact final overhead.
    let fixed = historical_encode(&facts, &input).unwrap().len();
    for length in [LIMIT - 1, LIMIT, LIMIT + 1] {
        facts.diagnostics[0].message = "x".repeat(length - fixed);
        let expected = historical_encode(&facts, &input);
        let profile = Profile::enabled(true);
        let actual = encode_facts(&facts, &input, Some(&profile));
        assert_eq!(
            profile.calls(Phase::FactsSerialize),
            1,
            "both exits close the original phase"
        );
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => {
                assert!(length <= LIMIT);
                assert_eq!(expected.len(), length);
                assert_eq!(actual.len(), length);
                assert_eq!(
                    actual, expected,
                    "complete boundary bytes, not a size-only assertion"
                );
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(length, LIMIT + 1);
                assert_eq!(
                    serde_json::to_value(&actual).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
                assert_eq!(actual.code, "codebase.fact_budget");
                assert_eq!(actual.message, "Source fact cache exceeds the 32 MiB limit");
                assert_eq!(actual.exit_code, 3);
                assert!(actual.details.is_null());
            }
            _ => panic!("32 MiB boundary acceptance differs at {length}"),
        }
        assert_eq!(facts.diagnostics[0].message.len(), length - fixed);
        assert!(
            facts.diagnostics[0]
                .message
                .bytes()
                .all(|byte| byte == b'x')
        );
    }
}
