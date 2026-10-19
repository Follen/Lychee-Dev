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
fn take_facts_packed_inner_validation_keeps_outer_integrity_and_exact_recovery() {
    let fixture = Fixture::new();
    let raw = files(1);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let original_index = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    let doc = &original_index.documents[0];
    let location = doc.facts_locator.as_ref().unwrap();
    let path = pack(&fixture.home, location);
    let original_pack = fs::read(&path).unwrap();
    let valid = index::encode_facts(&doc.facts, &doc.sha256, None).unwrap();
    for (name, body) in index::facts_decode_tests::malformed_records(&valid) {
        let body_hash: [u8; 32] = Sha256::digest(&body).into();
        let mut header = [0; HEADER];
        header[..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
        header[4..36].copy_from_slice(&digest(&facts_cache_key(&doc.path, &doc.sha256)).unwrap());
        header[36..].copy_from_slice(&body_hash);
        let mut packed = MAGIC.to_vec();
        packed.extend_from_slice(&header);
        packed.extend_from_slice(&body);
        fs::write(&path, packed).unwrap();
        let updated = Locator {
            group: location.group,
            offset: MAGIC.len() as u64,
            bytes: body.len() as u32,
            sha256: body_hash,
        };
        // Header/key/bounds/body SHA are all valid: this must reach the inner
        // envelope's original validation, rather than pass on an outer miss.
        let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
        let mut reader = cache
            .open_reader(&hex(&updated.group), &control)
            .unwrap()
            .unwrap();
        let records = directory(&mut reader, &control).unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sha256, body_hash);
        let mut observed = vec![0; body.len()];
        assert!(
            reader
                .read_at(updated.offset + HEADER as u64, &mut observed, &control)
                .unwrap()
        );
        assert_eq!(observed, body);
        assert_eq!(<[u8; 32]>::from(Sha256::digest(&observed)), updated.sha256);
        reader.finish(Ok(()), &control).unwrap();
        cache.finish(Ok(()), &control).unwrap();
        assert!(
            read_locator(&fixture.home, &updated, &doc.path, &doc.sha256, &control)
                .unwrap()
                .is_none(),
            "{name}"
        );
        let repaired = index::build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(repaired.summary["io"]["parsedDocuments"], 1, "{name}");
        assert_eq!(repaired.summary["io"]["cachedFacts"], 0, "{name}");
        assert_eq!(
            serde_json::to_vec(&repaired.documents[0].facts).unwrap(),
            serde_json::to_vec(&doc.facts).unwrap(),
            "{name}"
        );
        assert_eq!(
            repaired.documents[0].facts_locator, doc.facts_locator,
            "{name}"
        );
        assert_eq!(fs::read(&path).unwrap(), original_pack, "{name}");
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
        admitted: 2,
        missing: 0,
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

fn tree_inputs(paths: impl IntoIterator<Item = String>) -> Vec<git::TreeFile> {
    let mut files = paths
        .into_iter()
        .map(|path| git::TreeFile {
            path,
            object: "a".repeat(40),
            bytes: 1,
            asset: false,
            oversized: false,
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    files
}

fn planned_leaves(files: &[git::TreeFile]) -> Vec<Vec<String>> {
    let control = OperationControl::default();
    let mut start = 0;
    let mut previous = None;
    let mut leaves = Vec::new();
    while files[start..].iter().any(|f| !f.asset && !f.oversized) {
        let end = leaf_end(files, start, previous, &control).unwrap();
        let paths = files[start..end]
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.asset && !f.oversized)
            .map(|(at, f)| {
                previous = Some(start + at);
                f.path.clone()
            })
            .collect::<Vec<_>>();
        assert!(!paths.is_empty() && paths.len() <= LEAF);
        leaves.push(paths);
        start = end;
    }
    leaves
}

// Independent full trie oracle. It partitions every oversized node by the next
// individual bit, including the terminating zero byte. It never uses a previous
// leaf, a lookahead window, shared_path_bits or the production boundary helper.
fn reference_leaves(paths: &[String]) -> Vec<Vec<String>> {
    let mut leaves = Vec::new();
    let mut pending = vec![(0, paths.len(), 0)];
    while let Some((start, end, depth)) = pending.pop() {
        if start == end {
            continue;
        }
        if end - start <= LEAF {
            leaves.push(paths[start..end].to_vec());
            continue;
        }
        let split = start
            + paths[start..end].partition_point(|path| {
                let byte = path.as_bytes().get(depth / 8).copied().unwrap_or(0);
                byte & (1 << (7 - depth % 8)) == 0
            });
        if split < end {
            pending.push((split, end, depth + 1));
        }
        if start < split {
            pending.push((start, split, depth + 1));
        }
    }
    leaves
}

fn pack_members(leaves: &[Vec<String>]) -> Vec<Vec<String>> {
    leaves
        .iter()
        .flat_map(|leaf| leaf.chunks(GROUP).map(<[String]>::to_vec))
        .collect()
}

fn changed_members(before: &[Vec<String>], after: &[Vec<String>]) -> usize {
    after
        .iter()
        .filter(|members| !before.contains(members))
        .map(Vec::len)
        .sum()
}

#[test]
fn path_leaf_scalar_plan_matches_full_binary_trie_with_prefixes_utf8_and_skips() {
    let mut paths = Vec::new();
    for prefix in ["a", "a/b", "a/b/c", "ab", "é", "界面", "😀"] {
        paths.push(prefix.to_string());
        paths.extend((0..129).map(|i| format!("{prefix}{i:04}.lua")));
    }
    paths.extend((0..513).map(|i| format!("{}{i:04}.xml", "x".repeat(4000))));
    let mut files = tree_inputs(paths);
    for (i, file) in files.iter_mut().enumerate() {
        file.asset = i % 19 == 0;
        file.oversized = i % 23 == 0;
    }
    let eligible = files
        .iter()
        .filter(|f| !f.asset && !f.oversized)
        .map(|f| f.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(planned_leaves(&files), reference_leaves(&eligible));
    assert_eq!(shared_path_bits("a", "a\u{1}"), 15);
    assert_eq!(shared_path_bits("a", "a/b"), 10);
    assert_eq!(shared_path_bits("界", "界"), usize::MAX);
    assert_eq!(GROUP, 64);
    assert_eq!(LEAF, 512);
    assert!(
        size_of::<GroupState<'_>>() + GROUP * size_of::<Record>() + 4096 + 2048
            < GROUP_RETAINED_BYTES
    );
}

#[test]
fn path_leaf_insert_delete_rename_locality_matches_independent_complete_trie() {
    let mut state = 0x627a_c5f3_u32;
    let mut next = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state
    };
    for round in 0..32 {
        let mut names = std::collections::BTreeSet::new();
        while names.len() < 513 + round * 31 {
            let prefixes = ["A/", "A/a/", "Z/", "é/", "界/", "😀/"];
            names.insert(format!(
                "{}{:08}.lua",
                prefixes[next() as usize % prefixes.len()],
                next()
            ));
        }
        let files = tree_inputs(names);
        let paths = files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        let leaves = planned_leaves(&files);
        assert_eq!(leaves, reference_leaves(&paths));
        let old = pack_members(&leaves);
        for at in [0, 1, paths.len() / 3, paths.len() / 2, paths.len() - 1] {
            let mut deleted = paths.clone();
            deleted.remove(at);
            let new_name = format!("{}-new.lua", paths[at]);
            let mut inserted = paths.clone();
            inserted.push(new_name.clone());
            inserted.sort();
            let mut renamed = deleted.clone();
            renamed.push(new_name);
            renamed.sort();
            for (changed, maximum) in [
                (deleted, LEAF),
                (inserted, LEAF + 1),
                (renamed, 2 * LEAF + 1),
            ] {
                let expected = reference_leaves(&changed);
                let actual = planned_leaves(&tree_inputs(changed));
                assert_eq!(actual, expected);
                assert!(changed_members(&old, &pack_members(&actual)) <= maximum);
            }
        }
    }
    // Exhaust all subsets of seven byte/prefix families. Their 129-path weight
    // crosses the real 512 threshold; this is not a reduced-cap toy planner.
    for mask in 1..128 {
        let families = ["a", "a/b", "ab", "b", "é", "界", "😀"];
        let files = tree_inputs(
            families
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .flat_map(|(_, prefix)| (0..129).map(move |i| format!("{prefix}{i:04}.lua"))),
        );
        let paths = files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        assert_eq!(planned_leaves(&files), reference_leaves(&paths));
    }
}

fn wide_files(count_per_side: usize) -> BTreeMap<String, String> {
    ["A", "Z"]
        .into_iter()
        .flat_map(|side| {
            (0..count_per_side).map(move |i| {
                let (extension, raw) = match i % 4 {
                    0 => ("lua", "function Full() Target() end"),
                    1 => (
                        "xml",
                        "<Ui><Frame name=\"Frame\"/><Script file=\"Full.lua\"/></Ui>",
                    ),
                    2 => ("toc", "## Interface: 120100\nFull.xml\n"),
                    _ => ("lua", "return 1 // 2"),
                };
                (format!("{side}{i:04}.{extension}"), raw.to_string())
            })
        })
        .collect()
}

fn assert_original_fact_order(before: &index::Index, after: &index::Index, except: &[&str]) {
    let common = |index: &index::Index| {
        index
            .documents
            .iter()
            .filter(|d| !except.contains(&d.path.as_str()))
            .map(|d| serde_json::json!({"path":d.path,"facts":d.facts}))
            .collect::<Vec<_>>()
    };
    assert_eq!(common(before), common(after));
}

#[test]
fn stable_path_leaves_default_consumer_preserves_full_facts_across_snapshot_edits() {
    let fixture = Fixture::new();
    let original = wide_files(512);
    let first = fixture.pin(&original);
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let cold = index::build_uncached_profiled(&fixture.home, &first, &control, &profile).unwrap();
    assert_eq!(cold.documents.len(), 1024);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 16);
    for section in [
        "declarations",
        "relationships",
        "loads",
        "headers",
        "diagnostics",
    ] {
        assert!(cold.documents.iter().any(|d| {
            !serde_json::to_value(&d.facts).unwrap()[section]
                .as_array()
                .unwrap()
                .is_empty()
        }));
    }
    let modified_name = "A0000.lua";
    let inserted_name = "000_Inserted.lua";
    let renamed_name = "Z9000.lua";
    let mut modified = original.clone();
    modified.get_mut(modified_name).unwrap().push('\n');
    let mut inserted = original.clone();
    inserted.insert(inserted_name.into(), "function Inserted() end".into());
    let mut deleted = original.clone();
    deleted.remove(modified_name);
    let mut renamed = deleted.clone();
    renamed.insert(renamed_name.into(), original[modified_name].clone());
    for (raw, exact_parsed, maximum, except) in [
        (modified, Some(64), 64, vec![modified_name]),
        (inserted, Some(1), LEAF + 1, vec![inserted_name]),
        (deleted, Some(511), LEAF, vec![modified_name]),
        (
            renamed,
            None,
            2 * LEAF + 1,
            vec![modified_name, renamed_name],
        ),
    ] {
        let pin = fixture.pin(&raw);
        let changed = index::build_uncached(&fixture.home, &pin, &control).unwrap();
        let parsed = changed.summary["io"]["parsedDocuments"].as_u64().unwrap() as usize;
        if let Some(expected) = exact_parsed {
            assert_eq!(parsed, expected);
        }
        assert!(parsed <= maximum);
        assert_eq!(
            parsed + changed.summary["io"]["cachedFacts"].as_u64().unwrap() as usize,
            raw.len()
        );
        assert_original_fact_order(&cold, &changed, &except);
        let warm = index::build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(warm.summary["io"]["parsedDocuments"], 0);
        assert_eq!(all_facts(&warm), all_facts(&changed));
    }
    let restored = index::build_uncached(&fixture.home, &first, &control).unwrap();
    assert_eq!(restored.summary["io"]["cachedFacts"], 1024);
    assert_eq!(all_facts(&restored), all_facts(&cold));
}

#[test]
fn stable_path_leaf_split_merge_corruption_and_concurrent_recovery_use_real_cache() {
    let fixture = Fixture::new();
    let original = wide_files(256);
    let first = fixture.pin(&original);
    let control = OperationControl::default();
    let cold = index::build_uncached(&fixture.home, &first, &control).unwrap();
    // Exactly 512 paths are one leaf. Insertion splits it; deletion restores the
    // complete original leaf identity, even when this crosses several unary
    // ancestors. This exercises the real count threshold and cache consumer.
    let mut inserted = original.clone();
    inserted.insert("A0000.extra.lua".into(), "function Inserted() end".into());
    let second = fixture.pin(&inserted);
    let split = index::build_uncached(&fixture.home, &second, &control).unwrap();
    assert!(split.summary["io"]["parsedDocuments"].as_u64().unwrap() <= (LEAF + 1) as u64);
    assert_original_fact_order(&cold, &split, &["A0000.extra.lua"]);
    let old = index::build_uncached(&fixture.home, &first, &control).unwrap();
    assert_eq!(old.summary["io"]["cachedFacts"], 512);
    assert_eq!(all_facts(&old), all_facts(&cold));
    // Also start from only the 513-input snapshot, so a deletion-triggered
    // ancestor merge cannot be satisfied by an older preexisting 512 cache.
    let merge_fixture = Fixture::new();
    let split_pin = merge_fixture.pin(&inserted);
    let split_only = index::build_uncached(&merge_fixture.home, &split_pin, &control).unwrap();
    assert_eq!(all_facts(&split_only), all_facts(&split));
    let merge_pin = merge_fixture.pin(&original);
    let merged = index::build_uncached(&merge_fixture.home, &merge_pin, &control).unwrap();
    let reparsed = merged.summary["io"]["parsedDocuments"].as_u64().unwrap();
    assert!(reparsed > 0 && reparsed <= LEAF as u64);
    assert_eq!(all_facts(&merged), all_facts(&cold));
    // Corrupt one complete pack. Two default builders race through authenticated
    // Git reconstruction and private atomic replacement, with unrelated leaves
    // and the old snapshot still present. Neither may publish partial Facts.
    let location = split.documents[0].facts_locator.as_ref().unwrap();
    fs::write(pack(&fixture.home, location), b"corrupt").unwrap();
    let barrier = std::sync::Barrier::new(2);
    let rebuilt = std::thread::scope(|scope| {
        let run = || {
            barrier.wait();
            index::build_uncached(&fixture.home, &second, &OperationControl::default()).unwrap()
        };
        let first = scope.spawn(run);
        let second = scope.spawn(run);
        [first.join().unwrap(), second.join().unwrap()]
    });
    for result in &rebuilt {
        assert_eq!(all_facts(result), all_facts(&split));
    }
    let warm = index::build_uncached(&fixture.home, &second, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], 513);
    assert_eq!(all_facts(&warm), all_facts(&split));
}

#[test]
fn exact_pack_leaf_boundary_skips_real_asset_and_oversized_document_before_next_leaf() {
    let fixture = Fixture::new();
    let mut raw = wide_files(512);
    raw.insert("M.png".into(), "opaque fixture asset".into());
    raw.insert("M.lua".into(), " ".repeat(git::MAX_SOURCE_BYTES + 1));
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(cold.documents.len(), 1024);
    assert_eq!(cold.summary["io"]["parsedDocuments"], 1024);
    assert_eq!(cold.summary["skippedDocuments"], 1);
    assert_eq!(cold.assets.len(), 1);
    let warm = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], 1024);
    assert_eq!(all_facts(&warm), all_facts(&cold));
}

