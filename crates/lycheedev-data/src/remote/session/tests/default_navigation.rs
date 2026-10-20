//! Actual navigation dispatch over synthetic, authenticated offline CDN bodies.
use super::*;
use lycheedev_core::{
    Invocation, evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec},
};
use serde_json::{Value, json};
#[allow(dead_code)]
#[path = "../../../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, Table, table};

struct NavFixture {
    source: Fixture,
    snapshot: String,
}
impl NavFixture {
    fn new(tables: &[Table]) -> Self {
        assert!(!tables.is_empty() && tables.len() < 26);
        let mut source = Fixture::with_payload(false, None, tables[0].raw.clone());
        let route = catalog::Distribution {
            path: "tpr/test".into(),
            hosts: vec!["cdn.example.test".into()],
        };
        let old_build = route
            .config_url(&route.hosts[0], &source.pin.build_config)
            .unwrap();
        let mut ordered: Vec<_> = tables.iter().collect();
        ordered.sort_by_key(|table| table.id);
        let mut root = Vec::new();
        root.extend_from_slice(&(tables.len() as u32).to_le_bytes());
        root.extend_from_slice(&0u32.to_le_bytes());
        root.extend_from_slice(&0x40u32.to_le_bytes());
        let mut next = 0;
        for table in &ordered {
            assert!(table.id >= next);
            root.extend_from_slice(&(table.id - next).to_le_bytes());
            next = table.id + 1;
        }
        let mut rows = Vec::new();
        let mut objects = BTreeMap::new();
        for table in ordered {
            let ckey = hash(&table.raw);
            root.extend_from_slice(&casc::key(&ckey).unwrap());
            root.extend_from_slice(&0u64.to_le_bytes());
            let (ekey, encoded) = framed(&table.raw);
            rows.push((ckey, ekey.clone(), table.raw.len(), encoded.len()));
            objects.insert(ekey, encoded);
        }
        let rckey = hash(&root);
        let (rekey, rencoded) = framed(&root);
        rows.push((rckey.clone(), rekey.clone(), root.len(), rencoded.len()));
        objects.insert(rekey, rencoded);
        let enc = encoding(&rows);
        let (ekey, encoded) = framed(&enc);
        let build = format!(
            "root = {rckey}\nencoding = {} {ekey}\nencoding-size = {} {}\nbuild-uid = wow\n",
            hash(&enc),
            enc.len(),
            encoded.len()
        )
        .into_bytes();
        objects.insert(ekey, encoded);
        *source.backend.objects.get_mut().unwrap() = objects;
        source.pin.build_config = hash(&build);
        source.backend.metadata.remove(&old_build).unwrap();
        source.backend.metadata.insert(
            route
                .config_url(&route.hosts[0], &source.pin.build_config)
                .unwrap(),
            build,
        );
        let pin = selection::pin_selection(
            &source.home,
            SelectionSpec {
                data: Some(source.pin.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let base = format!(
            "https://raw.githubusercontent.com/wowdev/WoWDBDefs/{}/",
            source.pin.definition_commit
        );
        let manifest: Vec<_> = tables.iter().map(|table| json!({
            "tableName":table.name, "tableHash":format!("{:08X}",table.hash), "db2FileDataID":table.id
        })).collect();
        let mut definitions = vec![(
            "manifest.json".to_string(),
            serde_json::to_vec(&manifest).unwrap(),
        )];
        definitions.extend(tables.iter().map(|table| {
            (
                format!("definitions/{}.dbd", table.name),
                table
                    .dbd
                    .replace(catalog_fixture::BUILD, &source.pin.full_build)
                    .into_bytes(),
            )
        }));
        let metadata = crate::cache_fixture::Cache::new(&source.home);
        for (suffix, bytes) in definitions {
            let url = format!("{base}{suffix}");
            let blob = evidence::publish_blob(&source.home, &bytes, 4 << 20).unwrap();
            metadata
                .put(
                    &format!("definition/{}", storage::sha256(url.as_bytes())),
                    &json!({"url":url,"blob":blob}),
                    None,
                )
                .unwrap();
        }

        // Prime through the real authenticated Session, then invoke the default
        // provider offline. No test backend is injected into command dispatch.
        let mut admitted = source.admission();
        let mut reader = Session::borrowed_with(
            &mut admitted,
            &source.pin,
            source.options(),
            &source.budget,
            &source.backend,
            &source.backend,
        )
        .unwrap();
        let ids: Vec<_> = tables.iter().map(|table| table.id).collect();
        reader
            .read_batches(&ids, &mut |files| {
                assert_eq!(files.len(), tables.len());
                for (file, table) in files.iter().zip(tables) {
                    assert_eq!(file.bytes(), table.raw.as_slice());
                }
                Ok(Vec::new())
            })
            .unwrap();
        reader.close().unwrap();
        admitted.finish(Ok(())).unwrap();
        source.backend.calls.lock().unwrap().clear();
        Self {
            source,
            snapshot: pin.id,
        }
    }
    fn invocation(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        let mut inv = Invocation {
            command: command.into(),
            positional: None,
            flags: BTreeMap::from([
                ("--snapshot".into(), self.snapshot.clone()),
                ("--cdn".into(), "true".into()),
                ("--offline".into(), "true".into()),
            ]),
            home: self.source.home.clone(),
            cwd: self.source.home.clone(),
            control: OperationControl::default(),
        };
        inv.flags.extend(
            flags
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        inv
    }
    fn captures(&self) -> Vec<Value> {
        let metadata = Metadata::open(&self.source.home, false).unwrap();
        let values = metadata.list::<Value>("capture/", 100).unwrap();
        metadata.close().unwrap();
        values
    }
    fn verify(&self, outcome: &lycheedev_core::Outcome) {
        assert_eq!(outcome.data["source"], "cdn");
        assert!(!outcome.provenance.is_empty());
        for capture in &outcome.provenance {
            assert_eq!(capture["provenance"]["snapshot"], self.snapshot);
            let raw = evidence::read_blob(&self.source.home, &capture["blob"], 16 << 20).unwrap();
            assert_eq!(storage::sha256(&raw), capture["blob"]["sha256"]);
        }
        let raw = evidence::read_blob(&self.source.home, &outcome.provenance[0]["blob"], 16 << 20)
            .unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), outcome.data);
        for table in outcome.data["tables"].as_array().unwrap() {
            assert_eq!(table["contentVerified"], true);
        }
        assert!(self.source.backend.calls.lock().unwrap().is_empty());
    }
    fn close(self) {
        std::fs::remove_dir_all(self.source.home).unwrap();
    }
}

#[test]
fn default_remote_navigation_item_joins_two_authenticated_tables_and_captures() {
    let f = NavFixture::new(&catalog_fixture::items());
    let out = crate::dispatch(&f.invocation("data item get", &[("--item-id", "7")])).unwrap();
    assert_eq!(out.data["item"]["id"], 7);
    assert_eq!(out.data["item"]["slotName"], "Waist");
    assert_eq!(
        out.data["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Item", "ItemSparse"]
    );
    assert_eq!(out.data["resources"]["networkRequests"], 0);
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_navigation_aura_and_summon_profiles_use_real_spell_effect_rows() {
    use Cell::{Array, I, U};
    let f = NavFixture::new(&[table(
        "SpellEffect",
        501,
        &[
            ("ID", "uint"),
            ("SpellID", "uint"),
            ("Effect", "signed"),
            ("EffectIndex", "signed"),
            ("EffectMiscValue", "array3"),
        ],
        &[
            vec![U(1), U(7), I(6), I(0), Array(vec![U(0), U(0), U(0)])],
            vec![U(2), U(7), I(28), I(1), Array(vec![U(321), U(0), U(0)])],
        ],
    )]);
    let aura = crate::dispatch(&f.invocation("data spell auras", &[("--spell-id", "7")])).unwrap();
    assert_eq!(aura.data["hasAura"], json!([7]));
    f.verify(&aura);
    let summon = crate::dispatch(&f.invocation(
        "data spell summons",
        &[("--spell-id", "7"), ("--npc-id", "321")],
    ))
    .unwrap();
    assert_eq!(summon.data["count"], 1);
    assert_eq!(summon.data["summons"][0]["npcID"], 321);
    f.verify(&summon);
    f.close();
}

#[test]
fn default_remote_navigation_decor_preserves_limit_and_zero_model() {
    let f = NavFixture::new(&[catalog_fixture::decors(&catalog_fixture::decor_rows())]);
    let out = crate::dispatch(&f.invocation("data decor list", &[("--limit", "2")])).unwrap();
    assert_eq!(out.data["count"], 2);
    assert_eq!(out.data["truncated"], true);
    assert_eq!(out.data["truncationReason"], "limit_reached");
    f.verify(&out);
    let out = crate::dispatch(&f.invocation("data decor get", &[("--id", "10")])).unwrap();
    assert_eq!(out.data["item"]["modelFileDataID"], 0);
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_navigation_encounter_follows_dynamic_tables_and_two_captures() {
    use Cell::{I, Text, U};
    let f = NavFixture::new(&[
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("Title_lang", "text"),
                ("BodyText_lang", "text"),
                ("SpellID", "uint"),
                ("IconFlags", "signed"),
                ("Type", "signed"),
                ("DifficultyMask", "signed"),
                ("IconCreatureDisplayInfoID", "uint"),
                ("OrderIndex", "uint"),
                ("ParentSectionID", "uint"),
                ("FirstChildSectionID", "uint"),
                ("NextSiblingSectionID", "uint"),
            ],
            &[vec![
                U(1),
                U(132),
                Text("section".into()),
                Text("body".into()),
                U(123),
                I(0),
                I(1),
                I(-1),
                U(0),
                U(0),
                U(0),
                U(0),
                U(0),
            ]],
        ),
        table(
            "SpellName",
            332,
            &[("ID", "uint"), ("Name_lang", "text")],
            &[vec![U(123), Text("named spell".into())]],
        ),
        table(
            "SpellMisc",
            333,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("SpellIconFileDataID", "signed"),
            ],
            &[vec![U(1), U(123), I(999)]],
        ),
    ]);
    let out =
        crate::dispatch(&f.invocation("data encounter get", &[("--journal-encounter-id", "132")]))
            .unwrap();
    assert_eq!(out.provenance.len(), 2);
    assert_eq!(out.data["spellCount"], 1);
    assert_eq!(out.data["relatedSpells"][0]["name"], "named spell");
    assert_eq!(out.data["relatedSpells"][0]["iconFileDataID"], 999);
    assert_eq!(
        out.data["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["JournalEncounterSection", "SpellName", "SpellMisc"]
    );
    assert_eq!(out.provenance[1]["provenance"]["kind"], "relation-manifest");
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_navigation_holds_admission_until_capture_and_stops_without_capture() {
    let f = NavFixture::new(&catalog_fixture::items());
    let inv = f.invocation("data item get", &[("--item-id", "7")]);
    let before = f.captures().len();
    let mut called = false;
    let error = crate::navigate::dispatch_after_read(&inv, &mut || {
        called = true;
        let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
        assert!(
            lycheedev_core::workspace::data_resources::acquire(
                &f.source.home,
                Policy::Measurement,
                &short
            )
            .is_err()
        );
        inv.control.cancel_handle().store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert!(called);
    assert_eq!(error.exit_code, 7);
    assert_eq!(f.captures().len(), before);
    lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &OperationControl::default(),
    )
    .unwrap()
    .close()
    .unwrap();
    f.close();
}

#[test]
fn default_remote_navigation_admits_before_pin_and_rejects_conflicting_installation() {
    let f = NavFixture::new(&catalog_fixture::items());
    let control = OperationControl::default();
    let held = lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &control,
    )
    .unwrap();
    let mut inv = f.invocation("data item get", &[("--item-id", "7")]);
    inv.flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(100));
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        inv.control.check().unwrap_err().code
    );
    held.close().unwrap();
    inv.control = OperationControl::default();
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        "vault.record_missing"
    );
    inv.flags.insert("--snapshot".into(), f.snapshot.clone());
    inv.flags
        .insert("--installation".into(), "missing-client".into());
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        "records.metadata_limit"
    );
    assert!(f.captures().is_empty());
    f.close();
}

mod adaptive;

#[test]
fn default_remote_db2_navigation_and_encounter_keep_committed_captures_after_final_control_and_close()
 {
    use crate::capture::committed_tests;
    let tables: Vec<_> = committed_tests::tables()
        .into_iter()
        .map(|table| Table {
            name: table.name,
            id: table.id,
            hash: table.hash,
            layout: table.layout,
            raw: table.raw,
            dbd: table.dbd,
        })
        .collect();
    let fixture = NavFixture::new(&tables);
    for (inv, count) in committed_tests::invocations(&fixture.invocation("", &[])) {
        committed_tests::finalization_cases(&inv, count);
    }
    assert!(fixture.source.backend.calls.lock().unwrap().is_empty());
    fixture.close();
}
