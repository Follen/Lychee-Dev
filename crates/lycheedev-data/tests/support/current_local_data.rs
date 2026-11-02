//! Per-run, independently produced Go oracle for the selected installed client.
#![allow(dead_code)]
#[path = "data_cache.rs"]
mod data_cache;
use lycheedev_core::{
    evidence,
    selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::{definitions::Manifest, installed};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

// Same exact digits/exponent rule as numeric_json in compound/cte/aggregate
// parity. Numeric notation may differ; sign (including -0), integers and all
// nonzero decimal digits remain exact. Never compare through f64 or an epsilon.
fn numeric_key(number: &serde_json::Number) -> (bool, String, i32) {
    let text = number.to_string();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text.as_str(), 0i32), |(m, e)| (m, e.parse().unwrap()));
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len()) as i32;
    let mut digits = mantissa
        .replace('.', "")
        .trim_start_matches('0')
        .to_string();
    let mut power = exponent - fraction;
    while digits.ends_with('0') {
        digits.pop();
        power += 1;
    }
    if digits.is_empty() {
        digits = "0".into();
        power = 0;
    }
    (negative, digits, power)
}
pub fn json_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => numeric_key(a) == numeric_key(b),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| json_equal(a, b)))
        }
        _ => actual == expected,
    }
}

// serde_json's arbitrary_precision parser still parses the integer token -0 as
// i64(0). Keep that sign at this oracle-reading boundary, before it reaches Value.
// RawValue borrows the authenticated original tokens; other numbers retain the
// ordinary arbitrary-precision path, without an intermediate f64 conversion.
pub fn parse_json(bytes: &[u8]) -> Value {
    fn value(raw: &serde_json::value::RawValue) -> Value {
        let text = raw.get();
        match text.as_bytes()[0] {
            b'[' => {
                let items: Vec<&serde_json::value::RawValue> = serde_json::from_str(text).unwrap();
                Value::Array(items.into_iter().map(value).collect())
            }
            b'{' => {
                let items: BTreeMap<String, &serde_json::value::RawValue> =
                    serde_json::from_str(text).unwrap();
                Value::Object(
                    items
                        .into_iter()
                        .map(|(key, raw)| (key, value(raw)))
                        .collect(),
                )
            }
            _ if text == "-0" => Value::Number(serde_json::Number::from_f64(-0.0).unwrap()),
            _ => serde_json::from_str(text).unwrap(),
        }
    }
    value(serde_json::from_slice(bytes).unwrap())
}

