//! Private admission configuration reader, fixed Go vault/config.go contract.
//! Historical workspace::config remains a separate, unchanged API.
use crate::{Result, ToolkitError, control::OperationControl, evidence, storage};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{fmt, fs, io::Read, path::Path};

const MAXIMUM: usize = 65536;
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
    let mut value = evidence::GoSignedInteger(*place);
    scalar(raw, &mut value)?;
    *place = value.0;
    Ok(())
}
fn object(raw: &RawValue) -> Result<Object> {
    serde_json::from_str(raw.get()).map_err(|_| invalid())
}
fn decode(
    raw: &[u8],
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<usize> {
    control.check()?;
    check()?;
    if raw.len() > MAXIMUM || std::str::from_utf8(raw).is_err() || !crate::raw_json::is_valid(raw) {
        return Err(invalid());
    }
    // Bounded original-byte grammar pass precedes normalization; no synthetic
    // 128-depth cap on ignored schema-peek fields. At most 3x string scratch.
    control.check()?;
    check()?;
    let normalized = evidence::normalize_go_json_strings(raw, control)?;
    let fields: Object = serde_json::from_slice(&normalized).map_err(|_| invalid())?;
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
    Ok(workers as usize)
}
pub(super) fn read(
    home: &Path,
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<usize> {
    control.check()?;
    check()?;
    crate::workspace::open(home)?;
    control.check()?;
    check()?;
    let path = home.join("config.json");
    let info = match fs::symlink_metadata(&path) {
        Ok(info) => info,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
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
    super::finish(result, [], control.check(), check())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(raw: &[u8]) -> Result<usize> {
        decode(raw, &OperationControl::default(), &mut || Ok(()))
    }
    #[test]
    fn configuration_duplicate_null_case_and_nested_merge_match_go() {
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
    fn schema_peek_newer_precedes_unknown_fields_and_uses_go_depth() {
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
