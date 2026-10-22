//! Current Engine domain tasks with fixed business expectations and full capture.
use super::*;
use crate::capture::fixture::{self, Fixture};
#[path = "navigation_fixture_data.rs"]
mod data;
fn relations(expected: &[(&str, &str, u32, &str, u64, u32)]) -> Value {
    Value::Array(expected.iter().map(|&(table, field, id, kind, target, depth)| {
        json!({"fromTable":table,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth})
    }).collect())
}
fn witness(fx: &Fixture, mut request: Value) {
    let kind = request["kind"].as_str().unwrap().to_owned();
    request["mode"] = json!("static");
    let requested = (request["kind"] == "creature_display" && request["display_id"] == 0)
        .then(|| request["file_data_id"].clone());
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
    let report = &value["report"];
    match kind.as_str() {
        "item_get" => assert_eq!(report["item"]["id"], 7),
        "item_models" => {
            // Fixed tables select appearance 100/display 1000. Resource 50
            // selects file 502 before 508; resource 60 falls back to row ID 3.
            // Repeated resources and materials intentionally remain repeated.
            assert_eq!(report["itemID"], 1);
            assert_eq!(report["displayID"], 1000);
            assert_eq!(report["raceID"], 0);
            assert_eq!(report["gender"], 0);
            assert_eq!(report["models"], json!([502, 3, 502]));
            assert_eq!(report["textures"], json!([4, 4]));
            assert_eq!(report["geosetGroup"], json!([-1, 0]));
            assert_eq!(
                report["relations"],
                relations(&[
                    (
                        "ItemModifiedAppearance",
                        "ItemAppearanceID",
                        10,
                        "item_appearance",
                        100,
                        0
                    ),
                    (
                        "ItemAppearance",
                        "ItemDisplayInfoID",
                        100,
                        "item_display",
                        1000,
                        1
                    ),
                    (
                        "ItemDisplayInfo",
                        "ModelResourcesID",
                        1000,
                        "model_resource",
                        50,
                        2
                    ),
                    ("ModelFileData", "FileDataID", 2, "model_file", 502, 3),
                    ("ModelFileData", "FileDataID", 8, "model_file", 508, 3),
                    (
                        "ItemDisplayInfo",
                        "ModelResourcesID",
                        1000,
                        "model_resource",
                        60,
                        2
                    ),
                    ("ModelFileData", "FileDataID", 3, "model_file", 3, 3),
                    (
                        "ItemDisplayInfo",
                        "ModelResourcesID",
                        1000,
                        "model_resource",
                        50,
                        2
                    ),
                    ("ModelFileData", "FileDataID", 2, "model_file", 502, 3),
                    ("ModelFileData", "FileDataID", 8, "model_file", 508, 3),
                    (
                        "TextureFileData",
                        "MaterialResourcesID",
                        4,
                        "texture_file",
                        4,
                        2
                    ),
                    (
                        "TextureFileData",
                        "MaterialResourcesID",
                        4,
                        "texture_file",
                        4,
                        2
                    ),
                ])
            );
        }
        "item_textures" => {
            // Material-section rows 2,4,8,9 select resources 70,80,70,70.
            // Resource 80 skips positive UsageType row 6, then row 7's signed
            // -1 follows the unsigned/default interpretation and selects 807.
            assert_eq!(report["itemID"], 1);
            assert_eq!(report["displayID"], 1000);
            assert_eq!(
                report["textures"],
                json!([
                    {"section":-2,"fileDataID":4},
                    {"section":0,"fileDataID":807},
                    {"section":0,"fileDataID":4},
                    {"section":-9,"fileDataID":4},
                ])
            );
            assert_eq!(
                report["relations"],
                relations(&[
                    (
                        "ItemModifiedAppearance",
                        "ItemAppearanceID",
                        10,
                        "item_appearance",
                        100,
                        0
                    ),
                    (
                        "ItemAppearance",
                        "ItemDisplayInfoID",
                        100,
                        "item_display",
                        1000,
                        1
                    ),
                    (
                        "TextureFileData",
                        "MaterialResourcesID",
                        4,
                        "texture_file",
                        4,
                        2
                    ),
                    ("TextureFileData", "MaterialResourcesID", 7, "texture_file", 807, 2),
                    (
                        "TextureFileData",
                        "MaterialResourcesID",
                        4,
                        "texture_file",
                        4,
                        2
                    ),
                    (
                        "TextureFileData",
                        "MaterialResourcesID",
                        4,
                        "texture_file",
                        4,
                        2
                    ),
                ])
            );
        }
        "item_geosets" => {
            // Visibility 10 repeats after 20. Its rows 2,5,8 retain signed hide
            // values, including zero; the repeat must not be deduplicated.
            assert_eq!(report["itemID"], 1);
            assert_eq!(report["displayID"], 1000);
            assert_eq!(report["geosetGroup"], json!([-1, 0]));
            assert_eq!(report["helmetGeosetVis"], json!([10, 20, 10]));
            assert_eq!(report["helmetHide"], json!([-2, 0, 80, 30, -2, 0, 80]));
            assert_eq!(
                report["relations"],
                relations(&[
                    (
                        "ItemModifiedAppearance",
                        "ItemAppearanceID",
                        10,
                        "item_appearance",
                        100,
                        0
                    ),
                    (
                        "ItemAppearance",
                        "ItemDisplayInfoID",
                        100,
                        "item_display",
                        1000,
                        1
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        2,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        5,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        8,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        3,
                        "helmet_geoset_vis",
                        20,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        2,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        5,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                    (
                        "HelmetGeosetData",
                        "HelmetGeosetVisDataID",
                        8,
                        "helmet_geoset_vis",
                        10,
                        2
                    ),
                ])
            );
        }
        "spell_info" | "spell_auras" | "spell_summons" => {
            assert_eq!(report["spellID"], 100)
        }
        "decor_list" => assert_eq!(report["count"], 1),
        "decor_get" => assert!(report.is_object()),
        "creature_display" => {
            assert_eq!(report["requestedFileDataID"], 500);
            assert_eq!(report["fileDataID"], 500);
        }
        "creature_model" => assert_eq!(report["fileDataID"], 500),
        "encounter_get" => assert_eq!(report["journalEncounterID"], 132),
        _ => unreachable!(),
    }
    if let Some(expected) = requested {
        assert_eq!(report["requestedFileDataID"], expected);
    } else {
        assert!(report.get("requestedFileDataID").is_none());
    }
    let partial = matches!(kind.as_str(), "creature_display" | "creature_model");
    let truncated = matches!(kind.as_str(), "decor_list" | "spell_info");
    assert_eq!(report["complete"], !partial && !truncated);
    assert_eq!(report["partial"], partial);
    assert_eq!(report["truncated"], truncated);
    assert!(report["relations"].is_array());
    assert!(!value["sources"].as_array().unwrap().is_empty());
    let capture = &value["capture"];
    let id = capture["id"].as_str().unwrap();
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fx.home, id).unwrap(),
        *capture
    );
    let bytes = lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["report"], *report);
    assert_eq!(capture["complete"], report["complete"]);
    assert_eq!(capture["truncated"], report["truncated"]);
    drop(response);
    session.close().unwrap();
}
#[test]
fn all_twelve_tasks_preserve_business_and_full_capture() {
    let fx = Fixture::new(&fixture::items());
    witness(&fx, json!({"kind":"item_get","item_id":7}));
    let fx = Fixture::new(&data::models::tables());
    witness(
        &fx,
        json!({"kind":"item_models","item_id":1,"race_id":0,"gender":0}),
    );
    let fx = Fixture::new(&data::textures::tables());
    witness(&fx, json!({"kind":"item_textures","item_id":1}));
    let fx = Fixture::new(&data::geosets::tables());
    witness(&fx, json!({"kind":"item_geosets","item_id":1}));
    let fx = Fixture::new(&data::spell::tables());
    witness(
        &fx,
        json!({"kind":"spell_info","spell_id":100,"max_depth":1}),
    );
    witness(&fx, json!({"kind":"spell_auras","spell_id":100}));
    witness(
        &fx,
        json!({"kind":"spell_summons","spell_id":100,"npc_id":0}),
    );
    let rows = fixture::decor_rows();
    let fixture::Cell::U(id) = rows[0][0].clone() else {
        panic!("fixture ID")
    };
    let fx = Fixture::new(&[fixture::decors(&rows)]);
    witness(&fx, json!({"kind":"decor_list","limit":1}));
    witness(&fx, json!({"kind":"decor_get","field":"id","wanted":id}));
    let fx = Fixture::new(&data::creature::tables());
    witness(
        &fx,
        json!({"kind":"creature_display","display_id":0,"file_data_id":500}),
    );
    witness(&fx, json!({"kind":"creature_model","file_data_id":500}));
    let fx = data::encounter::fixture(&[data::encounter::section(1, 1, 0, 0, 0, 123)], true);
    witness(
        &fx,
        json!({"kind":"encounter_get","encounter_id":132,"max_depth":0}),
    );
}
#[test]
fn empty_encounter_does_not_open_spell_tables() {
    let fx = data::encounter::fixture(&[], false);
    witness(
        &fx,
        json!({"kind":"encounter_get","encounter_id":132,"max_depth":0}),
    );
}
#[test]
fn schema_has_twelve_strict_variants() {
    let s = navigation::schema();
    assert_eq!(s["oneOf"].as_array().unwrap().len(), 12);
    assert!(
        serde_json::from_value::<Task>(
            json!({"task":"navigate","request":{"kind":"sql","mode":"static"}})
        )
        .is_err()
    );
}
