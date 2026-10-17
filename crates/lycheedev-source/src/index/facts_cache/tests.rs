use super::*;
use crate::index::{self, facts_cache_tests::Fixture};
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn files(count: usize) -> BTreeMap<String, String> {
    (0..count)
        .map(|i| {
            (
                format!("D{i:03}.xml"),
                format!("<Ui><Frame name=\"F{i}\"/><Script file=\"R{i}.lua\"/></Ui>"),
            )
        })
        .collect()
}
fn pack(home: &Path, locator: &Locator) -> std::path::PathBuf {
    home.join("cache/source-facts-rust")
        .join(format!("{}.json", hex(&locator.group)))
}
fn all_facts(index: &index::Index) -> serde_json::Value {
    serde_json::json!(
        index
            .documents
            .iter()
            .map(|d| serde_json::json!({"path":d.path,"facts":d.facts}))
            .collect::<Vec<_>>()
    )
}
#[test]
fn cohort_fixed_metadata_and_locator_have_a_requested_capacity_proof() {
    assert!(size_of::<Option<Locator>>() <= LOCATOR_RETAINED_BYTES);
    assert_eq!(size_of::<Record>(), 80);
    // 5120 record bytes + 512 input-index bytes + fixed owner/hash/name state;
    // the native stage's existing 4096-byte admission also fits this reservation.
    assert!(
        GROUP * size_of::<Record>() + GROUP * size_of::<usize>() + 4096 + 2048
            < GROUP_RETAINED_BYTES
    );
    let locator = Locator {
        group: [17; 32],
        offset: 8,
        bytes: 99,
        sha256: [23; 32],
    };
    let raw = serde_json::to_vec(&locator).unwrap();
    assert_eq!(serde_json::from_slice::<Locator>(&raw).unwrap(), locator);
    assert!(
        serde_json::from_str::<Locator>(
            r#"{"group":"../bad","offset":8,"bytes":99,"sha256":"bad"}"#
        )
        .is_err()
    );
}
#[test]
fn complete_cohorts_reduce_publications_and_preserve_all_five_fact_arrays() {
    let fixture = Fixture::new();
    let mut raw = files(65);
    raw.remove("D000.xml");
    raw.insert("D000.toc".into(), "## Interface: 120100\nD004.xml\n".into());
    raw.remove("D001.xml");
    raw.insert("D001.lua".into(), "function Full() Target() end".into());
    raw.remove("D002.xml");
    raw.insert("D002.lua".into(), "return 1 // 2".into());
    raw.remove("D003.xml");
    raw.insert("Interface/AddOns/Blizzard_APIDocumentationGenerated/Group.lua".into(), "local S={Type=\"System\",Name=\"Fixture\",Functions={{Name=\"Run\",Arguments={{Name=\"x\",Type=\"number\"}}}}}".into());
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let cold = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(profile.calls(Phase::FactsPublishStage), 2);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 2);
    assert_eq!(profile.calls(Phase::FactsPublishStageClose), 2);
    assert!(cold.documents.iter().all(|d| d.facts_locator.is_some()));
    let warm = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], 65);
    assert_eq!(all_facts(&cold), all_facts(&warm));
    for section in [
        "declarations",
        "relationships",
        "loads",
        "headers",
        "diagnostics",
    ] {
        assert!(
            cold.documents
                .iter()
                .any(|d| !serde_json::to_value(&d.facts).unwrap()[section]
                    .as_array()
                    .unwrap()
                    .is_empty()),
            "{section}"
        );
    }
    for d in &cold.documents {
        assert_eq!(
            serde_json::to_value(
                read_locator(
                    &fixture.home,
                    d.facts_locator.as_ref().unwrap(),
                    &d.path,
                    &d.sha256,
                    &control
                )
                .unwrap()
                .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&d.facts).unwrap()
        );
    }
}
#[test]
fn unchanged_cohort_reuses_across_snapshots_and_changed_cohort_rebuild_is_bounded() {
    let fixture = Fixture::new();
    let mut raw = files(65);
    let first = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold = index::build_uncached(&fixture.home, &first, &control).unwrap();
    raw.get_mut("D000.xml").unwrap().push('\n');
    let second = fixture.pin(&raw);
    let changed = index::build_uncached(&fixture.home, &second, &control).unwrap();
    assert_eq!(changed.summary["io"]["parsedDocuments"], 64);
    assert_eq!(changed.summary["io"]["cachedFacts"], 1);
    assert_ne!(
        changed.documents[0].facts_locator.as_ref().unwrap().group,
        cold.documents[0].facts_locator.as_ref().unwrap().group
    );
    assert_eq!(
        changed.documents[64].facts_locator,
        cold.documents[64].facts_locator
    );
    let old_again = index::build_uncached(&fixture.home, &first, &control).unwrap();
    assert_eq!(old_again.summary["io"]["cachedFacts"], 65);
    assert_eq!(all_facts(&cold), all_facts(&old_again));
}
#[test]
fn insertion_and_deletion_can_rebuild_all_shifted_cohorts_but_old_snapshots_still_hit() {
    let fixture = Fixture::new();
    let original = files(128);
    let control = OperationControl::default();
    let first = fixture.pin(&original);
    let cold = index::build_uncached(&fixture.home, &first, &control).unwrap();
    let mut inserted = original.clone();
    inserted.insert(
        "A000.xml".into(),
        "<Ui><Frame name=\"Inserted\"/></Ui>".into(),
    );
    let pin = fixture.pin(&inserted);
    let profile = Profile::enabled(true);
    let changed = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(changed.summary["io"]["parsedDocuments"], 129);
    assert_eq!(changed.summary["io"]["cachedFacts"], 0);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 3);
    assert_eq!(
        all_facts(&cold),
        serde_json::json!(
            changed.documents[1..]
                .iter()
                .map(|d| serde_json::json!({"path":d.path,"facts":d.facts}))
                .collect::<Vec<_>>()
        )
    );
    let mut deleted = original.clone();
    deleted.remove("D000.xml");
    let pin = fixture.pin(&deleted);
    let profile = Profile::enabled(true);
    let changed = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(changed.summary["io"]["parsedDocuments"], 127);
    assert_eq!(changed.summary["io"]["cachedFacts"], 0);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 2);
    assert_eq!(
        all_facts(&changed),
        serde_json::json!(
            cold.documents[1..]
                .iter()
                .map(|d| serde_json::json!({"path":d.path,"facts":d.facts}))
                .collect::<Vec<_>>()
        )
    );
    let old = index::build_uncached(&fixture.home, &first, &control).unwrap();
    assert_eq!(old.summary["io"]["parsedDocuments"], 0);
    assert_eq!(old.summary["io"]["cachedFacts"], 128);
    assert_eq!(all_facts(&old), all_facts(&cold));
}
#[test]
fn one_corrupt_record_rebuilds_and_backfills_prior_hits_before_atomic_replacement() {
    let fixture = Fixture::new();
    let raw = files(3);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    let location = cold.documents[1].facts_locator.as_ref().unwrap();
    let path = pack(&fixture.home, location);
    let mut bytes = fs::read(&path).unwrap();
    bytes[location.offset as usize + HEADER] ^= 0xff;
    fs::write(path, bytes).unwrap();
    let profile = Profile::enabled(true);
    let repaired = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(repaired.summary["io"]["parsedDocuments"], 1);
    assert_eq!(repaired.summary["io"]["cachedFacts"], 2);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 1);
    assert_eq!(all_facts(&repaired), all_facts(&cold));
    for doc in &repaired.documents {
        assert_eq!(
            serde_json::to_value(
                read_locator(
                    &fixture.home,
                    doc.facts_locator.as_ref().unwrap(),
                    &doc.path,
                    &doc.sha256,
                    &control
                )
                .unwrap()
                .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&doc.facts).unwrap()
        );
    }
    let warm = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], 3);
    assert_eq!(all_facts(&warm), all_facts(&cold));
}
#[test]
fn persisted_query_locator_and_bad_pack_recover_from_the_exact_git_input() {
    let fixture = Fixture::new();
    let raw = files(3);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold = index::build_with_control(&fixture.home, &pin, &control).unwrap();
    let warm = index::build_with_control(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedIndex"], true);
    assert_eq!(
        warm.documents[0].facts_locator,
        cold.documents[0].facts_locator
    );
    let projected = crate::query_store::load_with_control(
        &fixture.home,
        &pin,
        "F1",
        "precise",
        "xml",
        &control,
    )
    .unwrap();
    assert_eq!(
        projected.documents[0].facts_locator,
        cold.documents[0].facts_locator
    );
    let expected = index::load_facts_with_control(&fixture.home, &pin, &warm, &control).unwrap();
    let path = pack(
        &fixture.home,
        warm.documents[0].facts_locator.as_ref().unwrap(),
    );
    let original = fs::read(&path).unwrap();
    for corruption in [b"broken".to_vec(), original[..original.len() - 1].to_vec()] {
        fs::write(&path, corruption).unwrap();
        let actual =
            index::load_facts_with_control(&fixture.home, &pin, &projected, &control).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        for doc in &warm.documents {
            assert!(index::facts_cache_path(&fixture.home, &doc.path, &doc.sha256).is_file());
        }
    }
    fs::remove_file(&path).unwrap();
    assert_eq!(
        serde_json::to_value(
            index::load_facts_from_documents_with_control(&fixture.home, &warm, &raw, &control)
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}
#[test]
fn corrupt_record_keys_lengths_hashes_and_locator_ranges_are_misses() {
    let fixture = Fixture::new();
    let raw = files(2);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let index = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    let doc = &index.documents[0];
    let location = doc.facts_locator.as_ref().unwrap();
    let path = pack(&fixture.home, location);
    let original = fs::read(&path).unwrap();
    for at in [
        0,
        location.offset as usize,
        location.offset as usize + 4,
        location.offset as usize + 36,
        location.offset as usize + HEADER,
    ] {
        let mut bad = original.clone();
        bad[at] ^= 0xff;
        fs::write(&path, bad).unwrap();
        assert!(
            read_locator(&fixture.home, location, &doc.path, &doc.sha256, &control)
                .unwrap()
                .is_none()
        );
    }
    fs::write(&path, original).unwrap();
    for offset in [0, u64::MAX, MAXIMUM as u64] {
        let mut bad = location.clone();
        bad.offset = offset;
        assert!(
            read_locator(&fixture.home, &bad, &doc.path, &doc.sha256, &control)
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn failed_partial_cohort_discards_only_its_private_stage_and_keeps_committed_groups() {
    let fixture = Fixture::new();
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let raw = b"Run()";
    let source_hash = storage::sha256(raw);
    let inputs = (0..65)
        .map(|i| git::TreeFile {
            path: format!("D{i:03}.lua"),
            object: "a".repeat(40),
            bytes: raw.len(),
            asset: false,
            oversized: false,
        })
        .collect::<Vec<_>>();
    let mut groups = Groups::new(&cache, &inputs, &control, &profile);
    let mut docs = Vec::new();
    for file in &inputs {
        let (facts, _, facts_locator) = groups
            .document(file, raw, &source_hash, None, &mut docs)
            .unwrap();
        docs.push(Document {
            path: file.path.clone(),
            text: Some(raw.to_vec()),
            sha256: source_hash.clone(),
            bytes: raw.len(),
            facts,
            facts_locator,
        });
        if docs.len() == 64 {
            break;
        }
    }
    // The second cohort has one input, so use a larger declared tail to ensure
    // its first completed document is still private when the failure arrives.
    groups.finish(Ok(())).unwrap();
    let tail = [
        inputs[64].clone(),
        git::TreeFile {
            path: "ZZ.lua".into(),
            ..inputs[64].clone()
        },
    ];
    let mut groups = Groups::new(&cache, &tail, &control, &profile);
    groups
        .document(&tail[0], raw, &source_hash, None, &mut [])
        .unwrap();
    let error = ToolkitError::new("fixture.primary", "earlier input failed", 5);
    assert_eq!(
        groups.finish::<()>(Err(error)).unwrap_err().code,
        "fixture.primary"
    );
    cache.finish(Ok(()), &control).unwrap();
    assert_eq!(profile.calls(Phase::FactsPublishRename), 1);
    assert_eq!(
        fs::read_dir(fixture.home.join("cache/source-facts-rust"))
            .unwrap()
            .count(),
        1
    );
    assert!(pack(&fixture.home, docs[0].facts_locator.as_ref().unwrap()).is_file());
}
#[test]
fn cohort_reader_and_writer_keep_the_original_cancel_and_deadline() {
    let fixture = Fixture::new();
    let raw = files(1);
    let pin = fixture.pin(&raw);
    let live = OperationControl::default();
    let index = index::build_uncached(&fixture.home, &pin, &live).unwrap();
    let doc = &index.documents[0];
    for control in [
        OperationControl::new(
            Instant::now() + Duration::from_secs(60),
            Arc::new(AtomicBool::new(true)),
        ),
        OperationControl::uncancelable(Instant::now() - Duration::from_secs(1)),
    ] {
        assert!(
            read_locator(
                &fixture.home,
                doc.facts_locator.as_ref().unwrap(),
                &doc.path,
                &doc.sha256,
                &control
            )
            .is_err()
        );
        assert!(index::build_uncached(&fixture.home, &pin, &control).is_err());
    }
}

#[test]
fn bounded_pack_prefix_commits_and_remaining_complete_record_uses_per_file_cache() {
    let fixture = Fixture::new();
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let key = [29; 32];
    let mut group = GroupState {
        key,
        inputs: [0; GROUP],
        count: 2,
        seen: 2,
        first_document: 0,
        reader: None,
        records: Vec::new(),
        writer: Some(cache.begin_write(&hex(&key), &control, None).unwrap()),
        offset: 8,
        overflow: false,
    };
    group
        .writer
        .as_mut()
        .unwrap()
        .write(MAGIC, &control, None)
        .unwrap();
    let mut facts = Facts::default();
    facts.headers.push(crate::facts::HeaderValue {
        key: "Large".into(),
        value: "x".repeat(17 << 20),
        line: 1,
    });
    let input = storage::sha256(b"authenticated large input fixture");
    let first = append(
        &cache,
        &mut group,
        "First.toc",
        &input,
        &facts,
        &control,
        &profile,
    )
    .unwrap()
    .unwrap();
    assert!(
        append(
            &cache,
            &mut group,
            "Second.toc",
            &input,
            &facts,
            &control,
            &profile
        )
        .unwrap()
        .is_none()
    );
    assert!(group.overflow);
    assert!(group.writer.is_none());
    assert!(fs::metadata(pack(&fixture.home, &first)).unwrap().len() <= MAXIMUM as u64);
    let legacy = cache
        .read(&facts_cache_key("Second.toc", &input), &control)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(decode_facts(&legacy, &input, &control).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&facts).unwrap()
    );
    drop(legacy);
    drop(group);
    cache.finish(Ok(()), &control).unwrap();
    assert_eq!(
        serde_json::to_value(
            read_locator(&fixture.home, &first, "First.toc", &input, &control)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(facts).unwrap()
    );
}

#[test]
fn one_oversized_envelope_retains_the_original_fact_budget_error() {
    let mut facts = Facts::default();
    facts.headers.push(crate::facts::HeaderValue {
        key: "Large".into(),
        value: "x".repeat(MAXIMUM),
        line: 1,
    });
    assert_eq!(
        encode_facts(&facts, &storage::sha256(b"input"), None)
            .unwrap_err()
            .code,
        "codebase.fact_budget"
    );
}
