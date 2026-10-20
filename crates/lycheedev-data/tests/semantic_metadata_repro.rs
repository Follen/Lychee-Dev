//! Reproductions for official WoWDBDefs metadata syntax rejected by Go323/Rust.
//! Official bytes are fixed at b9a63096cdd99b26967f17f9515377ae62cadfd7;
//! tables/pins below are authenticated synthetic fixtures, not client evidence.
#[path = "support/data_cache.rs"]
mod data_cache;
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use catalog_fixture::{BUILD, Cell::U, DEFS, Fixture, table};
use lycheedev_core::{Outcome, evidence, storage};
use serde_json::{Value, json};

const CHR_MODEL: &[u8] = include_bytes!("fixtures/metadata-official-b9a6309/ChrModelID.dbde");
const GARR_SOCKET: &[u8] =
    include_bytes!("fixtures/metadata-official-b9a6309/GarrTalentSocketType.dbde");
const RACE_MASK: &[u8] = include_bytes!("fixtures/metadata-official-b9a6309/RaceMask0.dbdf");
const WEATHER: &[u8] = include_bytes!("fixtures/metadata-official-b9a6309/WeatherType.dbde");

fn fixture(name: &'static str) -> Fixture {
    Fixture::new(&[table(
        name,
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(1)]],
    )])
}

fn cache(fx: &Fixture, suffix: &str, raw: &[u8]) -> Value {
    let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/meta/{suffix}");
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let blob = evidence::publish_blob(&fx.home, raw, 4 << 20).unwrap();
    data_cache::Cache::new(&fx.home)
        .put(&key, &json!({"url":url,"blob":blob}), None)
        .unwrap();
    blob
}

fn query(fx: &Fixture, sql: &str) -> Outcome {
    lycheedev_data::migration_oracle::dispatch(&fx.invocation("data sql", &[("--sql", sql)])).unwrap()
}

fn verify(fx: &Fixture, out: &Outcome, name: &str, mapping: Value, definitions: Value) {
    assert_eq!(
        out.data["semanticSources"],
        json!([{"table":name,"commit":DEFS,"build":BUILD,
            "mapping":mapping,"definitions":definitions}])
    );
    assert_eq!(out.data["sources"], json!([]));
    assert_eq!(out.data["timings"]["decodedSourceBytes"], 0);
    assert_eq!(out.data["complete"], true);
    assert_eq!(out.data["partial"], false);
    assert_eq!(out.provenance.len(), 1);
    let capture = &out.provenance[0];
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    assert_eq!(
        evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let raw = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
}

#[test]
fn official_metadata_fixture_bytes_keep_the_fixed_upstream_identity() {
    let manifest: Value = serde_json::from_str(include_str!(
        "fixtures/metadata-official-b9a6309/provenance.json"
    ))
    .unwrap();
    assert_eq!(manifest["commit"], "b9a63096cdd99b26967f17f9515377ae62cadfd7");
    for (index, raw) in [CHR_MODEL, GARR_SOCKET, WEATHER, RACE_MASK]
        .into_iter()
        .enumerate()
    {
        assert_eq!(manifest["files"][index]["bytes"], raw.len());
        assert_eq!(manifest["files"][index]["sha256"], storage::sha256(raw));
    }
}

#[test]
fn official_multiword_enum_names_reach_both_actual_meta_table_consumers() {
    // Exact mapping lines 87 and 147 from the same official commit.
    let cases = [
        (
            "ChrCustomizationOption",
            b"ENUM ChrCustomizationOption::ChrModelID ChrModelID // Not a real enum, but for ease\n".as_slice(),
            "enums/ChrModelID.dbde",
            CHR_MODEL,
            "SELECT Value,Name FROM meta.ChrCustomizationOption WHERE Value IN (1,7) ORDER BY Value",
            json!([[1,"Human Male"],[7,"Night Elf Male"]]),
        ),
        (
            "GarrTalentSocketProperties",
            b"ENUM GarrTalentSocketProperties::GarrTalentSocketType GarrTalentSocketType\n".as_slice(),
            "enums/GarrTalentSocketType.dbde",
            GARR_SOCKET,
            "SELECT Value,Name FROM meta.GarrTalentSocketProperties ORDER BY Value",
            json!([[0,"None"],[1,"Spell"],[2,"Soulbind Conduit"]]),
        ),
    ];
    for (name, mapping_raw, path, raw, sql, expected) in cases {
        let fx = fixture(name);
        let mapping = cache(&fx, "mapping.dbdm", mapping_raw);
        let values = cache(&fx, path, raw);
        let out = query(&fx, sql);
        assert_eq!(out.data["result"]["rows"], expected);
        verify(&fx, &out, name, mapping, json!({path:values}));
    }
}

#[test]
fn official_multiword_flags_keep_unnamed_and_exact_numeric_values() {
    let fx = fixture("CharacterLoadout");
    // Exact mapping line 365 from the same official commit.
    let mapping = cache(
        &fx,
        "mapping.dbdm",
        b"FLAGS CharacterLoadout::RaceMasks[0] RaceMask0\n",
    );
    let values = cache(&fx, "flags/RaceMask0.dbdf", RACE_MASK);
    let out = query(
        &fx,
        "SELECT Field,Kind,Value,Name FROM meta.CharacterLoadout WHERE Value IN (8,512,4194304,2147483648) ORDER BY Value",
    );
    assert_eq!(
        out.data["result"]["rows"],
        json!([
            ["RaceMasks[0]","FLAGS",8,"Night Elf"],
            ["RaceMasks[0]","FLAGS",512,"Blood Elf"],
            ["RaceMasks[0]","FLAGS",4194304,null],
            ["RaceMasks[0]","FLAGS",2147483648u64,"Kul Tiran"]
        ])
    );
    verify(&fx, &out, "CharacterLoadout", mapping, json!({"flags/RaceMask0.dbdf":values}));
}

#[test]
fn official_weather_build_presets_skip_historical_rows_for_current_build() {
    let fx = fixture("Weather");
    // Exact mapping line 344 from the same official commit.
    let mapping = cache(&fx, "mapping.dbdm", b"ENUM Weather::Type WeatherType\n");
    let values = cache(&fx, "enums/WeatherType.dbde", WEATHER);
    let out = query(&fx, "SELECT Value,Name FROM meta.Weather ORDER BY Value");
    assert_eq!(
        out.data["result"]["rows"],
        json!([[0,"Clear"],[1,"Rain"],[2,"Snow"],[3,"Sandstorm"],[4,"Miscellaneous"],[5,"Fire"]])
    );
    verify(&fx, &out, "Weather", mapping, json!({"enums/WeatherType.dbde":values}));
}
