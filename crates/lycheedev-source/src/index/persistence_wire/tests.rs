use super::*;
use crate::index::{self, model::DiagnosticSample};
use lycheedev_core::{selection::SourcePin, storage};
use serde_json::{Value, json};

pub(crate) fn fixture() -> (Index, SourcePin) {
    let pin = SourcePin {
        repository: "elvui".into(),
        product: "main".into(),
        exact_commit: "1".repeat(40),
        parser_revision: "source-v1".into(),
        requested_ref: "fixture".into(),
    };
    let mut summary = SourceSummary::empty(&pin);
    summary.documents = 2;
    summary.ast_files = 1;
    summary.assets = 2;
    summary.declarations = 2;
    summary.relationships = 1;
    summary.diagnostics = 2;
    summary.complete = false;
    let mut sample = Samples::default();
    for _ in 0..2 {
        assert!(sample.push(DiagnosticSample {
            path: "目录/雪.lua".into(),
            line: 0,
            message: "diagnostic\n\0雪".into()
        }));
    }
    summary.diagnostic_sample = Some(sample);
    summary.io = json!({"cold":true});
    let mut declaration = Symbol {
        id: String::new(),
        kind: "declaration".into(),
        name: "名\n\0é\\".into(),
        target: String::new(),
        category: "lua-function".into(),
        confidence: "exact".into(),
        path: "目录/雪.lua".into(),
        line: 1,
        end_line: 3,
        signature: "名(雪)".into(),
        scope: "local".into(),
        ordinal: 0,
    };
    declaration.id = index::stable_id(&pin, &declaration);
    let mut duplicate = declaration.clone();
    duplicate.ordinal = 1;
    duplicate.id = index::stable_id(&pin, &duplicate);
    let relationship = Symbol {
        id: String::new(),
        kind: "relationship".into(),
        name: "caller".into(),
        target: "名".into(),
        category: "call".into(),
        confidence: "inferred".into(),
        path: "目录/雪.lua".into(),
        line: 2,
        end_line: 2,
        signature: String::new(),
        scope: String::new(),
        ordinal: 0,
    };
    let documents = vec![
        Document {
            path: "目录/雪.lua".into(),
            text: Some(b"function Excluded() end".to_vec()),
            sha256: "a".repeat(64),
            bytes: 23,
            facts: crate::facts::lua(b"function Excluded() end", false),
            facts_locator: None,
        },
        Document {
            path: "Empty.lua".into(),
            text: Some(Vec::new()),
            sha256: storage::sha256(b""),
            bytes: 0,
            facts: Default::default(),
            facts_locator: Some(Locator {
                group: std::array::from_fn(|i| (i * 8) as u8),
                offset: 8,
                bytes: 81,
                sha256: std::array::from_fn(|i| 255 - i as u8),
            }),
        },
    ];
    let assets = vec![
        AssetRecord {
            path: "Image.png".into(),
            content_hash: "b".repeat(64),
            bytes: 24,
            extension: ".png".into(),
            mime: "image/png".into(),
            format: "png".into(),
            width: u32::MAX,
            height: 4096,
            local: Some(BlobRef {
                sha256: "b".repeat(64),
                bytes: 24,
            }),
        },
        AssetRecord {
            path: "Large.blp".into(),
            content_hash: String::new(),
            bytes: (64 << 20) + 1,
            extension: ".blp".into(),
            mime: "image/x-blp".into(),
            format: "blp".into(),
            width: 0,
            height: 0,
            local: None,
        },
    ];
    (
        Index {
            documents,
            assets,
            symbols: vec![declaration, duplicate, relationship],
            summary,
            compatibility_gaps: BTreeSet::from(["api".into(), "event".into(), "mixin".into()]),
            home: "not-persisted".into(),
        },
        pin,
    )
}
fn public(mut value: Value) -> Value {
    value["summary"].as_object_mut().unwrap().remove("io");
    value
}
fn frame(value: &impl Serialize, identity: &str) -> Vec<u8> {
    let payload = postcard::to_allocvec(value).unwrap();
    let mut raw = Vec::from(MAGIC.as_slice());
    raw.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    raw.extend_from_slice(&identity_bytes(identity).unwrap());
    raw.extend_from_slice(&payload);
    raw
}
pub(crate) fn unchecked(index: &Index, pin: &SourcePin) -> Vec<u8> {
    frame(&IndexWire(index, false), &index::fingerprint(pin))
}
fn reject_both(raw: &[u8], identity: &str) {
    let control = OperationControl::default();
    let full = decode::<Vec<Document>, Vec<AssetRecord>, Vec<Symbol>>(raw, identity, &control)
        .err()
        .expect("full must reject");
    let projected =
        decode::<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>(raw, identity, &control)
            .err()
            .expect("summary must reject");
    assert_eq!(
        serde_json::to_value(full).unwrap(),
        serde_json::to_value(projected).unwrap()
    );
}
#[test]
fn binary_index_complete_public_fields_omissions_unicode_duplicates_and_ids_are_preserved() {
    let (index, pin) = fixture();
    let identity = index::fingerprint(&pin);
    let before = serde_json::to_value(&index).unwrap();
    let raw = encode(&index, &identity, &OperationControl::default()).unwrap();
    assert_eq!(&raw[..8], MAGIC);
    assert_eq!(
        raw.len() - HEADER,
        u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize
    );
    let full: Index = decode(&raw, &identity, &OperationControl::default()).unwrap();
    assert_eq!(
        public(serde_json::to_value(&full).unwrap()),
        public(before.clone())
    );
    assert_eq!(serde_json::to_value(&index).unwrap(), before);
    assert!(
        full.documents
            .iter()
            .all(|d| d.text.is_none() && d.facts.declarations.is_empty())
    );
    for symbol in &full.symbols[..2] {
        assert_eq!(symbol.id, index::stable_id(&pin, symbol));
    }
    assert_ne!(full.symbols[0].id, full.symbols[1].id);
    let value = serde_json::to_value(&full).unwrap();
    assert!(value["symbols"][2].get("id").is_none());
    assert!(value["symbols"][2].get("ordinal").is_none());
    assert_eq!(
        value["summary"]["diagnosticSample"][0],
        value["summary"]["diagnosticSample"][1]
    );
    assert!(value["assets"][1]["local"].is_null());
    assert_eq!(
        value["assets"][0],
        json!({"path":"Image.png","contentHash":"b".repeat(64),"bytes":24,"extension":".png","mime":"image/png","format":"png","width":u32::MAX,"height":4096,"local":{"sha256":"b".repeat(64),"bytes":24}})
    );
}
#[test]
fn binary_index_validate_only_shares_all_rows_and_avoids_row_ownership() {
    let (mut index, pin) = fixture();
    for _ in 0..2000 {
        index.symbols.push(index.symbols[0].clone());
    }
    index.summary.declarations += 2000;
    let id = index::fingerprint(&pin);
    let control = OperationControl::default();
    let raw = encode(&index, &id, &control).unwrap();
    let (full, full_left): (Index, _) = decode_mode(&raw, &id, &control, false).unwrap();
    let (summary, summary_left): (
        Index<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>,
        _,
    ) = decode_mode(&raw, &id, &control, false).unwrap();
    assert_eq!(summary.documents.len(), full.documents.len());
    assert_eq!(summary.assets.len(), full.assets.len());
    assert_eq!(summary.symbols.len(), full.symbols.len());
    assert_eq!(
        serde_json::to_value(&summary.summary).unwrap(),
        serde_json::to_value(&full.summary).unwrap()
    );
    assert_eq!(summary.compatibility_gaps, full.compatibility_gaps);
    assert!(summary_left - full_left > 2000 * size_of::<Symbol>());
}
#[test]
fn binary_index_every_truncation_tail_header_identity_and_utf8_error_rejects_both_consumers() {
    let (index, pin) = fixture();
    let id = index::fingerprint(&pin);
    let raw = encode(&index, &id, &OperationControl::default()).unwrap();
    for end in 0..raw.len() {
        reject_both(&raw[..end], &id);
    }
    for at in [0, 8, 12, 43] {
        let mut bad = raw.clone();
        bad[at] ^= 1;
        reject_both(&bad, &id);
    }
    let mut tail = raw.clone();
    tail.push(0);
    let n = (tail.len() - HEADER) as u32;
    tail[8..12].copy_from_slice(&n.to_le_bytes());
    reject_both(&tail, &id);
    let mut bad = raw;
    let at = bad
        .windows("名".len())
        .position(|w| w == "名".as_bytes())
        .unwrap();
    bad[at] = 0xff;
    reject_both(&bad, &id);
}
#[test]
fn binary_index_authenticated_bad_business_fields_counts_and_unknown_categories_are_hard_errors() {
    let (mut index, pin) = fixture();
    let id = index::fingerprint(&pin);
    for mutation in 0..8 {
        let (mut row, _) = fixture();
        match mutation {
            0 => row.summary.documents += 1,
            1 => row.summary.repository = "other".into(),
            2 => row.symbols[0].kind = "unknown".into(),
            3 => row.documents[0].sha256 = "not-a-hash".into(),
            4 => row.documents[0].path = "../outside.lua".into(),
            5 => row.assets[0].local.as_mut().unwrap().bytes += 1,
            6 => row.summary.complete = true,
            _ => {
                row.compatibility_gaps.insert("unknown".into());
            }
        }
        reject_both(&frame(&IndexWire(&row, false), &id), &id);
    }
    index.summary.schema = "source-map.future".into();
    assert!(encode(&index, &id, &OperationControl::default()).is_err());
}
#[test]
fn binary_index_declared_lengths_are_bounded_before_owned_reserve() {
    struct Declared(usize);
    impl Serialize for Declared {
        fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
            s.serialize_seq(Some(self.0))?.end()
        }
    }
    let (index, pin) = fixture();
    let id = index::fingerprint(&pin);
    for count in [DOCUMENTS + 1, usize::MAX] {
        let raw = frame(
            &(
                &index.summary,
                Declared(count),
                Vec::<u8>::new(),
                Vec::<u8>::new(),
                Vec::<u8>::new(),
            ),
            &id,
        );
        reject_both(&raw, &id);
    }
    assert!(maximum_owned().unwrap() > MAXIMUM);
}
#[test]
fn binary_index_private_coordinates_are_u64_and_checked_for_pointer_width() {
    #[derive(Deserialize)]
    struct Coordinate(#[serde(with = "super::super::model::coordinate")] usize);
    let bytes = postcard::to_allocvec(&u64::MAX).unwrap();
    if usize::BITS < 64 {
        assert!(postcard::from_bytes::<Coordinate>(&bytes).is_err());
    } else {
        assert_eq!(
            postcard::from_bytes::<Coordinate>(&bytes).unwrap().0,
            usize::MAX
        );
    }
    let overflow = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];
    assert!(postcard::from_bytes::<Coordinate>(&overflow).is_err());
}
#[test]
fn binary_index_immediate_cancel_and_deadline_survive_bad_tail_and_partial_ownership() {
    let (mut index, pin) = fixture();
    for _ in 0..50 {
        index.symbols.push(index.symbols[0].clone());
    }
    index.summary.declarations += 50;
    let id = index::fingerprint(&pin);
    let mut raw = encode(&index, &id, &OperationControl::default()).unwrap();
    raw.push(255);
    let len = (raw.len() - HEADER) as u32;
    raw[8..12].copy_from_slice(&len.to_le_bytes());
    for step in [1, 12, 80] {
        let control = OperationControl::default();
        let _stop = CancelAtStep::new(step, &control);
        let error = decode::<Vec<Document>, Vec<AssetRecord>, Vec<Symbol>>(&raw, &id, &control)
            .err()
            .unwrap();
        assert_eq!(error.code, "command.cancelled");
        assert!(error.details.is_null());
    }
    let expired = OperationControl::uncancelable(std::time::Instant::now());
    let error =
        decode::<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>(&raw, &id, &expired)
            .err()
            .unwrap();
    assert_eq!(error.code, "command.deadline");
}
#[test]
fn binary_index_encode_observes_cancellation_without_returning_a_partial_cache() {
    let (mut index, pin) = fixture();
    index.symbols[0].name = "雪".repeat(100_000);
    let id = index::fingerprint(&pin);
    let control = OperationControl::default();
    let _stop = CancelAtStep::new(9, &control);
    assert_eq!(
        encode(&index, &id, &control).unwrap_err().code,
        "command.cancelled"
    );
}
#[test]
fn binary_index_query_metadata_uses_same_typed_rows_and_cumulative_owned_admission() {
    let (index, pin) = fixture();
    let id = index::fingerprint(&pin);
    let control = OperationControl::default();
    let raw = encode_metadata(&index, &id, &control).unwrap();
    let (metadata, remaining) = decode_metadata(&raw, &id, &control).unwrap();
    assert!(metadata.symbols.is_empty());
    assert_eq!(
        serde_json::to_value(&metadata.assets).unwrap(),
        serde_json::to_value(&index.assets).unwrap()
    );
    assert!(remaining < maximum_owned().unwrap());
    assert!(decode::<Vec<Document>, Vec<AssetRecord>, Vec<Symbol>>(&raw, &id, &control).is_err());
    assert_eq!(
        own_symbols(std::iter::empty(), SYMBOLS + 1, remaining, &control)
            .unwrap_err()
            .code,
        "codebase.source_budget"
    );
}
#[test]
fn binary_index_public_domain_rejects_unknown_asset_and_summary_fields() {
    let (index, _) = fixture();
    let mut summary = serde_json::to_value(index.summary).unwrap();
    summary["unknown"] = json!(null);
    assert!(serde_json::from_value::<SourceSummary>(summary).is_err());
    let mut asset = serde_json::to_value(&index.assets[0]).unwrap();
    asset["unknown"] = json!(false);
    assert!(serde_json::from_value::<AssetRecord>(asset).is_err());
}

