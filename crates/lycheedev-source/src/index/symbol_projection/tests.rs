use super::*;
use crate::{
    facts::{Declaration, HeaderValue, LoadReference, Relationship, SyntaxNote},
    index::{
        self, AssetRecord, Index, facts_cache_tests::Fixture, persistence_wire, profile::Profile,
    },
};
use crate::request::{SelectedSource, SourceContext, SourceRequest};
use lycheedev_core::{control::OperationControl, selection};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

type DeferredIndex = Index<Vec<Document>, Vec<AssetRecord>, DeferredSymbols>;

fn project_symbols(pin:&SourcePin, documents:&[Document])->Vec<Symbol> {
    let mut rows=Vec::new();
    for doc in documents { rows.document(pin,&doc.path,&doc.facts).unwrap(); }
    rows
}
fn rich_facts() -> Facts {
    let mut facts = Facts::default();
    for (name, signature, scope, line) in [
        ("名\n\0é\\", "F(雪)", "local", usize::MAX),
        ("", "", "", 0),
        ("名\n\0é\\", "F(雪)", "local", usize::MAX),
    ] {
        facts.declarations.push(Declaration {
            name: name.into(),
            category: "lua-function".into(),
            scope: scope.into(),
            line,
            end_line: line,
            signature: signature.into(),
        });
    }
    for _ in 0..2 {
        facts.relationships.push(Relationship {
            from: "<file>".into(),
            to: "目标\n\0".into(),
            category: "call".into(),
            confidence: "inferred".into(),
            line: 9,
        });
        facts.loads.push(LoadReference {
            path: "目录/雪.lua".into(),
            line: 0,
            kind: "xml-script".into(),
        });
        facts.headers.push(HeaderValue {
            key: "Interface".into(),
            value: String::new(),
            line: 1,
        });
        facts.diagnostics.push(SyntaxNote {
            message: "note\n雪".into(),
            line: 2,
        });
    }
    facts
}
fn fixture() -> (Index, DeferredIndex, SourcePin) {
    let (mut full, pin) = persistence_wire::tests::fixture();
    full.documents[0].facts = rich_facts();
    full.documents[1].facts = Facts::default();
    full.symbols = project_symbols(&pin, &full.documents);
    full.summary.declarations = full
        .documents
        .iter()
        .map(|d| d.facts.declarations.len())
        .sum();
    full.summary.relationships = full
        .documents
        .iter()
        .map(|d| d.facts.relationships.len())
        .sum();
    let mut deferred = DeferredSymbols::default();
    for d in &full.documents {
        deferred.document(&pin, &d.path, &d.facts).unwrap();
    }
    let projected = Index {
        documents: full.documents.clone(),
        assets: full.assets.clone(),
        symbols: deferred,
        summary: full.summary.clone(),
        compatibility_gaps: full.compatibility_gaps.clone(),
        home: full.home.clone(),
    };
    (full, projected, pin)
}
fn error<T>(value: Result<T>) -> Vec<u8> {
    serde_json::to_vec(&value.err().expect("expected error")).unwrap()
}

#[test]
fn borrowed_symbol_mapping_preserves_current_json_and_postcard_bytes_for_all_four_arrays() {
    let (full, projected, pin) = fixture();
    let before = serde_json::to_vec(&full).unwrap();
    let expected_rows = postcard::to_allocvec(&SymbolsWire{rows:&full.symbols,documents:&full.documents,summary:&full.summary}).unwrap();
    let projected_rows = SymbolsWire {
        rows: &projected.symbols,
        documents: &projected.documents,
        summary: &projected.summary,
    };
    assert_eq!(
        postcard::to_allocvec(&projected_rows).unwrap(),
        expected_rows
    );
    let mut actual = Vec::new();
    for d in &full.documents {
        actual.document(&pin, &d.path, &d.facts).unwrap();
    }
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&full.symbols).unwrap()
    );
    let identity = index::fingerprint(&pin);
    let control = OperationControl::default();
    let owned = persistence_wire::encode(&full, &identity, &control).unwrap();
    let borrowed = persistence_wire::encode(&projected, &identity, &control).unwrap();
    assert_eq!(borrowed, owned);
    let decoded: Index = persistence_wire::decode(&borrowed, &identity, &control).unwrap();
    assert_eq!(
        serde_json::to_vec(&decoded.symbols).unwrap(),
        serde_json::to_vec(&full.symbols).unwrap()
    );
    assert_eq!(serde_json::to_vec(&full).unwrap(), before);
    let public = serde_json::to_value(&decoded).unwrap();
    assert!(public["assets"][1]["local"].is_null());
    assert!(public["documents"][0].get("factsLocator").is_none());
    assert!(public["documents"][1]["factsLocator"].is_object());
    assert!(public["symbols"][1].get("signature").is_none());
    assert!(public["symbols"][3].get("id").is_none());
    assert_ne!(decoded.symbols[0].id, decoded.symbols[2].id);
    assert_eq!(decoded.symbols[3].name, decoded.symbols[4].name);
}

