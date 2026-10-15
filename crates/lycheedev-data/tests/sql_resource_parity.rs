//! Go 3.2 logical accounting slice: metadata-only SQL. Execution NOT_RUN.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, DEFS, Fixture, table};
use lycheedev_core::{evidence, metadata::Metadata, storage};
use serde_json::{Value, json};
fn fixture() -> Fixture {
    Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(1)]],
    )])
}
fn cache(fx: &Fixture, suffix: &str, raw: &[u8]) {
    let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/{suffix}");
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let blob = evidence::publish_blob(&fx.home, raw, 4 << 20).unwrap();
    let mut metadata = Metadata::open(&fx.home, true).unwrap();
    let generation = metadata.generation(&key).unwrap();
    metadata
        .put(&key, &json!({"url":url,"blob":blob}), generation)
        .unwrap();
}
#[test]
fn sixteen_mebibyte_comment_definitions_admit_with_exact_go_resources() {
    let fx = fixture();
    let mapping = (0..16)
        .map(|i| format!("ENUM Sample::Value D{i}\n"))
        .collect::<String>();
    cache(&fx, "meta/mapping.dbdm", mapping.as_bytes());
    let mut raw = vec![b'x'; 1 << 20];
    raw[..2].copy_from_slice(b"//");
    *raw.last_mut().unwrap() = b'\n';
    assert_eq!(
        storage::sha256(&raw),
        "db8236835a7df60fe4e1ae1e42ffa317a51f0a4311dd4b493458acdcc713d38b"
    );
    for i in 0..16 {
        cache(&fx, &format!("meta/enums/D{i}.dbde"), &raw);
    }
    let mut loaded = 0u64;
    let mut decode = 0u64;
    // The fixture's exact manifest and DBD are read once, alongside each URL.
    for suffix in std::iter::once("manifest.json".to_owned())
        .chain(std::iter::once("definitions/Sample.dbd".to_owned()))
        .chain(std::iter::once("meta/mapping.dbdm".to_owned()))
        .chain((0..16).map(|i| format!("meta/enums/D{i}.dbde")))
    {
        let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/{suffix}");
        let key = format!("definition/{}", storage::sha256(url.as_bytes()));
        let entry: Value = Metadata::open(&fx.home, false).unwrap().read(&key).unwrap();
        let bytes = entry["blob"]["bytes"].as_u64().unwrap();
        loaded += bytes;
        decode += bytes.div_ceil(65536);
    }
    let out = lycheedev_data::dispatch(
        &fx.invocation("data sql", &[("--sql", "SELECT * FROM meta.Sample")]),
    )
    .unwrap();
    let metadata = loaded * 4 + mapping.len() as u64 + (16 << 20) + 16 * 256;
    assert_eq!(
        out.data["resources"],
        json!({"retainedBytes": (64 << 20) + metadata, "metadataBytes":metadata,"decodeWork":decode+16,"networkRequests":0,"networkBytes":0})
    );
    assert_eq!(out.data["result"]["rows"], json!([]));
    assert_eq!(
        out.data["semanticSources"][0]["definitions"]
            .as_object()
            .unwrap()
            .len(),
        16
    );
    let capture = &out.provenance[0];
    let bytes = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), out.data);
    assert!(
        out.data["resourceAccounting"]["used"]["retainedBytes"]
            .as_u64()
            .unwrap()
            > loaded * 8
    );
}
#[test]
fn projection_and_publication_do_not_change_same_source_logical_usage() {
    // Go requires FROM. Keep a real one-row source fixed while varying the
    // projected output; this checks executor/publication accounting, not full
    // physical-provider Go parity (which has its own remaining audit).
    // Each projection starts with a fresh provider cache. Cold and warm
    // provider reads have different charges independently of the expression.
    let mut expected_resources = None;
    let mut expected_cache = None;
    for (sql, expected_row) in [
        ("SELECT 1 FROM Sample", json!([1])),
        ("SELECT 1 + 2 FROM Sample", json!([3])),
        (
            "SELECT 'longer output string' FROM Sample",
            json!(["longer output string"]),
        ),
    ] {
        let fx = fixture();
        let out = lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
            .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
        assert_eq!(out.data["result"]["rows"], json!([expected_row]), "{sql}");
        let cache = out.data["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|source| source["file"]["decodedCache"].clone())
            .collect::<Vec<_>>();
        assert!(!cache.is_empty(), "{sql}: no physical source evidence");
        assert!(
            cache.iter().all(Value::is_object),
            "{sql}: missing cache evidence"
        );
        if let Some(expected) = &expected_cache {
            assert_eq!(&cache, expected, "{sql}: provider cache state differs");
        } else {
            expected_cache = Some(cache);
        }
        if let Some(expected) = &expected_resources {
            assert_eq!(&out.data["resources"], expected, "{sql}");
        } else {
            assert!(out.data["resources"]["retainedBytes"].as_u64().unwrap() >= 64 << 20);
            expected_resources = Some(out.data["resources"].clone());
        }
    }
}
#[test]
fn source_free_select_is_go_syntax_error_and_creates_no_capture() {
    let fx = fixture();
    let captures = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap()
            .len()
    };
    let before = captures();
    for sql in ["SELECT 1", "SELECT 1 + 2", "SELECT 'longer output string'"] {
        let error = match lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)])) {
            Ok(_) => panic!("{sql}: Go requires FROM"),
            Err(error) => error,
        };
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("query.invalid_syntax", 2),
            "{sql}"
        );
        assert_eq!(captures(), before, "{sql}");
    }
}

#[test]
fn failed_sql_never_publishes_a_capture() {
    let fx = fixture();
    let captures = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap()
            .len()
    };
    let before = captures();
    for sql in [
        "SELECT * FROM meta.Missing",
        "SELECT missing FROM meta.Sample",
        "SELECT * FROM",
    ] {
        assert!(lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)])).is_err());
        assert_eq!(captures(), before);
    }
}
