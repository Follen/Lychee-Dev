//! Frozen item-geoset behavior through authenticated CASC/DBD/WDC fixtures.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Cell, Cell::{Array, F, I, U}, Fixture, Table, table};
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

const TABLES: &[&str] = &["ItemSparse","ItemModifiedAppearance","ItemAppearance","ItemDisplayInfo","HelmetGeosetData"];
fn array(a: u32, b: u32, c: u32) -> Cell { Array(vec![U(a),U(b),U(c)]) }
fn displays(rows: &[Vec<Cell>]) -> Table {
    table("ItemDisplayInfo",604,&[("ID","uint"),("GeosetGroup","array2signed"),("HelmetGeosetVis","array3")],rows)
}
fn helmets(rows: &[Vec<Cell>]) -> Table {
    table("HelmetGeosetData",605,&[("ID","uint"),("HelmetGeosetVisDataID","uint"),("HideGeosetGroup","signed")],rows)
}
fn tables() -> Vec<Table> {
    vec![
        table("ItemSparse",601,&[("ID","uint")],&(1..=6).map(|id|vec![U(id)]).collect::<Vec<_>>()),
        table("ItemModifiedAppearance",602,&[("ID","uint"),("ItemID","uint"),("ItemAppearanceID","uint")],&[
            vec![U(10),U(1),U(100)],vec![U(30),U(2),U(100)],vec![U(20),U(2),U(200)],
            vec![U(40),U(3),U(300)],vec![U(60),U(5),U(999)],vec![U(50),U(5),U(100)],vec![U(70),U(6),U(600)],
        ]),
        table("ItemAppearance",603,&[("ID","uint"),("ItemDisplayInfoID","uint")],&[
            vec![U(100),U(1000)],vec![U(300),U(999)],vec![U(600),U(6000)],
        ]),
        displays(&[
            vec![U(1000),Array(vec![I(-1),I(0)]),array(10,20,10)],
            vec![U(6000),Array(vec![I(0),I(0)]),array(99,0,99)],
        ]),
        helmets(&[vec![U(8),U(10),I(80)],vec![U(2),U(10),I(-2)],vec![U(5),U(10),I(0)],vec![U(3),U(20),I(30)]]),
    ]
}
fn run(fx: &Fixture, item: &str) -> Outcome {
    let out=lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id",item)])).unwrap();
    fx.verify(&out);
    assert_eq!(out.provenance[0]["provenance"]["kind"],"item-geosets");
    assert_eq!(out.provenance[0]["provenance"]["locator"],format!("item:geosets:{item}"));
    assert_eq!(out.data["tables"].as_array().unwrap().iter().map(|t|t["name"].as_str().unwrap()).collect::<Vec<_>>(),TABLES);
    out
}
fn relation(from: &str, field: &str, id: u32, kind: &str, target: u64, depth: u32) -> Value {
    json!({"fromTable":from,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth})
}
fn hide_relation(id: u32, vis: u64) -> Value {
    relation("HelmetGeosetData","HelmetGeosetVisDataID",id,"helmet_geoset_vis",vis,2)
}
fn captures(fx: &Fixture) -> usize {
    Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap().len()
}

#[test]
fn helmet_visibility_order_duplicates_and_signed_hide_values_preserve_complete_capture() {
    let fx=Fixture::new(&tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["displayID"],1000);assert_eq!(out.data["geosetGroup"],json!([-1,0]));
    assert_eq!(out.data["helmetGeosetVis"],json!([10,20,10]));
    assert_eq!(out.data["helmetHide"],json!([-2,0,80,30,-2,0,80]));
    assert_eq!(out.data["partialReasons"],Value::Null);assert_eq!(out.data["truncated"],false);
    assert_eq!(out.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",10,"item_appearance",100,0),
        relation("ItemAppearance","ItemDisplayInfoID",100,"item_display",1000,1),
        hide_relation(2,10),hide_relation(5,10),hide_relation(8,10),hide_relation(3,20),
        hide_relation(2,10),hide_relation(5,10),hide_relation(8,10),
    ]));
}

