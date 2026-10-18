//! Fixed Go 3.2 SQL CSV export contracts. Execution status: NOT_RUN.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::{evidence, metadata::Metadata, storage};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixture() -> Fixture {
    Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])])
}
fn run(
    fx: &Fixture,
    output: &Path,
    overwrite: bool,
) -> lycheedev_core::Result<lycheedev_core::Outcome> {
    let path = output.to_str().unwrap();
    let mut inv = fx.invocation(
        "data sql",
        &[
            ("--sql", "SELECT 18446744073709551615 AS value FROM Sample"),
            ("--encoding", "csv"),
            ("--output", path),
        ],
    );
    if overwrite {
        inv.flags.insert("--overwrite".into(), "true".into());
    }
    lycheedev_data::dispatch(&inv)
}
fn sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.manifest.json", path.display()))
}
fn capture_count(fx: &Fixture) -> usize {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 1000)
        .unwrap()
        .len()
}
#[test]
fn sql_csv_end_to_end_pair_exact_bytes_and_capture_dependency() {
    let fx = fixture();
    let path = fx.root.join("query.csv");
    let out = run(&fx, &path, false).unwrap();
    let raw = fs::read(&path).unwrap();
    assert_eq!(raw, b"value\n18446744073709551615\n");
    let manifest_raw = fs::read(sidecar(&path)).unwrap();
    assert_eq!(manifest_raw.last(), Some(&b'\n'));
    assert_eq!(
        serde_json::from_slice::<Value>(&manifest_raw).unwrap(),
        out.data
    );
    assert_eq!(out.data["schema"], "lycheedev.query-csv.v1");
    assert_eq!(out.data["sha256"], storage::sha256(&raw));
    assert_eq!(out.data["bytes"], raw.len());
    assert_eq!(out.data["rows"], 1);
    assert_eq!(out.data["complete"], true);
    assert_eq!(out.provenance.len(), 2);
    let query = &out.provenance[0];
    let csv = &out.provenance[1];
    assert_eq!(out.data["queryCapture"], query["id"]);
    assert_eq!(out.data["csvCapture"], csv["id"]);
    assert_eq!(csv["dependencies"][0], query["id"]);
    assert_eq!(
        evidence::read_blob(&fx.home, &csv["blob"], 32 << 20).unwrap(),
        raw
    );
    assert!(evidence::remove_capture(&fx.home, query["id"].as_str().unwrap()).is_err());
    evidence::remove_capture(&fx.home, csv["id"].as_str().unwrap()).unwrap();
    evidence::remove_capture(&fx.home, query["id"].as_str().unwrap()).unwrap();
}
#[test]
fn sql_csv_conflict_does_not_modify_either_destination_or_archive_csv() {
    let fx = fixture();
    let path = fx.root.join("query.csv");
    fs::write(&path, b"old-data").unwrap();
    fs::write(sidecar(&path), b"old-manifest").unwrap();
    let before = capture_count(&fx);
    assert_eq!(
        run(&fx, &path, false).unwrap_err().code,
        "records.export_conflict"
    );
    assert_eq!(fs::read(&path).unwrap(), b"old-data");
    assert_eq!(fs::read(sidecar(&path)).unwrap(), b"old-manifest");
    assert_eq!(capture_count(&fx), before + 1); // Query archives before exporter preflight.
}
#[test]
fn sql_csv_existing_manifest_prevents_data_publication() {
    let fx = fixture();
    let path = fx.root.join("query.csv");
    fs::write(sidecar(&path), b"old-manifest").unwrap();
    assert_eq!(
        run(&fx, &path, false).unwrap_err().code,
        "records.export_conflict"
    );
    assert!(!path.exists());
    assert_eq!(fs::read(sidecar(&path)).unwrap(), b"old-manifest");
}
#[test]
fn sql_csv_explicit_overwrite_replaces_both_files() {
    let fx = fixture();
    let path = fx.root.join("query.csv");
    fs::write(&path, b"old-data").unwrap();
    fs::write(sidecar(&path), b"old-manifest").unwrap();
    let out = run(&fx, &path, true).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"value\n18446744073709551615\n");
    assert_eq!(out.data["complete"], true);
}
#[test]
fn sql_csv_rejects_workspace_and_directory_targets() {
    let fx = fixture();
    assert_eq!(
        run(&fx, &fx.home.join("query.csv"), false)
            .unwrap_err()
            .code,
        "records.export_path"
    );
    let path = fx.root.join("query.csv");
    fs::create_dir(&path).unwrap();
    assert_eq!(
        run(&fx, &path, true).unwrap_err().code,
        "records.export_conflict"
    );
}
#[test]
fn sql_csv_flag_errors_precede_query_selection() {
    let fx = fixture();
    let invocation = |flags: &[(&str, &str)]| {
        let mut inv = fx.invocation("data sql", flags);
        inv.flags.insert("--sql".into(), "SELECT ID FROM Sample".into());
        // An explicit missing pin cannot fall back to an ancestor project.
        inv.flags.insert("--snapshot".into(), "PIN-missing".into());
        inv
    };
    for flags in [
        vec![("--encoding", "csv")],
        vec![("--output", "out.csv")],
        vec![("--overwrite", "true")],
    ] {
        let error = lycheedev_data::dispatch(&invocation(&flags)).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments", "{flags:?}");
        assert_eq!(
            error.message,
            "data sql CSV export requires --encoding csv and --output; --overwrite is CSV-only",
            "{flags:?}"
        );
        assert_eq!(error.exit_code, 2, "{flags:?}");
    }
    for flags in [
        vec![],
        vec![("--encoding", "csv"), ("--output", "out.csv")],
        vec![
            ("--encoding", "csv"),
            ("--output", "out.csv"),
            ("--overwrite", "true"),
        ],
    ] {
        let error = lycheedev_data::dispatch(&invocation(&flags)).unwrap_err();
        assert_eq!(error.code, "vault.record_missing", "{flags:?}");
        assert_eq!(
            error.message,
            "document pin/PIN-missing does not exist",
            "{flags:?}"
        );
        assert_eq!(error.exit_code, 3, "{flags:?}");
    }
}

#[test]
fn sql_csv_manifest_conflict_precedes_workspace_rejection() {
    let fx = fixture();
    let path = fx.home.join("query.csv");
    fs::write(sidecar(&path), b"old-manifest").unwrap();
    assert_eq!(
        run(&fx, &path, false).unwrap_err().code,
        "records.export_conflict"
    );
    assert!(!path.exists());
}

#[test]
fn sql_csv_empty_output_without_csv_keeps_normal_query() {
    let fx = fixture();
    let inv = fx.invocation(
        "data sql",
        &[("--sql", "SELECT ID FROM Sample"), ("--output", "")],
    );
    let out = lycheedev_data::dispatch(&inv).unwrap();
    assert!(out.data.get("result").is_some());
    assert_eq!(out.provenance.len(), 1);
}