#[test]
fn borrowed_symbol_projection_empty_rows_and_null_optional_summary_keep_exact_wire() {
    let (mut full, _, pin) = fixture();
    for document in &mut full.documents {
        document.facts = Facts::default();
    }
    full.symbols.clear();
    full.summary.declarations = 0;
    full.summary.relationships = 0;
    full.summary.diagnostics = 0;
    full.summary.diagnostic_sample = None;
    full.summary.complete = true;
    let projected = Index {
        documents: full.documents.clone(),
        assets: full.assets.clone(),
        symbols: DeferredSymbols::default(),
        summary: full.summary.clone(),
        compatibility_gaps: full.compatibility_gaps.clone(),
        home: full.home.clone(),
    };
    let identity = index::fingerprint(&pin);
    assert_eq!(
        persistence_wire::encode(&full, &identity, &OperationControl::default()).unwrap(),
        persistence_wire::encode(&projected, &identity, &OperationControl::default()).unwrap()
    );
    assert!(serde_json::to_value(&full.summary).unwrap()["diagnosticSample"].is_null());
}

#[test]
fn borrowed_symbol_projection_borrows_every_non_id_string_and_retains_no_row_vector() {
    let (full, projected, pin) = fixture();
    let mut owned_bytes = full.symbols.capacity() * size_of::<Symbol>();
    for s in &full.symbols {
        owned_bytes += [
            &s.id,
            &s.kind,
            &s.name,
            &s.target,
            &s.category,
            &s.confidence,
            &s.path,
            &s.signature,
            &s.scope,
        ]
        .iter()
        .map(|s| s.capacity())
        .sum::<usize>();
    }
    assert_eq!(size_of::<DeferredSymbols>(), size_of::<usize>());
    let mut observed = 0;
    let mut peak_id_capacity = 0;
    for document in &projected.documents {
        visit(
            &pin.repository,
            &pin.exact_commit,
            &document.path,
            &document.facts,
            |row| {
                for field in [
                    &row.kind,
                    &row.name,
                    &row.target,
                    &row.category,
                    &row.confidence,
                    &row.path,
                    &row.signature,
                    &row.scope,
                ] {
                    assert!(matches!(field, Cow::Borrowed(_)));
                }
                match &row.id {
                    Cow::Owned(id) => {
                        assert_eq!(row.kind, "declaration");
                        assert_eq!(id.len(), 36);
                        peak_id_capacity = peak_id_capacity.max(id.capacity());
                    }
                    Cow::Borrowed(id) => {
                        assert_ne!(row.kind, "declaration");
                        assert!(id.is_empty());
                    }
                }
                assert_eq!(row.path.as_ptr(), document.path.as_ptr());
                observed += 1;
                Ok::<_, ()>(())
            },
        )
        .unwrap();
    }
    assert_eq!(observed, full.symbols.len());
    assert!(
        owned_bytes
            > size_of::<DeferredSymbols>() + size_of::<Symbol<Cow<'_, str>>>() + peak_id_capacity
    );
    let identity = index::fingerprint(&pin);
    let owned_output =
        persistence_wire::encode(&full, &identity, &OperationControl::default()).unwrap();
    struct ObserveSerialization;
    impl Drop for ObserveSerialization {
        fn drop(&mut self) {
            SERIALIZATION_PEAK.with(|slot| slot.set(None));
        }
    }
    SERIALIZATION_PEAK.with(|slot| assert_eq!(slot.replace(Some((0, 0))), None));
    let _observation = ObserveSerialization;
    let projected_output =
        persistence_wire::encode(&projected, &identity, &OperationControl::default()).unwrap();
    assert_eq!(projected_output, owned_output);
    assert_eq!(projected_output.capacity(), owned_output.capacity());
    let (live_id, live_output) = SERIALIZATION_PEAK.with(|slot| slot.get().unwrap());
    assert!(live_id >= 36 && live_output > 0);
    assert!(live_id <= peak_id_capacity);
    assert!(live_output <= projected_output.capacity());
    assert_eq!(ACTIVE_ID_BYTES.with(|slot| slot.get()), 0);
    // This directly observes ID/output overlap. Original Document/Facts/raw
    // owners remain live in both encoders; it is not a whole-process RSS cap.
    assert_eq!(projected.documents.len(), full.documents.len());
}

