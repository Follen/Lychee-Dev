//! Frozen item texture sections through authenticated CASC/DBD/WDC inputs.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Cell, Cell::{F, I, U}, Fixture, Table, table};
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

const TABLES: &[&str] = &["ItemSparse","ItemModifiedAppearance","ItemAppearance","ItemDisplayInfo","ItemDisplayInfoMaterialRes","TextureFileData"];
fn sections(rows: &[Vec<Cell>]) -> Table {
    table("ItemDisplayInfoMaterialRes",705,&[("ID","uint"),("ItemDisplayInfoID","uint"),("MaterialResourcesID","signed"),("ComponentSection","signed")],rows)
}
fn textures(rows: &[Vec<Cell>]) -> Table {
    table("TextureFileData",706,&[("ID","uint"),("MaterialResourcesID","uint"),("FileDataID","uint"),("UsageType","signed")],rows)
}
fn tables() -> Vec<Table> {
    vec![
        table("ItemSparse",701,&[("ID","uint")],&(1..=7).map(|id|vec![U(id)]).collect::<Vec<_>>()),
        table("ItemModifiedAppearance",702,&[("ID","uint"),("ItemID","uint"),("ItemAppearanceID","uint")],&[
            vec![U(10),U(1),U(100)],vec![U(30),U(2),U(100)],vec![U(20),U(2),U(200)],
            vec![U(40),U(3),U(300)],vec![U(60),U(5),U(999)],vec![U(50),U(5),U(100)],
            vec![U(70),U(6),U(600)],vec![U(80),U(7),U(700)],
        ]),
        table("ItemAppearance",703,&[("ID","uint"),("ItemDisplayInfoID","uint")],&[
            vec![U(100),U(1000)],vec![U(300),U(999)],vec![U(600),U(6000)],vec![U(700),U(0)],
        ]),
        table("ItemDisplayInfo",704,&[("ID","uint")],&[vec![U(1000)],vec![U(6000)],vec![U(0)]]),
        sections(&[
            vec![U(9),U(1000),I(70),I(-9)],vec![U(2),U(1000),I(70),I(-2)],
            vec![U(3),U(1000),I(0),I(3)],vec![U(4),U(1000),I(80),I(0)],
            vec![U(5),U(1000),I(90),I(5)],vec![U(6),U(1000),I(99),I(6)],
            vec![U(7),U(1000),I(-1),I(7)],vec![U(8),U(1000),I(70),I(0)],
            vec![U(10),U(0),I(70),I(10)],
        ]),
        textures(&[
            vec![U(5),U(70),U(705),I(0)],vec![U(0),U(70),U(0),I(0)],vec![U(1),U(70),U(701),I(1)],
            vec![U(4),U(70),U(0),I(0)],vec![U(6),U(80),U(806),I(1)],vec![U(7),U(80),U(807),I(-1)],
            vec![U(8),U(90),U(908),I(1)],
        ]),
    ]
}
fn run(fx: &Fixture, item: &str) -> Outcome {
    let out=lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id",item)])).unwrap();
    fx.verify(&out);
    assert_eq!(out.provenance[0]["provenance"]["kind"],"item-textures");
    assert_eq!(out.provenance[0]["provenance"]["locator"],format!("item:textures:{item}"));
    assert_eq!(out.data["tables"].as_array().unwrap().iter().map(|t|t["name"].as_str().unwrap()).collect::<Vec<_>>(),TABLES);
    out
}
fn relation(from: &str, field: &str, id: u32, kind: &str, target: u32, depth: u32) -> Value {
    json!({"fromTable":from,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth})
}
fn texture_relation(id: u32, file: u32) -> Value {
    relation("TextureFileData","MaterialResourcesID",id,"texture_file",file,2)
}
fn expected_textures() -> Value {
    json!([{"section":-2,"fileDataID":4},{"section":0,"fileDataID":807},{"section":0,"fileDataID":4},{"section":-9,"fileDataID":4}])
}
fn captures(fx: &Fixture) -> usize {
    Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap().len()
}

