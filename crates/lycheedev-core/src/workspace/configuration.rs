//! Strict Data admission configuration retains fixed-Go decoding semantics.
//! Only the new Data policy decoder is also shared by historical config APIs.
use crate::{Result, ToolkitError, control::OperationControl, storage};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{fmt, fs, io::Read, path::Path};

const MAXIMUM: usize = 65536;
pub const DEFAULT_DATA_CACHE_MAX_BYTES: u64 = 5_000_000_000;
const MAX_CACHE_BYTES: u64 = 1 << 45;

/// Effective strict Data-admission values. Historical config APIs retain their
/// original Value validation/unknown-field behavior and share only Data policy.
/// A missing file gets defaults. Omitted old fields in a present document keep
/// Go's zero values; only the new Data policy defaults when absent or null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub cache_max_bytes: u64,
    pub download_workers: usize,
    pub data_cache_max_bytes: u64,
}
impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            cache_max_bytes: 20 << 30,
            download_workers: 0,
            data_cache_max_bytes: DEFAULT_DATA_CACHE_MAX_BYTES,
        }
    }
}
impl WorkspaceConfig {
    pub fn to_value(self) -> serde_json::Value {
        serde_json::json!({
            "schema":"lycheedev.config.v1",
            "cache":{"maxBytes":self.cache_max_bytes},
            "download":{"workers":self.download_workers},
            "dataCache":{"maxBytes":self.data_cache_max_bytes},
        })
    }
}

pub(super) fn data_policy_from_raw(raw: &[u8]) -> Result<u64> {
    let fields: Object = serde_json::from_slice(raw).map_err(|_| invalid())?;
    let mut value = DEFAULT_DATA_CACHE_MAX_BYTES;
    for (name, raw) in fields.0 {
        if folded(&name) == "datacache" {
            merge_data_policy(&raw, &mut value)?;
        }
    }
    Ok(value)
}
pub(super) fn is_data_policy_key(key: &str) -> bool {
    folded(key) == "datacache"
}
fn merge_data_policy(raw: &RawValue, value: &mut u64) -> Result<()> {
    for (name, raw) in object(raw)?.0 {
        if folded(&name) != "maxbytes" {
            return Err(invalid());
        }
        data_bytes(&raw, value)?;
    }
    Ok(())
}

