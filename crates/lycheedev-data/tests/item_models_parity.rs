//! Item-model business behavior through real authenticated CASC/DBD/WDC inputs.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Cell, Cell::{Array, I, U}, Fixture, Table, table};
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

const TABLES: &[&str] = &["ItemSparse","ItemModifiedAppearance","ItemAppearance","ItemDisplayInfo","ModelFileData","TextureFileData","ComponentModelFileData"];
fn array(a: u32, b: u32, c: u32) -> Cell { Array(vec![U(a),U(b),U(c)]) }
fn modified(rows: &[Vec<Cell>]) -> Table {
    table("ItemModifiedAppearance",502,&[("ID","uint"),("ItemID","uint"),("ItemAppearanceID","uint")],rows)
}
fn models(rows: &[Vec<Cell>]) -> Table {
    table("ModelFileData",505,&[("ID","uint"),("ModelResourcesID","uint"),("FileDataID","uint")],rows)
}
fn displays(rows: &[Vec<Cell>]) -> Table {
    table("ItemDisplayInfo",504,&[("ID","uint"),("ModelResourcesID","array3"),("ModelMaterialResourcesID","array3"),("GeosetGroup","array2signed")],rows)
}
fn tables() -> Vec<Table> {
    vec![
        table("ItemSparse",501,&[("ID","uint")],&(1..=6).map(|id|vec![U(id)]).collect::<Vec<_>>()),
        modified(&[
            vec![U(10),U(1),U(100)],vec![U(30),U(2),U(100)],vec![U(20),U(2),U(200)],
            vec![U(40),U(3),U(300)],vec![U(60),U(5),U(999)],vec![U(50),U(5),U(100)],vec![U(70),U(6),U(600)],
        ]),
        table("ItemAppearance",503,&[("ID","uint"),("ItemDisplayInfoID","uint")],&[
            vec![U(100),U(1000)],vec![U(300),U(999)],vec![U(600),U(6000)],
        ]),
        displays(&[
            vec![U(1000),array(50,60,50),array(70,80,70),Array(vec![I(-1),I(0)])],
            vec![U(6000),array(99,0,0),array(99,0,0),Array(vec![I(0),I(0)])],
        ]),
        models(&[vec![U(8),U(50),U(508)],vec![U(0),U(50),U(0)],vec![U(2),U(50),U(502)],vec![U(3),U(60),U(0)]]),
        table("TextureFileData",506,&[("ID","uint"),("MaterialResourcesID","uint"),("FileDataID","uint"),("UsageType","signed")],&[
            vec![U(5),U(70),U(705),I(0)],vec![U(0),U(70),U(0),I(0)],vec![U(1),U(70),U(701),I(1)],
            vec![U(4),U(70),U(0),I(0)],vec![U(6),U(80),U(806),I(1)],
        ]),
        table("ComponentModelFileData",507,&[("ID","uint"),("RaceID","signed"),("GenderIndex","signed")],&[
            vec![U(502),I(1),I(0)],vec![U(508),I(2),I(1)],vec![U(3),I(2),I(1)],
        ]),
    ]
}
fn run(fx: &Fixture, item: &str, flags: &[(&str,&str)]) -> Outcome {
    let mut flags=flags.to_vec();flags.push(("--item-id",item));
    let out=lycheedev_data::dispatch(&fx.invocation("data item models",&flags)).unwrap();
    fx.verify(&out);
    assert_eq!(out.provenance[0]["provenance"]["kind"],"item-models");
    assert_eq!(out.provenance[0]["provenance"]["locator"],format!("item:models:{item}"));
    assert_eq!(out.data["tables"].as_array().unwrap().iter().map(|t|t["name"].as_str().unwrap()).collect::<Vec<_>>(),TABLES);
    out
}
fn relation(from: &str, field: &str, id: u32, kind: &str, target: u32, depth: u32) -> Value {
    json!({"fromTable":from,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth})
}
fn captures(fx: &Fixture) -> usize {
    Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap().len()
}