#[test]
fn sections_select_first_qualifying_file_and_preserve_signed_values_duplicates_and_capture() {
    let fx=Fixture::new(&tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["displayID"],1000);assert_eq!(out.data["textures"],expected_textures());
    assert_eq!(out.data["partialReasons"],Value::Null);assert_eq!(out.data["truncated"],false);
    assert_eq!(out.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",10,"item_appearance",100,0),
        relation("ItemAppearance","ItemDisplayInfoID",100,"item_display",1000,1),
        texture_relation(4,4),texture_relation(7,807),texture_relation(4,4),texture_relation(4,4),
    ]));
}

#[test]
fn shared_appearance_missing_sections_and_existing_zero_display_keep_exact_empty_semantics() {
    let fx=Fixture::new(&tables());let missing=run(&fx,"2");
    assert_eq!(missing.data["partialReasons"],json!(["relation_ambiguous","relation_unresolved"]));
    assert_eq!(missing.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",30,"item_appearance",100,0),
        relation("ItemModifiedAppearance","ItemAppearanceID",20,"item_appearance",200,0),
    ]));
    let display=run(&fx,"3");assert_eq!(display.data["partialReasons"],json!(["relation_unresolved"]));
    for out in [&missing,&display] {assert_eq!(out.data["displayID"],0);assert_eq!(out.data["textures"],json!([]));assert!(!out.complete);}
    let none=run(&fx,"4");assert!(none.complete);assert_eq!(none.data["displayID"],0);assert_eq!(none.data["relations"],json!([]));
    let ambiguous=run(&fx,"5");assert_eq!(ambiguous.data["partialReasons"],json!(["relation_ambiguous"]));
    assert_eq!(ambiguous.data["textures"],expected_textures());
    assert_eq!(ambiguous.data["relations"][0],relation("ItemModifiedAppearance","ItemAppearanceID",60,"item_appearance",999,0));
    let absent=run(&fx,"6");assert!(absent.complete);assert_eq!(absent.data["displayID"],6000);assert_eq!(absent.data["textures"],json!([]));
    let zero=run(&fx,"7");assert!(zero.complete);assert_eq!(zero.data["displayID"],0);assert_eq!(zero.data["textures"],json!([]));
    assert_eq!(zero.data["relations"],json!([
        relation("ItemModifiedAppearance","ItemAppearanceID",80,"item_appearance",700,0),
        relation("ItemAppearance","ItemDisplayInfoID",700,"item_display",0,1),
    ]));
    let before=captures(&fx);let error=lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id","999")])).unwrap_err();
    assert_eq!((error.code.as_str(),error.exit_code),("records.record_not_found",3));assert_eq!(captures(&fx),before);
}

fn float_tables() -> Vec<Table> {
    let mut all=tables();
    all[4]=table("ItemDisplayInfoMaterialRes",705,&[("ID","uint"),("ItemDisplayInfoID","uint"),("MaterialResourcesID","float"),("ComponentSection","float")],&[vec![U(1),U(1000),F(70.75),F(-2.75)]]);
    all[5]=table("TextureFileData",706,&[("ID","uint"),("MaterialResourcesID","uint"),("FileDataID","float"),("UsageType","float")],&[vec![U(1),U(70),F(704.75),F(8589934592.0)]]);all
}
fn missing_fields_tables() -> Vec<Table> {
    let mut all=tables();all[4]=table("ItemDisplayInfoMaterialRes",705,&[("ID","uint"),("ItemDisplayInfoID","uint"),("MaterialResourcesID","uint")],&[vec![U(1),U(1000),U(70)]]);
    all[5]=table("TextureFileData",706,&[("ID","uint"),("MaterialResourcesID","uint")],&[vec![U(11),U(70)]]);all
}
#[test]
fn row_coercions_and_missing_optional_fields_reuse_shared_texture_resolution() {
    let fx=Fixture::new(&float_tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["textures"],json!([{"section":-2,"fileDataID":704}]));
    assert_eq!(out.data["relations"][2],texture_relation(1,704));
    let fx=Fixture::new(&missing_fields_tables());let out=run(&fx,"1");assert!(out.complete);
    assert_eq!(out.data["textures"],json!([{"section":0,"fileDataID":11}]));
    assert_eq!(out.data["relations"][2],texture_relation(11,11));
}