#[test]
fn path_leaf_planning_keeps_original_cancellation_deadline_and_rejects_unsorted_inputs() {
    let files = tree_inputs((0..1024).map(|i| format!("D{i:04}.lua")));
    for control in [
        OperationControl::new(
            Instant::now() + Duration::from_secs(60),
            Arc::new(AtomicBool::new(true)),
        ),
        OperationControl::uncancelable(Instant::now() - Duration::from_secs(1)),
    ] {
        assert!(leaf_end(&files, 0, None, &control).is_err());
    }
    let mut unsorted = files;
    unsorted.swap(1, 2);
    assert_eq!(
        leaf_end(&unsorted, 0, None, &OperationControl::default())
            .unwrap_err()
            .code,
        "input.invalid"
    );
}

#[test]
fn legacy_ordinal_pack_and_v13_index_keep_identity_while_current_default_rebuilds() {
    let fixture = Fixture::new();
    let raw = files(3);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let mut old = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    let expected = all_facts(&old);
    let current_pack = pack(
        &fixture.home,
        old.documents[0].facts_locator.as_ref().unwrap(),
    );
    let old_pack_bytes = fs::read(&current_pack).unwrap();
    let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
    let mut hash = Sha256::new();
    hash.update(b"facts-pack-v1\0facts-layout-v11-go323-generated-origins\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0");
    for file in &inputs {
        for value in [file.path.as_bytes(), file.object.as_bytes()] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value);
        }
        hash.update((file.bytes as u64).to_le_bytes());
    }
    let old_group: [u8; 32] = hash.finalize().into();
    for doc in &mut old.documents {
        doc.facts_locator.as_mut().unwrap().group = old_group;
    }
    let old_pack = pack(
        &fixture.home,
        old.documents[0].facts_locator.as_ref().unwrap(),
    );
    assert_ne!(old_pack, current_pack);
    fs::write(&old_pack, &old_pack_bytes).unwrap();
    fs::remove_file(&current_pack).unwrap();
    for doc in &old.documents {
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
    let old_identity = storage::sha256(format!("rust-index-v13\0facts-pack-v1\0facts-layout-v11-go323-generated-origins\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}",pin.repository,pin.product,pin.exact_commit,pin.parser_revision).as_bytes());
    assert_ne!(old_identity, index::fingerprint(&pin));
    let old_bytes = serde_json::to_vec(&old).unwrap();
    let old_blob = lycheedev_core::cache::publish_derived_controlled(
        &fixture.home,
        &old_bytes,
        128 << 20,
        &control,
    )
    .unwrap();
    let old_key = format!("source/rust-index/{old_identity}");
    let manifest = serde_json::json!({"schema":"lycheedev.source-index.rust.v6","identity":old_identity,"blob":old_blob});
    crate::persistence::put_manifest(&fixture.home, &old_key, &manifest, &control).unwrap();
    assert!(!index::is_ready_with_control(&fixture.home, &pin, &control).unwrap());
    let rebuilt = index::build_with_control(&fixture.home, &pin, &control).unwrap();
    assert_eq!(rebuilt.summary["io"]["parsedDocuments"], 3);
    assert_eq!(rebuilt.summary["io"]["cachedFacts"], 0);
    assert_eq!(all_facts(&rebuilt), expected);
    assert_eq!(fs::read(old_pack).unwrap(), old_pack_bytes);
    let warm = index::build_with_control(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedIndex"], true);
    assert_eq!(warm.summary["io"]["cachedFacts"], 3);
}

#[test]
fn cache_aware_default_consumer_avoids_all_workers_on_complete_pack_hits() {
    let fixture = Fixture::new();
    let raw = wide_files(40);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cold_profile = Profile::enabled(true);
    let cold =
        index::build_uncached_profiled(&fixture.home, &pin, &control, &cold_profile).unwrap();
    assert_eq!(cold.summary["io"]["parsedDocuments"], 80);
    if std::thread::available_parallelism().is_ok_and(|n| n.get() >= 2) {
        assert_eq!(cold_profile.calls(Phase::FactsWorker), 80);
    }
    let profile = Profile::enabled(true);
    // This is the real index rebuild consumer, not the whole-index shortcut.
    let warm = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(warm.summary["io"]["cachedFacts"], 80);
    assert_eq!(profile.calls(Phase::FactsWorker), 0);
    assert_eq!(profile.calls(Phase::FactsWorkerWait), 0);
    assert_eq!(profile.calls(Phase::FactsParse), 0);
    assert_eq!(all_facts(&cold), all_facts(&warm));
    for section in [
        "declarations",
        "relationships",
        "loads",
        "headers",
        "diagnostics",
    ] {
        assert!(cold.documents.iter().any(|d| {
            !serde_json::to_value(&d.facts).unwrap()[section]
                .as_array()
                .unwrap()
                .is_empty()
        }));
    }
}

#[test]
fn cache_aware_default_consumer_dispatches_only_missing_snapshot_groups() {
    let fixture = Fixture::new();
    let raw = wide_files(512);
    let original = fixture.pin(&raw);
    let control = OperationControl::default();
    let baseline = index::build_uncached(&fixture.home, &original, &control).unwrap();
    let mut modified = raw.clone();
    modified.get_mut("A0000.lua").unwrap().push('\n');
    let mut inserted = raw.clone();
    inserted.insert("000_Inserted.lua".into(), "function Inserted() end".into());
    let mut deleted = raw.clone();
    deleted.remove("A0000.lua");
    let mut renamed = deleted.clone();
    renamed.insert("Z9000.lua".into(), raw["A0000.lua"].clone());
    for (changed, expected, except) in [
        (modified, Some(64), vec!["A0000.lua"]),
        (inserted, Some(1), vec!["000_Inserted.lua"]),
        (deleted, Some(511), vec!["A0000.lua"]),
        (renamed, None, vec!["A0000.lua", "Z9000.lua"]),
    ] {
        let pin = fixture.pin(&changed);
        let profile = Profile::enabled(true);
        let rebuilt =
            index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
        let parsed = rebuilt.summary["io"]["parsedDocuments"].as_u64().unwrap();
        if let Some(expected) = expected {
            assert_eq!(parsed, expected);
        }
        assert!(parsed <= 1025);
        assert_eq!(profile.calls(Phase::FactsParse), parsed);
        // No generated/large/capped parser inputs in this fixture. On a
        // parallel host, exactly the real miss set can enter pure workers.
        if std::thread::available_parallelism().is_ok_and(|n| n.get() >= 2) {
            assert_eq!(
                profile.calls(Phase::FactsWorker) + profile.calls(Phase::FactsParseSequential),
                parsed
            );
            assert_eq!(
                profile.calls(Phase::FactsWorkerWait),
                profile.calls(Phase::FactsWorker)
            );
            if expected.is_some() {
                assert_eq!(profile.calls(Phase::FactsWorker), parsed);
            }
        }
        assert_original_fact_order(&baseline, &rebuilt, &except);
    }
}

fn admitted_document(
    groups: &mut Groups<'_>,
    file: &git::TreeFile,
    raw: &[u8],
    docs: &mut Vec<Document>,
) -> Result<bool> {
    let sha256 = storage::sha256(raw);
    let (facts, hit, facts_locator) = groups.document(file, raw, &sha256, None, docs)?;
    docs.push(Document {
        path: file.path.clone(),
        text: Some(raw.to_vec()),
        sha256,
        bytes: raw.len(),
        facts,
        facts_locator,
    });
    Ok(hit)
}

#[test]
fn cache_aware_legacy_presence_rechecks_same_key_replacement_and_growth() {
    use facts_pipeline::Admission;
    for change in ["replace", "grow", "remove"] {
        let fixture = Fixture::new();
        let raw = files(2);
        let pin = fixture.pin(&raw);
        let control = OperationControl::default();
        let profile = Profile::enabled(true);
        let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
        let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
        let body = raw[&inputs[0].path].as_bytes();
        let sha = storage::sha256(body);
        let key = facts_cache_key(&inputs[0].path, &sha);
        let expected = crate::facts::ParseMode::for_path(&inputs[0].path).parse(body);
        let envelope = encode_facts(&expected, &sha, None).unwrap();
        cache.publish(&key, &envelope, &control).unwrap();
        let path = fixture
            .home
            .join("cache/source-facts-rust")
            .join(format!("{key}.json"));
        let mut groups = Groups::new(&cache, &inputs, &control, &profile);
        assert!(matches!(
            groups.admit(&inputs[0], body, true),
            Admission::Barrier
        ));
        match change {
            "replace" => fs::write(&path, b"different complete bytes").unwrap(),
            "grow" => {
                use std::io::Write;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(b" trailing garbage")
                    .unwrap();
            }
            _ => fs::remove_file(&path).unwrap(),
        }
        let mut docs = Vec::new();
        assert!(!admitted_document(&mut groups, &inputs[0], body, &mut docs).unwrap());
        assert_eq!(
            serde_json::to_value(&docs[0].facts).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        admitted_document(
            &mut groups,
            &inputs[1],
            raw[&inputs[1].path].as_bytes(),
            &mut docs,
        )
        .unwrap();
        groups.finish(Ok(())).unwrap();
        cache.finish(Ok(()), &control).unwrap();
    }
}

#[test]
fn cache_aware_later_probe_error_is_bound_to_its_own_document() {
    use facts_pipeline::Admission;
    let fixture = Fixture::new();
    let raw = files(2);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let second = raw[&inputs[1].path].as_bytes();
    let key = facts_cache_key(&inputs[1].path, &storage::sha256(second));
    fs::create_dir(
        fixture
            .home
            .join("cache/source-facts-rust")
            .join(format!("{key}.json")),
    )
    .unwrap();
    let mut groups = Groups::new(&cache, &inputs, &control, &profile);
    let first = raw[&inputs[0].path].as_bytes();
    assert!(matches!(
        groups.admit(&inputs[0], first, true),
        Admission::Parse
    ));
    assert!(matches!(
        groups.admit(&inputs[1], second, false),
        Admission::Barrier
    ));
    assert_eq!(groups.admission_error.as_ref().unwrap().0, 1);
    let mut docs = Vec::new();
    assert!(!admitted_document(&mut groups, &inputs[0], first, &mut docs).unwrap());
    assert_eq!(docs.len(), 1);
    assert_eq!(profile.calls(Phase::FactsParse), 1);
    // The later probe must not be consumed by the earlier pending document.
    let expected = groups.admission_error.as_ref().unwrap().1.code.clone();
    let error = admitted_document(&mut groups, &inputs[1], second, &mut docs).unwrap_err();
    assert_eq!(error.code, expected);
    assert_eq!(groups.finish::<()>(Err(error)).unwrap_err().code, expected);
    cache.finish(Ok(()), &control).unwrap();
    assert!(
        !fs::read_dir(fixture.home.join("cache/source-facts-rust"))
            .unwrap()
            .any(|p| p
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".lycheedev-cache-"))
    );
}

#[test]
fn cache_aware_pipeline_preserves_earlier_consumer_failure_over_later_probe_error() {
    let fixture = Fixture::new();
    let raw = files(3);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let key = facts_cache_key(
        &inputs[1].path,
        &storage::sha256(raw[&inputs[1].path].as_bytes()),
    );
    fs::create_dir(
        fixture
            .home
            .join("cache/source-facts-rust")
            .join(format!("{key}.json")),
    )
    .unwrap();
    let mut groups = Groups::new(&cache, &inputs, &control, &profile);
    let mut docs = Vec::new();
    let mut consumed = Vec::new();
    let result = facts_pipeline::visit_with_cpu_limit_and_retained(
        &fixture.home,
        &pin,
        &inputs,
        &control,
        &profile,
        2,
        GROUP_RETAINED_BYTES,
        groups.consumer(|groups, file, data, prepared| {
            consumed.push(file.path.clone());
            let sha = storage::sha256(&data);
            let (facts, _, facts_locator) =
                groups.document(file, &data, &sha, prepared, &mut docs)?;
            docs.push(Document {
                path: file.path.clone(),
                text: Some(data),
                sha256: sha,
                bytes: file.bytes,
                facts,
                facts_locator,
            });
            // An earlier ordered publication/consumer error must still win
            // after the later file's real cache-directory probe has failed.
            Err(ToolkitError::new(
                "fixture.earlier_consumer",
                "first document failed after Facts",
                5,
            ))
        }),
    );
    assert_eq!(
        result.as_ref().unwrap_err().code,
        "fixture.earlier_consumer"
    );
    assert_eq!(consumed, [inputs[0].path.clone()]);
    assert_eq!(profile.calls(Phase::FactsWorker), 1);
    assert_eq!(groups.admission_error.as_ref().unwrap().0, 1);
    assert_eq!(
        groups.finish(result).unwrap_err().code,
        "fixture.earlier_consumer"
    );
    cache.finish(Ok(()), &control).unwrap();
    assert!(
        !fs::read_dir(fixture.home.join("cache/source-facts-rust"))
            .unwrap()
            .any(|p| p
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".lycheedev-cache-"))
    );
}

