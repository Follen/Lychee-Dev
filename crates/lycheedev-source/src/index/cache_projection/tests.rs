use super::*;
use crate::index::{self, facts_cache_tests::Fixture};
use lycheedev_core::{Invocation, Outcome, evidence, selection, storage};
use serde::Serialize;
use serde_json::json;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::Instant,
};

// Freeze the previous full root schema independently of the new generic root.
// Keep its actual Rust name: serde's struct error text uses this identifier.
mod historical {
    use super::*;
    #[derive(Serialize, Deserialize)]
    pub(super) struct Index {
        pub documents: Vec<Document>,
        pub assets: Vec<Value>,
        pub symbols: Vec<Symbol>,
        pub summary: Value,
        #[serde(rename = "compatibilityGaps")]
        pub compatibility_gaps: BTreeSet<String>,
        #[serde(skip)]
        pub home: PathBuf,
    }
}

fn payload() -> Value {
    json!({
        "documents": [{"path":"目录/雪.lua","sha256":"a".repeat(64),"bytes":17,
            "factsLocator":{"group":"0123456789abcdef".repeat(4),"offset":u64::MAX,
                "bytes":u32::MAX,"sha256":"fedcba9876543210".repeat(4)}}],
        "assets": [null, true, 17, -7, 1.25, "雪\n\u{0}", {"array":[false,null]}],
        "symbols": [{"id":"SYM-é","kind":"declaration","name":"雪","target":"T",
            "category":"function","confidence":"exact","path":"目录/雪.lua",
            "line":usize::MAX,"endLine":0,"signature":"F(é)","scope":"local","ordinal":usize::MAX}],
        "summary": {"schema":"lycheedev.source-map.v2","repository":"elvui","product":"main",
            "commit":"1".repeat(40),"parser":"source-v1","documents":999,
            "skippedDocuments":0,"diagnostics":1,"diagnosticSample":[{"message":"雪","line":2}],
            "complete":false,"io":{"unknown":null},"coverage":null},
        "compatibilityGaps": ["xml", "lua", "xml", "unknown"]
    })
}