// Exercise exact output boundaries at small size without changing production
// MAXIMUM or allocating a second 128MiB fixture. This is the same Postcard
// Flavor contract as the bounded production output, not an alternate codec.
struct Limited {
    raw: Vec<u8>,
    maximum: usize,
}
impl postcard::ser_flavors::Flavor for Limited {
    type Output = Vec<u8>;
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        self.try_extend(&[byte])
    }
    fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
        if self
            .raw
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.maximum)
        {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.raw.extend_from_slice(bytes);
        Ok(())
    }
    fn finalize(self) -> postcard::Result<Vec<u8>> {
        Ok(self.raw)
    }
}
#[test]
fn borrowed_symbol_wire_exact_size_and_every_short_buffer_match_current_row_encoding() {
    let (full, projected, _) = fixture();
    let expected = postcard::to_allocvec(&SymbolsWire{rows:&full.symbols,documents:&full.documents,summary:&full.summary}).unwrap();
    for maximum in 0..=expected.len() + 1 {
        let old = postcard::serialize_with_flavor(
            &SymbolsWire{rows:&full.symbols,documents:&full.documents,summary:&full.summary},
            Limited {
                raw: vec![],
                maximum,
            },
        );
        let new = postcard::serialize_with_flavor(
            &SymbolsWire {
                rows: &projected.symbols,
                documents: &projected.documents,
                summary: &projected.summary,
            },
            Limited {
                raw: vec![],
                maximum,
            },
        );
        assert_eq!(new, old, "limit {maximum}");
        assert_eq!(new.is_ok(), maximum >= expected.len());
    }
}

#[test]
fn borrowed_symbol_encode_keeps_budget_identity_first_error_and_cancellation() {
    let (mut full, mut projected, pin) = fixture();
    let identity = index::fingerprint(&pin);
    for step in 1..=16 {
        let read = |deferred: bool| {
            let control = OperationControl::default();
            let _cancel = persistence_wire::CancelAtStep::new(step, &control);
            let result = if deferred {
                persistence_wire::encode(&projected, &identity, &control)
            } else {
                persistence_wire::encode(&full, &identity, &control)
            };
            result.map_err(|e| serde_json::to_vec(&e).unwrap())
        };
        assert_eq!(read(false), read(true), "cancel step {step}");
    }
    full.summary.schema = "invalid".into();
    projected.summary.schema = "invalid".into();
    projected.symbols.count = usize::MAX;
    assert_eq!(
        error(persistence_wire::encode(
            &full,
            &identity,
            &OperationControl::default()
        )),
        error(persistence_wire::encode(
            &projected,
            &identity,
            &OperationControl::default()
        ))
    );
    projected.summary.schema = "lycheedev.source-map.v2".into();
    let over = error(persistence_wire::encode(
        &projected,
        &identity,
        &OperationControl::default(),
    ));
    assert_eq!(
        serde_json::from_slice::<Value>(&over).unwrap()["code"],
        "codebase.source_budget"
    );
    projected.symbols.count = 2_000_000;
    projected
        .symbols
        .document(&pin, "late.lua", &rich_facts())
        .unwrap();
    assert!(projected.symbols.count > 2_000_000); // admission stays at encode
    assert_eq!(
        error(persistence_wire::encode(
            &projected,
            &identity,
            &OperationControl::default()
        )),
        over
    );
}