#[test]
fn shared_appearance_choice_retains_ambiguity_missing_chain_and_empty_helmet_semantics() {
    let fx=Fixture::new(&tables());let missing=run(&fx,"2");
    assert_eq!(missing.data["partialReasons"],json!(["relation_ambiguous","relation_unresolved"]));
    assert_eq!(missing.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",30,"item_appearance",100,0),
        relation("ItemModifiedAppearance","ItemAppearanceID",20,"item_appearance",200,0),
    ]));
    let display=run(&fx,"3");assert_eq!(display.data["partialReasons"],json!(["relation_unresolved"]));
    for out in [&missing,&display] {
        assert_eq!(out.data["displayID"],0);assert!(!out.complete);
        for key in ["geosetGroup","helmetGeosetVis","helmetHide"] {assert_eq!(out.data[key],json!([]));}
    }
    let none=run(&fx,"4");assert!(none.complete);assert_eq!(none.data["displayID"],0);
    for key in ["geosetGroup","helmetGeosetVis","helmetHide","relations"] {assert_eq!(none.data[key],json!([]));}
    let ambiguous=run(&fx,"5");assert_eq!(ambiguous.data["partialReasons"],json!(["relation_ambiguous"]));
    assert_eq!(ambiguous.data["helmetHide"],json!([-2,0,80,30,-2,0,80]));
    assert_eq!(ambiguous.data["relations"][0],relation("ItemModifiedAppearance","ItemAppearanceID",60,"item_appearance",999,0));
    let absent=run(&fx,"6");assert!(absent.complete);assert_eq!(absent.data["displayID"],6000);
    assert_eq!(absent.data["helmetGeosetVis"],json!([99,0,99]));assert_eq!(absent.data["helmetHide"],json!([]));
    assert_eq!(absent.data["relations"].as_array().unwrap().len(),2);
    let before=captures(&fx);let error=lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id","999")])).unwrap_err();
    assert_eq!((error.code.as_str(),error.exit_code),("records.record_not_found",3));assert_eq!(captures(&fx),before);
}

fn scalar_tables() -> Vec<Table> {
    let mut all=tables();all[3]=table("ItemDisplayInfo",604,&[("ID","uint"),("GeosetGroup","signed"),("HelmetGeosetVis","uint")],&[vec![U(1000),I(-1),U(10)]]);all
}
fn negative_tables() -> Vec<Table> {
    let mut all=tables();all[3]=table("ItemDisplayInfo",604,&[("ID","uint"),("GeosetGroup","array2signed"),("HelmetGeosetVis","array2signed")],&[vec![U(1000),Array(vec![I(-1),I(0)]),Array(vec![I(-1),I(10)])]]);all
}
fn wide_tables() -> Vec<Table> {
    let mut all=tables();all[3]=table("ItemDisplayInfo",604,&[("ID","uint"),("GeosetGroup","array2signed"),("HelmetGeosetVis","array2float")],&[vec![U(1000),Array(vec![I(-1),I(0)]),Array(vec![F(4294967296.0),F(4294967808.0)])]]);
    all[4]=helmets(&[vec![U(11),U(0),I(110)],vec![U(12),U(512),I(120)]]);all
}
fn missing_hide_tables() -> Vec<Table> {
    let mut all=tables();all[4]=table("HelmetGeosetData",605,&[("ID","uint"),("HelmetGeosetVisDataID","uint")],&[vec![U(1),U(10)]]);all
}
#[test]
fn scalar_negative_and_wide_visibility_keep_go_array_coercion_and_selector_cast() {
    let fx=Fixture::new(&scalar_tables());let out=run(&fx,"1");assert!(out.complete);
    for key in ["geosetGroup","helmetGeosetVis","helmetHide"] {assert_eq!(out.data[key],json!([]));}
    let fx=Fixture::new(&negative_tables());let out=run(&fx,"1");
    assert_eq!(out.data["helmetGeosetVis"],json!([-1,10]));assert_eq!(out.data["helmetHide"],json!([-2,0,80]));
    let fx=Fixture::new(&wide_tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["helmetGeosetVis"],json!([4294967296_u64,4294967808_u64]));
    assert_eq!(out.data["helmetHide"],json!([110,120]));
    assert_eq!(out.data["relations"][2],hide_relation(11,4294967296));
    assert_eq!(out.data["relations"][3],hide_relation(12,4294967808));
    let fx=Fixture::new(&missing_hide_tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["helmetHide"],json!([0,0]));
}

fn bounded_tables(count: u32) -> Vec<Table> {
    let mut all=tables();all[3]=displays(&[vec![U(1000),Array(vec![I(0),I(0)]),array(10,0,0)]]);
    all[4]=helmets(&(1..=count).map(|id|vec![U(id),U(10),I(id as i32)]).collect::<Vec<_>>());all
}
#[test]
fn real_row_and_relation_limits_keep_all_admitted_hide_values_after_link_truncation() {
    for (count,reason) in [(65536,"relation_budget"),(65537,"row_budget")] {
        let fx=Fixture::new(&bounded_tables(count));let out=run(&fx,"1");
        assert_eq!(out.data["helmetHide"],json!((1..=65536).collect::<Vec<_>>()));
        assert_eq!(out.data["relations"].as_array().unwrap().len(),65536);
        assert_eq!(out.data["relations"][65535]["fromID"],65534);
        assert_eq!(out.data["truncationReason"],reason);assert_eq!(out.data["truncated"],true);
        assert_eq!(out.data["partialReasons"],Value::Null);assert!(!out.complete);assert_eq!(out.warnings.len(),1);
    }
}

