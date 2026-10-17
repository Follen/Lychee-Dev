use super::*;
use crate::{
    facts::Facts,
    index::{self, facts_cache_tests::Fixture},
};
use lycheedev_core::{control::OperationControl, storage};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
};

fn empty() -> Index {
    Index {
        documents: Vec::new(),
        assets: Vec::new(),
        symbols: Vec::new(),
        summary: json!({}),
        compatibility_gaps: BTreeSet::new(),
        home: PathBuf::from("not-persisted"),
    }
}

fn historical_value(index: &Index) -> Value {
    let mut value = serde_json::to_value(index).unwrap();
    value["summary"].as_object_mut().unwrap().remove("io");
    value
}

fn identical(index: &Index) -> Vec<u8> {
    let before = serde_json::to_value(index).unwrap();
    let expected = serde_json::to_vec(&historical_value(index)).unwrap();
    let actual = encode(index).unwrap();
    assert_eq!(actual, expected, "complete canonical bytes must not change");
    assert_eq!(
        serde_json::to_value(index).unwrap(),
        before,
        "borrowed encoding must not mutate the live index"
    );
    actual
}

fn symbol(mask: usize) -> Symbol {
    Symbol {
        id: if mask & 1 != 0 {
            "SYM-é雪\"".into()
        } else {
            String::new()
        },
        kind: ["declaration", "relationship", "load", "header"][mask % 4].into(),
        name: "名\n\0é\\".into(),
        target: if mask & 2 != 0 {
            "目标".into()
        } else {
            String::new()
        },
        category: "function".into(),
        confidence: "exact".into(),
        path: "目录/é雪.lua".into(),
        line: usize::MAX,
        end_line: 0,
        signature: if mask & 4 != 0 {
            "F(é, 雪)".into()
        } else {
            String::new()
        },
        scope: if mask & 8 != 0 {
            "local".into()
        } else {
            String::new()
        },
        ordinal: if mask & 16 != 0 { usize::MAX } else { 0 },
    }
}

#[test]
fn borrowed_index_empty_bytes_and_summary_io_omission_match_historical_wire() {
    let mut index = empty();
    assert_eq!(
        identical(&index),
        br#"{"assets":[],"compatibilityGaps":[],"documents":[],"summary":{},"symbols":[]}"#
    );
    index.summary = json!({"io": {"elapsedMs": 99}, "nested": {"io": null}, "null": null});
    let raw = identical(&index);
    let value: Value = serde_json::from_slice(&raw).unwrap();
    assert!(value["summary"].get("io").is_none());
    assert!(value["summary"]["nested"].get("io").unwrap().is_null());
    assert_eq!(index.summary["io"]["elapsedMs"], 99);
}