#[test]
fn model_material_order_variants_fallbacks_duplicates_and_capture_match_go() {
    let fx=Fixture::new(&tables());let default=run(&fx,"1",&[]);
    assert!(default.complete);assert_eq!(default.data["partialReasons"],Value::Null);
    assert_eq!(default.data["models"],json!([502,3,502]));assert_eq!(default.data["textures"],json!([4,4]));
    assert_eq!(default.data["geosetGroup"],json!([-1,0]));assert_eq!(default.data["displayID"],1000);
    assert_eq!(default.data["raceID"],0);assert_eq!(default.data["gender"],0);
    assert_eq!(default.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",10,"item_appearance",100,0),
        relation("ItemAppearance","ItemDisplayInfoID",100,"item_display",1000,1),
        relation("ItemDisplayInfo","ModelResourcesID",1000,"model_resource",50,2),
        relation("ModelFileData","FileDataID",2,"model_file",502,3),
        relation("ModelFileData","FileDataID",8,"model_file",508,3),
        relation("ItemDisplayInfo","ModelResourcesID",1000,"model_resource",60,2),
        relation("ModelFileData","FileDataID",3,"model_file",3,3),
        relation("ItemDisplayInfo","ModelResourcesID",1000,"model_resource",50,2),
        relation("ModelFileData","FileDataID",2,"model_file",502,3),
        relation("ModelFileData","FileDataID",8,"model_file",508,3),
        relation("TextureFileData","MaterialResourcesID",4,"texture_file",4,2),
        relation("TextureFileData","MaterialResourcesID",4,"texture_file",4,2),
    ]));
    let preferred=run(&fx,"1",&[("--race-id","2"),("--gender","1")]);
    assert!(preferred.complete);assert_eq!(preferred.data["models"],json!([508,3,508]));
    assert_eq!(preferred.data["relations"],default.data["relations"]);
    for flags in [vec![("--race-id","1")],vec![("--gender","1")],vec![("--race-id","-1"),("--gender","-2")],vec![("--race-id","9223372036854775807")]] {
        let out=run(&fx,"1",&flags);assert!(!out.complete);assert_eq!(out.data["models"],json!([502,3,502]));
        assert_eq!(out.data["partialReasons"],json!(["variant_unavailable"]));assert_eq!(out.data["truncated"],false);
        assert_eq!(out.data["relations"],default.data["relations"]);assert_eq!(out.warnings.len(),1);
    }
}

#[test]
fn appearance_ambiguity_keeps_all_links_and_missing_chain_differs_from_no_appearance() {
    let fx=Fixture::new(&tables());
    let missing=run(&fx,"2",&[]);
    assert_eq!(missing.data["displayID"],0);assert_eq!(missing.data["models"],json!([]));
    assert_eq!(missing.data["partialReasons"],json!(["relation_ambiguous","relation_unresolved"]));
    assert_eq!(missing.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",30,"item_appearance",100,0),
        relation("ItemModifiedAppearance","ItemAppearanceID",20,"item_appearance",200,0),
    ]));
    let display=run(&fx,"3",&[]);assert_eq!(display.data["displayID"],0);
    assert_eq!(display.data["partialReasons"],json!(["relation_unresolved"]));
    assert_eq!(display.data["relations"][1],relation("ItemAppearance","ItemDisplayInfoID",300,"item_display",999,1));
    let none=run(&fx,"4",&[]);assert!(none.complete);
    for name in ["models","textures","geosetGroup","relations"] {assert_eq!(none.data[name],json!([]));}
    let ambiguous=run(&fx,"5",&[]);assert_eq!(ambiguous.data["partialReasons"],json!(["relation_ambiguous"]));
    assert_eq!(ambiguous.data["models"],json!([502,3,502]));assert_eq!(ambiguous.data["displayID"],1000);
    assert_eq!(ambiguous.data["relations"][0],relation("ItemModifiedAppearance","ItemAppearanceID",60,"item_appearance",999,0));
    let no_resource=run(&fx,"6",&[("--race-id","2")]);assert!(no_resource.complete);
    assert_eq!(no_resource.data["models"],json!([]));assert_eq!(no_resource.data["textures"],json!([]));
    assert_eq!(no_resource.data["relations"][2],relation("ItemDisplayInfo","ModelResourcesID",6000,"model_resource",99,2));
    let before=captures(&fx);
    let error=lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id","999")])).unwrap_err();
    assert_eq!((error.code.as_str(),error.exit_code),("records.record_not_found",3));assert_eq!(captures(&fx),before);
}

fn scalar_tables() -> Vec<Table> {
    let mut all=tables();all[3]=table("ItemDisplayInfo",504,&[("ID","uint"),("ModelResourcesID","uint"),("ModelMaterialResourcesID","uint"),("GeosetGroup","signed")],&[vec![U(1000),U(50),U(70),I(9)]]);all
}
#[test]
fn scalar_resource_fields_are_not_arrays_and_unused_component_rows_are_not_decoded() {
    let fx=Fixture::new(&scalar_tables());let out=run(&fx,"1",&[]);assert!(out.complete);
    for name in ["models","textures","geosetGroup"] {assert_eq!(out.data[name],json!([]));}
    assert_eq!(out.data["relations"].as_array().unwrap().len(),2);
    let mut all=tables();all[6]=table("ComponentModelFileData",507,&[("ID","uint"),("RaceID","signed"),("GenderIndex","signed"),("Unused","text")],&[vec![U(502),I(1),I(0),Cell::BadString]]);
    let fx=Fixture::new(&all);assert!(run(&fx,"1",&[]).complete);
    let before=captures(&fx);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id","1"),("--race-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),before);
}