fn raw(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn same_error(actual: serde_json::Error, expected: &serde_json::Error) {
    assert_eq!(actual.to_string(), expected.to_string());
    assert_eq!(actual.line(), expected.line());
    assert_eq!(actual.column(), expected.column());
    assert_eq!(actual.classify(), expected.classify());
}

fn same_decode(bytes: &[u8]) -> Option<SummaryIndex> {
    let expected = serde_json::from_slice::<historical::Index>(bytes);
    let full = serde_json::from_slice::<Index>(bytes);
    let projected = serde_json::from_slice::<SummaryIndex>(bytes);
    match (expected, full, projected) {
        (Ok(expected), Ok(full), Ok(projected)) => {
            assert_eq!(
                serde_json::to_value(&full).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(full.home, expected.home);
            assert_eq!(projected.home, expected.home);
            assert_eq!(projected.documents.len(), expected.documents.len());
            assert_eq!(projected.assets.len(), expected.assets.len());
            assert_eq!(projected.symbols.len(), expected.symbols.len());
            assert_eq!(projected.summary, expected.summary);
            assert_eq!(projected.compatibility_gaps, expected.compatibility_gaps);
            assert_eq!(
                index::coverage(&projected.summary, projected.documents.len()),
                full.coverage()
            );
            assert!(
                full.documents
                    .iter()
                    .all(|document| document.text.is_none())
            );
            Some(projected)
        }
        (Err(expected), Err(full), Err(projected)) => {
            same_error(full, &expected);
            same_error(projected, &expected);
            None
        }
        _ => panic!(
            "full historical and projected acceptance differ for {} bytes",
            bytes.len()
        ),
    }
}

#[test]
fn warm_summary_schema_matches_full_fields_order_duplicates_and_sequence_form() {
    let mut value = payload();
    for field in ["documents", "assets", "symbols"] {
        let rows = value[field].as_array_mut().unwrap();
        rows.push(rows[0].clone());
    }
    assert!(same_decode(&raw(&value)).is_some());
    let reversed = format!(
        "{{\"symbols\":{},\"summary\":{},\"documents\":{},\"compatibilityGaps\":{},\"assets\":{}}}",
        value["symbols"],
        value["summary"],
        value["documents"],
        value["compatibilityGaps"],
        value["assets"]
    );
    assert!(same_decode(reversed.as_bytes()).is_some());
    let sequence = json!([
        value["documents"],
        value["assets"],
        value["symbols"],
        value["summary"],
        value["compatibilityGaps"]
    ]);
    assert!(same_decode(&raw(&sequence)).is_some());
    for length in 0..5 {
        let shortened = Value::Array(sequence.as_array().unwrap()[..length].to_vec());
        assert!(same_decode(&raw(&shortened)).is_none());
    }
    let mut longer = sequence;
    longer.as_array_mut().unwrap().push(json!("extra"));
    assert!(same_decode(&raw(&longer)).is_none());
    assert!(same_decode(b"{}").is_none());
    for token in ["null", "false", "1", "\"text\""] {
        assert!(same_decode(token.as_bytes()).is_none());
    }
}

#[test]
fn warm_summary_required_duplicate_and_wrong_collection_errors_match_full() {
    let value = payload();
    for field in [
        "documents",
        "assets",
        "symbols",
        "summary",
        "compatibilityGaps",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        // A missing Value field follows serde's historical missing-field rule.
        same_decode(&raw(&missing));
        for token in ["null", "false", "1", "\"text\"", "{}"] {
            if field != "summary" {
                let mut invalid = value.clone();
                invalid[field] = serde_json::from_str(token).unwrap();
                assert!(same_decode(&raw(&invalid)).is_none());
            }
            let encoded = serde_json::to_string(&value).unwrap();
            let duplicate = format!("{},\"{field}\":{token}}}", &encoded[..encoded.len() - 1]);
            assert!(same_decode(duplicate.as_bytes()).is_none());
        }
    }
    for field in ["home", "unknown"] {
        let encoded = serde_json::to_string(&value).unwrap();
        let ignored = format!(
            "{},\"{field}\":false,\"{field}\":{{\"deep\":[1,null]}}}}",
            &encoded[..encoded.len() - 1]
        );
        assert!(same_decode(ignored.as_bytes()).is_some());
    }
}

#[test]
fn warm_summary_decodes_all_document_symbol_and_locator_business_fields() {
    let value = payload();
    for (collection, fields) in [
        ("documents", vec!["path", "sha256", "bytes", "factsLocator"]),
        (
            "symbols",
            vec![
                "id",
                "kind",
                "name",
                "target",
                "category",
                "confidence",
                "path",
                "line",
                "endLine",
                "signature",
                "scope",
                "ordinal",
            ],
        ),
    ] {
        for field in fields {
            let mut missing = value.clone();
            missing[collection][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            same_decode(&raw(&missing));
            for replacement in [
                Value::Null,
                json!(false),
                json!(-1),
                json!(1.25),
                json!("text"),
                json!([]),
                json!({}),
            ] {
                let mut modified = value.clone();
                modified[collection][0][field] = replacement;
                same_decode(&raw(&modified));
            }
            let encoded = value[collection][0].to_string();
            let duplicate = format!("{},\"{field}\":null}}", &encoded[..encoded.len() - 1]);
            let mut root = value.clone();
            root[collection] = json!([]);
            let root = root.to_string().replace(
                &format!("\"{collection}\":[]"),
                &format!("\"{collection}\":[{duplicate}]"),
            );
            assert!(same_decode(root.as_bytes()).is_none());
        }
    }
    for field in ["group", "offset", "bytes", "sha256"] {
        let mut missing = value.clone();
        missing["documents"][0]["factsLocator"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(same_decode(&raw(&missing)).is_none());
        for replacement in [
            Value::Null,
            json!(-1),
            json!(1.5),
            json!("A".repeat(64)),
            json!("a".repeat(63)),
            json!("g".repeat(64)),
            json!(u64::MAX),
        ] {
            let mut modified = value.clone();
            modified["documents"][0]["factsLocator"][field] = replacement;
            same_decode(&raw(&modified));
        }
    }
    let mut ignored = value;
    ignored["documents"][0]["text"] = json!({"not":"bytes"});
    ignored["documents"][0]["facts"] = json!(false);
    ignored["documents"][0]["unknown"] = json!([1, null]);
    ignored["symbols"][0]["unknown"] = json!([1, null]);
    assert!(same_decode(&raw(&ignored)).is_some());
}

#[test]
fn warm_summary_numeric_depth_unknown_and_malformed_tail_errors_match_full() {
    for number in [
        "-0",
        "1.2300",
        "1E+02",
        "18446744073709551616",
        "123456789012345678901234567890",
        "1e9999",
    ] {
        for field in ["assets", "unknown", "home"] {
            let encoded = payload().to_string();
            let with_number = if field == "assets" {
                let mut value = payload();
                value["assets"] = json!([]);
                value
                    .to_string()
                    .replace("\"assets\":[]", &format!("\"assets\":[{number}]"))
            } else {
                format!("{},\"{field}\":{number}}}", &encoded[..encoded.len() - 1])
            };
            same_decode(with_number.as_bytes());
        }
    }
    for depth in [24, 127, 128, 129, 256] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        for field in ["assets", "unknown", "home"] {
            let mut value = payload();
            value["assets"] = json!([]);
            let encoded = value.to_string();
            let deep = if field == "assets" {
                encoded.replace("\"assets\":[]", &format!("\"assets\":[{nested}]"))
            } else {
                format!("{},\"{field}\":{nested}}}", &encoded[..encoded.len() - 1])
            };
            same_decode(deep.as_bytes());
        }
    }
    let encoded = raw(&payload());
    for suffix in [b" x".as_slice(), b" null", b" {}", b"\xff"] {
        let mut bytes = encoded.clone();
        bytes.extend_from_slice(suffix);
        assert!(same_decode(&bytes).is_none());
    }
    for end in [0, 1, encoded.len() / 2, encoded.len() - 1] {
        assert!(same_decode(&encoded[..end]).is_none());
    }
    let mut invalid_utf8 = encoded;
    let at = invalid_utf8.iter().position(|byte| *byte == b'a').unwrap();
    invalid_utf8[at] = 0xff;
    assert!(same_decode(&invalid_utf8).is_none());
}

#[test]
fn warm_summary_counted_decoder_drops_each_complete_business_row_before_next() {
    thread_local! {
        static LIVE: Cell<usize> = const { Cell::new(0) };
        static PEAK: Cell<usize> = const { Cell::new(0) };
    }
    struct Row;
    impl<'de> Deserialize<'de> for Row {
        fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
            let _: Value = Deserialize::deserialize(d)?;
            LIVE.with(|live| {
                live.set(live.get() + 1);
                PEAK.with(|peak| peak.set(peak.get().max(live.get())));
            });
            Ok(Self)
        }
    }
    impl Drop for Row {
        fn drop(&mut self) {
            LIVE.with(|live| live.set(live.get() - 1));
        }
    }
    let counted: Counted<Row> = serde_json::from_str("[{},null,1,[],false]").unwrap();
    assert_eq!(counted.len(), 5);
    assert_eq!(LIVE.with(Cell::get), 0);
    assert_eq!(PEAK.with(Cell::get), 1);
    PEAK.with(|peak| peak.set(0));
    let full: Vec<Row> = serde_json::from_str("[{},null,1,[],false]").unwrap();
    assert_eq!(LIVE.with(Cell::get), 5);
    assert_eq!(PEAK.with(Cell::get), 5);
    let moved = Counted::from(full);
    assert_eq!(moved.len(), 5);
    assert_eq!(LIVE.with(Cell::get), 0);
}

fn pin() -> SourcePin {
    SourcePin {
        repository: "elvui".into(),
        product: "main".into(),
        requested_ref: "fixture".into(),
        exact_commit: "1".repeat(40),
        parser_revision: "source-v1".into(),
    }
}
fn manifest_key(pin: &SourcePin) -> String {
    format!("source/rust-index/{}", index::fingerprint(pin))
}
fn read_manifest(home: &Path, pin: &SourcePin) -> Value {
    crate::persistence::read(home, &manifest_key(pin), &OperationControl::default()).unwrap()
}
fn publish(home: &Path, pin: &SourcePin, bytes: &[u8]) -> (Value, PathBuf) {
    let control = OperationControl::default();
    let blob = lycheedev_core::cache::publish_derived_controlled(home, bytes, 128 << 20, &control)
        .unwrap();
    let digest = blob["sha256"].as_str().unwrap();
    let path = home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let manifest = json!({"schema":"lycheedev.source-index.rust.v6","identity":index::fingerprint(pin),"blob":blob});
    crate::persistence::put_manifest(home, &manifest_key(pin), &manifest, &control).unwrap();
    (manifest, path)
}
fn error_value<T>(result: lycheedev_core::Result<T>) -> Value {
    match result {
        Ok(_) => panic!("expected error"),
        Err(error) => serde_json::to_value(error).unwrap(),
    }
}
fn comparable_error(home: &Path, pin: &SourcePin, control: &OperationControl) -> Value {
    let full = error_value(index::build_with_control(home, pin, control));
    let summary = error_value(build_summary_with_control(home, pin, control));
    assert_eq!(summary, full);
    summary
}
fn no_elapsed(mut value: Value) -> Value {
    if let Some(io) = value.get_mut("io").and_then(Value::as_object_mut) {
        assert!(io.remove("elapsedMs").unwrap().is_u64());
    }
    value
}
fn assert_closed(path: &Path) {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(path).unwrap();
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
}

#[test]
fn warm_summary_real_cache_authenticates_full_bytes_and_preserves_every_output_field() {
    let fixture = Fixture::new();
    let pin = pin();
    let bytes = raw(&payload());
    let (manifest, path) = publish(&fixture.home, &pin, &bytes);
    // A valid warm cache needs no Git input, even when that source is absent.
    fs::rename(
        crate::git::mirror(&fixture.home, &pin.repository),
        fixture.home.join("parked.git"),
    )
    .unwrap();
    let full =
        index::build_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    let (summary, coverage) =
        build_summary_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    assert_eq!(
        no_elapsed(summary.clone()),
        no_elapsed(full.summary.clone())
    );
    assert_eq!(no_elapsed(coverage.clone()), no_elapsed(full.coverage()));
    assert_eq!(
        coverage["analyzedDocuments"], 1,
        "actual collection count, not summary.documents=999"
    );
    assert_eq!(summary["io"]["indexBytesRead"], bytes.len());
    assert_eq!(summary["io"]["cachedFacts"], 1);
    assert_eq!(summary["io"]["cachedIndex"], true);
    assert_eq!(summary["io"]["gitTreeProcesses"], 0);
    assert_eq!(summary["io"]["gitBlobProcesses"], 0);
    assert_eq!(summary["io"]["sourceBytesRead"], 0);
    assert_eq!(summary["io"]["parsedDocuments"], 0);
    assert_eq!(summary["io"]["deadlineMs"], 180000);
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_closed(&path);
}

#[test]
fn warm_summary_real_cache_rejects_corruption_bad_manifest_and_all_identity_fields() {
    let fixture = Fixture::new();
    let pin = pin();
    let bytes = raw(&payload());
    let (manifest, path) = publish(&fixture.home, &pin, &bytes);
    let mut corrupt = bytes.clone();
    let at = corrupt.iter().position(|byte| *byte == b'1').unwrap();
    corrupt[at] = b'2';
    fs::write(&path, &corrupt).unwrap();
    let error = comparable_error(&fixture.home, &pin, &OperationControl::default());
    assert_eq!(error["message"], "Cached index content differs");
    assert_closed(&path);
    fs::write(&path, &bytes).unwrap();
    for invalid in [json!("invalid-digest"), json!(null), json!("A".repeat(64))] {
        let mut bad = manifest.clone();
        bad["blob"]["sha256"] = invalid;
        crate::persistence::put_manifest(
            &fixture.home,
            &manifest_key(&pin),
            &bad,
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(
            comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
            "Invalid cached index digest"
        );
    }
    let mut wrong_length = manifest;
    wrong_length["blob"]["bytes"] = json!(bytes.len() + 1);
    crate::persistence::put_manifest(
        &fixture.home,
        &manifest_key(&pin),
        &wrong_length,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
        "Cached index content differs"
    );
    for field in ["commit", "repository", "product", "parser"] {
        let mut value = payload();
        value["summary"][field] = json!("wrong");
        let (_, path) = publish(&fixture.home, &pin, &raw(&value));
        assert_eq!(
            comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
            "Cached index identity or budget differs"
        );
        assert_closed(&path);
    }
}

#[test]
fn warm_summary_real_cache_decodes_bad_business_rows_before_identity_and_budget() {
    let fixture = Fixture::new();
    let pin = pin();
    let mut value = payload();
    value["summary"]["repository"] = json!("wrong");
    value["symbols"][0]["line"] = json!(-1);
    let (_, path) = publish(&fixture.home, &pin, &raw(&value));
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["code"],
        "input.invalid"
    );
    assert_closed(&path);
    let mut value = payload();
    value["assets"] = Value::Array(vec![Value::Null; 200001]);
    let (_, path) = publish(&fixture.home, &pin, &raw(&value));
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
        "Cached index identity or budget differs"
    );
    assert_closed(&path);
    value["symbols"][0]["line"] = json!(-1);
    publish(&fixture.home, &pin, &raw(&value));
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["code"],
        "input.invalid"
    );
    value["symbols"][0]["line"] = json!(0);
    let mut malformed = raw(&value);
    malformed.truncate(malformed.len() - 1);
    publish(&fixture.home, &pin, &malformed);
    let error = comparable_error(&fixture.home, &pin, &OperationControl::default());
    assert_eq!(error["code"], "input.invalid");
    let historical = match serde_json::from_slice::<historical::Index>(&malformed) {
        Err(error) => error,
        Ok(_) => panic!("malformed root tail must fail after all business rows"),
    };
    assert!(historical.is_eof());
    assert_eq!(error["message"], historical.to_string());
}