fn section_bound_tables(count: u32) -> Vec<Table> {
    let mut all=tables();all[4]=sections(&(1..=count).map(|id|vec![U(id),U(1000),I(70),I(id as i32)]).collect::<Vec<_>>());
    all[5]=textures(&[vec![U(1),U(70),U(700),I(0)]]);all
}
fn texture_bound_tables() -> Vec<Table> {
    let mut all=tables();all[4]=sections(&[vec![U(1),U(1000),I(70),I(-1)]]);
    all[5]=textures(&(1..=65537).map(|id|vec![U(id),U(70),U(id+100000),I(0)]).collect::<Vec<_>>());all
}
#[test]
fn actual_section_texture_row_and_relation_limits_keep_full_admitted_sections() {
    for (count,reason) in [(65536,"relation_budget"),(65537,"row_budget")] {
        let fx=Fixture::new(&section_bound_tables(count));let out=run(&fx,"1");
        let values=out.data["textures"].as_array().unwrap();assert_eq!(values.len(),65536);
        for (index,value) in values.iter().enumerate() {assert_eq!(*value,json!({"section":index+1,"fileDataID":700}));}
        assert_eq!(out.data["relations"].as_array().unwrap().len(),65536);
        assert_eq!(out.data["relations"][65535],texture_relation(1,700));
        assert_eq!(out.data["truncationReason"],reason);assert_eq!(out.data["truncated"],true);
        assert_eq!(out.data["partialReasons"],Value::Null);assert!(!out.complete);assert_eq!(out.warnings.len(),1);
    }
    let fx=Fixture::new(&texture_bound_tables());let out=run(&fx,"1");
    assert_eq!(out.data["textures"],json!([{"section":-1,"fileDataID":100001}]));
    assert_eq!(out.data["truncationReason"],"row_budget");assert_eq!(out.data["relations"].as_array().unwrap().len(),3);assert!(!out.complete);
}