#[test]
fn binary_index_physical_bytes_do_not_reimpose_logical_json_limit() {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_add(bytes.len()).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (mut index, pin) = fixture();
    index.symbols[0].name = "\0".repeat((MAXIMUM / 6) + 1);
    let mut logical = Count(0);
    serde_json::to_writer(&mut logical, &index).unwrap();
    assert!(logical.0 > MAXIMUM);
    let identity = index::fingerprint(&pin);
    let raw = encode(&index, &identity, &OperationControl::default()).unwrap();
    assert!(raw.len() < MAXIMUM);
    let full: Index = decode(&raw, &identity, &OperationControl::default()).unwrap();
    assert_eq!(full.symbols[0].name, index.symbols[0].name);
    let summary: Index<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>> =
        decode(&raw, &identity, &OperationControl::default()).unwrap();
    assert_eq!(summary.symbols.len(), full.symbols.len());
}
#[test]
fn binary_index_owned_text_vector_and_query_count_admission_are_exact() {
    let control = OperationControl::default();
    let failure = RefCell::new(None);
    let mut ctx = Context {
        control: &control,
        failure: &failure,
        remaining: 2,
        payload: 100,
        declarations: 0,
        relationships: 0,
    };
    assert!(ctx.text::<postcard::Error>("雪").is_err());
    assert_eq!(ctx.remaining, 2);
    assert_eq!(
        failure.borrow_mut().take().unwrap().code,
        "codebase.source_budget"
    );
    ctx.remaining = size_of::<Symbol>() - 1;
    assert!(<Vec<Symbol> as Collection<Symbol>>::reserve::<postcard::Error>(1, &mut ctx).is_err());
    assert_eq!(ctx.remaining, size_of::<Symbol>() - 1);
    assert_eq!(
        failure.borrow_mut().take().unwrap().code,
        "codebase.source_budget"
    );
    assert_eq!(
        own_symbols(std::iter::empty(), 1, maximum_owned().unwrap(), &control)
            .unwrap_err()
            .code,
        "codebase.source_integrity"
    );
    let row = Symbol {
        id: "",
        kind: "load",
        name: "",
        target: "",
        category: "",
        confidence: "",
        path: "a.lua",
        line: 0,
        end_line: 0,
        signature: "",
        scope: "",
        ordinal: 0,
    };
    assert_eq!(
        own_symbols(
            std::iter::once(Ok(row)),
            0,
            maximum_owned().unwrap(),
            &control
        )
        .unwrap_err()
        .code,
        "codebase.source_integrity"
    );
}
#[test]
fn binary_index_public_summary_matches_current_public_shape_including_sample_order() {
    let (index, pin) = fixture();
    assert_eq!(
        serde_json::to_value(&index.summary).unwrap(),
        json!({
            "schema":"lycheedev.source-map.v2", "storage":"file-cache", "repository":pin.repository,
            "product":pin.product, "commit":pin.exact_commit, "parser":pin.parser_revision,
            "documents":2,"astFiles":1,"skippedDocuments":0,"declarations":2,"relationships":1,
            "assets":2,"diagnostics":2,"complete":false,"io":{"cold":true},
            "diagnosticSample":[{"path":"目录/雪.lua","line":0,"message":"diagnostic\n\0雪"},{"path":"目录/雪.lua","line":0,"message":"diagnostic\n\0雪"}]
        })
    );
    let mut bad = serde_json::to_value(&index.summary).unwrap();
    bad["diagnosticSample"] = json!(vec![json!({"path":"a.lua","line":1,"message":"x"}); 21]);
    assert!(serde_json::from_value::<SourceSummary>(bad).is_err());
}