#[test]
fn warm_summary_real_cache_accepts_asset_limit_and_rejects_document_limit() {
    let fixture = Fixture::new();
    let pin = pin();
    let mut value = payload();
    value["assets"] = Value::Array(vec![Value::Null; 200000]);
    publish(&fixture.home, &pin, &raw(&value));
    let full =
        index::build_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    let (summary, _) =
        build_summary_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    assert_eq!(no_elapsed(summary), no_elapsed(full.summary));
    value["assets"] = json!([]);
    value["documents"] = Value::Array(vec![json!({"path":"","sha256":"","bytes":0}); 100001]);
    publish(&fixture.home, &pin, &raw(&value));
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
        "Cached index identity or budget differs"
    );
}

#[test]
fn warm_summary_real_cache_cancel_and_deadline_keep_existing_first_error() {
    let fixture = Fixture::new();
    let pin = pin();
    let (manifest, path) = publish(&fixture.home, &pin, &raw(&payload()));
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        comparable_error(&fixture.home, &pin, &cancelled)["code"],
        "command.cancelled"
    );
    let expired = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        comparable_error(&fixture.home, &pin, &expired)["code"],
        "command.deadline"
    );
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_closed(&path);
    build_summary_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
}

fn invoke(home: &Path, snapshot: &str, command: &str) -> Invocation {
    Invocation {
        home: home.into(),
        cwd: home.into(),
        command: command.into(),
        positional: None,
        flags: BTreeMap::from([("--snapshot".into(), snapshot.into())]),
        control: OperationControl::default(),
    }
}
fn assert_capture(home: &Path, outcome: &Outcome, pin: &SourcePin, snapshot: &str, kind: &str) {
    assert_eq!(outcome.provenance.len(), 1);
    let capture = &outcome.provenance[0];
    assert_eq!(capture["provenance"]["kind"], kind);
    assert_eq!(capture["provenance"]["sourceCommit"], pin.exact_commit);
    assert_eq!(capture["provenance"]["snapshot"], snapshot);
    assert_eq!(capture["complete"], outcome.complete);
    let saved = evidence::read_capture(home, capture["id"].as_str().unwrap()).unwrap();
    assert_eq!(&saved, capture);
    let bytes =
        evidence::read_blob(home, &capture["blob"], evidence::DEFAULT_CAPTURE_BYTES).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&outcome.data).unwrap());
    assert_eq!(capture["blob"]["sha256"], storage::sha256(&bytes));
}

