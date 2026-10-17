use super::*;
use lycheedev_core::{cache::SourceFactsCache, control::OperationControl};
use std::{fs, time::Instant};

// Keep the historical owning-clone path independent of the production move.
fn cloned_reference(raw: &[u8], sha256: &str, control: &OperationControl) -> Result<Option<Facts>> {
    let envelope = serde_json::from_slice::<Value>(raw).ok();
    if let Some(envelope) = envelope
        && envelope["schema"] == "lycheedev.source-facts.rust.v11"
        && envelope["inputSHA256"] == sha256
        && envelope["factsSHA256"] == storage::sha256(&serde_json::to_vec(&envelope["facts"])?)
        && let Ok(parsed) = serde_json::from_value(envelope["facts"].clone())
    {
        control.check()?;
        return Ok(Some(parsed));
    }
    control.check()?;
    Ok(None)
}

fn envelope(facts: Value, input: &str) -> Value {
    json!({
        "schema":"lycheedev.source-facts.rust.v11",
        "inputSHA256":input,
        "factsSHA256":storage::sha256(&serde_json::to_vec(&facts).unwrap()),
        "facts":facts,
    })
}

fn full_facts() -> Value {
    let mut facts = json!({
        "declarations":[
            {"name":"雪é\n\0\\\"","category":"function","line":0,"endLine":usize::MAX},
            {"name":"Scoped","category":"function","scope":"local","signature":"F(雪)","line":2,"endLine":3},
            {"name":"Empty","category":"function","scope":"","signature":"","line":4,"endLine":4}
        ],
        "relationships":[{"from":"雪","to":"é","category":"call","confidence":"literal","line":2}],
        "loads":[{"path":"目录/雪.lua","line":3,"kind":"script"}],
        "headers":[{"key":"Title","value":"é雪\n\0","line":4}],
        "diagnostics":[{"message":"diagnostic 雪","line":5}]
    });
    for field in [
        "declarations",
        "relationships",
        "loads",
        "headers",
        "diagnostics",
    ] {
        let rows = facts[field].as_array_mut().unwrap();
        rows.push(rows[0].clone());
    }
    facts
}

fn same_bytes(raw: &[u8], input: &str) -> Option<Facts> {
    let control = OperationControl::default();
    let historical = cloned_reference(raw, input, &control).unwrap();
    let actual = decode_facts(raw, input, &control).unwrap();
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&historical).unwrap(),
        "complete typed bytes, including order and duplicates"
    );
    if let Some(facts) = &actual {
        assert!(facts.raw_relationship_targets.is_none());
    }
    actual
}

fn malformed_envelopes(input: &str) -> Vec<(String, Vec<u8>)> {
    let good = envelope(full_facts(), input);
    let mut cases = Vec::new();
    let mut add = |name: String, value: Value| {
        cases.push((name, serde_json::to_vec(&value).unwrap()));
    };
    for root in [Value::Null, json!(true), json!(7), json!("text"), json!([])] {
        add(format!("root {root}"), root);
    }
    for field in ["schema", "inputSHA256", "factsSHA256"] {
        let mut absent = good.clone();
        absent.as_object_mut().unwrap().remove(field);
        add(format!("absent {field}"), absent);
        for wrong in [Value::Null, json!(17), json!("wrong")] {
            let mut bad = good.clone();
            bad[field] = wrong;
            add(format!("wrong {field}: {}", bad[field]), bad);
        }
    }
    let mut absent = envelope(Value::Null, input);
    absent.as_object_mut().unwrap().remove("facts");
    add("absent facts with authenticated null hash".into(), absent);
    for facts in [
        Value::Null,
        json!(false),
        json!(1),
        json!("facts"),
        json!([]),
        json!({}),
    ] {
        add(format!("invalid facts {facts}"), envelope(facts, input));
    }
    for field in [
        "declarations",
        "relationships",
        "loads",
        "headers",
        "diagnostics",
    ] {
        let mut absent = full_facts();
        absent.as_object_mut().unwrap().remove(field);
        add(format!("absent array {field}"), envelope(absent, input));
        for wrong in [Value::Null, json!({}), json!([null])] {
            let mut facts = full_facts();
            facts[field] = wrong;
            add(
                format!("invalid array {field}: {}", facts[field]),
                envelope(facts, input),
            );
        }
    }
    for wrong in [
        json!(-1),
        json!(1.5),
        json!("1"),
        Value::Null,
        serde_json::from_str("18446744073709551616").unwrap(),
    ] {
        let mut facts = full_facts();
        facts["declarations"][0]["line"] = wrong;
        add(
            format!("invalid line {}", facts["declarations"][0]["line"]),
            envelope(facts, input),
        );
    }
    for field in ["name", "scope", "signature"] {
        let mut facts = full_facts();
        facts["declarations"][0][field] = Value::Null;
        add(format!("null string {field}"), envelope(facts, input));
    }
    cases.push(("invalid JSON".into(), b"{".to_vec()));
    cases.push(("trailing JSON".into(), b"{}{}".to_vec()));
    cases
}