#[test]
fn borrowed_symbol_projection_rekeys_every_declaration_when_only_commit_changes() {
    let (full, mut projected, mut pin) = fixture();
    pin.exact_commit = "b".repeat(40);
    projected.summary.commit = pin.exact_commit.clone();
    let raw = persistence_wire::encode(
        &projected,
        &index::fingerprint(&pin),
        &OperationControl::default(),
    )
    .unwrap();
    let decoded: Index = persistence_wire::decode(
        &raw,
        &index::fingerprint(&pin),
        &OperationControl::default(),
    )
    .unwrap();
    let expected = project_symbols(&pin, &projected.documents);
    assert_eq!(
        serde_json::to_vec(&decoded.symbols).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    for (old, new) in full.symbols.iter().zip(&decoded.symbols) {
        if old.kind == "declaration" {
            assert_ne!(old.id, new.id);
        } else {
            assert_eq!(old.id, new.id);
        }
    }
}

fn source_fixture() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Empty.lua".into(), "".into()),
        ("Main.lua".into(), "local function Probe() Target() end\nfunction Probe() return 1 end\n".into()),
        ("Main.toc".into(), "## Interface: 120100\n## Notes: 雪\nMain.lua\nMain.lua\n".into()),
        ("Main.xml".into(), "<Ui><Frame name=\"Duplicate\"/><Frame name=\"Duplicate\"/><Script file=\"Main.lua\"/></Ui>".into()),
    ])
}
#[test]
fn borrowed_symbol_real_full_and_deferred_rebuilds_match_complete_index_and_facts_bytes() {
    let fixture = Fixture::new();
    let mut sources = source_fixture();
    for mode in ["cold", "change", "insert", "delete", "rename"] {
        match mode {
            "change" => {
                sources.insert(
                    "Main.lua".into(),
                    "function Changed() Target() end\n".into(),
                );
            }
            "insert" => {
                sources.insert("Before.lua".into(), "function Added() end\n".into());
            }
            "delete" => {
                sources.remove("Main.xml");
            }
            "rename" => {
                let raw = sources.remove("Main.lua").unwrap();
                sources.insert("Renamed.lua".into(), raw);
            }
            _ => (),
        }
        let pin = fixture.pin(&sources);
        let control = OperationControl::default();
        let full = index::build_uncached(&fixture.home, &pin, &control).unwrap();
        let projected: DeferredIndex =
            index::build_uncached_projected(&fixture.home, &pin, &control, &Profile::new())
                .unwrap();
        let identity = index::fingerprint(&pin);
        assert_eq!(
            persistence_wire::encode(&projected, &identity, &control).unwrap(),
            persistence_wire::encode(&full, &identity, &control).unwrap(),
            "{mode}"
        );
        assert_eq!(projected.summary.io["parsedDocuments"], 0);
        assert_eq!(projected.summary.io["cachedFacts"], sources.len());
        for (old, new) in full.documents.iter().zip(&projected.documents) {
            assert_eq!(
                serde_json::to_vec(&old.facts).unwrap(),
                serde_json::to_vec(&new.facts).unwrap()
            );
            assert_eq!(old.text, new.text);
        }
        assert_eq!(
            serde_json::to_vec(&full.symbols).unwrap(),
            serde_json::to_vec(&project_symbols(&pin, &full.documents)).unwrap()
        );
    }
}

#[test]
fn borrowed_symbol_real_repair_retains_prior_facts_for_full_pack_backfill() {
    let fixture = Fixture::new();
    let sources: BTreeMap<String, String> = (0..3)
        .map(|n| {
            (
                format!("D{n}.xml"),
                format!("<Ui><Frame name=\"F{n}\"/></Ui>"),
            )
        })
        .collect();
    let pin = fixture.pin(&sources);
    let control = OperationControl::default();
    let full = index::build_uncached(&fixture.home, &pin, &control).unwrap();
    let location = full.documents[1].facts_locator.as_ref().unwrap();
    let group: String = location.group.iter().map(|b| format!("{b:02x}")).collect();
    let path = fixture
        .home
        .join("cache/source-facts-rust")
        .join(format!("{group}.json"));
    let mut bytes = fs::read(&path).unwrap();
    bytes[location.offset as usize + 68] ^= 0xff;
    fs::write(&path, bytes).unwrap();
    let projected: DeferredIndex =
        index::build_uncached_projected(&fixture.home, &pin, &control, &Profile::new()).unwrap();
    assert_eq!(projected.summary.io["parsedDocuments"], 1);
    assert_eq!(projected.summary.io["cachedFacts"], 2);
    let identity = index::fingerprint(&pin);
    assert_eq!(
        persistence_wire::encode(&projected, &identity, &control).unwrap(),
        persistence_wire::encode(&full, &identity, &control).unwrap()
    );
    for d in &projected.documents {
        let cached = index::facts_cache::read_locator(
            &fixture.home,
            d.facts_locator.as_ref().unwrap(),
            &d.path,
            &d.sha256,
            &control,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&cached).unwrap(),
            serde_json::to_vec(&d.facts).unwrap()
        );
    }
}