#[test]
fn warm_summary_cli_cold_then_warm_keeps_full_cache_query_inspect_and_capture_consumers() {
    let fixture = Fixture::new();
    let source = fixture.pin(&BTreeMap::from([
        (
            "Main.lua".into(),
            "function SummaryProbe()\n return 1\nend\n".into(),
        ),
        ("Main.toc".into(), "## Interface: 120100\nMain.lua\n".into()),
    ]));
    let selected = selection::pin_selection(
        &fixture.home,
        selection::SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let inv = invoke(&fixture.home, &selected.id, "source index");
    let cold = crate::dispatch(&inv).unwrap();
    assert!(cold.data["io"].get("cachedIndex").is_none());
    assert_eq!(cold.data["io"]["parsedDocuments"], 2);
    assert_capture(&fixture.home, &cold, &source, &selected.id, "source-query");
    let manifest = read_manifest(&fixture.home, &source);
    let digest = manifest["blob"]["sha256"].as_str().unwrap();
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(storage::sha256(&bytes), digest);
    let persisted: historical::Index = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(persisted.documents.len(), 2);
    assert!(
        persisted
            .symbols
            .iter()
            .any(|symbol| symbol.name == "SummaryProbe")
    );
    assert_eq!(persisted.summary, {
        let mut v = cold.data.clone();
        v.as_object_mut().unwrap().remove("io");
        v
    });
    let full =
        index::build_with_control(&fixture.home, &source, &OperationControl::default()).unwrap();
    let warm = crate::dispatch(&inv).unwrap();
    assert_eq!(
        no_elapsed(warm.data.clone()),
        no_elapsed(full.summary.clone())
    );
    assert_eq!(
        no_elapsed(warm.coverage.clone()),
        no_elapsed(full.coverage())
    );
    assert_eq!(warm.selection, cold.selection);
    assert_eq!(warm.complete, cold.complete);
    assert_capture(&fixture.home, &warm, &source, &selected.id, "source-query");
    let mut query = invoke(&fixture.home, &selected.id, "source query");
    query.positional = Some("SummaryProbe".into());
    query.flags.insert("--mode".into(), "precise".into());
    let searched = crate::dispatch(&query).unwrap();
    assert!(
        searched.data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "SummaryProbe")
    );
    assert_capture(
        &fixture.home,
        &searched,
        &source,
        &selected.id,
        "source-query",
    );
    let mut inspect = invoke(&fixture.home, &selected.id, "source inspect");
    inspect
        .flags
        .insert("--symbol".into(), "SummaryProbe".into());
    let inspected = crate::dispatch(&inspect).unwrap();
    assert!(
        inspected.data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "SummaryProbe")
    );
    assert_capture(
        &fixture.home,
        &inspected,
        &source,
        &selected.id,
        "source-inspect",
    );
    assert_eq!(read_manifest(&fixture.home, &source), manifest);
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn warm_summary_cli_preserves_explicit_summary_coverage_including_null() {
    let fixture = Fixture::new();
    let source = pin();
    let selected = selection::pin_selection(
        &fixture.home,
        selection::SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    for coverage in [Value::Null, json!({"private":[1, null, "雪"]})] {
        let mut value = payload();
        value["summary"]["coverage"] = coverage.clone();
        publish(&fixture.home, &source, &raw(&value));
        let outcome =
            crate::dispatch(&invoke(&fixture.home, &selected.id, "source index")).unwrap();
        assert_eq!(outcome.coverage, coverage);
        assert_eq!(outcome.data["coverage"], coverage);
        assert_capture(
            &fixture.home,
            &outcome,
            &source,
            &selected.id,
            "source-query",
        );
    }
}

#[test]
fn warm_summary_missing_object_or_obsolete_manifest_keeps_original_cold_fallback() {
    let fixture = Fixture::new();
    let source = fixture.pin(&BTreeMap::from([(
        "Main.lua".into(),
        "function RecoverSummary() end\n".into(),
    )]));
    let (cold, _) =
        build_summary_with_control(&fixture.home, &source, &OperationControl::default()).unwrap();
    let expected = {
        let mut value = cold.clone();
        value.as_object_mut().unwrap().remove("io");
        value
    };
    for miss in ["object", "schema", "identity"] {
        let mut manifest = read_manifest(&fixture.home, &source);
        if miss == "object" {
            let digest = manifest["blob"]["sha256"].as_str().unwrap();
            fs::remove_file(
                fixture
                    .home
                    .join("cache/objects")
                    .join(&digest[..2])
                    .join(&digest[2..]),
            )
            .unwrap();
        } else {
            manifest[miss] = json!("obsolete");
            crate::persistence::put_manifest(
                &fixture.home,
                &manifest_key(&source),
                &manifest,
                &OperationControl::default(),
            )
            .unwrap();
        }
        let (rebuilt, _) =
            build_summary_with_control(&fixture.home, &source, &OperationControl::default())
                .unwrap();
        assert!(rebuilt["io"].get("cachedIndex").is_none());
        let mut actual = rebuilt;
        actual.as_object_mut().unwrap().remove("io");
        assert_eq!(actual, expected);
        let full = index::build_with_control(&fixture.home, &source, &OperationControl::default())
            .unwrap();
        assert_eq!(full.documents.len(), 1);
        assert!(
            full.symbols
                .iter()
                .any(|symbol| symbol.name == "RecoverSummary")
        );
    }
}

thread_local! {
    static STOP_DURING_ASSETS: std::cell::RefCell<Option<lycheedev_core::control::TrackedCancellation>> = const { std::cell::RefCell::new(None) };
}
struct StopGuard;
impl Drop for StopGuard {
    fn drop(&mut self) {
        STOP_DURING_ASSETS.with(|stop| stop.borrow_mut().take());
    }
}
// Test-only collection injection stops the original control during decode,
// without introducing any hook or cancellation check into production parsing.
struct CancelAfter<C>(C);
impl<C: CollectionLength> CollectionLength for CancelAfter<C> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl<C: From<Vec<Value>>> From<Vec<Value>> for CancelAfter<C> {
    fn from(rows: Vec<Value>) -> Self {
        Self(rows.into())
    }
}
impl<'de, C: Deserialize<'de>> Deserialize<'de> for CancelAfter<C> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let rows = C::deserialize(d)?;
        STOP_DURING_ASSETS.with(|stop| stop.borrow().as_ref().unwrap().cancel());
        Ok(Self(rows))
    }
}
fn cancel_during_decode(home: &Path, pin: &SourcePin, projected: bool) -> Value {
    let control = OperationControl::default();
    STOP_DURING_ASSETS.with(|stop| {
        assert!(
            stop.borrow_mut()
                .replace(control.tracked_cancel_handle().unwrap())
                .is_none()
        )
    });
    let _guard = StopGuard;
    if projected {
        error_value(index::build_material_projected_with_control::<
            Counted<Document>,
            CancelAfter<Counted<Value>>,
            Counted<Symbol>,
        >(home, pin, &control))
    } else {
        error_value(index::build_material_projected_with_control::<
            Vec<Document>,
            CancelAfter<Vec<Value>>,
            Vec<Symbol>,
        >(home, pin, &control))
    }
}