fn bounded_tables(count: u32) -> Vec<Table> {
    let mut all=tables();
    all[3]=displays(&[vec![U(1000),array(50,0,0),array(0,0,0),Array(vec![I(0),I(0)])]]);
    all[4]=models(&(1..=count).map(|id|vec![U(id),U(50),U(id+100000)]).collect::<Vec<_>>());all
}
#[test]
fn actual_related_row_and_relation_bounds_keep_selected_model_and_capture() {
    for (count,reason) in [(65536,"relation_budget"),(65537,"row_budget")] {
        let fx=Fixture::new(&bounded_tables(count));let out=run(&fx,"1",&[]);
        assert_eq!(out.data["models"],json!([100001]));assert_eq!(out.data["relations"].as_array().unwrap().len(),65536);
        assert_eq!(out.data["relations"][65535]["fromID"],65533);
        assert_eq!(out.data["truncationReason"],reason);assert_eq!(out.data["truncated"],true);
        assert_eq!(out.data["partialReasons"],Value::Null);assert!(!out.complete);assert_eq!(out.warnings.len(),1);
    }
}

fn invalid_model_tables() -> Vec<Table> {
    let mut all=tables();all[4]=table("ModelFileData",505,&[("ID","uint"),("ModelResourcesID","uint"),("Unused","text")],&[vec![U(1),U(999),Cell::BadString]]);all
}
#[test]
fn required_tables_open_in_order_and_nonmatching_rows_are_fully_decoded() {
    let mut all=tables();all.truncate(1);all[0].raw.pop();let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);
    let fx=Fixture::new(&tables()[..6]);
    for item in ["1","4","999"] {
        assert!(lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id",item)])).is_err());
    }
    assert_eq!(captures(&fx),0,"the required component table opens even without a requested variant or appearance");
    let fx=Fixture::new(&invalid_model_tables());
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);
    let mut all=tables();all[4]=table("ModelFileData",505,&[("ID","uint"),("ModelResourcesID","text")],&[vec![U(1),Cell::Text("50".into())]]);
    let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item models",&[("--item-id","1")])).unwrap_err().code,"records.field_type_mismatch");
    assert_eq!(captures(&fx),0);
}

#[test]
fn shared_authenticated_reads_warm_query_and_argument_control_failures() {
    let fx=Fixture::new(&tables());let cold=run(&fx,"1",&[]);let warm=run(&fx,"1",&[]);
    for out in [&cold,&warm] {for proof in out.data["tableVerification"].as_array().unwrap() {
        for key in ["encodingFramingEKey","encodingRequestedPagesMD5","rootWholeDecodedCKey","contentWholeDecodedCKey"] {assert_eq!(proof[key],true);}
    }}
    for key in ["models","textures","geosetGroup","relations"] {assert_eq!(cold.data[key],warm.data[key]);}
    let mut independent=0;
    for name in TABLES {
        let mut inv=fx.invocation("data db2 schema",&[]);inv.positional=Some((*name).into());
        independent+=lycheedev_data::dispatch(&inv).unwrap().data["resources"]["metadataBytes"].as_u64().unwrap();
    }
    assert!(warm.data["resources"]["metadataBytes"].as_u64().unwrap()<independent);
    let before=captures(&fx);
    for flags in [vec![],vec![("--item-id","0")],vec![("--item-id","-1")],vec![("--item-id","4294967296")],
        vec![("--item-id","1"),("--race-id","9223372036854775808")],vec![("--item-id","1"),("--gender","bad")]] {
        assert_eq!(lycheedev_data::dispatch(&fx.invocation("data item models",&flags)).unwrap_err().exit_code,2);
    }
    let mut inv=fx.invocation("data item models",&[("--item-id","1")]);inv.control.cancel_handle().store(true,Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code,7);
    inv.control=OperationControl::new(Instant::now(),Arc::new(AtomicBool::new(false)));
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code,"command.deadline");assert_eq!(captures(&fx),before);
}

#[test]
#[ignore = "prepare authenticated fixtures for frozen Go/Rust public item-models comparison"]
fn prepare_item_models_public_fixtures() {
    let root=PathBuf::from(std::env::var_os("LYCHEEDEV_ITEM_MODELS_FIXTURE").expect("fresh fixture root"));
    assert!(!root.exists());std::fs::create_dir_all(&root).unwrap();
    let mut invalid=tables();invalid.truncate(1);invalid[0].raw.pop();
    for (name,tables) in [("baseline",tables()),("scalarResources",scalar_tables()),("firstInvalidThenMissing",invalid),
        ("missingComponent",tables().into_iter().take(6).collect()),("invalidNonmatchingModel",invalid_model_tables()),
        ("relationBound",bounded_tables(65536)),("rowBound",bounded_tables(65537))] {
        Fixture::at(root.join(name),&tables,"zhCN",0x40);
    }
    std::fs::write(root.join("item-models-cases.json"),serde_json::to_vec_pretty(&json!({
        "scope":"authenticated synthetic CASC/DBD/WDC; no real-client claim","baselineItems":[1,2,3,4,5,6,999],
        "variantFlags":[{}, {"raceID":2,"gender":1},{"raceID":1,"gender":0},{"raceID":0,"gender":1},{"raceID":-1,"gender":-2}],
        "fixtures":["baseline","scalarResources","firstInvalidThenMissing","missingComponent","invalidNonmatchingModel","relationBound","rowBound"]
    })).unwrap()).unwrap();
}