#[test]
fn take_facts_full_bytes_keep_all_arrays_unicode_optional_fields_and_duplicates() {
    let input = storage::sha256(b"input");
    let value = full_facts();
    let raw = serde_json::to_vec(&envelope(value.clone(), &input)).unwrap();
    let actual = same_bytes(&raw, &input).unwrap();
    let expected: Facts = serde_json::from_value(value).unwrap();
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert_eq!(actual.declarations.len(), 4);
    assert_eq!(actual.relationships.len(), 2);
    assert_eq!(actual.loads.len(), 2);
    assert_eq!(actual.headers.len(), 2);
    assert_eq!(actual.diagnostics.len(), 2);
    assert_eq!(actual.declarations[0].end_line, usize::MAX);
    assert_eq!(actual.declarations[0].name, actual.declarations[3].name);
    assert_eq!(actual.declarations[1].scope, "local");
    assert!(actual.declarations[2].signature.is_empty());
    // Re-encoding the complete typed facts retains the old cache bytes too.
    assert_eq!(
        encode_facts(&actual, &input, None).unwrap(),
        encode_facts(&expected, &input, None).unwrap()
    );
}

#[test]
fn take_facts_empty_and_deep_unknown_values_keep_complete_hash_authentication() {
    let input = storage::sha256(b"empty");
    let empty = serde_json::to_value(Facts::default()).unwrap();
    let mut deep: Value = serde_json::from_str(r#"{"number":123456789012345678901234567890,"decimal":1.2300,"exponent":1E+02,"null":null}"#).unwrap();
    for _ in 0..24 {
        deep = json!({"unknown":[deep]});
    }
    let mut facts = empty.clone();
    facts["unknown"] = deep;
    let valid = envelope(facts, &input);
    let raw = serde_json::to_vec(&valid).unwrap();
    let decoded = same_bytes(&raw, &input).unwrap();
    assert_eq!(
        serde_json::to_vec(&decoded).unwrap(),
        serde_json::to_vec(&Facts::default()).unwrap()
    );
    let mut changed = valid;
    changed["facts"]["unknown"] = json!("ignored by Facts but still authenticated");
    assert!(same_bytes(&serde_json::to_vec(&changed).unwrap(), &input).is_none());
    let empty_raw = serde_json::to_vec(&envelope(empty, &input)).unwrap();
    assert!(same_bytes(&empty_raw, &input).is_some());
}

#[test]
fn take_facts_malformed_missing_null_and_wrong_types_match_clone_misses() {
    let input = storage::sha256(b"malformed");
    for (name, raw) in malformed_envelopes(&input) {
        assert!(same_bytes(&raw, &input).is_none(), "accepted {name}");
    }
}

#[test]
fn take_facts_duplicate_keys_and_number_spellings_keep_value_parse_contract() {
    let input = storage::sha256(b"duplicates");
    let facts_raw = r#"{"declarations":null,"declarations":[{"name":"before","name":"after雪","category":"function","line":-0,"endLine":2}],"relationships":[],"loads":[],"headers":[],"diagnostics":[],"unknown":{"n":1E02,"n":1.2300}}"#;
    let parsed: Value = serde_json::from_str(facts_raw).unwrap();
    let digest = storage::sha256(&serde_json::to_vec(&parsed).unwrap());
    let raw = format!(
        r#"{{"schema":"wrong","schema":"lycheedev.source-facts.rust.v11","inputSHA256":"{input}","factsSHA256":"{digest}","facts":null,"facts":{facts_raw}}}"#
    );
    let facts = same_bytes(raw.as_bytes(), &input).unwrap();
    assert_eq!(facts.declarations[0].name, "after雪");
    assert_eq!(facts.declarations[0].line, 0);
    // A final duplicate null must win even if an earlier facts value was valid.
    let raw = format!(
        r#"{{"schema":"lycheedev.source-facts.rust.v11","inputSHA256":"{input}","factsSHA256":"{}","facts":{facts_raw},"facts":null}}"#,
        storage::sha256(b"null")
    );
    assert!(same_bytes(raw.as_bytes(), &input).is_none());
}

fn stopped(expired: bool) -> OperationControl {
    if expired {
        OperationControl::uncancelable(Instant::now())
    } else {
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        control
    }
}

#[test]
fn take_facts_hit_and_miss_keep_exact_cancelled_and_expired_errors() {
    let input = storage::sha256(b"stopped");
    let mut cases = malformed_envelopes(&input);
    cases.push((
        "valid".into(),
        serde_json::to_vec(&envelope(full_facts(), &input)).unwrap(),
    ));
    for expired in [false, true] {
        for (name, raw) in &cases {
            let expected = cloned_reference(raw, &input, &stopped(expired)).unwrap_err();
            let actual = decode_facts(raw, &input, &stopped(expired)).unwrap_err();
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&expected).unwrap(),
                "{name}"
            );
            assert_eq!(
                actual.code,
                if expired {
                    "command.deadline"
                } else {
                    "command.cancelled"
                }
            );
        }
    }
}