fn invalid_helmet_tables() -> Vec<Table> {
    let mut all=tables();all[4]=table("HelmetGeosetData",605,&[("ID","uint"),("HelmetGeosetVisDataID","uint"),("Unused","text")],&[vec![U(1),U(999),Cell::BadString]]);all
}
#[test]
fn required_fifth_table_and_full_scan_errors_preserve_order_and_no_capture() {
    let mut all=tables();all.truncate(1);all[0].raw.pop();let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);
    let fx=Fixture::new(&tables()[..4]);
    for item in ["1","4","999"] {assert!(lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id",item)])).is_err());}
    assert_eq!(captures(&fx),0);
    let fx=Fixture::new(&invalid_helmet_tables());
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);assert!(run(&fx,"4").complete,"no appearance does not scan helmet rows");
    let mut all=tables();all[4]=table("HelmetGeosetData",605,&[("ID","uint"),("HelmetGeosetVisDataID","text")],&[vec![U(1),Cell::Text("10".into())]]);
    let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item geosets",&[("--item-id","1")])).unwrap_err().code,"records.field_type_mismatch");
    assert_eq!(captures(&fx),0);
}

#[test]
fn five_table_authenticated_session_and_warm_read_keep_argument_and_control_failures() {
    let fx=Fixture::new(&tables());let cold=run(&fx,"1");let warm=run(&fx,"1");
    for out in [&cold,&warm] {for proof in out.data["tableVerification"].as_array().unwrap() {
        for key in ["encodingFramingEKey","encodingRequestedPagesMD5","rootWholeDecodedCKey","contentWholeDecodedCKey"] {assert_eq!(proof[key],true);}
    }}
    for key in ["geosetGroup","helmetGeosetVis","helmetHide","relations"] {assert_eq!(cold.data[key],warm.data[key]);}
    let mut independent=0;
    for name in TABLES {
        let mut inv=fx.invocation("data db2 schema",&[]);inv.positional=Some((*name).into());
        independent+=lycheedev_data::dispatch(&inv).unwrap().data["resources"]["metadataBytes"].as_u64().unwrap();
    }
    assert!(warm.data["resources"]["metadataBytes"].as_u64().unwrap()<independent);
    let before=captures(&fx);
    for flags in [vec![],vec![("--item-id","0")],vec![("--item-id","-1")],vec![("--item-id","4294967296")]] {
        assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item geosets",&flags)).unwrap_err().exit_code,2);
    }
    let mut inv=fx.invocation("data item geosets",&[("--item-id","1")]);inv.control.cancel_handle().store(true,Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code,7);
    inv.control=OperationControl::new(Instant::now(),Arc::new(AtomicBool::new(false)));
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code,"command.deadline");assert_eq!(captures(&fx),before);
}

#[test]
#[ignore = "prepare authenticated fixtures for frozen Go/Rust public item-geosets comparison"]
fn prepare_item_geosets_public_fixtures() {
    let root=PathBuf::from(std::env::var_os("LYCHEEDEV_ITEM_GEOSETS_FIXTURE").expect("fresh fixture root"));
    assert!(!root.exists());std::fs::create_dir_all(&root).unwrap();
    let mut invalid=tables();invalid.truncate(1);invalid[0].raw.pop();
    for (name,tables) in [("baseline",tables()),("scalarVisibility",scalar_tables()),("negativeVisibility",negative_tables()),
        ("wideVisibility",wide_tables()),("missingHideField",missing_hide_tables()),("firstInvalidThenMissing",invalid),
        ("missingHelmet",tables().into_iter().take(4).collect()),("invalidNonmatchingHelmet",invalid_helmet_tables()),
        ("relationBound",bounded_tables(65536)),("rowBound",bounded_tables(65537))] {
        Fixture::at(root.join(name),&tables,"zhCN",0x40);
    }
    std::fs::write(root.join("item-geosets-cases.json"),serde_json::to_vec_pretty(&json!({
        "scope":"authenticated synthetic CASC/DBD/WDC; no real-client claim","baselineItems":[1,2,3,4,5,6,999],
        "fixtures":["baseline","scalarVisibility","negativeVisibility","wideVisibility","missingHideField","firstInvalidThenMissing","missingHelmet","invalidNonmatchingHelmet","relationBound","rowBound"]
    })).unwrap()).unwrap();
}