#[test]
fn warm_summary_late_cancel_preserves_complete_decode_error_then_checked_close_order() {
    let fixture = Fixture::new();
    let pin = pin();
    let bytes = raw(&payload());
    let (manifest, path) = publish(&fixture.home, &pin, &bytes);
    let full = cancel_during_decode(&fixture.home, &pin, false);
    let projected = cancel_during_decode(&fixture.home, &pin, true);
    assert_eq!(projected, full);
    assert_eq!(projected["code"], "command.cancelled");
    assert!(
        projected["details"].is_null(),
        "identical late stop is not duplicated"
    );
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_closed(&path);
    let mut bad = payload();
    bad["symbols"][0]["line"] = json!(-1);
    for bytes in [raw(&bad), {
        let mut tail = bytes;
        tail.push(b'x');
        tail
    }] {
        let (manifest, path) = publish(&fixture.home, &pin, &bytes);
        let full = cancel_during_decode(&fixture.home, &pin, false);
        let projected = cancel_during_decode(&fixture.home, &pin, true);
        assert_eq!(projected, full);
        assert_eq!(projected["code"], "input.invalid");
        assert_eq!(projected["details"]["schema"], "lycheedev.file-cleanup.v1");
        assert_eq!(projected["details"]["primary"]["code"], "input.invalid");
        assert_eq!(projected["details"]["control"]["code"], "command.cancelled");
        assert!(projected["details"]["cleanup"].is_null());
        assert_eq!(read_manifest(&fixture.home, &pin), manifest);
        assert_closed(&path);
    }
}