#[test]
fn cache_aware_cancelled_pipeline_joins_and_recovers_without_losing_committed_groups() {
    let fixture = Fixture::new();
    let raw = files(80);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let profile = Profile::enabled(true);
    let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
    let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
    let mut groups = Groups::new(&cache, &inputs, &control, &profile);
    let mut docs = Vec::new();
    let result = facts_pipeline::visit_with_cpu_limit_and_retained(
        &fixture.home,
        &pin,
        &inputs,
        &control,
        &profile,
        2,
        GROUP_RETAINED_BYTES,
        groups.consumer(|groups, file, data, prepared| {
            control.check()?;
            let sha = storage::sha256(&data);
            let (facts, _, facts_locator) =
                groups.document(file, &data, &sha, prepared, &mut docs)?;
            docs.push(Document {
                path: file.path.clone(),
                text: Some(data),
                sha256: sha,
                bytes: file.bytes,
                facts,
                facts_locator,
            });
            if docs.len() == 65 {
                assert!(
                    profile.calls(Phase::FactsWorker) > 0,
                    "cancellation fixture must consume a real worker result before stopping"
                );
                cancel.cancel();
            }
            Ok(())
        }),
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, control.check().unwrap_err().code);
    assert_eq!(docs.len(), 65);
    assert_eq!(profile.calls(Phase::FactsPublishRename), 1);
    assert!(groups.finish::<()>(Err(error)).is_err());
    // finish closes the held native owners even when its original control is
    // already stopped; every pure worker was joined before visit returned.
    assert!(cache.finish(Ok(()), &control).is_err());
    let entries = fs::read_dir(fixture.home.join("cache/source-facts-rust"))
        .unwrap()
        .map(|p| p.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert!(
        !entries[0]
            .to_string_lossy()
            .starts_with(".lycheedev-cache-")
    );
    let live = OperationControl::default();
    let recovered = index::build_uncached(&fixture.home, &pin, &live).unwrap();
    assert_eq!(recovered.summary["io"]["cachedFacts"], 64);
    assert_eq!(recovered.summary["io"]["parsedDocuments"], 16);
    let profile = Profile::enabled(true);
    let warm = index::build_uncached_profiled(&fixture.home, &pin, &live, &profile).unwrap();
    assert_eq!(profile.calls(Phase::FactsWorker), 0);
    assert_eq!(all_facts(&warm), all_facts(&recovered));
}

#[test]
fn cache_aware_non_worker_rows_and_corrupt_record_keep_original_order() {
    let fixture = Fixture::new();
    let mut raw = files(66);
    raw.insert(
        "D000a.lua".into(),
        format!("--{}\nfunction Large() end", "x".repeat(65 << 10)),
    );
    raw.insert("D030a.png".into(), "opaque asset".into());
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let baseline = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(baseline.documents.len(), 67);
    let location = baseline.documents[63].facts_locator.as_ref().unwrap();
    let path = pack(&fixture.home, location);
    let mut bytes = fs::read(&path).unwrap();
    bytes[location.offset as usize + HEADER] ^= 0xff;
    fs::write(&path, bytes).unwrap();
    let profile = Profile::enabled(true);
    let repaired = index::build_uncached_profiled(&fixture.home, &pin, &control, &profile).unwrap();
    assert_eq!(repaired.summary["io"]["parsedDocuments"], 1);
    assert_eq!(repaired.summary["io"]["cachedFacts"], 66);
    assert_eq!(profile.calls(Phase::FactsWorker), 0);
    assert_eq!(profile.calls(Phase::FactsParseSequential), 1);
    assert_eq!(all_facts(&repaired), all_facts(&baseline));
}

#[test]
fn cache_aware_current_retention_and_encoding_prelude_precedes_deferred_cache_error() {
    use facts_pipeline::Admission;
    for overflow in [false, true] {
        let fixture = Fixture::new();
        let raw = files(2);
        let pin = fixture.pin(&raw);
        let control = OperationControl::default();
        let profile = Profile::enabled(true);
        let inputs = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
        let cache = SourceFactsCache::open(&fixture.home, &control).unwrap();
        let body = raw[&inputs[0].path].as_bytes();
        let key = facts_cache_key(&inputs[0].path, &storage::sha256(body));
        fs::create_dir(
            fixture
                .home
                .join("cache/source-facts-rust")
                .join(format!("{key}.json")),
        )
        .unwrap();
        let mut groups = Groups::new(&cache, &inputs, &control, &profile);
        let mut docs = Vec::new();
        if overflow {
            assert!(matches!(
                groups.admit(&inputs[0], body, true),
                Admission::Barrier
            ));
            assert!(groups.admission_error.is_some());
        } else {
            // Invalid UTF-8 itself is a barrier and never probes the bad cache.
            assert!(matches!(
                groups.admit(&inputs[0], b"\xff", true),
                Admission::Barrier
            ));
            assert!(groups.admission_error.is_none());
        }
        let mut retained = if overflow { 256 << 20 } else { 0 };
        let data: &[u8] = if overflow { body } else { b"\xff" };
        // This is the same prelude invoked by the default index consumer,
        // using a synthetic retained counter rather than allocating 256 MiB.
        let result = index::document_input_sha(&inputs[0], data, &mut retained)
            .and_then(|sha| groups.document(&inputs[0], data, &sha, None, &mut docs));
        let expected = if overflow {
            "codebase.source_budget"
        } else {
            "command.failed"
        };
        assert_eq!(result.as_ref().unwrap_err().code, expected);
        assert_eq!(groups.finish(result).unwrap_err().code, expected);
        cache.finish(Ok(()), &control).unwrap();
        assert_eq!(profile.calls(Phase::FactsParse), 0);
    }
}