pub struct Fixture {
    pub document: Value,
    pub pin: DataPin,
    pub installation: PathBuf,
    directory: PathBuf,
}
impl Fixture {
    pub fn load() -> Self {
        let path = PathBuf::from(
            std::env::var("LYCHEEDEV_RUST_CURRENT_DATA_FIXTURE")
                .expect("run the current installed-data Go oracle producer first"),
        );
        let document = parse_json(&std::fs::read(&path).unwrap());
        assert_eq!(document["schema"], "lycheedev.local-data-go-oracle.v1");
        assert_eq!(
            document["oracleCommit"],
            "29bdbdaff5f064ec212ff2064380b1ccac652eda"
        );
        let pin: DataPin = serde_json::from_value(document["pin"]["data"].clone()).unwrap();
        let installation = PathBuf::from(document["installation"].as_str().unwrap());
        let fixture = Self {
            document,
            pin,
            installation,
            directory: path.parent().unwrap().into(),
        };
        let _ = fixture.bytes(&fixture.document["buildInfo"]);
        assert_eq!(
            fixture.document["definitions"]["commit"],
            fixture.pin.definition_commit
        );
        fixture
    }
    pub fn bytes(&self, descriptor: &Value) -> Vec<u8> {
        let path = self.directory.join(descriptor["path"].as_str().unwrap());
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(
            raw.len() as u64,
            descriptor["bytes"].as_u64().unwrap(),
            "{}",
            path.display()
        );
        assert_eq!(
            storage::sha256(&raw),
            descriptor["sha256"],
            "{}",
            path.display()
        );
        raw
    }
    pub fn json(&self, descriptor: &Value) -> Value {
        parse_json(&self.bytes(descriptor))
    }
    pub fn oracle(&self, name: &str) -> Value {
        self.json(&self.document["expected"][name])
    }
    pub fn rows(&self) -> Value {
        let rows = self.json(&self.document["map"]["rows"]);
        assert_eq!(
            rows["rows"].as_array().unwrap().len() as u64,
            self.document["map"]["rows"]["count"].as_u64().unwrap()
        );
        assert_eq!(
            rows["ids"].as_array().unwrap().len(),
            rows["rows"].as_array().unwrap().len()
        );
        rows
    }
    pub fn record_id(&self) -> String {
        self.document["probes"]["recordId"]
            .as_u64()
            .unwrap()
            .to_string()
    }
    pub fn table_identity(&self) -> lycheedev_data::definitions::TableIdentity {
        Manifest::parse(&self.bytes(&self.document["definitions"]["manifest"]))
            .unwrap()
            .lookup("Map")
            .unwrap()
    }
    pub fn resolve(&self) -> installed::ResolvedLocalTarget {
        let resolved = installed::resolve_target(&installed::LocalTargetRequest {
            installation: self.installation.clone(),
            product: Some(self.pin.product.clone()),
            full_build: Some(self.pin.full_build.clone()),
            region: self.pin.region.clone(),
            language: self.pin.language.clone(),
            definition_commit: self.pin.definition_commit.clone(),
        })
        .unwrap();
        assert_eq!(
            resolved.data, self.pin,
            "the installed client changed after Go input capture"
        );
        resolved
    }
    pub fn initialize(&self, home: &Path) -> String {
        assert!(
            !home.exists(),
            "CLI proof home must be a fresh output workspace"
        );
        workspace::initialize(home).unwrap();
        let resolved = self.resolve();
        let pin = selection::pin_selection(
            home,
            SelectionSpec {
                data: Some(resolved.data),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pin.id, self.document["pin"]["id"]);
        let metadata = data_cache::Cache::new(home);
        for (resource, descriptor) in [
            ("manifest.json", &self.document["definitions"]["manifest"]),
            ("definitions/Map.dbd", &self.document["definitions"]["map"]),
        ] {
            let raw = self.bytes(descriptor);
            let url = format!(
                "https://raw.githubusercontent.com/wowdev/WoWDBDefs/{}/{resource}",
                self.pin.definition_commit
            );
            let blob = evidence::publish_blob(home, &raw, 4 << 20).unwrap();
            metadata
                .put(
                    &format!("definition/{}", storage::sha256(url.as_bytes())),
                    &json!({"url":url,"blob":blob}),
                    None,
                )
                .unwrap();
        }
        pin.id
    }
    pub fn context(&self, home: &Path, pin: &str) -> lycheedev_data::request::DataContext {
        lycheedev_data::request::DataContext {
            home: home.into(),
            cwd: home.into(),
            control: Default::default(),
            source: lycheedev_data::request::SourceOptions {
                snapshot: Some(pin.into()),
                installation: Some(self.installation.clone()),
                offline: true,
                ..Default::default()
            },
        }
    }
    pub fn assert_json_equal(&self, actual: &Value, expected: &Value, context: &str) {
        assert!(
            json_equal(actual, expected),
            "{context}: JSON values differ"
        );
    }
    pub fn locale_mask(&self) -> u32 {
        match self.pin.language.as_str() {
            "enUS" => 2,
            "koKR" => 4,
            "frFR" => 0x10,
            "deDE" => 0x20,
            "zhCN" => 0x40,
            "esES" => 0x80,
            "zhTW" => 0x100,
            "enGB" => 0x200,
            "esMX" => 0x1000,
            "ruRU" => 0x2000,
            "ptBR" => 0x4000,
            "itIT" => 0x8000,
            "ptPT" => 0x10000,
            other => panic!("unsupported fixture locale {other}"),
        }
    }
}
