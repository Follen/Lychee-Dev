//! Explicit retained inputs for the current paired Agent consumers.
//! These tests construct authenticated files only; they never run a query.
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use catalog_fixture::{Cell, Fixture, table};
use lycheedev_core::public_json::NumberLayout;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "explicit owned catalog fixture destination required"]
fn prepare_catalog_agent_fixture() {
    let root = PathBuf::from(
        std::env::var_os("LYCHEEDEV_CATALOG_FIXTURE").expect("fresh owned catalog destination"),
    );
    let mut tables = catalog_fixture::items();
    tables.push(catalog_fixture::decors(&catalog_fixture::decor_rows()));
    Fixture::at(root, &tables, "zhCN", 0x40);
    // The outer owner retains this cold pin/definition-only input after reap.
}

#[test]
#[ignore = "explicit owned numeric fixture destination required"]
fn prepare_numeric_agent_fixture() {
    let root = PathBuf::from(
        std::env::var_os("LYCHEEDEV_NUMERIC_CSV_FIXTURE").expect("fresh owned numeric destination"),
    );
    let rows: Vec<_> = (0..16_384u32)
        .map(|index| {
            // All four fractions are exact f32 values throughout this range.
            let value = index as f32 + 1.5;
            vec![
                Cell::U(index + 1),
                Cell::F(value),
                Cell::F(-value),
                Cell::F(0.25),
                Cell::F(-0.25),
            ]
        })
        .collect();
    let numeric = table(
        "NumericSample",
        404,
        &[
            ("ID", "uint"),
            ("F0", "float"),
            ("F1", "float"),
            ("F2", "float"),
            ("F3", "float"),
        ],
        &rows,
    );
    let fixture = Fixture::at(root, &[numeric], "zhCN", 0x40);
    let identity_path = fixture.root.join("input-identity.json");
    let mut identity: Value = serde_json::from_slice(&fs::read(&identity_path).unwrap()).unwrap();
    // Current ABI observation for the existing reader; no historical layout
    // cost is subtracted from product time or used to generate expected rows.
    identity["numericCsvABI"] = json!({
        "stringNumberLayoutBytes": size_of::<(String, NumberLayout)>(),
        "usizeNumberLayoutBytes": size_of::<(usize, NumberLayout)>(),
    });
    fs::write(identity_path, serde_json::to_vec_pretty(&identity).unwrap()).unwrap();
}
