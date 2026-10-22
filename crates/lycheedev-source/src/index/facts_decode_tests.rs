use super::*;
use crate::facts::binary::{HEADER, MAGIC};
use lycheedev_core::{cache::SourceFactsCache, control::OperationControl};
use std::{fs, time::Instant};

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

fn stopped(expired: bool) -> OperationControl {
    if expired {
        OperationControl::uncancelable(Instant::now())
    } else {
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        control
    }
}

fn with_payload(valid: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut raw = valid[..HEADER].to_vec();
    raw[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    use sha2::Digest;
    raw[44..HEADER].copy_from_slice(&sha2::Sha256::digest(payload));
    raw.extend_from_slice(payload);
    raw
}

pub(super) fn malformed_records(valid: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut cases = Vec::new();
    for (name, offset) in [
        ("schema", 7),
        ("input SHA", 12),
        ("payload SHA", 44),
        ("payload bytes", HEADER),
    ] {
        let mut raw = valid.to_vec();
        raw[offset] ^= 1;
        cases.push((name.into(), raw));
    }
    for length in [0, 1, u32::MAX] {
        let mut raw = valid.to_vec();
        raw[8..12].copy_from_slice(&length.to_le_bytes());
        cases.push((format!("length {length}"), raw));
    }
    cases.push(("missing header".into(), valid[..HEADER - 1].to_vec()));
    let mut trailing = valid.to_vec();
    trailing.push(0);
    cases.push(("outer trailing".into(), trailing));
    let mut payload = valid[HEADER..].to_vec();
    payload.push(0);
    cases.push((
        "authenticated trailing field".into(),
        with_payload(valid, &payload),
    ));
    // These are malformed Postcard values with a fully valid envelope hash.
    for (name, payload) in [
        ("missing lists", vec![0, 0, 0, 0]),
        ("count overflow", vec![255; 11]),
        (
            "count exceeds payload",
            postcard::to_allocvec(&u64::MAX).unwrap(),
        ),
        (
            "name length exceeds payload",
            vec![1, 255, 255, 255, 255, 15],
        ),
        (
            "invalid UTF-8 name",
            vec![1, 1, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            "coordinate overflow",
            [vec![1, 0, 0, 0], vec![255; 11]].concat(),
        ),
    ] {
        cases.push((name.into(), with_payload(valid, &payload)));
    }
    cases
}

#[test]
fn binary_decode_matches_complete_public_facts_without_mutating_the_producer() {
    let input = storage::sha256(b"complete source");
    let expected: Facts = serde_json::from_value(full_facts()).unwrap();
    let before = serde_json::to_vec(&expected).unwrap();
    let raw = encode_facts(&expected, &input, None).unwrap();
    assert_eq!(&raw[..8], MAGIC);
    let actual = decode_facts(&raw, &input, &OperationControl::default())
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_vec(&actual).unwrap(), before);
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert_eq!(serde_json::to_vec(&expected).unwrap(), before);
    assert!(actual.raw_relationship_targets.is_none());
    assert_eq!(actual.declarations[0].end_line, usize::MAX);
}

#[test]
fn binary_decode_authenticates_schema_input_length_sha_and_rejects_authenticated_malformed_payloads()
 {
    let input = storage::sha256(b"corruption");
    let facts: Facts = serde_json::from_value(full_facts()).unwrap();
    let valid = encode_facts(&facts, &input, None).unwrap();
    let control = OperationControl::default();
    for (name, raw) in malformed_records(&valid) {
        assert!(
            decode_facts(&raw, &input, &control).unwrap().is_none(),
            "accepted {name}"
        );
    }
    for end in 0..valid[HEADER..].len() {
        let raw = with_payload(&valid, &valid[HEADER..HEADER + end]);
        assert!(
            decode_facts(&raw, &input, &control).unwrap().is_none(),
            "accepted truncation {end}"
        );
    }
    assert!(
        decode_facts(&valid, &"g".repeat(64), &control)
            .unwrap()
            .is_none()
    );
    assert!(
        decode_facts(&valid, &input.to_uppercase(), &control)
            .unwrap()
            .is_none()
    );
}

#[test]
fn binary_hit_and_misses_preserve_exact_cancelled_and_expired_errors() {
    let input = storage::sha256(b"stop");
    let facts: Facts = serde_json::from_value(full_facts()).unwrap();
    let valid = encode_facts(&facts, &input, None).unwrap();
    let mut cases = malformed_records(&valid);
    cases.push(("hit".into(), valid));
    for expired in [false, true] {
        for (name, raw) in &cases {
            let expected = stopped(expired).check().unwrap_err();
            let actual = decode_facts(raw, &input, &stopped(expired)).unwrap_err();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap(),
                "{name}"
            );
        }
    }
}

#[test]
fn binary_loose_consumer_preserves_hit_precedence_and_repairs_each_corruption_from_original() {
    let fixture = facts_cache_tests::Fixture::new();
    let control = OperationControl::default();
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let source = b"<Ui><Frame name=\"Original\"/></Ui>";
    let input = storage::sha256(source);
    let path = "Fixture.xml";
    let key = facts_cache_key(path, &input);
    let cached: Facts = serde_json::from_value(full_facts()).unwrap();
    let valid = encode_facts(&cached, &input, None).unwrap();
    cache.publish(&key, &valid, &control).unwrap();
    let profile = Profile::enabled(true);
    let (hit_facts, hit) = document_facts_profiled(
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
    assert!(hit);
    assert_eq!(
        serde_json::to_vec(&hit_facts).unwrap(),
        serde_json::to_vec(&cached).unwrap()
    );
    assert_eq!(profile.calls(Phase::FactsParse), 0);
    assert_eq!(profile.calls(Phase::FactsPublish), 0);
    assert_eq!(cache.read(&key, &control).unwrap().unwrap(), valid);
    let expected = facts::xml(source);
    let repaired = encode_facts(&expected, &input, None).unwrap();
    let corrupt = malformed_records(&valid);
    for (name, raw) in corrupt {
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
        assert!(!hit, "{name}");
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
fn binary_loose_entry_stop_keeps_reads_parsing_and_publication_unstarted() {
    let fixture = facts_cache_tests::Fixture::new();
    let live = OperationControl::default();
    let cache = SourceFactsCache::open(&fixture.home, &live).unwrap();
    let source = b"<Ui/>";
    let input = storage::sha256(source);
    let path = "Stopped.xml";
    let key = facts_cache_key(path, &input);
    for raw in [
        encode_facts(&Facts::default(), &input, None).unwrap(),
        b"{".to_vec(),
    ] {
        cache.publish(&key, &raw, &live).unwrap();
        for expired in [false, true] {
            let profile = Profile::enabled(true);
            let error = document_facts_profiled(
                &fixture.home,
                path,
                source,
                &input,
                &stopped(expired),
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
fn binary_facts_cohort_locator_and_loose_keep_complete_bytes() {
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
    assert_eq!(warm.summary.io["cachedFacts"], raw.len());
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
    let loose = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(loose.summary.io["cachedFacts"], raw.len());
    assert_eq!(loose.summary.io["parsedDocuments"], 0);
    for (expected, actual) in cold.documents.iter().zip(&loose.documents) {
        assert_eq!(
            serde_json::to_vec(&actual.facts).unwrap(),
            serde_json::to_vec(&expected.facts).unwrap()
        );
        assert!(actual.facts.raw_relationship_targets.is_none());
    }
}
