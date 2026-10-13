//! Real authenticated local provider/capture path for frozen-Go creature rules.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)] // Shared provider fixture also contains item/decor builders.
mod fixture;
use fixture::{
    Cell::{self, Array, F, I, I64, U},
    Fixture, Table, table,
};
use serde_json::{Value, json};

fn tables() -> Vec<Table> {
    vec![
        table(
            "CreatureDisplayInfo",
            321,
            &[
                ("ID", "uint"),
                ("ModelID", "uint"),
                ("TextureVariationFileDataID", "array3"),
            ],
            &[
                vec![U(30), U(2), Array(vec![U(700), U(0), U(701)])],
                vec![U(10), U(2), Array(vec![U(800), U(0), U(800)])],
                vec![U(20), U(3), Array(vec![U(900), U(0), U(0)])],
                vec![U(40), U(99), Array(vec![U(0), U(0), U(0)])],
                vec![U(50), U(0), Array(vec![U(0), U(0), U(0)])],
            ],
        ),
        table(
            "CreatureModelData",
            322,
            &[("ID", "uint"), ("FileDataID", "uint")],
            &[vec![U(3), U(500)], vec![U(2), U(500)], vec![U(4), U(600)]],
        ),
        table(
            "CreatureDisplayInfoGeosetData",
            323,
            &[
                ("ID", "uint"),
                ("CreatureDisplayInfoID", "uint"),
                ("GeosetIndex", "signed"),
                ("GeosetValue", "signed"),
            ],
            &[
                vec![U(3), U(10), I(2), I(5)],
                vec![U(1), U(10), I(0), I(4)],
                vec![U(2), U(30), I(1), I(7)],
            ],
        ),
    ]
}
fn run(fx: &Fixture, command: &str, flags: &[(&str, &str)]) -> lycheedev_core::Outcome {
    let outcome = lycheedev_data::dispatch(&fx.invocation(command, flags)).unwrap();
    fx.verify(&outcome);
    outcome
}
#[test]
fn exact_display_arrays_order_relations_and_evidence() {
    let fx = Fixture::new(&tables());
    let r = run(&fx, "data creature display", &[("--display-id", "10")]);
    assert!(r.complete);
    for (key, value) in [
        ("displayID", json!(10)),
        ("modelID", json!(2)),
        ("fileDataID", json!(500)),
        ("modelFileDataID", json!(500)),
        ("textures", json!([800, 800])),
        ("variations", json!([104, 305])),
    ] {
        assert_eq!(r.data[key], value);
    }
    assert_eq!(
        r.data["relations"],
        json!([{"fromTable":"CreatureModelData","fromField":"FileDataID","fromID":2,"toKind":"model_file","toID":500,"depth":1}])
    );
    assert_eq!(
        r.provenance[0]["provenance"]["locator"],
        "creature:display:10"
    );
    assert_eq!(
        r.data["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "CreatureDisplayInfo",
            "CreatureModelData",
            "CreatureDisplayInfoGeosetData"
        ]
    );
}
#[test]
fn file_key_uses_first_model_and_lowest_display_with_ambiguity() {
    let fx = Fixture::new(&tables());
    let r = run(&fx, "data creature display", &[("--file-data-id", "500")]);
    assert!(!r.complete);
    assert_eq!(r.data["displayID"], 10);
    assert_eq!(r.data["partialReasons"], json!(["relation_ambiguous"]));
    assert_eq!(r.data["truncated"], false);
    assert_eq!(
        r.data["relations"],
        json!([
            {"fromTable":"CreatureDisplayInfo","fromField":"ModelID","fromID":10,"toKind":"creature_display","toID":10,"depth":0},
            {"fromTable":"CreatureDisplayInfo","fromField":"ModelID","fromID":30,"toKind":"creature_display","toID":30,"depth":0},
            {"fromTable":"CreatureModelData","fromField":"FileDataID","fromID":2,"toKind":"model_file","toID":500,"depth":1},
        ])
    );
    assert_eq!(
        r.provenance[0]["provenance"]["locator"],
        "creature:display:file:500"
    );
}
#[test]
fn model_collects_each_display_and_empty_model_is_complete() {
    let fx = Fixture::new(&tables());
    let r = run(&fx, "data creature model", &[("--file-data-id", "500")]);
    assert_eq!(
        r.data["displays"],
        json!([
            {"displayID":10,"modelID":2,"fileDataID":500,"textures":[800,800],"variations":[104,305]},
            {"displayID":30,"modelID":2,"fileDataID":500,"textures":[700,701],"variations":[207]},
            {"displayID":20,"modelID":3,"fileDataID":500,"textures":[900],"variations":[]},
        ])
    );
    assert_eq!(r.data["partialReasons"], json!(["relation_ambiguous"]));
    assert_eq!(
        r.provenance[0]["provenance"]["locator"],
        "creature:model:500"
    );
    for id in ["600", "999"] {
        let empty = run(&fx, "data creature model", &[("--file-data-id", id)]);
        assert!(empty.complete);
        assert_eq!(empty.data["displays"], json!([]));
    }
}
#[test]
fn missing_relation_is_partial_but_missing_display_is_an_error() {
    let fx = Fixture::new(&tables());
    for id in ["40", "50"] {
        let r = run(&fx, "data creature display", &[("--display-id", id)]);
        assert!(!r.complete);
        assert_eq!(r.data["partialReasons"], json!(["relation_unresolved"]));
        assert_eq!(r.data["fileDataID"], 0);
        assert_eq!(r.data["textures"], json!([]));
        assert_eq!(r.data["variations"], json!([]));
    }
    for flags in [
        vec![("--display-id", "999")],
        vec![("--file-data-id", "600")],
        vec![("--file-data-id", "999")],
    ] {
        let e =
            lycheedev_data::dispatch(&fx.invocation("data creature display", &flags)).unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.exit_code),
            ("records.record_not_found", 3)
        );
    }
}
#[test]
fn conflicting_empty_keys_and_cancellation_do_not_create_results() {
    let fx = Fixture::new(&tables());
    for flags in [
        vec![],
        vec![("--display-id", "0")],
        vec![("--display-id", "10"), ("--file-data-id", "500")],
    ] {
        let e =
            lycheedev_data::dispatch(&fx.invocation("data creature display", &flags)).unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.exit_code),
            ("records.request_conflict", 2)
        );
    }
    let e = lycheedev_data::dispatch(&fx.invocation("data creature model", &[])).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.exit_code, e.message.as_str()),
        (
            "command.invalid_arguments",
            2,
            "data creature model requires --file-data-id"
        )
    );
    let e =
        lycheedev_data::dispatch(&fx.invocation("data creature model", &[("--file-data-id", "0")]))
            .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.exit_code),
        ("records.request_conflict", 2)
    );
    let mut inv = fx.invocation("data creature model", &[("--file-data-id", "500")]);
    inv.control = lycheedev_core::control::OperationControl::new(
        std::time::Instant::now() + std::time::Duration::from_secs(5),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
    );
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    assert_eq!(
        lycheedev_core::metadata::Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn large_model_capture_limit_reports_frozen_go_budget_error_without_commit() {
    let rows: Vec<_> = (0..65536)
        .map(|i| {
            vec![
                U(3_000_000_000 + i),
                U(3_333_333_333),
                Array(vec![U(u32::MAX), U(u32::MAX), U(u32::MAX)]),
            ]
        })
        .collect();
    let fx = Fixture::new(&[
        table(
            "CreatureDisplayInfo",
            321,
            &[
                ("ID", "uint"),
                ("ModelID", "uint"),
                ("TextureVariationFileDataID", "array3"),
            ],
            &rows,
        ),
        table(
            "CreatureModelData",
            322,
            &[("ID", "uint"), ("FileDataID", "uint")],
            &[vec![U(3_333_333_333), U(3_444_444_444)]],
        ),
        table(
            "CreatureDisplayInfoGeosetData",
            323,
            &[
                ("ID", "uint"),
                ("CreatureDisplayInfoID", "uint"),
                ("GeosetIndex", "signed"),
                ("GeosetValue", "signed"),
            ],
            &[],
        ),
    ]);
    let e = lycheedev_data::dispatch(
        &fx.invocation("data creature model", &[("--file-data-id", "3444444444")]),
    )
    .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.exit_code, e.message.as_str()),
        ("query.budget_exceeded", 3, "relational.budget_exceeded")
    );
    assert_eq!(
        lycheedev_core::metadata::Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        0
    );
}

