//! Request-owned keys. Private key bytes are never persisted or printed.
use crate::fault;
use lycheedev_core::{ToolkitError, control::OperationControl};
mod http;
#[cfg(test)]
mod tests;
use lycheedev_core::{Invocation, Result, evidence, storage};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};
pub const PUBLIC_COMMIT: &str = "71b75360752840dd62a412013a38b5a972524a50";
pub const PUBLIC_SHA256: &str = "6d83241ed776f9e74e27ed65d1ec52f1a1464a4e9b16bed497258fdd61020b52";
pub const PUBLIC_BYTES: usize = 983550;
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct KeySource {
    pub kind: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
}
pub struct KeySet {
    keys: BTreeMap<u64, Vec<u8>>,
}
impl fmt::Debug for KeySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("records.KeySet(redacted)")
    }
}
fn invalid() -> lycheedev_core::ToolkitError {
    fault(
        "records.invalid_key_document",
        4,
        "Invalid bounded key document",
    )
}
fn add(keys: &mut BTreeMap<u64, Vec<u8>>, name: &str, value: &str) -> Result<()> {
    if name.len() != 16
        || ![32, 64].contains(&value.len())
        || !name
            .bytes()
            .chain(value.bytes())
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    if keys.len() >= 65536 {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Key count budget exceeded",
        ));
    }
    let id = u64::from_str_radix(name, 16).map_err(|_| invalid())?;
    let raw = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>>>()?;
    if keys.insert(id, raw).is_some() {
        return Err(invalid());
    }
    Ok(())
}
struct JsonKeys;
impl<'de> Visitor<'de> for JsonKeys {
    type Value = BTreeMap<u64, Vec<u8>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string-to-string key object")
    }
    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let mut keys = BTreeMap::new();
        while let Some((name, value)) = map.next_entry::<String, String>()? {
            add(&mut keys, &name, &value).map_err(|_| de::Error::custom("invalid key entry"))?;
        }
        Ok(keys)
    }
}
impl KeySet {
    pub fn parse(raw: &[u8], format: &str, expected: &str) -> Result<Self> {
        Self::parse_checked(raw, format, expected, || Ok(()))
    }
    fn parse_checked(raw: &[u8], format: &str, expected: &str,
        mut check: impl FnMut() -> Result<()>) -> Result<Self> {
        check()?;
        if raw.len() > 4 << 20 {
            return Err(fault(
                "records.metadata_limit",
                3,
                "Key document byte budget exceeded",
            ));
        }
        if !lycheedev_core::selection::valid_digest(expected, 64)
            || storage::sha256(raw) != expected
        {
            return Err(fault(
                "container.integrity",
                4,
                "Key document digest differs",
            ));
        }
        check()?;
        let keys = match format {
            "text" => {
                let mut keys = BTreeMap::new();
                for line in raw.split(|b| *b == b'\n') {
                    check()?;
                    if line.len() > 65536 {
                        return Err(invalid());
                    }
                    let text = std::str::from_utf8(line).map_err(|_| invalid())?.trim();
                    if text.is_empty() || text.starts_with('#') {
                        continue;
                    }
                    let fields = text.split_whitespace().collect::<Vec<_>>();
                    if fields.len() != 2 {
                        return Err(invalid());
                    }
                    add(&mut keys, fields[0], fields[1])?;
                }
                keys
            }
            "json" => {
                let mut parser = serde_json::Deserializer::from_slice(raw);
                let keys = serde::de::Deserializer::deserialize_map(&mut parser, JsonKeys)
                    .map_err(|_| invalid())?;
                parser.end().map_err(|_| invalid())?;
                keys
            }
            _ => return Err(invalid()),
        };
        if keys.is_empty() {
            return Err(invalid());
        }
        check()?;
        Ok(Self { keys })
    }
    pub fn keys(&self) -> &BTreeMap<u64, Vec<u8>> {
        &self.keys
    }
}
pub struct Context {
    pub source: KeySource,
    keys: BTreeMap<u64, Vec<u8>>,
    home: PathBuf,
    offline: bool,
    deadline: Instant,
    control: OperationControl,
    public_result: Option<Result<()>>,
    budget: Option<std::sync::Arc<crate::resource::Budget>>,
}
impl Context {
    pub fn prepare(inv: &Invocation, deadline: Instant) -> Result<Self> {
        Self::prepare_budgeted(inv, deadline, None)
    }
    pub fn prepare_budgeted(
        inv: &Invocation,
        deadline: Instant,
        budget: Option<std::sync::Arc<crate::resource::Budget>>,
    ) -> Result<Self> {
        Self::prepare_scoped(&inv.home, inv.flag("--key-file"), inv.flag_bool("--offline"),
            &inv.control, deadline, budget)
    }
    pub(crate) fn prepare_scoped(
        home: &Path,
        key_file: Option<&str>,
        offline: bool,
        control: &OperationControl,
        deadline: Instant,
        budget: Option<std::sync::Arc<crate::resource::Budget>>,
    ) -> Result<Self> {
        control.check()?;
        let mut context = Self {
            source: KeySource {
                kind: "public".into(),
                state: "not_needed".into(),
                commit: PUBLIC_COMMIT.into(),
                sha256: PUBLIC_SHA256.into(),
            },
            keys: BTreeMap::new(),
            home: home.to_owned(),
            offline,
            deadline,
            control: control.clone(),
            public_result: None,
            budget,
        };
        if let Some(path) = key_file {
            let file = std::fs::File::open(path).map_err(|_| invalid())?;
            let metadata = file.metadata().map_err(|_| invalid())?;
            if !metadata.is_file() || metadata.len() > 4 << 20 {
                return Err(invalid());
            }
            let mut raw = Vec::new();
            if let Some(budget) = &context.budget {
                budget.charge(crate::resource::Cost::metadata(metadata.len() as usize * 8))?;
            }
            file.take((4 << 20) + 1)
                .read_to_end(&mut raw)
                .map_err(|_| invalid())?;
            if raw.len() as u64 != metadata.len() {
                return Err(invalid());
            }
            let sha256 = storage::sha256(&raw);
            let format = if Path::new(path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                "json"
            } else {
                "text"
            };
            context.keys = KeySet::parse(&raw, format, &sha256)?.keys;
            context.source = KeySource {
                kind: "file".into(),
                state: "loaded".into(),
                commit: String::new(),
                sha256,
            };
        }
        Ok(context)
    }
    pub fn keys(&self) -> &BTreeMap<u64, Vec<u8>> {
        &self.keys
    }
    pub fn ensure_public(&mut self) -> Result<()> {
        self.ensure_public_with(&http::PublicFetcher)
    }
    fn check(&self) -> Result<()> {
        self.control.check()?;
        if let Some(budget) = &self.budget { budget.check()?; }
        if Instant::now() >= self.deadline {
            return Err(fault("records.query_resource_budget", 3, "Data query deadline exceeded"));
        }
        Ok(())
    }
    fn ensure_public_with(&mut self, fetcher: &impl http::Fetcher) -> Result<()> {
        if self.source.kind != "public" { return Ok(()); }
        if let Some(result) = &self.public_result { return result.clone(); }
        let result = self.load_public(fetcher);
        self.source.state = if result.is_ok() { "loaded" }
            else if self.source.state == "unavailable_offline" { "unavailable_offline" }
            else { "unavailable" }.into();
        self.public_result = Some(result.clone());
        result
    }
    fn load_public(&mut self, fetcher: &impl http::Fetcher) -> Result<()> {
        self.check()?;
        let blob = json!({"sha256":PUBLIC_SHA256,"bytes":PUBLIC_BYTES});
        if let Some(budget) = &self.budget {
            budget.charge(crate::resource::Cost {
                decode_work: PUBLIC_BYTES.div_ceil(65536) as u64,
                ..crate::resource::Cost::metadata(PUBLIC_BYTES * 8 + 65536)
            })?;
        }
        let path = evidence::blob_path(&self.home, PUBLIC_SHA256)?;
        storage::guard_path(&self.home, &path)?;
        self.check()?;
        let raw = if path.try_exists()? {
            evidence::read_blob_bounded_with_check(&self.home, &blob, 4 << 20,
                &self.control, || self.check())?
        } else {
            if self.offline {
                self.source.state = "unavailable_offline".into();
                return Err(fault("container.key_unavailable", 3,
                    "Pinned public keys are unavailable offline"));
            }
            let raw = fetcher.fetch(&self.control, self.budget.as_deref(), self.deadline)?;
            self.check()?;
            if raw.len() != PUBLIC_BYTES || storage::sha256(&raw) != PUBLIC_SHA256 {
                return Err(fault("container.integrity", 4, "Pinned public key bytes differ"));
            }
            evidence::publish_blob_bounded_with_check(&self.home, &raw, 4 << 20,
                &self.control, || self.check())?;
            raw
        };
        self.check()?;
        let keys = KeySet::parse_checked(&raw, "text", PUBLIC_SHA256, || self.check())?;
        self.check()?;
        self.keys = keys.keys;
        Ok(())
    }
    pub fn value(&self) -> Value {
        serde_json::to_value(&self.source).expect("serializable source identity")
    }
}