#[test]
fn take_facts_loose_consumer_preserves_hit_precedence_and_exact_repaired_bytes() {
    let fixture = facts_cache_tests::Fixture::new();
    let control = OperationControl::default();
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let path = "Move.xml";
    let source = b"<Ui><Frame name=\"Original\"/><Script file=\"Original.lua\"/></Ui>";
    let input = storage::sha256(source);
    let key = facts_cache_key(path, &input);
    let valid = serde_json::to_vec(&envelope(full_facts(), &input)).unwrap();
    cache.publish(&key, &valid, &control).unwrap();
    let profile = Profile::enabled(true);
    let prepared = facts_pipeline::PreparedFacts {
        mode: facts::ParseMode::Xml,
        facts: Facts::fresh(),
        parse_elapsed: std::time::Duration::ZERO,
    };
    let (hit_facts, hit) = document_facts_profiled(
        &fixture.home,
        path,
        source,
        &input,
        &control,
        Some(&profile),
        &cache,
        Some(prepared),
    )
    .unwrap();
    assert!(hit);
    assert_eq!(
        serde_json::to_vec(&hit_facts).unwrap(),
        serde_json::to_vec(&cloned_reference(&valid, &input, &control).unwrap().unwrap()).unwrap()
    );
    assert!(hit_facts.raw_relationship_targets.is_none());
    assert_eq!(profile.calls(Phase::FactsParse), 0);
    assert_eq!(profile.calls(Phase::FactsPublish), 0);
    assert_eq!(cache.read(&key, &control).unwrap().unwrap(), valid);
    let expected = facts::xml(source);
    let repaired = encode_facts(&expected, &input, None).unwrap();
    for (name, raw) in malformed_envelopes(&input) {
        // Inject each malformed body into the real cache path so the next
        // default read must authenticate and replace exactly these bytes.
        fs::write(facts_cache_path(&fixture.home, path, &input), raw).unwrap();
        let profile = Profile::enabled(true);
        let (actual, hit) = document_facts_profiled(
            &fixture.home,
            path,
            source,
            &input,
            &control,
            Some(&profile),
            &cache,
            None,
        )
        .unwrap();
        assert!(!hit, "malformed hit: {name}");
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&expected).unwrap(),
            "{name}"
        );
        assert_eq!(profile.calls(Phase::FactsParseSequential), 1, "{name}");
        assert_eq!(
            cache.read(&key, &control).unwrap().unwrap(),
            repaired,
            "{name}"
        );
    }
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn take_facts_loose_entry_stop_keeps_reads_parsing_and_publication_unstarted() {
    let fixture = facts_cache_tests::Fixture::new();
    let live = OperationControl::default();
    let cache = SourceFactsCache::open(&fixture.home, &live).unwrap();
    let source = b"<Ui/>";
    let input = storage::sha256(source);
    let path = "Stopped.xml";
    let key = facts_cache_key(path, &input);
    for raw in [
        serde_json::to_vec(&envelope(full_facts(), &input)).unwrap(),
        b"{".to_vec(),
    ] {
        cache.publish(&key, &raw, &live).unwrap();
        for expired in [false, true] {
            let control = stopped(expired);
            let profile = Profile::enabled(true);
            let error = document_facts_profiled(
                &fixture.home,
                path,
                source,
                &input,
                &control,
                Some(&profile),
                &cache,
                None,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                if expired {
                    "command.deadline"
                } else {
                    "command.cancelled"
                }
            );
            for phase in [Phase::FactsRead, Phase::FactsParse, Phase::FactsPublish] {
                assert_eq!(profile.calls(phase), 0);
            }
            assert_eq!(
                fs::read(facts_cache_path(&fixture.home, path, &input)).unwrap(),
                raw
            );
        }
    }
    cache.finish(Ok(()), &live).unwrap();
}