#[test]
#[ignore = "explicit fresh retained fixture for independent frozen-Go public differential"]
fn prepare_creature_public_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CREATURE_FIXTURE").expect("fresh owned fixture root"),
    );
    let fx = Fixture::at(root, &tables(), "zhCN", 0x40);
    let identity: Value =
        serde_json::from_slice(&std::fs::read(fx.root.join("input-identity.json")).unwrap())
            .unwrap();
    assert_eq!(identity["snapshot"], fx.pin);
}

// These constructors preserve the original six tests and the original public
// baseline. The expanded suite adds real typed table bytes and bounded scans.
fn display_rows() -> Vec<Vec<Cell>> {
    vec![
        vec![U(30), U(2), Array(vec![U(700), U(0), U(701)])],
        vec![U(10), U(2), Array(vec![U(800), U(0), U(800)])],
        vec![U(20), U(3), Array(vec![U(900), U(0), U(0)])],
        vec![U(40), U(99), Array(vec![U(0), U(0), U(0)])],
        vec![U(50), U(0), Array(vec![U(0), U(0), U(0)])],
    ]
}
fn model_rows() -> Vec<Vec<Cell>> {
    vec![vec![U(3), U(500)], vec![U(2), U(500)], vec![U(4), U(600)]]
}
fn geoset_rows() -> Vec<Vec<Cell>> {
    vec![
        vec![U(3), U(10), I(2), I(5)],
        vec![U(1), U(10), I(0), I(4)],
        vec![U(2), U(30), I(1), I(7)],
    ]
}
fn display_table(rows: &[Vec<Cell>]) -> Table {
    table(
        "CreatureDisplayInfo",
        321,
        &[
            ("ID", "uint"),
            ("ModelID", "uint"),
            ("TextureVariationFileDataID", "array3"),
        ],
        rows,
    )
}
fn model_table(rows: &[Vec<Cell>]) -> Table {
    table(
        "CreatureModelData",
        322,
        &[("ID", "uint"), ("FileDataID", "uint")],
        rows,
    )
}
fn geoset_table(rows: &[Vec<Cell>], kind: &str) -> Table {
    table(
        "CreatureDisplayInfoGeosetData",
        323,
        &[
            ("ID", "uint"),
            ("CreatureDisplayInfoID", "uint"),
            ("GeosetIndex", kind),
            ("GeosetValue", kind),
        ],
        rows,
    )
}
fn no_textures() -> Cell {
    Array(vec![U(0), U(0), U(0)])
}
fn variant(name: &str) -> Vec<Table> {
    let mut all = tables();
    match name {
        "creature" => {}
        "firstModelNoDisplays" => {
            all[0] = display_table(
                &display_rows()
                    .into_iter()
                    .filter(|row| !matches!(row[1], U(2)))
                    .collect::<Vec<_>>(),
            )
        }
        "modelFileZero" => {
            let mut rows = model_rows();
            rows[1][1] = U(0);
            all[1] = model_table(&rows);
        }
        "scalarTextures" | "signedTextures" | "floatTextures" => {
            let mut rows = display_rows();
            for row in &mut rows {
                let selected = matches!(row[0], U(10));
                row[2] = match name {
                    "scalarTextures" => U(if selected { 800 } else { 0 }),
                    "signedTextures" => Array(if selected {
                        vec![I(-1), I(800)]
                    } else {
                        vec![I(0), I(0)]
                    }),
                    _ => Array(if selected {
                        vec![F(800.75), F(900.5)]
                    } else {
                        vec![F(0.0), F(0.0)]
                    }),
                };
            }
            let kind = match name {
                "scalarTextures" => "uint",
                "signedTextures" => "array2signed",
                _ => "array2float",
            };
            all[0] = table(
                "CreatureDisplayInfo",
                321,
                &[
                    ("ID", "uint"),
                    ("ModelID", "uint"),
                    ("TextureVariationFileDataID", kind),
                ],
                &rows,
            );
        }
        "missingPresentation" => {
            let mut displays = display_rows();
            for row in &mut displays {
                row.truncate(2);
            }
            all[0] = table(
                "CreatureDisplayInfo",
                321,
                &[("ID", "uint"), ("ModelID", "uint")],
                &displays,
            );
            let mut geosets = geoset_rows();
            for row in &mut geosets {
                row.truncate(2);
            }
            all[2] = table(
                "CreatureDisplayInfoGeosetData",
                323,
                &[("ID", "uint"), ("CreatureDisplayInfoID", "uint")],
                &geosets,
            );
        }
        "signedGeosets" => {
            all[2] = geoset_table(
                &[
                    vec![U(1), U(10), I(-1), I(0)],
                    vec![U(2), U(10), I(-2), I(-3)],
                    vec![U(3), U(10), I(0), I(-7)],
                ],
                "signed",
            )
        }
        "floatGeosets" => all[2] = geoset_table(&[vec![U(1), U(10), F(-2.75), F(-3.75)]], "float"),
        "wideGeosets" => {
            all[2] = geoset_table(
                &[
                    vec![U(1), U(10), I64(i64::MAX), I64(0)],
                    vec![U(2), U(10), I64(i64::MAX - 1), I64(0)],
                    vec![U(3), U(10), I64(i64::MIN), I64(0)],
                    vec![U(4), U(10), I64(0), I64(i64::MAX)],
                ],
                "signed64",
            )
        }
        "missingDisplay" => {
            all.remove(0);
        }
        "missingModel" => {
            all.remove(1);
        }
        "missingGeoset" => {
            all.remove(2);
        }
        "firstInvalidThenMissing" => {
            all[0].raw.pop();
            all.remove(1);
        }
        "invalidNonmatchingDisplay" => {
            let mut rows = display_rows();
            for row in &mut rows {
                row.push(Cell::Text(String::new()));
            }
            rows.push(vec![U(999), U(99), no_textures(), Cell::BadString]);
            all[0] = table(
                "CreatureDisplayInfo",
                321,
                &[
                    ("ID", "uint"),
                    ("ModelID", "uint"),
                    ("TextureVariationFileDataID", "array3"),
                    ("Unused", "text"),
                ],
                &rows,
            );
        }
        "invalidNonmatchingModel" => {
            let mut rows = model_rows();
            for row in &mut rows {
                row.push(Cell::Text(String::new()));
            }
            rows.push(vec![U(99), U(999), Cell::BadString]);
            all[1] = table(
                "CreatureModelData",
                322,
                &[("ID", "uint"), ("FileDataID", "uint"), ("Unused", "text")],
                &rows,
            );
        }
        "invalidNonmatchingGeoset" => {
            let mut rows = geoset_rows();
            for row in &mut rows {
                row.push(Cell::Text(String::new()));
            }
            rows.push(vec![U(999), U(999), I(0), I(0), Cell::BadString]);
            all[2] = table(
                "CreatureDisplayInfoGeosetData",
                323,
                &[
                    ("ID", "uint"),
                    ("CreatureDisplayInfoID", "uint"),
                    ("GeosetIndex", "signed"),
                    ("GeosetValue", "signed"),
                    ("Unused", "text"),
                ],
                &rows,
            );
        }
        "modelFileMissing" => {
            let mut rows = model_rows();
            for row in &mut rows {
                row.truncate(1);
            }
            all[1] = table("CreatureModelData", 322, &[("ID", "uint")], &rows);
        }
        "modelFileFloat" => {
            all[1] = table(
                "CreatureModelData",
                322,
                &[("ID", "uint"), ("FileDataID", "float")],
                &[
                    vec![U(3), F(500.5)],
                    vec![U(2), F(500.75)],
                    vec![U(4), F(600.0)],
                ],
            )
        }
        "displayModelMissing" => {
            let mut rows = display_rows();
            for row in &mut rows {
                row.remove(1);
            }
            all[0] = table(
                "CreatureDisplayInfo",
                321,
                &[("ID", "uint"), ("TextureVariationFileDataID", "array3")],
                &rows,
            );
        }
        "displayModelFloat" => {
            let mut rows = display_rows();
            for row in &mut rows {
                row[1] = F(match row[0] {
                    U(10) => 2.75,
                    U(30) => 2.5,
                    _ => match row[1] {
                        U(value) => value as f32,
                        _ => unreachable!(),
                    },
                });
            }
            all[0] = table(
                "CreatureDisplayInfo",
                321,
                &[
                    ("ID", "uint"),
                    ("ModelID", "float"),
                    ("TextureVariationFileDataID", "array3"),
                ],
                &rows,
            );
        }
        "geosetDisplayMissing" => {
            let mut rows = geoset_rows();
            for row in &mut rows {
                row.remove(1);
            }
            all[2] = table(
                "CreatureDisplayInfoGeosetData",
                323,
                &[
                    ("ID", "uint"),
                    ("GeosetIndex", "signed"),
                    ("GeosetValue", "signed"),
                ],
                &rows,
            );
        }
        "geosetDisplayFloat" => {
            let mut rows = geoset_rows();
            for row in &mut rows {
                row[1] = F(match row[1] {
                    U(value) => value as f32,
                    _ => unreachable!(),
                });
            }
            all[2] = table(
                "CreatureDisplayInfoGeosetData",
                323,
                &[
                    ("ID", "uint"),
                    ("CreatureDisplayInfoID", "float"),
                    ("GeosetIndex", "signed"),
                    ("GeosetValue", "signed"),
                ],
                &rows,
            );
        }
        "geosetExact" | "geosetRowBound" => {
            let count = if name == "geosetExact" { 65536 } else { 65537 };
            all[2] = geoset_table(
                &(1..=count)
                    .map(|id| vec![U(id), U(10), I(0), I(id as i32)])
                    .collect::<Vec<_>>(),
                "signed",
            );
        }
        "relationExact" | "relationBound" | "displayRowBound" => {
            let count = match name {
                "relationExact" => 65535,
                "relationBound" => 65536,
                _ => 65537,
            };
            all[0] = display_table(
                &(1..=count)
                    .map(|id| vec![U(id), U(2), no_textures()])
                    .collect::<Vec<_>>(),
            );
            all[1] = model_table(&[vec![U(2), U(500)]]);
            all[2] = geoset_table(&[], "signed");
        }
        "modelExact" | "modelRowBound" => {
            let count = if name == "modelExact" { 65536 } else { 65537 };
            all[0] = display_table(&[vec![U(10), U(1), no_textures()]]);
            all[1] = model_table(
                &(1..=count)
                    .map(|id| vec![U(id), U(500)])
                    .collect::<Vec<_>>(),
            );
            all[2] = geoset_table(&[], "signed");
        }
        "captureOverflow" => {
            all[0] = display_table(
                &(0..65536)
                    .map(|id| {
                        vec![
                            U(3_000_000_000 + id),
                            U(3_333_333_333),
                            Array(vec![U(u32::MAX), U(u32::MAX), U(u32::MAX)]),
                        ]
                    })
                    .collect::<Vec<_>>(),
            );
            all[1] = model_table(&[vec![U(3_333_333_333), U(3_444_444_444)]]);
            all[2] = geoset_table(&[], "signed");
        }
        _ => panic!("unknown creature fixture {name}"),
    }
    all
}
const PUBLIC_VARIANTS: &[&str] = &[
    "creature",
    "firstModelNoDisplays",
    "modelFileZero",
    "scalarTextures",
    "signedTextures",
    "floatTextures",
    "missingPresentation",
    "signedGeosets",
    "floatGeosets",
    "wideGeosets",
    "missingDisplay",
    "missingModel",
    "missingGeoset",
    "firstInvalidThenMissing",
    "invalidNonmatchingDisplay",
    "invalidNonmatchingModel",
    "invalidNonmatchingGeoset",
    "modelFileMissing",
    "modelFileFloat",
    "displayModelMissing",
    "displayModelFloat",
    "geosetDisplayMissing",
    "geosetDisplayFloat",
    "geosetExact",
    "geosetRowBound",
    "relationExact",
    "relationBound",
    "displayRowBound",
    "modelExact",
    "modelRowBound",
    "captureOverflow",
];
fn captures(fx: &Fixture) -> usize {
    lycheedev_core::metadata::Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 1000)
        .unwrap()
        .len()
}

