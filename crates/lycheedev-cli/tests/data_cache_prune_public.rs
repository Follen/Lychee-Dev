//! Real CLI processes and authenticated synthetic producer inputs. No game,
//! user cache, network source, or hand-written generation receipt is used.
#[allow(dead_code)]
#[path = "../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};
use lycheedev_core::{cache, storage, workspace::data_disk::DataCachePaths};
use serde_json::{Value, json};
use std::{fs, process::Command};
fn invoke(f: &Fixture, args: &[&str], exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
        .args(args)
        .arg("--home")
        .arg(&f.home)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit), "{args:?}: {output:?}");
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{args:?}: {error}: {output:?}"));
    assert_eq!(value["schema"], "lycheedev.result.v1");
    assert_eq!(value["ok"], exit == 0);
    value
}
#[test]
fn actual_cache_prune_cli_selects_only_explicit_data_scope_and_preserves_default_contract() {
    let f = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(7), Cell::Text("public prune".into())]],
    )]);
    let describe = invoke(&f, &["describe"], 0);
    let definitions = describe["result"]["commands"].as_array().unwrap();
    assert_eq!(definitions.len(), 85);
    let command = definitions
        .iter()
        .find(|d| d["path"] == "cache prune")
        .unwrap();
    assert_eq!(command["arguments"], json!([]));
    assert_eq!(command["mutates"], true);
    assert_eq!(
        command["flags"],
        json!([
            "--home <root>",
            "--format text|json|jsonl",
            "--target-bytes <n>",
            "--max-objects <n>",
            "--uncommitted",
            "--dry-run",
            "--data-derived"
        ])
    );
    assert!(
        command["summary"]
            .as_str()
            .unwrap()
            .contains("not downloads, shared blobs or CAP")
    );
    assert!(
        command["summary"]
            .as_str()
            .unwrap()
            .contains("distinct object-group attempt cap")
    );
    let prepared = invoke(
        &f,
        &[
            "data",
            "cache",
            "prepare",
            "Sample",
            "--snapshot",
            &f.pin,
            "--installation",
            f.client.to_str().unwrap(),
            "--offline",
        ],
        0,
    );
    let generation = prepared["result"]["generation"].as_str().unwrap();
    let sha = prepared["result"]["object"]["sha256"].as_str().unwrap();
    let paths = DataCachePaths::new(&f.home);
    let manifest_path = paths.generations.join(format!("{generation}.json"));
    let object_path = paths.object(sha).unwrap();
    let manifest = fs::read(&manifest_path).unwrap();
    let object = fs::read(&object_path).unwrap();
    assert_eq!(storage::sha256(&object), sha);
    let ordinary = b"default ordinary cache remains its own scope";
    cache::publish(&f.home, ordinary, ordinary.len(), None).unwrap();
    let default = invoke(&f, &["cache", "prune"], 0);
    assert_eq!(
        default["result"],
        json!({
            "targetBytes":0,"beforeBytes":ordinary.len(),"afterBytes":0,"reclaimedBytes":ordinary.len(),
            "removedObjects":1,"removedUncommitted":0,"skippedProtected":0,"skippedInUse":0,
            "skippedUncommitted":0,"skippedForeign":0,"truncated":false,"complete":true,"dryRun":false
        })
    );
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
    assert_eq!(fs::read(&object_path).unwrap(), object);
    // Republish through the normal generic cache API; explicit Data pruning
    // must not change this otherwise-reclaimable physical ordinary object.
    cache::publish(&f.home, ordinary, ordinary.len(), None).unwrap();
    let digest = storage::sha256(ordinary);
    let ordinary_path = f
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let rejected = invoke(
        &f,
        &["cache", "prune", "--data-derived", "--uncommitted"],
        2,
    );
    assert_eq!(rejected["error"]["code"], "command.invalid_arguments");
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
    let dry = invoke(
        &f,
        &[
            "cache",
            "prune",
            "--data-derived",
            "--dry-run",
            "--max-objects",
            "1",
        ],
        0,
    );
    assert_eq!(
        dry["result"]["scope"],
        "data-derived-v1-objects-and-generations"
    );
    assert_eq!(dry["result"]["dryRun"], true);
    assert_eq!(dry["result"]["removedObjects"], 1);
    assert_eq!(dry["result"]["attemptedObjects"], 1);
    assert_eq!(dry["result"]["afterBytes"], 0);
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
    assert_eq!(fs::read(&object_path).unwrap(), object);
    let actual = invoke(
        &f,
        &[
            "cache",
            "prune",
            "--data-derived",
            "--target-bytes",
            "0",
            "--max-objects",
            "1",
        ],
        0,
    );
    assert_eq!(actual["result"]["removedObjects"], 1);
    assert_eq!(actual["result"]["removedGenerations"], 1);
    assert_eq!(
        actual["result"]["reclaimedBytes"],
        (manifest.len() + object.len()) as u64
    );
    assert_eq!(
        actual["result"]["coverage"]["automaticEvictionEnabled"],
        false
    );
    assert_eq!(
        actual["result"]["coverage"]["totalDataCoverageComplete"],
        false
    );
    assert!(actual["result"]["retainedOtherDataBytes"].as_u64().unwrap() > 0);
    assert!(!manifest_path.exists());
    assert!(!object_path.exists());
    assert_eq!(fs::read(ordinary_path).unwrap(), ordinary);
    fs::remove_dir_all(f.root).unwrap();
}