fn invalid_section_tables() -> Vec<Table> {
    let mut all=tables();all[4]=table("ItemDisplayInfoMaterialRes",705,&[("ID","uint"),("ItemDisplayInfoID","uint"),("Unused","text")],&[vec![U(1),U(999),Cell::BadString]]);all
}
fn invalid_texture_tables() -> Vec<Table> {
    let mut all=tables();all[5]=table("TextureFileData",706,&[("ID","uint"),("MaterialResourcesID","uint"),("Unused","text")],&[vec![U(1),U(999),Cell::BadString]]);all
}
#[test]
fn six_required_tables_and_full_nonmatching_row_decode_fail_without_captures() {
    let mut all=tables();all.truncate(1);all[0].raw.pop();let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
    assert_eq!(captures(&fx),0);
    for count in [4,5] {
        let fx=Fixture::new(&tables()[..count]);
        for item in ["1","4","7","999"] {assert!(lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id",item)])).is_err());}
        assert_eq!(captures(&fx),0);
    }
    for all in [invalid_section_tables(),invalid_texture_tables()] {
        let fx=Fixture::new(&all);
        assert_eq!(lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id","1")])).unwrap_err().code,"table.invalid_format");
        assert_eq!(captures(&fx),0);
        assert!(run(&fx,"4").complete);assert!(run(&fx,"7").complete,"zero display stops before scanning even when its row exists");
    }
    let fx=Fixture::new(&invalid_texture_tables());assert!(run(&fx,"6").complete,"no section does not scan texture rows");
    let mut all=tables();all[4]=table("ItemDisplayInfoMaterialRes",705,&[("ID","uint"),("ItemDisplayInfoID","text")],&[vec![U(1),Cell::Text("1000".into())]]);
    let fx=Fixture::new(&all);
    assert_eq!(lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&[("--item-id","1")])).unwrap_err().code,"records.field_type_mismatch");assert_eq!(captures(&fx),0);
}

#[test]
fn six_table_authentication_warm_reuse_and_argument_control_errors_stay_bounded() {
    let fx=Fixture::new(&tables());let cold=run(&fx,"1");let warm=run(&fx,"1");
    for out in [&cold,&warm] {for proof in out.data["tableVerification"].as_array().unwrap() {
        for key in ["encodingFramingEKey","encodingRequestedPagesMD5","rootWholeDecodedCKey","contentWholeDecodedCKey"] {assert_eq!(proof[key],true);}
    }}
    for key in ["textures","relations"] {assert_eq!(cold.data[key],warm.data[key]);}
    let mut independent=0;
    for name in TABLES {
        let mut inv=fx.invocation("data db2 schema",&[]);inv.positional=Some((*name).into());
        independent+=lycheedev_data::migration_oracle::dispatch(&inv).unwrap().data["resources"]["metadataBytes"].as_u64().unwrap();
    }
    assert!(warm.data["resources"]["metadataBytes"].as_u64().unwrap()<independent);
    let before=captures(&fx);
    for flags in [vec![],vec![("--item-id","0")],vec![("--item-id","-1")],vec![("--item-id","4294967296")]] {
        assert_eq!(lycheedev_data::migration_oracle::dispatch(&fx.invocation("data item textures",&flags)).unwrap_err().exit_code,2);
    }
    let mut inv=fx.invocation("data item textures",&[("--item-id","1")]);inv.control.cancel_handle().store(true,Ordering::Release);
    assert_eq!(lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err().exit_code,7);
    inv.control=OperationControl::new(Instant::now(),Arc::new(AtomicBool::new(false)));
    assert_eq!(lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err().code,"command.deadline");assert_eq!(captures(&fx),before);
}

#[test]
#[ignore = "prepare authenticated fixtures for frozen Go/Rust public item-textures comparison"]
fn prepare_item_textures_public_fixtures() {
    let root=PathBuf::from(std::env::var_os("LYCHEEDEV_ITEM_TEXTURES_FIXTURE").expect("fresh fixture root"));
    assert!(!root.exists());std::fs::create_dir_all(&root).unwrap();
    let mut invalid=tables();invalid.truncate(1);invalid[0].raw.pop();
    for (name,tables) in [("baseline",tables()),("floatFields",float_tables()),("missingFields",missing_fields_tables()),
        ("firstInvalidThenMissing",invalid),("missingMaterialTable",tables().into_iter().take(4).collect()),
        ("missingTextureTable",tables().into_iter().take(5).collect()),("invalidNonmatchingSection",invalid_section_tables()),
        ("invalidNonmatchingTexture",invalid_texture_tables()),("relationBound",section_bound_tables(65536)),
        ("sectionRowBound",section_bound_tables(65537)),("textureRowBound",texture_bound_tables())] {
        Fixture::at(root.join(name),&tables,"zhCN",0x40);
    }
    std::fs::write(root.join("item-textures-cases.json"),serde_json::to_vec_pretty(&json!({
        "scope":"authenticated synthetic CASC/DBD/WDC; no real-client claim","baselineItems":[1,2,3,4,5,6,7,999],
        "fixtures":["baseline","floatFields","missingFields","firstInvalidThenMissing","missingMaterialTable","missingTextureTable","invalidNonmatchingSection","invalidNonmatchingTexture","relationBound","sectionRowBound","textureRowBound"]
    })).unwrap()).unwrap();
}
