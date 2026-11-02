//! Independent business expectations through Session -> Engine -> DataFusion.
use super::*;
use crate::capture::fixture::{Cell, table};

fn run(fx: &Fixture, mut request: Value) -> Value {
    request["mode"] = json!("static");
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime
        .block_on(session.execute(
            Task::Navigate {
                request: serde_json::from_value(request).unwrap(),
            },
            OperationControl::uncancelable(Instant::now() + Duration::from_secs(120)),
        ))
        .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let capture = &value["capture"];
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let bytes = lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["report"], value["report"]);
    assert_eq!(capture["complete"], value["report"]["complete"]);
    assert_eq!(capture["truncated"], value["report"]["truncated"]);
    drop(response);
    session.close().unwrap();
    value
}

fn complete(mut report: Value, expected_relations: Value) -> Value {
    report.as_object_mut().unwrap().extend(
        json!({
            "relations":expected_relations,"complete":true,"partial":false,"partialReasons":null,
            "truncated":false,"truncationReason":"none"
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    report
}

#[test]
fn spell_info_preserves_fractional_effect_speed_range_and_automatic_capture() {
    let fx = Fixture::new(&data::spell::tables());
    let value = run(
        &fx,
        json!({"kind":"spell_info","spell_id":100,"max_depth":5}),
    );
    let report = &value["report"];
    assert_eq!(
        report["spells"]["7922"]["effects"][0]["effectBasePoints"],
        -1000.5
    );
    assert_eq!(
        report["spells"]["100"]["effects"],
        json!([
            {"effectIndex":0,"effect":96,"effectAura":0,"effectTriggerSpell":null,"effectAuraPeriod":null,"effectBasePoints":0.0,"effectMechanic":0,"implicitTarget":[6,0],"effectRadiusIndex":[0,0],"effectMiscValue":[0,0]},
            {"effectIndex":2,"effect":64,"effectAura":0,"effectTriggerSpell":7922,"effectAuraPeriod":null,"effectBasePoints":0.0,"effectMechanic":0,"implicitTarget":[6,0],"effectRadiusIndex":[0,0],"effectMiscValue":[0,0]}
        ])
    );
    assert_eq!(
        report["spells"]["7922"]["effects"],
        json!([
            {"effectIndex":0,"effect":6,"effectAura":12,"effectTriggerSpell":null,"effectAuraPeriod":180000,"effectBasePoints":-1000.5,"effectMechanic":0,"implicitTarget":[6,0],"effectRadiusIndex":[0,0],"effectMiscValue":[0,0]}
        ])
    );
    assert_eq!(report["spells"]["100"]["misc"]["speed"], 1.25);
    assert_eq!(report["spells"]["7922"]["misc"]["speed"], 2.5);
    assert_eq!(
        report["spells"]["100"]["misc"]["range"],
        json!({"displayName":"Charge","rangeMin":[8.5,8.25],"rangeMax":[25.75,25.5]})
    );
    assert_eq!(
        report["spells"]["7922"]["misc"]["range"],
        report["spells"]["100"]["misc"]["range"]
    );
    assert_eq!(report["complete"], true);
    assert_eq!(report["truncated"], false);
}

#[test]
fn spell_info_preserves_complete_closure_effects_parents_and_description_links() {
    let fx = Fixture::new(&data::spell::closure_tables());
    let value = run(
        &fx,
        json!({"kind":"spell_info","spell_id":10,"max_depth":5}),
    );
    let effect = |kind, trigger, misc| {
        json!({
            "effectIndex":0,"effect":kind,"effectAura":0,"effectTriggerSpell":trigger,
            "effectAuraPeriod":null,"effectBasePoints":0.0,"effectMechanic":0,
            "implicitTarget":[6,0],"effectRadiusIndex":[0,0],"effectMiscValue":[misc,0]
        })
    };
    let expected = complete(
        json!({
            "spellID":10,"seedCount":1,"totalCount":4,"chainDepth":2,
            "triggers":{"10":[20],"20":[10]},"descRefs":{"10":[12345,77],"20":[10]},
            "spells":{
                "10":{"spellID":10,"name":"A","description":"see $12345x and $@spellname77; twice $@spellname77; self $@spellname10; not $1234d or $4294967296x","auraDescription":"$@spellname77","isSeed":true,"triggeredBy":[20],"misc":null,"effects":[effect(64,json!(20),20)]},
                "20":{"spellID":20,"name":"B","description":null,"auraDescription":"from $@spellname10","isSeed":false,"triggeredBy":[10],"misc":null,"effects":[effect(64,json!(10),0)]},
                "77":{"spellID":77,"name":"Description child","description":null,"auraDescription":null,"isSeed":false,"triggeredBy":[10],"misc":null,"effects":[]},
                "12345":{"spellID":12345,"name":"Numeric child","description":null,"auraDescription":null,"isSeed":false,"triggeredBy":[10],"misc":null,"effects":[]}
            }
        }),
        relations(&[
            ("SpellEffect", "EffectTriggerSpell", 4, "spell", 20, 0),
            ("Spell", "Description_lang", 10, "spell", 12345, 0),
            ("Spell", "Description_lang", 10, "spell", 77, 0),
            ("Spell", "Description_lang", 10, "spell", 77, 0),
            ("Spell", "AuraDescription_lang", 10, "spell", 77, 0),
            ("SpellEffect", "EffectTriggerSpell", 5, "spell", 10, 1),
            ("Spell", "AuraDescription_lang", 20, "spell", 10, 1),
        ]),
    );
    assert_eq!(value["report"], expected);
}

#[test]
fn spell_auras_and_summons_preserve_exact_rows_relations_duplicates_and_npc_filter() {
    let fx = Fixture::new(&data::spell::tables());
    assert_eq!(
        run(&fx, json!({"kind":"spell_auras","spell_id":7922}))["report"],
        complete(
            json!({"spellID":7922,"hasAura":[7922],"noAura":[]}),
            relations(&[("SpellEffect", "SpellID", 3, "spell", 7922, 0)])
        )
    );
    assert_eq!(
        run(&fx, json!({"kind":"spell_auras","spell_id":100}))["report"],
        complete(
            json!({"spellID":100,"hasAura":[],"noAura":[100]}),
            relations(&[
                ("SpellEffect", "SpellID", 1, "spell", 100, 0),
                ("SpellEffect", "SpellID", 2, "spell", 100, 0)
            ])
        )
    );
    let fx = Fixture::new(&data::spell::summon_tables());
    let summon = |id, index, npc| {
        json!({"spellID":300,"npcID":npc,"effectIndex":index,"row":{
            "ID":id,"SpellID":300,"EffectIndex":index,"Effect":28,"EffectAura":0,
            "EffectTriggerSpell":0,"EffectAuraPeriod":0,"EffectBasePointsF":0.0,"EffectMechanic":0,
            "ImplicitTarget":[6,0],"EffectRadiusIndex":[0,0],"EffectMiscValue":[npc,0]
        }})
    };
    assert_eq!(
        run(
            &fx,
            json!({"kind":"spell_summons","spell_id":300,"npc_id":0})
        )["report"],
        complete(
            json!({"spellID":300,"npcID":0,"count":3,"summons":[summon(9,1,500),summon(10,2,500),summon(11,3,600)]}),
            relations(&[
                ("SpellEffect", "EffectMiscValue", 9, "npc", 500, 0),
                ("SpellEffect", "EffectMiscValue", 10, "npc", 500, 0),
                ("SpellEffect", "EffectMiscValue", 11, "npc", 600, 0)
            ])
        )
    );
    assert_eq!(
        run(
            &fx,
            json!({"kind":"spell_summons","spell_id":300,"npc_id":500})
        )["report"],
        complete(
            json!({"spellID":300,"npcID":500,"count":2,"summons":[summon(9,1,500),summon(10,2,500)]}),
            relations(&[
                ("SpellEffect", "EffectMiscValue", 9, "npc", 500, 0),
                ("SpellEffect", "EffectMiscValue", 10, "npc", 500, 0)
            ])
        )
    );
}

fn section(
    id: u32,
    order: u32,
    parent: u32,
    first: u32,
    next: u32,
    spell: u32,
    children: Option<Value>,
) -> Value {
    let mut value = json!({"id":id,"title":format!("节{id}🙂"),"bodyText":"$74986A1\r\n正文","spellID":spell,
        "iconFlags":2,"type":1,"difficultyMask":-1,"iconCreatureDisplayInfoID":0,"orderIndex":order,
        "parentSectionID":parent,"firstChildSectionID":first,"nextSiblingSectionID":next});
    if spell != 0 {
        value["spellIDs"] = json!([spell]);
    }
    if let Some(children) = children {
        value["children"] = children;
    }
    value
}

#[test]
fn encounter_preserves_full_tree_sibling_order_parent_order_spells_files_and_relations() {
    let fx = data::encounter::fixture(
        &[
            data::encounter::section(2, 20, 1, 0, 0, 123),
            data::encounter::section(5, 2, 4, 0, 0, 0),
            data::encounter::section(1, 10, 0, 3, 0, 123),
            data::encounter::section(3, 30, 1, 0, 2, 456),
            data::encounter::section(4, 5, 0, 0, 0, 0),
            data::encounter::section(6, 1, 4, 0, 0, 0),
        ],
        true,
    );
    assert_eq!(
        run(
            &fx,
            json!({"kind":"encounter_get","encounter_id":132,"max_depth":32})
        )["report"],
        complete(
            json!({
                "journalEncounterID":132,"sectionCount":6,"spellCount":2,"spellIDs":[123,456],
                "sections":[
                    section(4,5,0,0,0,0,Some(json!([section(6,1,4,0,0,0,None),section(5,2,4,0,0,0,None)]))),
                    section(1,10,0,3,0,123,Some(json!([section(3,30,1,0,2,456,None),section(2,20,1,0,0,123,None)])))
                ],
                "relatedSpells":[{"spellID":123,"name":"Personal Phalanx","iconFileDataID":132337},{"spellID":456,"name":null,"iconFileDataID":0}],
                "relatedFiles":[{"kind":"icon_file","fileDataID":132337,"ownerKind":"spell","ownerID":123}],"unresolvedSectionIDs":[]
            }),
            relations(&[
                (
                    "JournalEncounterSection",
                    "ParentSectionID",
                    6,
                    "section",
                    4,
                    1
                ),
                (
                    "JournalEncounterSection",
                    "ParentSectionID",
                    5,
                    "section",
                    4,
                    1
                ),
                ("JournalEncounterSection", "SpellID", 1, "spell", 123, 0),
                (
                    "JournalEncounterSection",
                    "FirstChildSectionID",
                    1,
                    "section",
                    3,
                    1
                ),
                (
                    "JournalEncounterSection",
                    "NextSiblingSectionID",
                    1,
                    "section",
                    2,
                    1
                ),
                ("JournalEncounterSection", "SpellID", 3, "spell", 456, 1),
                ("JournalEncounterSection", "SpellID", 2, "spell", 123, 1),
                (
                    "SpellMisc",
                    "SpellIconFileDataID",
                    1,
                    "icon_file",
                    132337,
                    1
                ),
            ])
        )
    );
}

#[test]
fn creature_preserves_display_selection_model_order_texture_duplicates_variations_and_relations() {
    let fx = Fixture::new(&data::creature::tables());
    let mut selected = complete(
        json!({"displayID":10,"modelID":2,"fileDataID":500,"modelFileDataID":500,
        "textures":[800,800],"variations":[104,305],"requestedFileDataID":500}),
        relations(&[
            (
                "CreatureDisplayInfo",
                "ModelID",
                10,
                "creature_display",
                10,
                0,
            ),
            (
                "CreatureDisplayInfo",
                "ModelID",
                30,
                "creature_display",
                30,
                0,
            ),
            ("CreatureModelData", "FileDataID", 2, "model_file", 500, 1),
        ]),
    );
    selected["complete"] = json!(false);
    selected["partial"] = json!(true);
    selected["partialReasons"] = json!(["relation_ambiguous"]);
    assert_eq!(
        run(
            &fx,
            json!({"kind":"creature_display","display_id":0,"file_data_id":500})
        )["report"],
        selected
    );
    assert_eq!(
        run(
            &fx,
            json!({"kind":"creature_display","display_id":30,"file_data_id":0})
        )["report"],
        complete(
            json!({"displayID":30,"modelID":2,"fileDataID":500,"modelFileDataID":500,"textures":[700,701],"variations":[207]}),
            relations(&[("CreatureModelData", "FileDataID", 2, "model_file", 500, 1)])
        )
    );
    let mut models = complete(
        json!({"fileDataID":500,"displays":[
            {"displayID":10,"modelID":2,"fileDataID":500,"textures":[800,800],"variations":[104,305]},
            {"displayID":30,"modelID":2,"fileDataID":500,"textures":[700,701],"variations":[207]},
            {"displayID":20,"modelID":3,"fileDataID":500,"textures":[900],"variations":[]}
        ]}),
        relations(&[
            (
                "CreatureDisplayInfo",
                "ModelID",
                10,
                "creature_display",
                10,
                0,
            ),
            (
                "CreatureDisplayInfo",
                "ModelID",
                30,
                "creature_display",
                30,
                0,
            ),
            (
                "CreatureDisplayInfo",
                "ModelID",
                20,
                "creature_display",
                20,
                0,
            ),
        ]),
    );
    models["complete"] = json!(false);
    models["partial"] = json!(true);
    models["partialReasons"] = json!(["relation_ambiguous"]);
    assert_eq!(
        run(&fx, json!({"kind":"creature_model","file_data_id":500}))["report"],
        models
    );
}

fn empty_selection_fixture(stage: usize, partial: bool) -> (Fixture, Value, &'static str) {
    let (name, field, request) = match stage {
        0 => (
            "HouseDecor",
            "ItemID",
            json!({"kind":"decor_get","field":"item_id","wanted":99}),
        ),
        1 => (
            "CreatureModelData",
            "FileDataID",
            json!({"kind":"creature_display","display_id":0,"file_data_id":99}),
        ),
        2 => (
            "CreatureDisplayInfo",
            "ModelID",
            json!({"kind":"creature_display","display_id":0,"file_data_id":500}),
        ),
        _ => unreachable!(),
    };
    let mut selected = if partial {
        fixture::encrypted_sections(350)
    } else {
        table(
            "Sample",
            350,
            &[("ID", "uint"), ("Value", "uint")],
            &[vec![Cell::U(1), Cell::U(42)]],
        )
    };
    selected.name = name;
    selected.dbd = selected.dbd.replace("Value", field);
    let encoded = partial.then(|| fixture::encrypted_frame(&selected.raw).1);
    let mut tables = vec![selected];
    if stage == 2 {
        tables.push(table(
            "CreatureModelData",
            351,
            &[("ID", "uint"), ("FileDataID", "uint")],
            &[vec![Cell::U(99), Cell::U(500)]],
        ));
    }
    let fx = if let Some(encoded) = encoded {
        // Reuse the fixture-owned fresh root without introducing a second home.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-domain-unavailable-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        Fixture::with_encoded_tables_at(root, &tables, 350, encoded)
    } else {
        Fixture::new(&tables)
    };
    (fx, request, name)
}

fn empty_selection_error(stage: usize, partial: bool) -> lycheedev_core::ToolkitError {
    let (fx, mut request, name) = empty_selection_fixture(stage, partial);
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = || OperationControl::uncancelable(Instant::now() + Duration::from_secs(120));
    let query = serde_json::from_value(
        json!({"kind":"plan","plan":{"op":"scan","table":{"name":name,"mode":"static"}}}),
    )
    .unwrap();
    let response = runtime
        .block_on(session.execute(Task::Query { query }, control()))
        .unwrap();
    let Response::Query {
        result, complete, ..
    } = &response
    else {
        panic!("query receipt")
    };
    assert_eq!(
        *complete, !partial,
        "fixture coverage must precede the domain assertion"
    );
    let page = runtime
        .block_on(session.execute(
            Task::Page {
                handle: result.handle.clone(),
                offset: 0,
                limit: 10,
            },
            control(),
        ))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&page).unwrap()["rows"],
        json!([[1, 42]])
    );
    runtime
        .block_on(session.execute(
            Task::Release {
                handle: result.handle.clone(),
            },
            control(),
        ))
        .unwrap();
    drop(page);
    drop(response);
    request["mode"] = json!("static");
    let response = runtime.block_on(session.execute(
        Task::Navigate {
            request: serde_json::from_value(request).unwrap(),
        },
        control(),
    ));
    let error = response
        .err()
        .expect("empty selector must report absence or unavailable coverage");
    session.close().unwrap();
    error
}

fn assert_unavailable(stage: usize) {
    let error = empty_selection_error(stage, true);
    assert_eq!(
        error.code, "data.domain.record_unavailable",
        "selector stage {stage}"
    );
    assert_eq!(error.exit_code, 3);
}
#[test]
fn partial_empty_decor_selector_does_not_claim_record_not_found() {
    assert_unavailable(0);
}
#[test]
fn partial_empty_creature_model_selector_does_not_claim_record_not_found() {
    assert_unavailable(1);
}
#[test]
fn partial_empty_creature_display_selector_does_not_claim_record_not_found() {
    assert_unavailable(2);
}

#[test]
fn complete_empty_decor_and_creature_selectors_report_record_not_found() {
    for stage in 0..3 {
        let error = empty_selection_error(stage, false);
        assert_eq!(
            error.code, "records.record_not_found",
            "selector stage {stage}"
        );
        assert_eq!(error.exit_code, 3);
    }
}