fn data_bytes(raw: &RawValue, place: &mut u64) -> Result<()> {
    if raw.get() == "null" {
        return Ok(());
    }
    let bytes = if raw.get().starts_with('"') {
        let text: String = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
        let text = text.trim();
        let digits = text.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return Err(invalid());
        }
        let number: u64 = text[..digits].parse().map_err(|_| invalid())?;
        let unit = text[digits..].trim().to_ascii_lowercase();
        let scale = match unit.as_str() {
            "b" => 1,
            "kb" => 1_000,
            "mb" => 1_000_000,
            "gb" => 1_000_000_000,
            "tb" => 1_000_000_000_000,
            "kib" => 1 << 10,
            "mib" => 1 << 20,
            "gib" => 1 << 30,
            "tib" => 1 << 40,
            _ => return Err(invalid()),
        };
        number.checked_mul(scale).ok_or_else(invalid)?
    } else {
        let mut integer_value = 0i64;
        integer(raw, &mut integer_value)?;
        u64::try_from(integer_value).map_err(|_| invalid())?
    };
    if bytes > MAX_CACHE_BYTES {
        return Err(invalid());
    }
    *place = bytes;
    Ok(())
}
fn invalid() -> ToolkitError {
    ToolkitError::new("vault.config_format", "vault.config_format", 4)
}
fn newer(schema: &str) -> bool {
    let Some(digits) = schema.strip_prefix("lycheedev.config.v") else {
        return false;
    };
    if digits.is_empty() {
        return false;
    }
    let mut version = 0i64;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return false;
        }
        version = version.wrapping_mul(10).wrapping_add(i64::from(b - b'0'));
    }
    version > 1
}
fn folded(key: &str) -> String {
    key.chars()
        .map(|ch| match ch {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}
#[derive(Default)]
struct Object(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Map;
        impl<'de> Visitor<'de> for Map {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Object, E> {
                Ok(Object::default())
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Object, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(Object(fields))
            }
        }
        d.deserialize_any(Map)
    }
}
fn scalar<'de, T: Deserialize<'de>>(raw: &'de RawValue, place: &mut T) -> Result<()> {
    if let Some(value) = serde_json::from_str::<Option<T>>(raw.get()).map_err(|_| invalid())? {
        *place = value;
    }
    Ok(())
}
fn integer(raw: &RawValue, place: &mut i64) -> Result<()> {
    let token = raw.get();
    if token == "null" {
        return Ok(());
    }
    let digits = token.strip_prefix('-').unwrap_or(token);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    *place = token.parse().map_err(|_| invalid())?;
    Ok(())
}
fn object(raw: &RawValue) -> Result<Object> {
    serde_json::from_str(raw.get()).map_err(|_| invalid())
}
fn decode(
    raw: &[u8],
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<WorkspaceConfig> {
    control.check()?;
    check()?;
    if raw.len() > MAXIMUM || std::str::from_utf8(raw).is_err() || !crate::raw_json::is_valid(raw) {
        return Err(invalid());
    }
    // Bounded original-byte grammar pass precedes decoding; no synthetic
    // 128-depth cap on ignored schema-peek fields.
    control.check()?;
    check()?;
    let fields: Object = serde_json::from_slice(raw).map_err(|_| invalid())?;
    let mut schema = String::new();
    for (name, value) in &fields.0 {
        control.check()?;
        check()?;
        if folded(name) == "schema" {
            scalar(value, &mut schema)?;
        }
    }
    if schema != "lycheedev.config.v1" {
        return Err(if newer(&schema) {
            ToolkitError::new("vault.config_schema_newer", "vault.config_schema_newer", 4)
        } else {
            invalid()
        });
    }
    let (mut cache_bytes, mut workers) = (0i64, 0i64);
    let mut data_cache_max_bytes = DEFAULT_DATA_CACHE_MAX_BYTES;
    for (name, raw) in &fields.0 {
        control.check()?;
        check()?;
        match folded(name).as_str() {
            "schema" => (),
            "cache" => {
                for (name, value) in object(raw)?.0 {
                    if folded(&name) != "maxbytes" {
                        return Err(invalid());
                    }
                    integer(&value, &mut cache_bytes)?;
                }
            }
            "datacache" => merge_data_policy(raw, &mut data_cache_max_bytes)?,
            "download" => {
                for (name, value) in object(raw)?.0 {
                    if folded(&name) != "workers" {
                        return Err(invalid());
                    }
                    integer(&value, &mut workers)?;
                }
            }
            _ => return Err(invalid()),
        }
    }
    if !(0..=(1i64 << 45)).contains(&cache_bytes) || !(0..=256).contains(&workers) {
        return Err(invalid());
    }
    control.check()?;
    check()?;
    Ok(WorkspaceConfig {
        cache_max_bytes: cache_bytes as u64,
        download_workers: workers as usize,
        data_cache_max_bytes,
    })
}
pub(super) fn read(
    home: &Path,
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<WorkspaceConfig> {
    control.check()?;
    check()?;
    crate::workspace::open_controlled_with_check(home, control, &mut *check)?;
    control.check()?;
    check()?;
    let path = home.join("config.json");
    let info = match fs::symlink_metadata(&path) {
        Ok(info) => info,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            check()?;
            return Ok(WorkspaceConfig::default());
        }
        Err(error) => return Err(ToolkitError::io(error)),
    };
    if !info.file_type().is_file() || info.len() > MAXIMUM as u64 {
        return Err(invalid());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3);
    }
    let mut file = options.open(path)?;
    let result = (|| {
        let mut bytes = Vec::with_capacity(info.len() as usize);
        let mut block = [0; 4096];
        loop {
            control.check()?;
            check()?;
            let amount = block.len().min(MAXIMUM + 1 - bytes.len());
            let n = file.read(&mut block[..amount])?;
            bytes.extend_from_slice(&block[..n]);
            if bytes.len() > MAXIMUM {
                return Err(invalid());
            }
            if n == 0 {
                break;
            }
        }
        decode(&bytes, control, check)
    })();
    let result = storage::finish_file(file, result, control);
    super::data_resources::finish(result, [], control.check(), check())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(raw: &[u8]) -> Result<usize> {
        decode(raw, &OperationControl::default(), &mut || Ok(())).map(|c| c.download_workers)
    }
    #[test]
    fn configuration_duplicate_null_case_and_nested_merge_match_wire_contract() {
        assert_eq!(parse(br#"{"Schema":"lycheedev.config.v1","cache":null,"download":{"workers":2},"DOWNLOAD":null,"Download":{"Workers":3},"download":{"workers":null}}"#).unwrap(),3);
        assert_eq!(parse(br#"{"schema":"lycheedev.config.v1"}"#).unwrap(), 0);
        assert_eq!(
            parse(br#"{"schema":"lycheedev.config.v1","download":{"workers":-0}}"#).unwrap(),
            0
        );
        assert_eq!(
            parse("{\"ſchema\":\"lycheedev.config.v1\",\"download\":{\"worKers\":8}}".as_bytes())
                .unwrap(),
            8
        );
    }
    #[test]
    fn configuration_rejects_unknown_shapes_tokens_and_bounds() {
        for tail in [
            r#", "future":0"#,
            r#", "cache":{"other":0}"#,
            r#", "download":{"Workers":1e0}"#,
            r#", "download":{"workers":-1}"#,
            r#", "download":{"workers":257}"#,
            r#", "cache":{"maxBytes":35184372088833}"#,
            r#", "download":[]"#,
        ] {
            let raw = format!(r#"{{"schema":"lycheedev.config.v1"{tail}}}"#);
            assert_eq!(
                parse(raw.as_bytes()).unwrap_err().code,
                "vault.config_format"
            );
        }
        for raw in [
            b"null".as_slice(),
            b"{}",
            b"[]",
            br#"{"schema":"lycheedev.config.v1"}{}"#,
            b"{\"schema\":\"\xff\"}",
        ] {
            assert_eq!(parse(raw).unwrap_err().code, "vault.config_format");
        }
    }
    #[test]
    fn schema_peek_newer_precedes_unknown_fields_and_uses_wire_depth() {
        assert_eq!(
            parse(br#"{"unknown":1,"schema":"lycheedev.config.v2"}"#)
                .unwrap_err()
                .code,
            "vault.config_schema_newer"
        );
        for (depth, code) in [
            (9999, "vault.config_schema_newer"),
            (10000, "vault.config_format"),
        ] {
            let raw = format!(
                "{{\"schema\":\"lycheedev.config.v2\",\"unknown\":{}0{}}}",
                "[".repeat(depth),
                "]".repeat(depth)
            );
            assert_eq!(parse(raw.as_bytes()).unwrap_err().code, code);
        }
        assert_eq!(
            parse(br#"{"schema":"lycheedev.config.v18446744073709551618"}"#)
                .unwrap_err()
                .code,
            "vault.config_schema_newer"
        );
        assert_eq!(
            parse(br#"{"schema":"lycheedev.config.v18446744073709551617"}"#)
                .unwrap_err()
                .code,
            "vault.config_format"
        );
        assert_eq!(
            parse(br#"{"schema":"lycheedev.config.v18446744073709551616"}"#)
                .unwrap_err()
                .code,
            "vault.config_format"
        );
    }
}

#[cfg(test)]
mod data_policy_tests {
    use super::*;
    use crate::workspace::{
        self,
        data_resources::{Policy, acquire},
    };
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    fn parse(value: &str) -> Result<WorkspaceConfig> {
        let raw =
            format!(r#"{{"schema":"lycheedev.config.v1","dataCache":{{"maxBytes":{value}}}}}"#);
        decode(raw.as_bytes(), &OperationControl::default(), &mut || Ok(()))
    }
    #[test]
    fn exact_decimal_default_does_not_change_legacy_cache_default_or_present_zeros() {
        assert_eq!(
            WorkspaceConfig::default().data_cache_max_bytes,
            5_000_000_000
        );
        assert_eq!(WorkspaceConfig::default().cache_max_bytes, 20u64 << 30);
        for raw in [
            r#"{"schema":"lycheedev.config.v1"}"#,
            r#"{"schema":"lycheedev.config.v1","dataCache":null}"#,
            r#"{"schema":"lycheedev.config.v1","dataCache":{"maxBytes":null}}"#,
        ] {
            let config =
                decode(raw.as_bytes(), &OperationControl::default(), &mut || Ok(())).unwrap();
            assert_eq!(config.data_cache_max_bytes, 5_000_000_000);
            assert_eq!(config.cache_max_bytes, 0); // Fixed reference present-file default.
            assert_eq!(config.download_workers, 0);
        }
    }
    #[test]
    fn numeric_bytes_and_explicit_decimal_binary_units_have_exact_values() {
        for (value, expected) in [
            ("5000000000", 5_000_000_000),
            (r#""5GB""#, 5_000_000_000),
            (r#""5 gb""#, 5_000_000_000),
            (r#""5GiB""#, 5u64 << 30),
            (r#""8B""#, 8),
            (r#""2KB""#, 2_000),
            (r#""2KiB""#, 2 << 10),
            (r#""2MB""#, 2_000_000),
            (r#""2MiB""#, 2 << 20),
            (r#""2TB""#, 2_000_000_000_000),
            (r#""2TiB""#, 2u64 << 40),
            ("0", 0),
            ("-0", 0),
            (r#""0 B""#, 0),
            ("35184372088832", 1u64 << 45),
        ] {
            assert_eq!(
                parse(value).unwrap().data_cache_max_bytes,
                expected,
                "{value}"
            );
        }
    }
    #[test]
    fn invalid_units_shape_fraction_sign_range_and_overflow_fail_closed() {
        for value in [
            r#""5""#,
            r#""5G""#,
            r#""5.0GB""#,
            r#""5e0GB""#,
            r#""+5GB""#,
            r#""-1GB""#,
            r#""five GB""#,
            r#""5GB extra""#,
            r#""5 bytes""#,
            r#""18446744073709551616B""#,
            r#""18446744073709551615GB""#,
            "-1",
            "1.0",
            "1e0",
            "true",
            "[]",
            "{}",
            "35184372088833",
            "18446744073709551615",
        ] {
            assert_eq!(
                parse(value).unwrap_err().code,
                "vault.config_format",
                "{value}"
            );
        }
    }
    #[test]
    fn new_field_uses_existing_case_duplicate_null_merge_rules_without_relaxing_old_units() {
        let raw = br#"{"SCHEMA":"lycheedev.config.v1","DATACACHE":{"MAXBYTES":"5GB"},"dataCache":null,"DataCache":{"maxBytes":"3GiB"},"datacache":{"maxbytes":null}}"#;
        assert_eq!(
            decode(raw, &OperationControl::default(), &mut || Ok(()))
                .unwrap()
                .data_cache_max_bytes,
            3u64 << 30
        );
        for raw in [
            br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":"5GB"}}"#.as_slice(),
            br#"{"schema":"lycheedev.config.v1","dataCache":{"unknown":1}}"#,
            br#"{"schema":"lycheedev.config.v1","dataCache":[]}"#,
            br#"{"schema":"lycheedev.config.v1","dataCache":{"maxBytes":"bad","maxBytes":1}}"#,
        ] {
            assert_eq!(
                decode(raw, &OperationControl::default(), &mut || Ok(()))
                    .unwrap_err()
                    .code,
                "vault.config_format"
            );
        }
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lychee-data-config-{}-{}-{}",
                std::process::id(),
                storage::sha256(storage::utc_now().as_bytes()),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            workspace::initialize(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn display_controlled_legacy_cache_and_data_admission_accept_identical_config() {
        let f = Fixture::new();
        let raw = br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":32},"download":{"workers":3},"DataCache":{"MaxBytes":"5GB"}}"#;
        fs::write(f.0.join("config.json"), raw).unwrap();
        let control = OperationControl::default();
        let typed = workspace::config_controlled(&f.0, &control).unwrap();
        assert_eq!(workspace::config(&f.0).unwrap(), typed.to_value());
        let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
        assert_eq!(resources.data_cache_max_bytes(), typed.data_cache_max_bytes);
        assert_eq!(resources.download_workers(), 1);
        let blob = crate::cache::publish_derived_controlled(&f.0, b"body", 4, &control).unwrap();
        assert_eq!(blob["bytes"], 4);
        let owner = resources
            .reserve_derived_cache(super::super::data_disk::AdditionalPeak::default())
            .unwrap();
        assert_eq!(
            owner.report().configured_max_bytes,
            typed.data_cache_max_bytes
        );
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        assert_eq!(fs::read(f.0.join("config.json")).unwrap(), raw);
    }
    #[test]
    fn historical_complete_configuration_and_missing_file_keep_effective_values() {
        let f = Fixture::new();
        let control = OperationControl::default();
        assert_eq!(
            workspace::config_controlled(&f.0, &control).unwrap(),
            WorkspaceConfig::default()
        );
        assert!(!f.0.join("config.json").exists());
        fs::write(f.0.join("config.json"), br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":21474836480},"download":{"workers":8}}"#).unwrap();
        let config = workspace::config_controlled(&f.0, &control).unwrap();
        assert_eq!(config.cache_max_bytes, 20u64 << 30);
        assert_eq!(config.download_workers, 8);
        assert_eq!(config.data_cache_max_bytes, 5_000_000_000);
    }
    #[test]
    fn all_config_consumers_reject_same_invalid_data_policy_without_rewriting() {
        let f = Fixture::new();
        let raw = br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":32},"download":{"workers":0},"dataCache":{"maxBytes":"5GiBB"}}"#;
        fs::write(f.0.join("config.json"), raw).unwrap();
        let control = OperationControl::default();
        assert_eq!(
            workspace::config(&f.0).unwrap_err().code,
            "vault.config_format"
        );
        assert_eq!(
            workspace::config_controlled(&f.0, &control)
                .unwrap_err()
                .code,
            "vault.config_format"
        );
        assert_eq!(
            acquire(&f.0, Policy::Ordinary, &control)
                .err()
                .unwrap()
                .code,
            "vault.config_format"
        );
        assert_eq!(
            crate::cache::publish_derived_controlled(&f.0, b"body", 4, &control)
                .unwrap_err()
                .code,
            "vault.config_format"
        );
        assert_eq!(fs::read(f.0.join("config.json")).unwrap(), raw);
    }
}

#[cfg(test)]
mod historical_public_tests {
    use super::*;
    use crate::workspace;
    #[test]
    fn original_public_missing_null_case_and_schema_errors_remain_unchanged() {
        for raw in [
            br#"{"schema":"lycheedev.config.v1"}"#.as_slice(),
            br#"{"schema":"lycheedev.config.v1","cache":null,"download":{"workers":0}}"#,
            br#"{"schema":"lycheedev.config.v1","Cache":{"maxBytes":1},"download":{"workers":0}}"#,
        ] {
            assert_eq!(
                workspace::legacy_configuration_value(Some(raw))
                    .unwrap_err()
                    .code,
                "vault.config_format"
            );
            // The historical Data decoder already accepted these spellings.
            assert!(decode(raw, &OperationControl::default(), &mut || Ok(())).is_ok());
        }
        let error = workspace::legacy_configuration_value(Some(br#"{"schema":"not-a-version"}"#))
            .unwrap_err();
        assert_eq!(error.code, "vault.config_schema_newer");
        assert_eq!(error.message, "unsupported configuration schema");
    }
    #[test]
    fn original_unknown_fields_survive_while_only_new_data_field_is_normalized() {
        let raw = br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":1,"custom":"keep"},"download":{"workers":0},"future":{"nested":[1,true]},"dataCache":{"maxBytes":"2GB"},"DataCache":{"MaxBytes":"3GiB"},"DATACACHE":null}"#;
        let value = workspace::legacy_configuration_value(Some(raw)).unwrap();
        assert_eq!(value["cache"]["custom"], "keep");
        assert_eq!(value["future"], serde_json::json!({"nested":[1,true]}));
        assert_eq!(value["dataCache"]["maxBytes"], 3u64 << 30);
        assert!(value.get("DataCache").is_none());
        assert!(value.get("DATACACHE").is_none());
        let rewritten = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            workspace::legacy_configuration_value(Some(&rewritten)).unwrap(),
            value
        );
        assert_eq!(
            decode(raw, &OperationControl::default(), &mut || Ok(()))
                .unwrap_err()
                .code,
            "vault.config_format"
        );
    }
    #[test]
    fn new_policy_duplicate_null_units_match_on_legacy_legal_documents() {
        let raw = br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":1},"download":{"workers":0},"dataCache":{"maxBytes":"2GB","maxBytes":null},"DataCache":null,"datacache":{"MAXBYTES":"3GiB"}}"#;
        let historical = workspace::legacy_configuration_value(Some(raw)).unwrap();
        let admitted = decode(raw, &OperationControl::default(), &mut || Ok(())).unwrap();
        assert_eq!(
            historical["dataCache"]["maxBytes"],
            admitted.data_cache_max_bytes
        );
    }
}

#[cfg(test)]
mod public_set_tests {
    use super::*;
    use crate::workspace;
    #[test]
    fn actual_config_set_keeps_unrelated_unknown_fields_and_normalized_data_budget() {
        let home = std::env::temp_dir().join(format!(
            "lychee-config-set-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes())
        ));
        struct Remove(std::path::PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
        workspace::initialize(&home).unwrap();
        let _remove = Remove(home.clone());
        let raw = br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":32,"custom":"keep"},"download":{"workers":0},"future":{"nested":[1,true]},"DataCache":{"MaxBytes":"5GB"}}"#;
        fs::write(home.join("config.json"), raw).unwrap();
        let ctx = crate::request::RequestContext {
            cwd: home.clone(),
            home: home.clone(),
            control: OperationControl::default(),
        };
        let result = workspace::set_config(&ctx, &workspace::ConfigUpdate { download_workers: Some(4), ..Default::default() }).unwrap();
        assert_eq!(result.data["download"]["workers"], 4);
        assert_eq!(result.data["dataCache"]["maxBytes"], 5_000_000_000u64);
        assert_eq!(result.data["cache"]["custom"], "keep");
        assert_eq!(
            result.data["future"],
            serde_json::json!({"nested":[1,true]})
        );
        assert_eq!(workspace::config(&home).unwrap(), result.data);
    }
}
