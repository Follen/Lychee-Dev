//! DataContext placement on actual multi-table CDN navigation and two-capture encounter.
use super::*;
fn report(value: &Value) {
    assert_eq!(
        value["memoryBudget"]["mode"],
        if cfg!(windows) {
            "windows_adaptive"
        } else {
            "fixed"
        }
    );
    for key in [
        "retainedBytes",
        "metadataBytes",
        "peakScratchBytes",
        "growths",
    ] {
        assert!(value["memoryBudget"][key].is_u64());
    }
    assert!(value["resources"]["retainedBytes"].is_u64());
}
#[test]
fn default_remote_navigation_shares_one_context_across_real_tables() {
    let f = NavFixture::new(&catalog_fixture::items());
    let inv = f.invocation("data item get", &[("--item-id", "7")]);
    let out = crate::migration_oracle::dispatch(&inv).unwrap();
    report(&out.data);
    assert_eq!(out.data["tables"].as_array().unwrap().len(), 2);
    for table in out.data["tables"].as_array().unwrap() {
        assert!(table.get("memoryBudget").is_none());
    }
    f.verify(&out);
    f.close();
}
#[test]
fn default_remote_encounter_memory_belongs_to_context_not_relation_manifest() {
    use Cell::{I, Text, U};
    let f = NavFixture::new(&[table(
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
            U(0),
            I(0),
            I(1),
            I(-1),
            U(0),
            U(0),
            U(0),
            U(0),
            U(0),
        ]],
    )]);
    let out =
        crate::migration_oracle::dispatch(&f.invocation("data encounter get", &[("--journal-encounter-id", "132")]))
            .unwrap();
    assert_eq!(out.provenance.len(), 2);
    report(&out.data);
    f.verify(&out);
    let raw = evidence::read_blob(&f.source.home, &out.provenance[1]["blob"], 16 << 20).unwrap();
    let manifest: Value = serde_json::from_slice(&raw).unwrap();
    assert!(manifest.get("memoryBudget").is_none());
    assert!(manifest.get("resources").is_none());
    f.close();
}