#[test]
fn borrowed_index_all_symbol_omissions_unicode_order_and_duplicates_match() {
    let mut index = empty();
    index.symbols = (0..32).rev().map(symbol).collect();
    index.symbols.push(symbol(31));
    index.symbols.push(symbol(31));
    index.compatibility_gaps = BTreeSet::from(["xml".into(), "lua".into(), "é雪".into()]);
    let raw = identical(&index);
    let value: Value = serde_json::from_slice(&raw).unwrap();
    let symbols = value["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 34);
    assert_eq!(symbols[0], symbols[32]);
    assert_eq!(symbols[32], symbols[33]);
    for field in ["id", "target", "signature", "scope", "ordinal"] {
        assert!(symbols[31].get(field).is_none(), "{field}");
        assert!(symbols[0].get(field).is_some(), "{field}");
    }
}

#[test]
fn borrowed_index_document_and_locator_bytes_match_all_digest_digits() {
    let mut index = empty();
    for locator in [
        None,
        Some(Locator {
            group: std::array::from_fn(|i| (i * 8) as u8),
            offset: u64::MAX,
            bytes: u32::MAX,
            sha256: std::array::from_fn(|i| 255 - i as u8),
        }),
    ] {
        index.documents.push(Document {
            path: "文档/é\".lua".into(),
            text: Some(b"function Excluded() end".to_vec()),
            sha256: "a".repeat(64),
            bytes: usize::MAX,
            facts: crate::facts::lua(b"function Excluded() end", false),
            facts_locator: locator,
        });
    }
    let raw = identical(&index);
    let value: Value = serde_json::from_slice(&raw).unwrap();
    assert!(value["documents"][0].get("factsLocator").is_none());
    assert_eq!(value["documents"][1]["factsLocator"]["offset"], u64::MAX);
    for document in value["documents"].as_array().unwrap() {
        assert!(document.get("text").is_none());
        assert!(document.get("facts").is_none());
    }
}

#[test]
fn borrowed_index_existing_values_preserve_number_lexemes_nulls_and_deep_fields() {
    let mut index = empty();
    // These are existing Value numbers, not floats reconstructed from text by
    // the persistence projection. The workspace enables arbitrary_precision.
    let numbers: Value = serde_json::from_str(
        r#"[0,-0,1E+02,1e+09,1.2300,18446744073709551616,-18446744073709551617,1e-400]"#,
    )
    .unwrap();
    index.assets = vec![
        numbers.clone(),
        Value::Null,
        json!({"z": null, "a": {"io": "retained"}}),
    ];
    for number in [
        0.0,
        -0.0,
        1e20,
        1e-20,
        f64::MIN,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        index
            .assets
            .push(Value::Number(serde_json::Number::from_f64(number).unwrap()));
    }
    let mut deep = json!({"io": null, "numbers": numbers, "text": "é雪\u{2028}\u{2029}\u{1}"});
    for depth in 0..48 {
        deep = json!({"z": depth, "a": [deep, null]});
    }
    index.summary = json!({"io": {"notPersisted": true}, "deep": deep, "empty": {}, "array": []});
    let raw = identical(&index);
    let value: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(value["assets"], json!(index.assets));
    assert_eq!(value["summary"]["deep"], index.summary["deep"]);
}

#[test]
fn borrowed_index_invalid_summary_keeps_required_object_contract() {
    for summary in [Value::Null, json!([]), json!(1), json!("text")] {
        let mut index = empty();
        index.summary = summary;
        assert!(std::panic::catch_unwind(|| historical_value(&index)).is_err());
        assert!(std::panic::catch_unwind(|| encode(&index)).is_err());
    }
}

#[test]
fn borrowed_index_private_marker_objects_remain_ordinary_value_maps() {
    let mut index = empty();
    index.assets = vec![
        json!({"$serde_json::private::Number": "1E02"}),
        json!({"$serde_json::private::RawValue": "[1,2]"}),
        json!({"$serde_json::private::Number": "invalid", "other": null}),
    ];
    index.summary = json!({"nested": {"$serde_json::private::Number": "1E02"}});
    // Compare the serializer contract only. Parsing these reserved-key objects
    // back through Value's deserializer would exercise a different contract.
    identical(&index);
}

struct FailingWriter {
    raw: Vec<u8>,
    remaining: usize,
}
impl io::Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("fixed writer failure"));
        }
        let size = bytes.len().min(self.remaining);
        self.raw.extend_from_slice(&bytes[..size]);
        self.remaining -= size;
        Ok(size)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn borrowed_index_writer_errors_and_restricted_capacity_keep_exact_prefixes() {
    let mut index = empty();
    index.symbols = (0..32).map(symbol).collect();
    let expected = identical(&index);
    let old = historical_value(&index);
    for capacity in [0, 1, 17, expected.len() / 2, expected.len() - 1] {
        let mut new_writer = FailingWriter {
            raw: Vec::new(),
            remaining: capacity,
        };
        let mut old_writer = FailingWriter {
            raw: Vec::new(),
            remaining: capacity,
        };
        let new_error = serde_json::to_writer(&mut new_writer, &IndexWire(&index)).unwrap_err();
        let old_error = serde_json::to_writer(&mut old_writer, &old).unwrap_err();
        assert!(new_error.is_io());
        assert_eq!(new_error.to_string(), old_error.to_string());
        assert_eq!(new_writer.raw, old_writer.raw);
        assert_eq!(new_writer.raw, expected[..capacity]);
        let mut fixed = vec![0; capacity];
        let error = serde_json::to_writer(fixed.as_mut_slice(), &IndexWire(&index)).unwrap_err();
        assert_eq!(error.io_error_kind(), Some(io::ErrorKind::WriteZero));
        assert_eq!(fixed, expected[..capacity]);
    }
    let mut fixed = vec![0; expected.len()];
    serde_json::to_writer(fixed.as_mut_slice(), &IndexWire(&index)).unwrap();
    assert_eq!(fixed, expected);
    assert_eq!(encode(&index).unwrap(), expected);
}

