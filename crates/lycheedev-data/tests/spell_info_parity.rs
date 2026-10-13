//! Authenticated CASC/DBD/WDC inputs for frozen Go spell-info behavior.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Cell, Cell::{Array, F, I, Text, U}, Fixture, Table, table};
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

fn ints(a: i32, b: i32) -> Cell { Array(vec![I(a), I(b)]) }
fn effect(id: u32, spell: u32, kind: i32, trigger: u32, misc: i32) -> Vec<Cell> {
    vec![U(id),U(spell),U(0),I(kind),I(0),U(trigger),U(0),F(0.0),I(0),ints(6,0),ints(0,0),ints(misc,0)]
}
fn effects(rows: &[Vec<Cell>]) -> Table {
    table("SpellEffect",303,&[("ID","uint"),("SpellID","uint"),("EffectIndex","uint"),("Effect","signed"),
        ("EffectAura","signed"),("EffectTriggerSpell","uint"),("EffectAuraPeriod","uint"),("EffectBasePointsF","float"),
        ("EffectMechanic","signed"),("ImplicitTarget","array2signed"),("EffectRadiusIndex","array2signed"),("EffectMiscValue","array2signed")],rows)
}
fn names(rows: &[Vec<Cell>]) -> Table { table("SpellName",301,&[("ID","uint"),("Name_lang","text")],rows) }
fn spells(rows: &[Vec<Cell>]) -> Table { table("Spell",302,&[("ID","uint"),("Description_lang","text"),("AuraDescription_lang","text")],rows) }
fn tables() -> Vec<Table> {
    let mut stun=effect(3,7922,6,0,0);stun[4]=I(12);stun[6]=U(180000);stun[7]=F(-1000.5);
    let mut trigger=effect(2,100,64,7922,0);trigger[2]=U(2);
    vec![
        names(&[vec![U(100),Text("Charge<&>🙂".into())],vec![U(7922),Text("Charge Stun".into())],vec![U(10),Text("A".into())],vec![U(20),Text("B".into())]]),
        spells(&[
            vec![U(100),Text("Charge an enemy, generate $/10;s2 rage, and stun it for $7922d.".into()),Text("".into())],
            vec![U(7922),Text("".into()),Text("Stunned.".into())],
            vec![U(10),Text("see $12345x and $@spellname77; twice $@spellname77; self $@spellname10; not $1234d or $4294967296x".into()),Text("$@spellname77".into())],
            vec![U(20),Text("".into()),Text("from $@spellname10".into())],
            vec![U(30),Text("self $@spellname30".into()),Text("".into())],
        ]),
        effects(&[trigger,effect(1,100,96,0,0),stun,effect(4,10,64,20,20),effect(5,20,64,10,0),effect(6,30,64,30,30),effect(7,40,64,10,20),effect(8,50,6,0,77)]),
        table("SpellMisc",304,&[("ID","uint"),("SpellID","uint"),("Attributes","array3"),("SchoolMask","signed"),("Speed","signed"),("SpellIconFileDataID","signed"),("CastingTimeIndex","uint"),("DurationIndex","uint"),("RangeIndex","uint")],&[
            vec![U(3),U(100),Array(vec![U(1),U(2),U(3)]),I(7),I(9),I(999),U(0),U(0),U(0)],
            vec![U(1),U(100),Array(vec![U(805634064),U(1024),U(0)]),I(1),I(0),I(132337),U(1),U(0),U(1)],
            vec![U(2),U(7922),Array(vec![U(327696),U(512),U(0)]),I(1),I(0),I(135860),U(1),U(2),U(1)],
        ]),
        table("SpellCastTimes",305,&[("ID","uint"),("Base","signed"),("Minimum","signed")],&[vec![U(1),I(0),I(0)]]),
        table("SpellDuration",306,&[("ID","uint"),("Duration","signed"),("MaxDuration","signed")],&[vec![U(2),I(1000),I(1000)]]),
        table("SpellRange",307,&[("ID","uint"),("DisplayName_lang","text"),("RangeMin","array2signed"),("RangeMax","array2signed")],&[vec![U(1),Text("Charge".into()),ints(8,8),ints(25,25)]]),
    ]
}
fn run(fx: &Fixture, seed: &str, flags: &[(&str,&str)]) -> Outcome {
    let mut flags=flags.to_vec();flags.push(("--spell-id",seed));
    let out=lycheedev_data::dispatch(&fx.invocation("data spell info",&flags)).unwrap();
    fx.verify(&out);
    assert_eq!(out.provenance[0]["provenance"]["kind"],"spell-info");
    assert_eq!(out.provenance[0]["provenance"]["locator"],format!("spell:info:{seed}"));
    assert_eq!(out.data["tables"].as_array().unwrap().iter().map(|t|t["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["SpellName","Spell","SpellEffect","SpellMisc","SpellCastTimes","SpellDuration","SpellRange"]);
    out
}
fn captures(fx: &Fixture) -> usize { Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap().len() }

#[test]
fn charge_details_float_array_nulls_relations_and_capture_preserve_go() {
    let fx=Fixture::new(&tables());let out=run(&fx,"100",&[]);let v=&out.data;
    assert!(out.complete);assert_eq!(v["partialReasons"],Value::Null);assert_eq!(v["truncated"],false);
    assert_eq!(v["seedCount"],1);assert_eq!(v["totalCount"],2);assert_eq!(v["chainDepth"],2);
    assert_eq!(v["triggers"],json!({"100":[7922]}));assert_eq!(v["descRefs"],json!({}));
    let charge=&v["spells"]["100"];let stun=&v["spells"]["7922"];
    assert_eq!(charge["name"],"Charge<&>🙂");assert_eq!(charge["auraDescription"],Value::Null);
    assert_eq!(charge["triggeredBy"],json!([]));assert_eq!(charge["isSeed"],true);
    assert_eq!(stun["description"],Value::Null);assert_eq!(stun["auraDescription"],"Stunned.");assert_eq!(stun["triggeredBy"],json!([100]));
    assert_eq!(charge["misc"],json!({"attributes":[805634064,1024,0],"schoolMask":1,"speed":0,"spellIconFileDataID":132337,
        "castTime":{"base":0,"minimum":0},"duration":null,"range":{"displayName":"Charge","rangeMin":[8,8],"rangeMax":[25,25]}}));
    assert_eq!(stun["misc"]["duration"],json!({"duration":1000,"maxDuration":1000}));
    assert_eq!(charge["effects"][0]["effectTriggerSpell"],Value::Null);assert_eq!(charge["effects"][1]["effectIndex"],2);
    assert_eq!(stun["effects"][0],json!({"effectIndex":0,"effect":6,"effectAura":12,"effectTriggerSpell":null,"effectAuraPeriod":180000,
        "effectBasePoints":-1000,"effectMechanic":0,"implicitTarget":[6,0],"effectRadiusIndex":[0,0],"effectMiscValue":[0,0]}));
    assert_eq!(v["relations"],json!([
        {"fromTable":"SpellEffect","fromField":"EffectTriggerSpell","fromID":2,"toKind":"spell","toID":7922,"depth":0},
        {"fromTable":"SpellMisc","fromField":"SpellIconFileDataID","fromID":1,"toKind":"icon_file","toID":132337,"depth":1},
        {"fromTable":"SpellMisc","fromField":"SpellIconFileDataID","fromID":2,"toKind":"icon_file","toID":135860,"depth":1}
    ]));
}

#[test]
fn cycles_duplicate_references_misc_children_and_missing_rows_keep_exact_semantics() {
    let fx=Fixture::new(&tables());let out=run(&fx,"40",&[]);let v=&out.data;
    assert!(out.complete);assert_eq!(v["chainDepth"],3);assert_eq!(v["totalCount"],5);
    assert_eq!(v["triggers"],json!({"40":[10,20],"10":[20],"20":[10]}));
    assert_eq!(v["descRefs"],json!({"10":[12345,77],"20":[10]}));
    assert_eq!(v["spells"]["10"]["triggeredBy"],json!([20,40]));assert_eq!(v["spells"]["20"]["triggeredBy"],json!([10,40]));
    assert_eq!(v["spells"]["77"],json!({"spellID":77,"name":null,"description":null,"auraDescription":null,
        "isSeed":false,"triggeredBy":[10],"misc":null,"effects":[]}));
    let refs=v["relations"].as_array().unwrap().iter().filter(|r|r["fromTable"]=="Spell"&&r["fromID"]==10&&r["toID"]==77).count();
    assert_eq!(refs,3,"duplicate text occurrences retain each source relation");
    assert_eq!(v["relations"][1]["fromField"],"EffectMiscValue");
    for seed in ["30","50","999"] {
        let out=run(&fx,seed,&[]);assert!(out.complete);assert_eq!(out.data["totalCount"],1);assert_eq!(out.data["chainDepth"],1);
        assert_eq!(out.data["triggers"],json!({}));assert_eq!(out.data["descRefs"],json!({}));assert_eq!(out.data["relations"],json!([]));
    }
}

fn chain_tables() -> Vec<Table> {
    let mut all=tables();let rows=(0..34).map(|n|effect(n+1,1000+n,64,1001+n,0)).collect::<Vec<_>>();all[2]=effects(&rows);all
}
#[test]
fn depth_default_zero_one_and_maximum_keep_full_boundary_leaf_detail() {
    let fx=Fixture::new(&tables());let out=run(&fx,"100",&[("--max-depth","1")]);
    assert!(!out.complete);assert_eq!(out.data["truncationReason"],"max_depth");assert_eq!(out.data["totalCount"],2);
    assert_eq!(out.data["spells"]["7922"]["effects"][0]["effectBasePoints"],-1000);
    assert_eq!(out.data["spells"]["7922"]["misc"]["duration"]["duration"],1000);
    assert_eq!(out.warnings.len(),1);
    let fx=Fixture::new(&chain_tables());
    for (flags,depth) in [(vec![],5),(vec![("--max-depth","0")],5),(vec![("--max-depth","32")],32)] {
        let out=run(&fx,"1000",&flags);assert_eq!(out.data["chainDepth"],depth);assert_eq!(out.data["totalCount"],depth+1);
        assert_eq!(out.data["truncationReason"],"max_depth");
        let leaf=(1000+depth).to_string();assert!(out.data["triggers"].get(&leaf).is_none());
        assert_eq!(out.data["spells"][&leaf]["effects"][0]["effectTriggerSpell"],1001+depth);
    }
}

#[test]
fn row_and_relation_budgets_are_visible_with_original_result_and_capture_flags() {
    let mut all=tables();
    // Minimal valid effect rows keep this actual 65,537-row result within the
    // independent 16 MiB evidence limit, so the row-bound behavior is observed.
    let rows=(1..=65537).map(|id|vec![U(id),U(100)]).collect::<Vec<_>>();
    all[2]=table("SpellEffect",303,&[("ID","uint"),("SpellID","uint")],&rows);
    let fx=Fixture::new(&all);let out=run(&fx,"100",&[]);
    assert_eq!(out.data["spells"]["100"]["effects"].as_array().unwrap().len(),65536);
    assert_eq!(out.data["truncationReason"],"row_budget");assert_eq!(out.data["chainDepth"],1);assert!(!out.complete);
    let mut all=tables();all[2]=effects(&[]);
    all[1]=spells(&[vec![U(100),Text("$@spellname77 ".repeat(65537)),Text("".into())]]);
    let fx=Fixture::new(&all);let out=run(&fx,"100",&[]);
    assert_eq!(out.data["relations"].as_array().unwrap().len(),65536);
    assert_eq!(out.data["truncationReason"],"relation_budget");assert_eq!(out.data["descRefs"],json!({"100":[77]}));
    assert_eq!(out.data["totalCount"],2);assert!(!out.complete);
}

#[test]
fn arguments_cancellation_deadline_and_ordered_table_failures_publish_no_capture() {
    let fx=Fixture::new(&tables());
    for flags in [vec![],vec![("--spell-id","0")],vec![("--spell-id","-1")],vec![("--spell-id","4294967296")],
        vec![("--spell-id","100"),("--max-depth","-1")],vec![("--spell-id","100"),("--max-depth","33")],vec![("--spell-id","100"),("--max-depth","bad")]] {
        assert_eq!(lycheedev_data::dispatch(&fx.invocation("data spell info",&flags)).unwrap_err().exit_code,2);
    }
    let mut inv=fx.invocation("data spell info",&[("--spell-id","100")]);inv.control.cancel_handle().store(true,Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code,7);
    inv.control=OperationControl::new(Instant::now(),Arc::new(AtomicBool::new(false)));
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code,"command.deadline");assert_eq!(captures(&fx),0);
    let mut first=names(&[vec![U(100),Text("A".into())]]);first.raw.pop();
    let fx=Fixture::new(&[first]);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data spell info",&[("--spell-id","100")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);
    let fx=Fixture::new(&tables()[..6]);
    assert!(lycheedev_data::dispatch(&fx.invocation("data spell info",&[("--spell-id","999")])).is_err());
    assert_eq!(captures(&fx),0,"all seven required tables must open even for an absent seed");
    let mut all=tables();all[2]=table("SpellEffect",303,&[("ID","uint"),("SpellID","uint"),("Unused","text")],&[vec![U(1),U(777),Cell::BadString]]);
    let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data spell info",&[("--spell-id","100")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0,"nonmatching effect rows are still fully decoded");
}

#[test]
fn seven_tables_share_authenticated_source_metadata_and_warm_query() {
    let fx=Fixture::new(&tables());let cold=run(&fx,"100",&[]);let warm=run(&fx,"100",&[]);
    for out in [&cold,&warm] {for proof in out.data["tableVerification"].as_array().unwrap() {
        for key in ["encodingFramingEKey","encodingRequestedPagesMD5","rootWholeDecodedCKey","contentWholeDecodedCKey"] {assert_eq!(proof[key],true);}
    }}
    assert_eq!(cold.data["spells"],warm.data["spells"]);assert_eq!(cold.data["relations"],warm.data["relations"]);
    let mut independent=0;
    for name in ["SpellName","Spell","SpellEffect","SpellMisc","SpellCastTimes","SpellDuration","SpellRange"] {
        let mut inv=fx.invocation("data db2 schema",&[]);inv.positional=Some(name.into());
        independent+=lycheedev_data::dispatch(&inv).unwrap().data["resources"]["metadataBytes"].as_u64().unwrap();
    }
    assert!(warm.data["resources"]["metadataBytes"].as_u64().unwrap()<independent);
}

#[test]
#[ignore = "prepare retained authentic inputs for frozen-Go/Rust public spell-info differential"]
fn prepare_spell_info_public_fixtures() {
    let root=PathBuf::from(std::env::var_os("LYCHEEDEV_SPELL_INFO_FIXTURE").expect("fresh fixture root"));
    assert!(!root.exists());std::fs::create_dir_all(&root).unwrap();
    let mut invalid_name=names(&[vec![U(100),Text("A".into())]]);invalid_name.raw.pop();
    let mut invalid_effect=tables();invalid_effect[2]=table("SpellEffect",303,&[("ID","uint"),("SpellID","uint"),("Unused","text")],&[vec![U(1),U(777),Cell::BadString]]);
    let mut row_bound=tables();row_bound[2]=table("SpellEffect",303,&[("ID","uint"),("SpellID","uint")],&(1..=65537).map(|id|vec![U(id),U(100)]).collect::<Vec<_>>());
    let mut relation_bound=tables();relation_bound[2]=effects(&[]);relation_bound[1]=spells(&[vec![U(100),Text("$@spellname77 ".repeat(65537)),Text("".into())]]);
    for (name,tables) in [("baseline",tables()),("chain",chain_tables()),("firstInvalidThenMissing",vec![invalid_name]),
        ("missingRange",tables().into_iter().take(6).collect()),("invalidNonmatchingEffect",invalid_effect),("rowBound",row_bound),("relationBound",relation_bound)] {
        Fixture::at(root.join(name),&tables,"zhCN",0x40);
    }
    std::fs::write(root.join("spell-info-cases.json"),serde_json::to_vec_pretty(&json!({
        "scope":"authenticated synthetic CASC/DBD/WDC; no real-client claim",
        "baselineSeeds":[100,10,30,40,50,999],"depthProfiles":[0,1,5,32,33],
        "fixtures":["baseline","chain","firstInvalidThenMissing","missingRange","invalidNonmatchingEffect","rowBound","relationBound"]
    })).unwrap()).unwrap();
}