#[test]
fn first_model_without_displays_does_not_fall_back_but_model_lists_later_candidates() {
    let fx = Fixture::new(&variant("firstModelNoDisplays"));
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data creature display", &[("--file-data-id", "500")])
        )
        .unwrap_err()
        .code,
        "records.record_not_found"
    );
    assert_eq!(captures(&fx), 0);
    let out = run(&fx, "data creature model", &[("--file-data-id", "500")]);
    assert_eq!(
        out.data["displays"],
        json!([{"displayID":20,"modelID":3,"fileDataID":500,"textures":[900],"variations":[]}])
    );
    assert_eq!(out.data["partialReasons"], json!(["relation_ambiguous"]));
}
#[test]
fn real_numeric_projection_arrays_and_signed_64bit_geosets_preserve_types() {
    for (name, key, value) in [
        ("scalarTextures", "textures", json!([])),
        ("signedTextures", "textures", json!([800])),
        ("floatTextures", "textures", json!([])),
        ("missingPresentation", "variations", json!([100, 100])),
        ("signedGeosets", "variations", json!([0, -103, 93])),
        ("floatGeosets", "variations", json!([-103])),
        (
            "wideGeosets",
            "variations",
            json!([0, -100, 100, i64::MIN + 99]),
        ),
        ("modelFileZero", "fileDataID", json!(0)),
        ("modelFileMissing", "fileDataID", json!(0)),
        ("modelFileFloat", "fileDataID", json!(500)),
        ("displayModelFloat", "modelID", json!(2)),
    ] {
        let fx = Fixture::new(&variant(name));
        let out = run(&fx, "data creature display", &[("--display-id", "10")]);
        assert_eq!(out.data[key], value, "{name}");
        assert!(out.complete, "{name}");
        if matches!(name, "modelFileZero" | "modelFileMissing") {
            assert_eq!(out.data["relations"], json!([]));
        }
    }
    let fx = Fixture::new(&variant("displayModelMissing"));
    let out = run(&fx, "data creature display", &[("--display-id", "10")]);
    assert_eq!(out.data["modelID"], 0);
    assert_eq!(out.data["partialReasons"], json!(["relation_unresolved"]));
}
#[test]
fn exact_match_fields_reject_missing_or_float_types_without_capture() {
    for prefix in ["modelFile", "displayModel", "geosetDisplay"] {
        for (suffix, code) in [
            ("Missing", "records.field_unknown"),
            ("Float", "records.field_type_mismatch"),
        ] {
            let fx = Fixture::new(&variant(&format!("{prefix}{suffix}")));
            let selector = if prefix == "geosetDisplay" {
                "--display-id"
            } else {
                "--file-data-id"
            };
            let value = if prefix == "geosetDisplay" {
                "10"
            } else {
                "500"
            };
            let error = lycheedev_data::dispatch(
                &fx.invocation("data creature display", &[(selector, value)]),
            )
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.exit_code, 2);
            assert_eq!(captures(&fx), 0);
        }
    }
}
#[test]
fn full_scans_decode_corrupt_nonmatching_rows_but_unreached_scans_do_not() {
    for name in ["invalidNonmatchingDisplay", "invalidNonmatchingModel"] {
        let fx = Fixture::new(&variant(name));
        for command in ["data creature display", "data creature model"] {
            assert_eq!(
                lycheedev_data::dispatch(&fx.invocation(command, &[("--file-data-id", "500")]))
                    .unwrap_err()
                    .code,
                "table.invalid_format"
            );
        }
        assert_eq!(captures(&fx), 0);
        assert!(run(&fx, "data creature display", &[("--display-id", "10")]).complete);
    }
    let fx = Fixture::new(&variant("invalidNonmatchingGeoset"));
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data creature display", &[("--display-id", "10")])
        )
        .unwrap_err()
        .code,
        "table.invalid_format"
    );
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data creature model", &[("--file-data-id", "500")])
        )
        .unwrap_err()
        .code,
        "table.invalid_format"
    );
    assert_eq!(captures(&fx), 0);
    assert!(run(&fx, "data creature model", &[("--file-data-id", "999")]).complete);
}
#[test]
fn three_tables_remain_required_before_empty_or_missing_selection() {
    for name in ["missingDisplay", "missingModel", "missingGeoset"] {
        let fx = Fixture::new(&variant(name));
        for (command, flag, value) in [
            ("data creature display", "--display-id", "10"),
            ("data creature display", "--file-data-id", "999"),
            ("data creature model", "--file-data-id", "999"),
        ] {
            assert!(lycheedev_data::dispatch(&fx.invocation(command, &[(flag, value)])).is_err());
        }
        assert_eq!(captures(&fx), 0);
    }
}
// Regression for the frozen-Go open order. Root first runs the public
// firstInvalidThenMissing counterexample against the existing CLI; production
// preparation order must not be changed before that observation.
#[test]
fn first_invalid_table_precedes_a_later_missing_required_table() {
    let fx = Fixture::new(&variant("firstInvalidThenMissing"));
    for (command, flag, value) in [
        ("data creature display", "--display-id", "10"),
        ("data creature display", "--file-data-id", "500"),
        ("data creature model", "--file-data-id", "500"),
    ] {
        assert_eq!(
            lycheedev_data::dispatch(&fx.invocation(command, &[(flag, value)]))
                .unwrap_err()
                .code,
            "table.invalid_format"
        );
    }
    assert_eq!(captures(&fx), 0);
}
#[test]
fn real_geoset_display_model_and_relation_boundaries_keep_complete_admitted_outputs() {
    for (name, reason) in [("geosetExact", "none"), ("geosetRowBound", "row_budget")] {
        let fx = Fixture::new(&variant(name));
        let out = run(&fx, "data creature display", &[("--display-id", "10")]);
        assert_eq!(
            out.data["variations"],
            json!((1..=65536).map(|id| 100 + id).collect::<Vec<_>>())
        );
        assert_eq!(out.data["truncationReason"], reason);
        assert_eq!(out.complete, reason == "none");
    }
    for (name, reason) in [
        ("relationExact", "none"),
        ("relationBound", "relation_budget"),
        ("displayRowBound", "row_budget"),
    ] {
        let fx = Fixture::new(&variant(name));
        let out = run(&fx, "data creature display", &[("--file-data-id", "500")]);
        assert_eq!(out.data["displayID"], 1);
        assert_eq!(out.data["truncationReason"], reason);
        assert_eq!(out.complete, reason == "none");
        let relations = out.data["relations"].as_array().unwrap();
        assert_eq!(relations.len(), 65536);
        let count = if name == "relationExact" {
            65535
        } else {
            65536
        };
        for (index, value) in relations[..count].iter().enumerate() {
            assert_eq!(
                *value,
                json!({"fromTable":"CreatureDisplayInfo","fromField":"ModelID","fromID":index+1,"toKind":"creature_display","toID":index+1,"depth":0})
            );
        }
        if name == "relationExact" {
            assert_eq!(
                relations[65535],
                json!({"fromTable":"CreatureModelData","fromField":"FileDataID","fromID":2,"toKind":"model_file","toID":500,"depth":1})
            );
        }
    }
    for (name, reason) in [("modelExact", "none"), ("modelRowBound", "row_budget")] {
        let fx = Fixture::new(&variant(name));
        for command in ["data creature display", "data creature model"] {
            let out = run(&fx, command, &[("--file-data-id", "500")]);
            assert_eq!(out.data["truncationReason"], reason);
            assert_eq!(out.data["partialReasons"], json!(["relation_ambiguous"]));
            if command == "data creature display" {
                assert_eq!(out.data["displayID"], 10);
            } else {
                assert_eq!(
                    out.data["displays"],
                    json!([{"displayID":10,"modelID":1,"fileDataID":500,"textures":[],"variations":[]}])
                )
            }
        }
    }
}
#[test]
fn cold_and_warm_three_table_requests_share_authentication_and_preserve_control() {
    let fx = Fixture::new(&tables());
    let cold = run(&fx, "data creature display", &[("--file-data-id", "500")]);
    let warm = run(&fx, "data creature display", &[("--file-data-id", "500")]);
    for out in [&cold, &warm] {
        for proof in out.data["tableVerification"].as_array().unwrap() {
            for field in [
                "encodingFramingEKey",
                "encodingRequestedPagesMD5",
                "rootWholeDecodedCKey",
                "contentWholeDecodedCKey",
            ] {
                assert_eq!(proof[field], true);
            }
        }
    }
    for field in ["displayID", "textures", "variations", "relations"] {
        assert_eq!(cold.data[field], warm.data[field]);
    }
    let before = captures(&fx);
    let mut inv = fx.invocation("data creature model", &[("--file-data-id", "500")]);
    inv.control
        .cancel_handle()
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    inv.control = lycheedev_core::control::OperationControl::new(
        std::time::Instant::now(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    assert_eq!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "command.deadline"
    );
    assert_eq!(captures(&fx), before);
}
#[test]
#[ignore = "prepare all authenticated creature variants for frozen-Go public comparison"]
fn prepare_creature_public_fixtures() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CREATURE_FIXTURE").expect("fresh creature suite root"),
    );
    assert!(!root.exists());
    std::fs::create_dir_all(&root).unwrap();
    for name in PUBLIC_VARIANTS {
        Fixture::at(root.join(name), &variant(name), "zhCN", 0x40);
    }
    std::fs::write(root.join("creature-cases.json"),serde_json::to_vec_pretty(&json!({
        "scope":"authenticated synthetic CASC/DBD/WDC; no real-client or execution claim",
        "fixtures":PUBLIC_VARIANTS,"baselineCases":"tests/rust-source-data/creature-public-cases.json",
        "extraCases":"tests/rust-source-data/creature-public-extra-cases.json"
    })).unwrap()).unwrap();
}