#[test]
fn borrowed_index_real_default_cold_warm_and_persisted_blob_match_historical_bytes() {
    let fixture = Fixture::new();
    let raw = BTreeMap::from([
        ("Main.lua".into(), "function Run(a) Target(a) end\n".into()),
        (
            "Main.xml".into(),
            "<Ui><Frame name=\"Frame\"/><Script file=\"Main.lua\"/></Ui>".into(),
        ),
        (
            "Main.toc".into(),
            "## Interface: 120100\n## Title: é雪\nMain.lua\nMain.xml\n".into(),
        ),
        ("Bad.lua".into(), "return 1 // 2".into()),
    ]);
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let (cold, digest) = index::build_material_with_control(&fixture.home, &pin, &control).unwrap();
    assert_eq!(cold.summary["io"]["parsedDocuments"], 4);
    assert!(
        cold.documents
            .iter()
            .all(|document| document.facts_locator.is_some())
    );
    assert!(!cold.symbols.is_empty());
    assert!(!cold.compatibility_gaps.is_empty());
    assert!(
        cold.documents
            .iter()
            .any(|document| !document.facts.diagnostics.is_empty())
    );
    let bytes = identical(&cold);
    assert_eq!(digest, storage::sha256(&bytes));
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let key = format!("source/rust-index/{}", index::fingerprint(&pin));
    let manifest: Value = crate::persistence::read(&fixture.home, &key, &control).unwrap();
    assert_eq!(manifest["blob"]["sha256"], digest);
    assert_eq!(manifest["blob"]["bytes"], bytes.len());
    let (warm, warm_digest) =
        index::build_material_with_control(&fixture.home, &pin, &control).unwrap();
    assert_eq!(warm.summary["io"]["cachedIndex"], true);
    assert_eq!(warm_digest, digest);
    assert_eq!(identical(&warm), bytes);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn borrowed_index_full_bytes_retain_publication_budget_cancel_and_integrity_checks() {
    use lycheedev_core::{cache::publish_derived_controlled, control::TrackedCancellation};
    use std::time::{Duration, Instant};
    let fixture = Fixture::new();
    let mut index = empty();
    index.summary = json!({"padding": "é雪".repeat(20000)});
    let bytes = identical(&index);
    assert!(bytes.len() > 65536);
    let control = OperationControl::default();
    let error =
        publish_derived_controlled(&fixture.home, &bytes, bytes.len() - 1, &control).unwrap_err();
    assert_eq!(error.code, "vault.cache_limit");
    let token = TrackedCancellation::new();
    let cancelled =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    token.cancel();
    assert_eq!(
        publish_derived_controlled(&fixture.home, &bytes, 128 << 20, &cancelled)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    let blob = publish_derived_controlled(&fixture.home, &bytes, 128 << 20, &control).unwrap();
    assert_eq!(blob["bytes"], bytes.len());
    assert_eq!(blob["sha256"], storage::sha256(&bytes));
    assert_eq!(
        publish_derived_controlled(&fixture.home, &bytes, 128 << 20, &control).unwrap(),
        blob
    );
    let digest = blob["sha256"].as_str().unwrap();
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    std::fs::write(&path, b"changed").unwrap();
    assert_eq!(
        publish_derived_controlled(&fixture.home, &bytes, 128 << 20, &control)
            .unwrap_err()
            .code,
        "vault.cache_integrity"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"changed");
}

#[test]
fn borrowed_index_persisted_fields_do_not_include_owned_source_or_facts() {
    let mut index = empty();
    index.documents.push(Document {
        path: "Empty.lua".into(),
        text: None,
        sha256: "0".repeat(64),
        bytes: 0,
        facts: Facts::default(),
        facts_locator: None,
    });
    let first = identical(&index);
    index.documents[0].text = Some(b"not a persisted field".to_vec());
    index.documents[0].facts = crate::facts::lua(b"function NotPersisted() end", false);
    index.home = PathBuf::from("another-home");
    assert_eq!(identical(&index), first);
}