#[test]
fn take_facts_default_cohort_locator_and_legacy_loose_keep_complete_bytes() {
    let fixture = facts_cache_tests::Fixture::new();
    let raw = BTreeMap::from([
        (
            "A.xml".into(),
            "<Ui><Frame name=\"雪\"/><Script file=\"Target.lua\"/></Ui>".into(),
        ),
        ("B.toc".into(), "## Title: é雪\nA.xml\nA.xml\n".into()),
        (
            "C.lua".into(),
            "function Run() Target() Target() end".into(),
        ),
        ("D.lua".into(), "return 1 // 2".into()),
    ]);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
    let warm = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], raw.len());
    let mut packs = BTreeSet::new();
    for (fresh, hit) in cold.documents.iter().zip(&warm.documents) {
        let locator = hit.facts_locator.as_ref().unwrap();
        let located =
            facts_cache::read_locator(&fixture.home, locator, &hit.path, &hit.sha256, &control)
                .unwrap()
                .unwrap();
        let expected = serde_json::to_vec(&fresh.facts).unwrap();
        assert_eq!(serde_json::to_vec(&hit.facts).unwrap(), expected);
        assert_eq!(serde_json::to_vec(&located).unwrap(), expected);
        assert!(hit.facts.raw_relationship_targets.is_none());
        assert!(located.raw_relationship_targets.is_none());
        assert_eq!(fresh.facts_locator, hit.facts_locator);
        let hex: String = locator.group.iter().map(|b| format!("{b:02x}")).collect();
        packs.insert(
            fixture
                .home
                .join("cache/source-facts-rust")
                .join(format!("{hex}.json")),
        );
        let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
        let encoded = encode_facts(&fresh.facts, &hit.sha256, None).unwrap();
        cache
            .publish(&facts_cache_key(&hit.path, &hit.sha256), &encoded, &control)
            .unwrap();
        cache.finish(Ok(()), &control).unwrap();
    }
    for pack in packs {
        fs::remove_file(pack).unwrap();
    }
    let legacy = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(legacy.summary["io"]["cachedFacts"], raw.len());
    assert_eq!(legacy.summary["io"]["parsedDocuments"], 0);
    for (expected, actual) in cold.documents.iter().zip(&legacy.documents) {
        assert_eq!(
            serde_json::to_vec(&actual.facts).unwrap(),
            serde_json::to_vec(&expected.facts).unwrap()
        );
        assert!(actual.facts.raw_relationship_targets.is_none());
    }
}