struct ObserveOwned;
impl ObserveOwned {
    fn start() -> Self {
        OWNED_ROWS.with(|rows| assert_eq!(rows.replace(Some(0)), None));
        Self
    }
    fn count(&self) -> usize {
        OWNED_ROWS.with(|rows| rows.get().unwrap())
    }
}
impl Drop for ObserveOwned {
    fn drop(&mut self) {
        OWNED_ROWS.with(|rows| rows.set(None));
    }
}
#[test]
fn borrowed_symbol_default_source_index_uses_deferred_rebuild_and_publishes_complete_owned_cache() {
    let fixture = Fixture::new();
    let pin = fixture.pin(&source_fixture());
    let selected = selection::pin_selection(
        &fixture.home,
        selection::SelectionSpec {
            source: Some(pin.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let request = SourceRequest::Index(SelectedSource::new(selected).unwrap());
    let ctx = SourceContext {
        cwd: fixture.home.clone(),
        home: fixture.home.clone(),
        control: OperationControl::default(),
    };
    let observation = ObserveOwned::start();
    let cold = crate::dispatch(&ctx, &request).unwrap();
    assert_eq!(observation.count(), 0);
    assert!(cold.data["declarations"].as_u64().unwrap() > 0);
    let full = index::build_with_control(&fixture.home, &pin, &ctx.control).unwrap();
    assert!(!full.symbols.is_empty());
    assert_eq!(observation.count(), 0); // warm decoder, not rebuild
    let fresh = index::build_uncached(&fixture.home, &pin, &ctx.control).unwrap();
    assert_eq!(observation.count(), fresh.symbols.len()); // hook is live
    assert_eq!(
        serde_json::to_vec(&full.symbols).unwrap(),
        serde_json::to_vec(&fresh.symbols).unwrap()
    );
    assert_eq!(cold.data["declarations"], json!(full.summary.declarations));
}

#[test]
fn borrowed_symbol_default_summary_encode_cancellation_keeps_main_manifest_and_capture_absent() {
    for step in [1, 8] {
        let fixture = Fixture::new();
        let pin = fixture.pin(&source_fixture());
        let selected = selection::pin_selection(
            &fixture.home,
            selection::SelectionSpec {
                source: Some(pin.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let request = SourceRequest::Index(SelectedSource::new(selected).unwrap());
        let ctx = SourceContext {
            cwd: fixture.home.clone(),
            home: fixture.home.clone(),
            control: OperationControl::default(),
        };
        let failed = {
            let _cancel = persistence_wire::CancelAtStep::new(step, &ctx.control);
            crate::dispatch(&ctx, &request)
                .err()
                .expect("encode must observe cancellation")
        };
        assert_eq!(failed.code, "command.cancelled", "step {step}");
        let metadata = lycheedev_core::metadata::Metadata::open(&fixture.home, false).unwrap();
        assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
        assert!(
            metadata
                .list::<Value>("source/rust-index/", 100)
                .unwrap()
                .is_empty()
        );
        metadata.close().unwrap();
        // Facts committed before the cancelled main encoder remain reusable.
        let mut recovered = ctx.clone();
        recovered.control = OperationControl::default();
        let result = crate::dispatch(&recovered, &request).unwrap();
        assert_eq!(result.data["io"]["parsedDocuments"], 0);
        assert_eq!(result.data["io"]["cachedFacts"], source_fixture().len());
        assert_eq!(result.provenance.len(), 1);
    }
}
