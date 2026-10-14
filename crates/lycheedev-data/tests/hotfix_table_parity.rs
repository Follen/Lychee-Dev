//! Go 3.2.0 3057a966 cache_fields_test.go / cache_scan_test.go and
//! records.hotfix.go named-table state transitions. Synthetic offline inputs.
use lycheedev_core::{
    Invocation,
    control::OperationControl,
    evidence,
    metadata::Metadata,
    selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::{
    hotfix::{self, CacheFilter},
    resource::{Budget, Cost},
    schema,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
const BUILD: &str = "12.1.0.69875";
const TABLE: u32 = 42;
const SIMPLE: &str =
    "COLUMNS\nint ID\nstring Name\n\nBUILD 12.1.0.69875\n$noninline,id$ID<u32>\nName\n";
fn definition(text: &str) -> schema::Definition {
    schema::Document::parse(text.as_bytes())
        .unwrap()
        .select(BUILD, "")
        .unwrap()
}
fn budget() -> Budget {
    Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    )
}
fn cache(version: u32, entries: &[(u32, u8, i32, &[u8])]) -> Vec<u8> {
    let mut raw = b"XFTH".to_vec();
    raw.extend(version.to_le_bytes());
    raw.extend(69875u32.to_le_bytes());
    if version >= 5 {
        raw.resize(44, 0);
    }
    for &(id, status, push, payload) in entries {
        let width = match version {
            1 | 7 => 24,
            9 => 32,
            _ => 28,
        };
        let mut header = vec![0; width];
        header[..4].copy_from_slice(b"XFTH");
        let (po, table, record, size, state) = match version {
            1 => (4, 12, 16, 8, 20),
            2..=6 => (8, 16, 20, 12, 24),
            7 => (4, 8, 12, 16, 20),
            8 => (4, 12, 16, 20, 24),
            _ => (8, 16, 20, 24, 28),
        };
        header[po..po + 4].copy_from_slice(&push.to_le_bytes());
        header[table..table + 4].copy_from_slice(&TABLE.to_le_bytes());
        header[record..record + 4].copy_from_slice(&id.to_le_bytes());
        header[size..size + 4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        header[state] = status;
        if version == 9 {
            header[4..8].copy_from_slice(&3u32.to_le_bytes());
        }
        raw.extend(header);
        raw.extend(payload);
    }
    raw
}
#[test]
fn go_decode_hotfix_fields_exact_types_arrays_relation_and_unicode() {
    let d = definition(
        "COLUMNS\nint ID\nstring Name\nlocstring Label\nint Small\nint Wide\nfloat Rate\nint Parent\nint Values\n\nBUILD 12.1.0.69875\n$noninline,id$ID<32>\nName\nLabel\nSmall<8>\nWide<u64>\nRate\n$noninline,relation$Parent<u16>\nValues<u16>[2]\n",
    );
    let mut raw = b"Name\0".to_vec();
    raw.extend("中文\0".as_bytes());
    raw.push(0xfe);
    raw.extend(u64::MAX.to_le_bytes());
    raw.extend(1.5f32.to_le_bytes());
    raw.extend(0x1234u16.to_le_bytes());
    raw.extend(0u16.to_le_bytes());
    raw.extend(u16::MAX.to_le_bytes());
    let fields = hotfix::decode_fields(&raw, 42, &d, Some(&budget())).unwrap();
    assert_eq!(
        serde_json::to_value(&fields).unwrap(),
        json!({"ID":42,"Name":"Name","Label":"中文","Small":-2,"Wide":u64::MAX,"Rate":1.5,"Parent":0x1234,"Values":[0,65535]})
    );
    assert!(
        serde_json::to_string(&fields)
            .unwrap()
            .contains("18446744073709551615")
    );
    raw.push(0);
    assert_eq!(
        hotfix::decode_fields(&raw, 42, &d, None).unwrap_err().code,
        "records.hotfix_fields"
    );
}
#[test]
fn go_decode_inline_and_noninline_header_identity() {
    let inline = definition("COLUMNS\nint ID\n\nBUILD 12.1.0.69875\n$id$ID<u32>\n");
    assert_eq!(
        hotfix::decode_fields(&42u32.to_le_bytes(), 42, &inline, None).unwrap()["ID"],
        42
    );
    assert_eq!(
        hotfix::decode_fields(&42u32.to_le_bytes(), 43, &inline, None)
            .unwrap_err()
            .code,
        "records.hotfix_fields"
    );
    let signed = definition("COLUMNS\nint ID\n\nBUILD 12.1.0.69875\n$noninline,id$ID<8>\n");
    assert_eq!(
        hotfix::decode_fields(&[], u32::MAX, &signed, None).unwrap()["ID"],
        -1
    );
    assert_eq!(
        hotfix::decode_fields(&[], 128, &signed, None)
            .unwrap_err()
            .code,
        "records.hotfix_fields"
    );
    let unsigned = definition("COLUMNS\nint ID\n\nBUILD 12.1.0.69875\n$noninline,id$ID<u8>\n");
    assert_eq!(
        hotfix::decode_fields(&[], 256, &unsigned, None)
            .unwrap_err()
            .code,
        "records.hotfix_fields"
    );
}
#[test]
fn go_decode_rejects_malformed_payload_and_definition_without_partial_rows() {
    let text = definition("COLUMNS\nstring Text\n\nBUILD 12.1.0.69875\nText\n");
    for raw in [
        &b""[..],
        b"unterminated",
        &[0xff, 0],
        b"a\0\0",
        &[1, 0, 0, 0, b'a'],
    ] {
        assert_eq!(
            hotfix::decode_fields(raw, 0, &text, None).unwrap_err().code,
            "records.hotfix_fields"
        );
    }
    let float = definition("COLUMNS\nfloat Value\n\nBUILD 12.1.0.69875\nValue\n");
    for bits in [0x7f800000u32, 0xff800000, 0x7fc00000] {
        assert_eq!(
            hotfix::decode_fields(&bits.to_le_bytes(), 0, &float, None)
                .unwrap_err()
                .code,
            "records.hotfix_fields"
        );
    }
    assert_eq!(
        hotfix::decode_fields(&[1], 0, &float, None)
            .unwrap_err()
            .code,
        "records.hotfix_fields"
    );
    let array = definition("COLUMNS\nstring Names\n\nBUILD 12.1.0.69875\nNames[2]\n");
    assert_eq!(
        hotfix::decode_fields(b"\0B\0", 0, &array, None).unwrap()["Names"],
        json!(["", "B"])
    );
    for kind in 0..7 {
        let mut invalid = definition(SIMPLE);
        match kind {
            0 => invalid.fields.clear(),
            1 => invalid.fields[1].name = "ID".into(),
            2 => invalid.fields[0].bits = 7,
            3 => invalid.fields[1].elements = 0,
            4 => invalid.fields[1].inline = false,
            5 => invalid.fields[1].identity = true,
            _ => invalid.fields[1].kind = "unknown".into(),
        }
        assert_eq!(
            hotfix::decode_fields(b"ok\0", 1, &invalid, None)
                .unwrap_err()
                .code,
            "records.hotfix_fields"
        );
    }
}
#[test]
fn go_decode_limits_cancellation_and_metered_v8_framing() {
    let mut d = definition("COLUMNS\nstring Text\n\nBUILD 12.1.0.69875\nText\n");
    let mut raw = vec![b'a'; 1 << 20];
    raw.push(0);
    assert!(hotfix::decode_fields(&raw, 0, &d, Some(&budget())).is_ok());
    raw.insert(0, b'a');
    assert_eq!(
        hotfix::decode_fields(&raw, 0, &d, None).unwrap_err().code,
        "records.hotfix_limit"
    );
    d.fields[0].array = true;
    d.fields[0].elements = 65537;
    assert_eq!(
        hotfix::decode_fields(&[], 0, &d, None).unwrap_err().code,
        "records.hotfix_limit"
    );
    let cancelled = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(Arc::new(AtomicBool::new(true))),
    );
    assert_eq!(
        hotfix::decode_fields(&[], 0, &d, Some(&cancelled))
            .unwrap_err()
            .exit_code,
        7
    );
    let raw = cache(8, &[(1, 1, -1, b"a\0")]);
    assert_eq!(
        hotfix::inspect_budgeted(
            &raw,
            CacheFilter {
                build: 69875,
                limit: 1,
                ..Default::default()
            },
            Some(&cancelled)
        )
        .unwrap_err()
        .exit_code,
        7
    );
    let tight = Budget::new(
        Cost {
            decode_work: 0,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    assert_eq!(
        hotfix::inspect_budgeted(
            &raw,
            CacheFilter {
                build: 69875,
                limit: 1,
                ..Default::default()
            },
            Some(&tight)
        )
        .unwrap_err()
        .code,
        "records.query_resource_budget"
    );
}
#[test]
fn go_named_page_states_preserve_physical_rows_and_validated_tail() {
    for version in 1..=9 {
        let invalid = if version >= 7 { 2 } else { 0 };
        let raw = cache(
            version,
            &[
                (42, 1, -3, b"hello\0"),
                (42, invalid, -2, b"invalid bytes"),
                (9, 1, -1, b""),
            ],
        );
        let filter = CacheFilter {
            build: 69875,
            limit: 20,
            ..Default::default()
        };
        let page = hotfix::inspect(&raw, filter.clone()).unwrap();
        let decoded = hotfix::decode_page(&raw, page, &definition(SIMPLE), &budget()).unwrap();
        assert_eq!(decoded.entries.len(), 3);
        assert_eq!(decoded.entries[0].decode_state, "decoded");
        assert_eq!(decoded.entries[0].fields.as_ref().unwrap()["Name"], "hello");
        assert_eq!(decoded.entries[1].decode_state, "not_valid");
        assert!(decoded.entries[1].fields.is_none());
        assert_eq!(decoded.entries[2].decode_state, "no_payload");
        assert!(decoded.entries[2].fields.is_none());
        let mut corrupt = raw;
        corrupt.push(0);
        assert_eq!(
            hotfix::inspect(&corrupt, CacheFilter { limit: 1, ..filter })
                .unwrap_err()
                .code,
            "records.hotfix_format"
        );
    }
    let mut text = vec![b'<'; 1 << 20];
    text.push(0);
    let raw = cache(9, &[(1, 1, 1, &text), (2, 1, 1, &text)]);
    let page = hotfix::inspect(
        &raw,
        CacheFilter {
            build: 69875,
            limit: 20,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        hotfix::decode_page(&raw, page, &definition(SIMPLE), &budget())
            .unwrap_err()
            .code,
        "records.hotfix_limit",
        "Go fields cap counts HTML-escaped JSON"
    );
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    file: PathBuf,
    pin: String,
}
impl Fixture {
    fn new(raw: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-table-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let file = root.join("DBCache.bin");
        fs::write(&file, raw).unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "cn".into(),
                    language: "zhCN".into(),
                    full_build: BUILD.into(),
                    build_config: "b".repeat(32),
                    cdn_config: "c".repeat(32),
                    definition_commit: "a".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let fixture = Self {
            root,
            home,
            file,
            pin,
        };
        fixture.definition(SIMPLE, false);
        fixture
    }
    fn definition(&self, text: &str, collision: bool) {
        let commit = "a".repeat(40);
        let manifest = if collision {
            json!([{"tableName":"Example","tableHash":"0000002A","db2FileDataID":1},{"tableName":"Other","tableHash":"0000002A","db2FileDataID":2}])
        } else {
            json!([{"tableName":"Example","tableHash":"0000002A","db2FileDataID":1}])
        };
        let manifest = serde_json::to_vec(&manifest).unwrap();
        for (path, raw) in [
            ("manifest.json", manifest.as_slice()),
            ("definitions/Example.dbd", text.as_bytes()),
        ] {
            let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/{path}");
            let blob = evidence::publish_blob(&self.home, raw, 4 << 20).unwrap();
            let key = format!("definition/{}", storage::sha256(url.as_bytes()));
            let mut metadata = Metadata::open(&self.home, true).unwrap();
            let generation = metadata.generation(&key).unwrap();
            metadata
                .put(&key, &json!({"url":url,"blob":blob}), generation)
                .unwrap();
        }
    }
    fn inv(&self) -> Invocation {
        Invocation {
            command: "data hotfix".into(),
            positional: None,
            flags: BTreeMap::from([
                ("--source".into(), "dbcache".into()),
                ("--snapshot".into(), self.pin.clone()),
                ("--dbcache".into(), self.file.to_string_lossy().into_owned()),
                ("--table".into(), "Example".into()),
                ("--offline".into(), "true".into()),
            ]),
            home: self.home.clone(),
            cwd: self.root.clone(),
            control: Default::default(),
        }
    }
    fn count(&self, prefix: &str) -> usize {
        Metadata::open(&self.home, false)
            .unwrap()
            .list::<Value>(prefix, 1000)
            .unwrap()
            .len()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn named_hotfix_dispatch_archives_validated_fields_and_paginates_same_changes_pin() {
    let raw = cache(
        9,
        &[
            (42, 1, 1, b"first\0"),
            (43, 1, 1, b"second\0"),
            (44, 2, 1, b"bad"),
            (45, 1, 1, b""),
        ],
    );
    let fixture = Fixture::new(&raw);
    let mut inv = fixture.inv();
    inv.flags.insert("--limit".into(), "2".into());
    let first = lycheedev_data::dispatch(&inv).unwrap();
    assert!(!first.complete);
    assert_eq!(
        first.data["page"]["entries"][0]["fields"],
        json!({"ID":42,"Name":"first"})
    );
    assert_eq!(first.data["page"]["nextIndex"], 1);
    assert_eq!(first.data["sources"]["identity"]["hash"], TABLE);
    assert_eq!(first.data["definition"]["build"], BUILD);
    assert_ne!(first.data["snapshot"], fixture.pin);
    assert_eq!(first.selection["changes"]["sha256"], storage::sha256(&raw));
    assert_eq!(first.provenance[0]["provenance"]["kind"], "hotfix-cache");
    assert_eq!(first.provenance[1]["provenance"]["kind"], "hotfix-records");
    let source = first.data["source"]["id"].as_str().unwrap().to_owned();
    let snapshot = first.data["snapshot"].as_str().unwrap().to_owned();
    inv.flags.remove("--dbcache");
    inv.flags.insert("--from".into(), source);
    inv.flags.insert("--snapshot".into(), snapshot.clone());
    inv.flags.insert("--after-index".into(), "1".into());
    let second = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(second.data["snapshot"], snapshot);
    assert_eq!(
        second.data["page"]["entries"][0]["decodeState"],
        "not_valid"
    );
    assert!(second.data["page"]["entries"][0].get("fields").is_none());
    assert_eq!(
        second.data["page"]["entries"][1]["decodeState"],
        "no_payload"
    );
    assert_eq!(second.data["page"]["truncated"], false);
    assert!(!second.complete);
    assert_eq!(fixture.count("pin/"), 2);
    assert_eq!(fixture.count("capture/"), 3);
    for capture in second.provenance {
        let archived =
            evidence::read_capture(&fixture.home, capture["id"].as_str().unwrap()).unwrap();
        assert_eq!(capture, archived);
        evidence::read_blob(&fixture.home, &capture["blob"], 32 << 20).unwrap();
    }
}
#[test]
fn named_hotfix_failures_never_publish_cache_or_changes_and_recover() {
    let fixture = Fixture::new(&cache(9, &[(42, 1, 1, b"missing terminator")]));
    assert_eq!(
        lycheedev_data::dispatch(&fixture.inv()).unwrap_err().code,
        "records.hotfix_fields"
    );
    assert_eq!(fixture.count("capture/"), 0);
    assert_eq!(fixture.count("pin/"), 1);
    fs::write(&fixture.file, cache(9, &[(42, 1, 1, b"ok\0")])).unwrap();
    fixture.definition(SIMPLE, true);
    assert_eq!(
        lycheedev_data::dispatch(&fixture.inv()).unwrap_err().code,
        "schema.ambiguous_definition"
    );
    fixture.definition(&SIMPLE.replace(BUILD, "12.1.0.69999"), false);
    assert_eq!(
        lycheedev_data::dispatch(&fixture.inv()).unwrap_err().code,
        "schema.definition_not_found"
    );
    assert_eq!(fixture.count("capture/"), 0);
    assert_eq!(fixture.count("pin/"), 1);
    fixture.definition(SIMPLE, false);
    assert!(lycheedev_data::dispatch(&fixture.inv()).unwrap().complete);
    assert_eq!(fixture.count("capture/"), 2);
    assert_eq!(fixture.count("pin/"), 2);
}
#[test]
fn named_hotfix_admission_cancellation_and_legal_unsupported_are_explicit() {
    let fixture = Fixture::new(&cache(7, &[(42, 1, 1, b"ok\0")]));
    for (flag, value, code) in [
        ("--table-hash", "42", "records.hotfix_identity"),
        ("--after-index", "0", "records.hotfix_identity"),
        ("--cursor", "0", "command.invalid_arguments"),
        ("--region-id", "3", "records.hotfix_filter_unsupported"),
        ("--scan", "true", "command.unsupported"),
        ("--encoding", "csv", "command.invalid_arguments"),
    ] {
        let mut inv = fixture.inv();
        inv.flags.insert(flag.into(), value.into());
        assert_eq!(
            lycheedev_data::dispatch(&inv).unwrap_err().code,
            code,
            "{flag}"
        );
    }
    for expired in [false, true] {
        let mut inv = fixture.inv();
        inv.control = OperationControl::new(
            if expired {
                Instant::now() - Duration::from_secs(1)
            } else {
                Instant::now() + Duration::from_secs(30)
            },
            Arc::new(AtomicBool::new(!expired)),
        );
        assert_eq!(
            lycheedev_data::dispatch(&inv).unwrap_err().code,
            if expired {
                "command.deadline"
            } else {
                "command.cancelled"
            }
        );
    }
    assert_eq!(fixture.count("capture/"), 0);
    assert_eq!(fixture.count("pin/"), 1);
    let mut raw = fixture.inv();
    raw.flags.remove("--table");
    raw.flags.insert("--table-hash".into(), "42".into());
    let result = lycheedev_data::dispatch(&raw).unwrap();
    assert!(result.data["page"]["entries"][0].get("fields").is_none());
    assert!(
        result.data["page"]["entries"][0]
            .get("decodeState")
            .is_none()
    );
}

#[test]
fn named_hotfix_fixed_changes_and_archived_context_cannot_be_retargeted() {
    let raw = cache(9, &[(42, 1, 1, b"original\0")]);
    let fixture = Fixture::new(&raw);
    let result = lycheedev_data::dispatch(&fixture.inv()).unwrap();
    let mut fixed = fixture.inv();
    fixed.flags.insert(
        "--snapshot".into(),
        result.data["snapshot"].as_str().unwrap().into(),
    );
    fs::write(&fixture.file, cache(9, &[(42, 1, 1, b"different\0")])).unwrap();
    assert_eq!(
        lycheedev_data::dispatch(&fixed).unwrap_err().code,
        "records.hotfix_identity"
    );
    assert_eq!(fixture.count("capture/"), 2);
    let mut other = selection::load_pinned(&fixture.home, &fixture.pin)
        .unwrap()
        .data
        .clone()
        .unwrap();
    other.region = "us".into();
    let other = selection::pin_selection(
        &fixture.home,
        SelectionSpec {
            data: Some(other),
            ..Default::default()
        },
    )
    .unwrap();
    fixed.flags.remove("--dbcache");
    fixed.flags.insert(
        "--from".into(),
        result.data["source"]["id"].as_str().unwrap().into(),
    );
    fixed.flags.insert("--snapshot".into(), other.id);
    assert_eq!(
        lycheedev_data::dispatch(&fixed).unwrap_err().code,
        "records.hotfix_identity"
    );
    assert_eq!(fixture.count("capture/"), 2);
    let mut directory = fixture.inv();
    directory.flags.insert(
        "--dbcache".into(),
        fixture.root.to_string_lossy().into_owned(),
    );
    assert_eq!(
        lycheedev_data::dispatch(&directory).unwrap_err().code,
        "records.hotfix_format"
    );
    for (flag, value) in [("--limit", "0"), ("--limit", "201"), ("--max-bytes", "0")] {
        let mut invalid = fixture.inv();
        invalid.flags.insert(flag.into(), value.into());
        let error = lycheedev_data::dispatch(&invalid).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.hotfix_limit", 2)
        );
    }
    fs::write(&fixture.file, raw).unwrap();
    fixed.flags.insert(
        "--snapshot".into(),
        result.data["snapshot"].as_str().unwrap().into(),
    );
    assert!(lycheedev_data::dispatch(&fixed).unwrap().complete);
}

#[test]
fn go_integral_float_pages_use_go_encoded_fields_limit() {
    // Go encoding/json: each value is `1`, not serde_json's `1.0`.
    // 32 x 65536 values encode to ~4 MiB, below the 8 MiB aggregate cap.
    let d = definition("COLUMNS\nfloat Values\n\nBUILD 12.1.0.69875\nValues[65536]\n");
    let payload = 1f32.to_le_bytes().repeat(65536);
    let entries: Vec<_> = (0..32).map(|id| (id, 1, 1, payload.as_slice())).collect();
    let raw = cache(9, &entries);
    let page = hotfix::inspect(
        &raw,
        CacheFilter {
            build: 69875,
            limit: 32,
            ..Default::default()
        },
    )
    .unwrap();
    let decoded = hotfix::decode_page(&raw, page, &d, &budget()).unwrap();
    assert_eq!(decoded.entries.len(), 32);
    assert_eq!(
        decoded.entries[31].fields.as_ref().unwrap()["Values"][65535],
        1.0
    );
}

#[test]
fn named_hotfix_above_default_metadata_capacity_remains_valid() {
    // A large unselected physical record is still fully framed and archived;
    // the selected table's exact DBD costs must not steal the input allowance.
    let mut raw = cache(9, &[(1, 1, 1, b"selected\0"), (2, 1, 1, &[])]);
    let second = raw.len() - 32;
    raw[second + 16..second + 20].copy_from_slice(&(TABLE + 1).to_le_bytes());
    let payload_bytes = 129 << 20;
    raw[second + 24..second + 28].copy_from_slice(&(payload_bytes as u32).to_le_bytes());
    raw.resize(raw.len() + payload_bytes, 0);
    let fixture = Fixture::new(&raw);
    drop(raw);
    let mut inv = fixture.inv();
    inv.flags
        .insert("--max-bytes".into(), (130usize << 20).to_string());
    let result = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(result.data["page"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.count("capture/"), 2);
    let mut bounded = fixture.inv();
    bounded
        .flags
        .insert("--max-bytes".into(), (128usize << 20).to_string());
    assert_eq!(
        lycheedev_data::dispatch(&bounded).unwrap_err().code,
        "records.hotfix_limit"
    );
    assert_eq!(fixture.count("capture/"), 2);
}

#[test]
fn named_hotfix_maximum_scalar_schema_page_releases_temporary_allocations() {
    // Fixed Go limits: 4096 fields x 200 records; each decoded object is
    // 40,961 bytes, so 8,192,200 aggregate bytes is legally below 8 MiB.
    let mut dbd = String::from("COLUMNS\n");
    for field in 0..4096 {
        dbd.push_str(&format!("int F{field:04}\n"));
    }
    dbd.push_str("\nBUILD 12.1.0.69875\n");
    for field in 0..4096 {
        dbd.push_str(&format!("F{field:04}<u8>\n"));
    }
    let payload = vec![0; 4096];
    let entries: Vec<_> = (0..200).map(|id| (id, 1, 1, payload.as_slice())).collect();
    let raw = cache(9, &entries);
    assert!(raw.len() < 1 << 20);
    let fixture = Fixture::new(&raw);
    fixture.definition(&dbd, false);
    let mut inv = fixture.inv();
    inv.flags
        .insert("--max-bytes".into(), (1usize << 20).to_string());
    inv.flags.insert("--limit".into(), "200".into());
    let result = lycheedev_data::dispatch(&inv).unwrap();
    let entries = result.data["page"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 200);
    assert_eq!(entries[199]["fields"]["F4095"], 0);
    assert_eq!(entries[199]["fields"].as_object().unwrap().len(), 4096);
    assert_eq!(fixture.count("capture/"), 2);
    assert_eq!(fixture.count("pin/"), 2);
}

#[test]
fn named_hotfix_current_and_legacy_pages_retain_cache_through_archived_continuation() {
    // Fixed Go records/capture_dependencies_test.go::
    // TestHotfixOrdinaryPagesProtectSourceAndContinue, with named DBD decoding.
    for legacy in [false, true] {
        let fixture = Fixture::new(&cache(
            9,
            &[(42, 1, 1, b"first\0"), (43, 1, 1, b"second\0")],
        ));
        let mut inv = fixture.inv();
        inv.flags.insert("--limit".into(), "1".into());
        let first = lycheedev_data::dispatch(&inv).unwrap();
        let source_id = first.data["source"]["id"].as_str().unwrap().to_owned();
        let current = first.provenance[1].clone();
        assert_eq!(current["dependencies"], json!([source_id]));
        let mut owner = current.clone();
        if legacy {
            let bytes = evidence::read_blob(&fixture.home, &current["blob"], 32 << 20).unwrap();
            owner = evidence::commit_capture(
                &fixture.home,
                &bytes,
                "application/json",
                current["provenance"].clone(),
                current["complete"].as_bool().unwrap(),
                current["truncated"].as_bool().unwrap(),
            )
            .unwrap();
            assert!(owner.get("dependencies").is_none());
            evidence::remove_capture(&fixture.home, current["id"].as_str().unwrap()).unwrap();
        }
        let original = evidence::read_blob(&fixture.home, &owner["blob"], 32 << 20).unwrap();
        let error = evidence::remove_capture(&fixture.home, &source_id).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("evidence.capture_referenced", 3)
        );
        inv.flags.remove("--dbcache");
        inv.flags.insert("--from".into(), source_id.clone());
        inv.flags.insert(
            "--snapshot".into(),
            first.data["snapshot"].as_str().unwrap().into(),
        );
        inv.flags.insert(
            "--after-index".into(),
            first.data["page"]["nextIndex"].to_string(),
        );
        let second = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(second.data["source"]["id"], source_id);
        assert_eq!(second.provenance[1]["dependencies"], json!([source_id]));
        assert_eq!(
            second.data["page"]["entries"][0]["fields"]["Name"],
            "second"
        );
        assert_eq!(
            evidence::read_capture(&fixture.home, owner["id"].as_str().unwrap()).unwrap(),
            owner
        );
        assert_eq!(
            evidence::read_blob(&fixture.home, &owner["blob"], 32 << 20).unwrap(),
            original
        );
        evidence::remove_capture(&fixture.home, owner["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            evidence::remove_capture(&fixture.home, &source_id)
                .unwrap_err()
                .code,
            "evidence.capture_referenced"
        );
        evidence::remove_capture(&fixture.home, second.provenance[1]["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            evidence::remove_capture(&fixture.home, &source_id).unwrap()["blobRemoved"],
            true
        );
    }
}
