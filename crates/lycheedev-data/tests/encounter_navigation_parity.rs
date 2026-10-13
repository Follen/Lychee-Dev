//! Authenticated synthetic CASC/DBD/WDC provider and archive regression against
//! frozen Go navigate_encounter.go. These are not real-client acceptance.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{
    Cell,
    Cell::{I, Text, U},
    Fixture, Table, table,
};
use lycheedev_core::{Outcome, control::OperationControl, evidence, metadata::Metadata};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

fn section(id: u32, order: u32, parent: u32, first: u32, next: u32, spell: u32) -> Vec<Cell> {
    vec![
        U(id),
        U(132),
        Text(format!("节{id}🙂")),
        Text("$74986A1\r\n正文".into()),
        U(spell),
        I(2),
        I(1),
        I(-1),
        U(0),
        U(order),
        U(parent),
        U(first),
        U(next),
    ]
}
fn sections(rows: &[Vec<Cell>]) -> Table {
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
        rows,
    )
}
fn spell_tables() -> Vec<Table> {
    vec![
        table(
            "SpellName",
            332,
            &[("ID", "uint"), ("Name_lang", "text")],
            &[
                vec![U(123), Text("Personal Phalanx".into())],
                vec![U(456), Text("".into())],
            ],
        ),
        table(
            "SpellMisc",
            333,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("SpellIconFileDataID", "signed"),
            ],
            &[
                vec![U(3), U(123), I(999)],
                vec![U(1), U(123), I(132337)],
                vec![U(2), U(456), I(-1)],
            ],
        ),
    ]
}
fn fixture(rows: &[Vec<Cell>], spells: bool) -> Fixture {
    let mut tables = vec![sections(rows)];
    if spells {
        tables.extend(spell_tables());
    }
    Fixture::new(&tables)
}
fn captures(fx: &Fixture) -> usize {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len()
}
fn run(fx: &Fixture, extra: &[(&str, &str)]) -> Outcome {
    let mut flags = vec![("--journal-encounter-id", "132")];
    flags.extend_from_slice(extra);
    let out = lycheedev_data::dispatch(&fx.invocation("data encounter get", &flags)).unwrap();
    assert_eq!(out.provenance.len(), 2);
    for capture in &out.provenance {
        let id = capture["id"].as_str().unwrap();
        assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), *capture);
        assert_eq!(capture["complete"], out.complete);
        assert_eq!(capture["truncated"], out.data["truncated"]);
        let mut inv = fx.invocation("evidence verify", &[]);
        inv.flags.clear();
        inv.positional = Some(id.into());
        assert_eq!(evidence::dispatch(&inv).unwrap().data["verified"], true);
    }
    let result: Value = serde_json::from_slice(
        &evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(result, out.data);
    let manifest: Value = serde_json::from_slice(
        &evidence::read_blob(&fx.home, &out.provenance[1]["blob"], 16 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest,
        json!({"operation":"encounter_get","root":"journal_encounter:132","snapshot":out.data["snapshot"],"dataBuild":fixture::BUILD,"relations":out.data["relations"],"relatedFiles":out.data["relatedFiles"],"complete":out.complete,"truncated":out.data["truncated"],"truncationReason":out.data["truncationReason"]})
    );
    assert_eq!(out.provenance[0]["provenance"]["kind"], "encounter-get");
    assert_eq!(
        out.provenance[1]["provenance"]["locator"],
        "encounter:relations:132"
    );
    out
}
fn ids(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn tree_structural_order_spell_icons_and_two_capture_manifest_match_go() {
    let mut rows = vec![
        section(819, 4, 0, 820, 0, 0),
        section(818, 3, 0, 0, 0, 0),
        section(817, 2, 0, 0, 818, 0),
        section(816, 1, 0, 0, 817, 0),
        section(820, 9, 819, 0, 821, 123),
        section(821, 1, 819, 0, 0, 0),
    ];
    rows[5][8] = U(94300);
    let fx = fixture(&rows, true);
    let out = run(&fx, &[]);
    assert!(out.complete);
    assert_eq!(out.data["sectionCount"], 6);
    assert_eq!(ids(&out.data["sections"]), vec![816, 817, 818, 819]);
    assert_eq!(ids(&out.data["sections"][3]["children"]), vec![820, 821]);
    assert_eq!(out.data["sections"][0]["difficultyMask"], -1);
    assert_eq!(out.data["sections"][0]["bodyText"], "$74986A1\r\n正文");
    assert!(out.data["sections"][0].get("children").is_none());
    assert_eq!(
        out.data["sections"][3]["children"][0]["spellIDs"],
        json!([123])
    );
    assert_eq!(out.data["spellIDs"], json!([123]));
    assert_eq!(
        out.data["relatedSpells"],
        json!([{"spellID":123,"name":"Personal Phalanx","iconFileDataID":132337}])
    );
    assert_eq!(
        out.data["relatedFiles"],
        json!([{"kind":"icon_file","fileDataID":132337,"ownerKind":"spell","ownerID":123}])
    );
    assert_eq!(
        out.data["relations"],
        json!([
            {"fromTable":"JournalEncounterSection","fromField":"FirstChildSectionID","fromID":819,"toKind":"section","toID":820,"depth":1},
            {"fromTable":"JournalEncounterSection","fromField":"NextSiblingSectionID","fromID":819,"toKind":"section","toID":821,"depth":1},
            {"fromTable":"JournalEncounterSection","fromField":"SpellID","fromID":820,"toKind":"spell","toID":123,"depth":1},
            {"fromTable":"JournalEncounterSection","fromField":"IconCreatureDisplayInfoID","fromID":821,"toKind":"creature_display","toID":94300,"depth":1},
            {"fromTable":"SpellMisc","fromField":"SpellIconFileDataID","fromID":1,"toKind":"icon_file","toID":132337,"depth":1}
        ])
    );
    // Multiple misc matches deliberately select the first row without setting
    // ambiguity or truncation (the frozen encounter contract differs from decor).
    assert_eq!(out.data["partialReasons"], Value::Null);
    assert_eq!(
        out.data["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["JournalEncounterSection", "SpellName", "SpellMisc"]
    );
}

#[test]
fn empty_structural_orphan_and_depth_cut_encounters_do_not_open_spell_tables() {
    for rows in [vec![], vec![section(1, 0, 0, 0, 0, 0)]] {
        let fx = fixture(&rows, false);
        let out = run(&fx, &[]);
        assert!(out.complete);
        assert_eq!(out.data["spellCount"], 0);
        assert_eq!(out.data["tables"].as_array().unwrap().len(), 1);
        for field in [
            "spellIDs",
            "relatedSpells",
            "relatedFiles",
            "unresolvedSectionIDs",
        ] {
            assert_eq!(out.data[field], json!([]));
        }
    }
    let fx = fixture(&[section(1, 0, 999, 0, 0, 123)], false);
    let out = run(&fx, &[]);
    assert_eq!(out.data["unresolvedSectionIDs"], json!([1]));
    assert_eq!(out.data["partialReasons"], json!(["relation_unresolved"]));
    assert_eq!(out.data["truncated"], false);
    assert_eq!(out.data["sections"], json!([]));
    let fx = fixture(
        &[
            section(1, 0, 0, 0, 0, 0),
            section(2, 0, 1, 0, 0, 0),
            section(3, 0, 2, 0, 0, 123),
        ],
        false,
    );
    let out = run(&fx, &[("--max-depth", "1")]);
    assert_eq!(out.data["truncationReason"], "max_depth");
    assert_eq!(out.data["unresolvedSectionIDs"], json!([3]));
    assert_eq!(out.data["spellIDs"], json!([]));
    assert_eq!(out.data["tables"].as_array().unwrap().len(), 1);
}

#[test]
fn parent_fallback_sort_and_missing_spells_preserve_go_null_and_completeness() {
    let fx = fixture(
        &[
            section(5, 0, 0, 0, 0, 456),
            section(4, 0, 0, 0, 0, 999),
            section(30, 2, 4, 0, 0, 123),
            section(20, 2, 4, 0, 0, 123),
            section(40, 1, 4, 0, 0, 0),
        ],
        true,
    );
    let out = run(&fx, &[]);
    assert!(out.complete);
    assert_eq!(ids(&out.data["sections"]), vec![4, 5]);
    assert_eq!(ids(&out.data["sections"][0]["children"]), vec![40, 20, 30]);
    assert_eq!(out.data["spellIDs"], json!([999, 123, 456]));
    assert_eq!(
        out.data["relatedSpells"][0],
        json!({"spellID":999,"name":null,"iconFileDataID":0})
    );
    assert_eq!(
        out.data["relatedSpells"][2],
        json!({"spellID":456,"name":null,"iconFileDataID":0})
    );
    assert_eq!(
        out.data["relations"][1],
        json!({"fromTable":"JournalEncounterSection","fromField":"ParentSectionID","fromID":40,"toKind":"section","toID":4,"depth":1})
    );
}

#[test]
fn unresolved_order_chain_precedence_and_unreachable_cycles_are_partial_only() {
    let fx = fixture(
        &[
            section(1, 0, 0, 20, 0, 0),
            section(20, 0, 1, 0, 77, 0),
            section(3, 0, 1, 0, 0, 123),
            section(4, 0, 5, 0, 0, 0),
            section(5, 0, 4, 0, 0, 0),
        ],
        false,
    );
    let out = run(&fx, &[]);
    assert_eq!(ids(&out.data["sections"][0]["children"]), vec![20]);
    assert_eq!(out.data["unresolvedSectionIDs"], json!([77, 3, 4, 5]));
    assert_eq!(out.data["truncated"], false);
    assert!(!out.complete);
}

#[test]
fn reachable_ancestor_and_sibling_cycles_fail_without_captures() {
    for rows in [
        vec![section(1, 0, 0, 2, 0, 0), section(2, 0, 1, 1, 0, 0)],
        vec![section(1, 0, 0, 2, 0, 0), section(2, 0, 1, 0, 2, 0)],
    ] {
        let fx = fixture(&rows, false);
        let error = lycheedev_data::dispatch(
            &fx.invocation("data encounter get", &[("--journal-encounter-id", "132")]),
        )
        .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.relation_cycle", 4)
        );
        assert_eq!(captures(&fx), 0);
    }
}

#[test]
fn default_and_explicit_depth_and_section_limits_probe_exactly() {
    let rows: Vec<_> = (1..=34).map(|id| section(id, 0, id - 1, 0, 0, 0)).collect();
    let fx = fixture(&rows, false);
    for depth in ["0", "32"] {
        let out = run(&fx, &[("--max-depth", depth)]);
        assert_eq!(out.data["truncationReason"], "max_depth");
        assert_eq!(out.data["unresolvedSectionIDs"], json!([34]));
    }
    for count in [10000, 10001] {
        let rows: Vec<_> = (1..=count).map(|id| section(id, id, 0, 0, 0, 0)).collect();
        let fx = fixture(&rows, false);
        let out = run(&fx, &[]);
        assert_eq!(out.data["sectionCount"], 10000);
        assert_eq!(out.complete, count == 10000);
        assert_eq!(out.data["truncated"], count > 10000);
        assert_eq!(out.data["partial"], false);
        assert_eq!(
            out.data["truncationReason"],
            if count > 10000 { "row_budget" } else { "none" }
        );
    }
}

#[test]
fn dynamic_spell_tables_share_authenticated_source_and_resource_budget() {
    let fx = fixture(&[section(1, 0, 0, 0, 0, 123)], true);
    let schema = |name: &str| {
        let mut inv = fx.invocation("data db2 schema", &[]);
        inv.positional = Some(name.into());
        lycheedev_data::dispatch(&inv).unwrap().data["resources"]["metadataBytes"]
            .as_u64()
            .unwrap()
    };
    let names = ["JournalEncounterSection", "SpellName", "SpellMisc"];
    for name in names {
        schema(name);
    }
    let independent: u64 = names.into_iter().map(schema).sum();
    let out = run(&fx, &[]);
    let together = out.data["resources"]["metadataBytes"].as_u64().unwrap();
    assert!(
        together > 0 && together < independent,
        "dynamic={together}, repeated={independent}"
    );
    for proof in out.data["tableVerification"].as_array().unwrap() {
        for key in [
            "encodingFramingEKey",
            "encodingRequestedPagesMD5",
            "rootWholeDecodedCKey",
            "contentWholeDecodedCKey",
        ] {
            assert_eq!(proof[key], true);
        }
    }
}

#[test]
fn arguments_cancellation_deadline_and_corrupt_or_missing_spell_content_fail_closed() {
    let fx = fixture(&[], false);
    for flags in [
        vec![],
        vec![("--journal-encounter-id", "0")],
        vec![("--journal-encounter-id", "-1")],
        vec![("--journal-encounter-id", "4294967296")],
        vec![("--journal-encounter-id", "132"), ("--max-depth", "-1")],
        vec![("--journal-encounter-id", "132"), ("--max-depth", "33")],
        vec![("--journal-encounter-id", "132"), ("--max-depth", "bad")],
    ] {
        assert_eq!(
            lycheedev_data::dispatch(&fx.invocation("data encounter get", &flags))
                .unwrap_err()
                .exit_code,
            2
        );
    }
    let mut inv = fx.invocation("data encounter get", &[("--journal-encounter-id", "132")]);
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    inv.control = OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(false)));
    assert_eq!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "command.deadline"
    );
    assert_eq!(captures(&fx), 0);
    let fx = fixture(&[section(1, 0, 0, 0, 0, 123)], false);
    assert!(
        lycheedev_data::dispatch(
            &fx.invocation("data encounter get", &[("--journal-encounter-id", "132")])
        )
        .is_err()
    );
    assert_eq!(captures(&fx), 0);
    let mut row = section(1, 0, 0, 0, 0, 0);
    row[3] = Cell::BadString;
    let fx = fixture(&[row], false);
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data encounter get", &[("--journal-encounter-id", "132")])
        )
        .unwrap_err()
        .code,
        "table.invalid_format"
    );
    assert_eq!(captures(&fx), 0);
}