#[test]
fn binary_index_repeated_symbol_paths_preserve_mixed_kinds_order_and_all_fields() {
    let (mut index, pin) = fixture();
    let template = index.symbols[0].clone();
    index.symbols.clear();
    for (ordinal, (path, kind)) in [
        ("目录/雪.lua", "declaration"),
        ("目录/雪.lua", "relationship"),
        ("目录/雪.lua", "load"),
        ("Other.lua", "header"),
        ("Other.lua", "declaration"),
        ("目录/雪.lua", "relationship"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = template.clone();
        row.path = path.into();
        row.kind = kind.into();
        row.ordinal = ordinal;
        row.id = index::stable_id(&pin, &row);
        index.symbols.push(row);
    }
    index.summary.declarations = 2;
    index.summary.relationships = 2;
    let id = index::fingerprint(&pin);
    let control = OperationControl::default();
    let raw = encode(&index, &id, &control).unwrap();
    let full: Index = decode(&raw, &id, &control).unwrap();
    assert_eq!(
        public(serde_json::to_value(&full).unwrap()),
        public(serde_json::to_value(&index).unwrap())
    );
    assert_eq!(encode(&full, &id, &control).unwrap(), raw);
    let projected: Index<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>> =
        decode(&raw, &id, &control).unwrap();
    assert_eq!(projected.symbols.len(), 6);
    assert_eq!(
        serde_json::to_value(projected.summary).unwrap(),
        serde_json::to_value(full.summary).unwrap()
    );
}

#[test]
fn binary_index_repeated_symbol_paths_do_not_admit_changed_or_initial_invalid_paths() {
    for bad in [
        "",
        "/absolute.lua",
        "../outside.lua",
        "目录/雪.lua/..",
        "目录/雪.lua//tail",
        "目录/雪.lua\\tail",
        "目录/雪.lua:tail",
        "目录/雪.lua\0tail",
        "目录/雪.lua\r",
        "目录/雪.lua\n",
        ".",
        "..",
    ] {
        for first in [false, true] {
            let (mut index, pin) = fixture();
            // Three equal paths in the ordinary fixture exercise the hit case;
            // a changed suffix must revalidate, not use a prefix/hash identity.
            if first {
                for row in &mut index.symbols {
                    row.path = bad.into();
                }
            } else {
                index.symbols[1].path = bad.into();
            }
            let raw = unchecked(&index, &pin);
            reject_both(&raw, &index::fingerprint(&pin));
        }
    }
    let (mut index, pin) = fixture();
    index.symbols[1].path = "a".repeat(4097);
    reject_both(&unchecked(&index, &pin), &index::fingerprint(&pin));
}

#[test]
fn binary_index_repeated_symbol_paths_still_validate_kind_and_summary_counts() {
    for mutation in 0..4 {
        let (mut index, pin) = fixture();
        match mutation {
            0 => index.symbols[1].kind = "unknown".into(),
            1 => index.symbols[1].kind = "header".into(),
            2 => index.summary.declarations += 1,
            _ => index.summary.relationships += 1,
        }
        reject_both(&unchecked(&index, &pin), &index::fingerprint(&pin));
    }
}

#[test]
fn binary_index_repeated_symbol_paths_keep_cancellation_in_full_and_summary_rows() {
    let (mut index, pin) = fixture();
    for _ in 0..1024 {
        index.symbols.push(index.symbols[0].clone());
    }
    index.summary.declarations += 1024;
    let id = index::fingerprint(&pin);
    let raw = encode(&index, &id, &OperationControl::default()).unwrap();
    // These checkpoints are deep into repeated rows in both consumers, not
    // merely in header/summary decoding. No path reuse may suppress controls.
    for step in [1000, 2000] {
        for summary in [false, true] {
            let control = OperationControl::default();
            let _stop = CancelAtStep::new(step, &control);
            let error = if summary {
                decode::<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>(
                    &raw, &id, &control,
                )
                .err()
            } else {
                decode::<Vec<Document>, Vec<AssetRecord>, Vec<Symbol>>(&raw, &id, &control).err()
            }
            .unwrap();
            assert_eq!(error.code, "command.cancelled");
            assert!(error.details.is_null());
        }
    }
}