#[test]
fn warm_summary_raw_cap_precedes_json_decode_and_never_publishes_fallback() {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let fixture = Fixture::new();
    let pin = pin();
    let (mut manifest, original_path) = publish(&fixture.home, &pin, b"not JSON");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&original_path)
        .unwrap();
    file.set_len((128 << 20) + 1).unwrap();
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
    // Authenticate the actual oversized bytes. A stale small-blob digest would
    // hide a removed cap behind the unrelated SHA mismatch branch. Stream this
    // fixture hash with fixed scratch rather than allocate another 128 MiB.
    let mut file = fs::File::open(&original_path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0usize;
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        bytes += read;
        hasher.update(&buffer[..read]);
    }
    assert_eq!(bytes, (128 << 20) + 1);
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
    let digest = format!("{:x}", hasher.finalize());
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::rename(original_path, &path).unwrap();
    manifest["blob"]["sha256"] = json!(digest);
    manifest["blob"]["bytes"] = json!((128 << 20) + 1);
    crate::persistence::put_manifest(
        &fixture.home,
        &manifest_key(&pin),
        &manifest,
        &OperationControl::default(),
    )
    .unwrap();
    let error = comparable_error(&fixture.home, &pin, &OperationControl::default());
    assert_eq!(error["code"], "codebase.source_integrity");
    assert_eq!(error["message"], "Cached index content differs");
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_eq!(fs::metadata(&path).unwrap().len(), (128 << 20) + 1);
    assert_closed(&path);
}