#[test]
fn spell_name_open_failure_precedes_missing_spell_misc_definition() {
    let mut names = spell_tables().remove(0);
    names.raw.pop();
    let fx = Fixture::new(&[sections(&[section(1, 0, 0, 0, 0, 123)]), names]);
    let error = lycheedev_data::dispatch(
        &fx.invocation("data encounter get", &[("--journal-encounter-id", "132")]),
    )
    .unwrap_err();
    assert_eq!(error.code, "table.invalid_format");
    assert_eq!(captures(&fx), 0);
}

#[test]
#[ignore = "explicit fresh retained fixtures for independent frozen-Go public differential"]
fn prepare_encounter_public_fixtures() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_ENCOUNTER_FIXTURE").expect("fresh owned fixture root"),
    );
    assert!(!root.exists(), "a fresh fixture directory is required");
    std::fs::create_dir_all(&root).unwrap();
    let mut baseline = vec![
        section(819, 4, 0, 820, 0, 0),
        section(818, 3, 0, 0, 0, 0),
        section(817, 2, 0, 0, 818, 0),
        section(816, 1, 0, 0, 817, 0),
        section(820, 9, 819, 0, 821, 123),
        section(821, 1, 819, 0, 0, 0),
    ];
    baseline[0][2] = Text("<标题>&\u{2028}\u{2029}🙂".into());
    baseline[5][8] = U(94300);
    for mut row in [
        section(901, 0, 0, 902, 0, 0),
        section(902, 0, 901, 901, 0, 0),
    ] {
        row[1] = U(133);
        baseline.push(row);
    }
    for mut row in [section(911, 0, 912, 0, 0, 0), section(912, 0, 911, 0, 0, 0)] {
        row[1] = U(134);
        baseline.push(row);
    }
    for n in 0..34 {
        let mut row = section(
            1000 + n,
            n,
            if n == 0 { 0 } else { 999 + n },
            if n == 33 { 0 } else { 1001 + n },
            0,
            0,
        );
        row[1] = U(136);
        baseline.push(row);
    }
    let mut base_tables = vec![sections(&baseline)];
    base_tables.extend(spell_tables());
    let mut corrupt_name = spell_tables().remove(0);
    corrupt_name.raw.pop();
    let mut corrupt_definition = spell_tables().remove(0);
    corrupt_definition.dbd = "not a DBD definition".into();
    let mut corrupt_body = section(1, 0, 0, 0, 0, 0);
    corrupt_body[3] = Cell::BadString;
    let expanded = (1..=6)
        .map(|id| {
            let mut row = section(id, id, 0, 0, 0, 0);
            row[3] = Text("<".repeat(500_000));
            row
        })
        .collect::<Vec<_>>();
    let crowded = (1..=10_001)
        .map(|id| section(id, id, 0, 0, 0, 0))
        .collect::<Vec<_>>();
    let variants = vec![
        ("baseline", base_tables, 0x40),
        (
            "missingSpell",
            vec![sections(&[section(1, 0, 0, 0, 0, 123)])],
            0x40,
        ),
        (
            "badSpellThenMissing",
            vec![sections(&[section(1, 0, 0, 0, 0, 123)]), corrupt_name],
            0x40,
        ),
        (
            "badDefinitionThenMissing",
            vec![sections(&[section(1, 0, 0, 0, 0, 123)]), corrupt_definition],
            0x40,
        ),
        ("badBody", vec![sections(&[corrupt_body])], 0x40),
        ("htmlCap", vec![sections(&expanded)], 0x40),
        ("rowBudget", vec![sections(&crowded)], 0x40),
        (
            "wrongLocale",
            vec![sections(&[section(1, 0, 0, 0, 0, 0)])],
            0x2,
        ),
    ];
    let mut fixtures = serde_json::Map::new();
    for (name, tables, mask) in variants {
        let fx = Fixture::at(root.join(name), &tables, "zhCN", mask);
        let identity = fx.root.join("input-identity.json");
        fixtures.insert(name.into(), json!({"identity":identity,"sha256":lycheedev_core::storage::sha256(&std::fs::read(&identity).unwrap())}));
    }
    std::fs::write(
        root.join("suite.json"),
        serde_json::to_vec_pretty(
            &json!({"schema":"lycheedev.catalog-public-fixtures.v1","fixtures":fixtures}),
        )
        .unwrap(),
    )
    .unwrap();
}
