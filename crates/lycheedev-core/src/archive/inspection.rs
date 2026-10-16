//! Doctor's read-only journal boundary. It does not prepare or resume a switch.
use super::*;
use crate::control::OperationControl;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{fmt, io::Read};
#[derive(Debug, Default)]
pub(crate) struct Journal {
    pub state: String,
    pub archive_path: String,
    schema: String,
    root: String,
    path: String,
}
fn field(k: &str) -> String {
    k.chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
fn timestamp<E: serde::de::Error>(raw: &RawValue) -> std::result::Result<(), E> {
    if raw.get() == "null" {
        return Ok(());
    }
    let time = raw
        .get()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| E::custom("Time.UnmarshalJSON: input is not a JSON string"))?;
    crate::evidence::parse_go_timestamp(time)
        .ok_or_else(|| E::custom("invalid archive timestamp"))?;
    Ok(())
}
#[derive(Default)]
struct Step;
impl<'de> Deserialize<'de> for Step {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Step;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an archive step")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Step, E> {
                Ok(Step)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> std::result::Result<Step, M::Error> {
                while let Some(key) = m.next_key::<String>()? {
                    match field(&key).as_str() {
                        "id" | "detail" => {
                            m.next_value::<Option<String>>()?;
                        }
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "json: unknown field {key:?}"
                            )));
                        }
                    }
                }
                Ok(Step)
            }
        }
        d.deserialize_any(Fields)
    }
}
impl<'de> Deserialize<'de> for Journal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Journal;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an archive journal")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Journal, E> {
                Ok(Journal::default())
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Journal, M::Error> {
                let mut value = Journal::default();
                while let Some(key) = m.next_key::<String>()? {
                    let into = match field(&key).as_str() {
                        "schema" => &mut value.schema,
                        "root" => &mut value.root,
                        "archivepath" => &mut value.archive_path,
                        "journalpath" => &mut value.path,
                        "state" => &mut value.state,
                        "createdat" | "completedat" => {
                            timestamp::<M::Error>(&m.next_value::<Box<RawValue>>()?)?;
                            continue;
                        }
                        "markers" => {
                            m.next_value::<Option<Vec<Option<String>>>>()?;
                            continue;
                        }
                        "steps" => {
                            m.next_value::<Option<Vec<Step>>>()?;
                            continue;
                        }
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "json: unknown field {key:?}"
                            )));
                        }
                    };
                    if let Some(v) = m.next_value::<Option<String>>()? {
                        *into = v;
                    }
                }
                Ok(value)
            }
        }
        d.deserialize_any(Fields)
    }
}
fn invalid(detail: impl fmt::Display) -> ToolkitError {
    ToolkitError::new(
        "vault.archive_state",
        format!("vault.archive_state: {detail}"),
        4,
    )
}
fn decode(raw: &[u8], root: &Path, path: &Path, control: &OperationControl) -> Result<Journal> {
    std::str::from_utf8(raw).map_err(|_| invalid("archive journal encoding"))?;
    // Go uses Decoder.Decode once; trailing bytes belong to a later JSON value
    // and are not read as a second journal. Preserve original first-token bytes.
    let end = crate::raw_json::first_value_end(raw)
        .map_err(|e| invalid(format!("archive journal format: {e}")))?;
    let first = &raw[..end];
    let normalized = crate::evidence::normalize_go_json_strings(first, control)?;
    let value: Journal = serde_json::from_slice(&normalized)
        .map_err(|e| invalid(format!("archive journal format: {e}")))?;
    if value.schema != "lycheedev.archive.v1"
        || value.root != root.to_string_lossy()
        || value.path != path.to_string_lossy()
    {
        return Err(invalid("archive journal identity"));
    }
    if !["planned", "moved", "complete"].contains(&value.state.as_str()) {
        return Err(invalid(format!("archive journal state {:?}", value.state)));
    }
    control.check()?;
    Ok(value)
}
pub(crate) fn journal_controlled(
    root: &Path,
    control: &OperationControl,
) -> Result<Option<Journal>> {
    control.check()?;
    let root = absolute(root)?;
    let path = journal_path(&root)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    control.check()?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err(invalid("archive journal shape"));
    }
    storage::guard_path_with_check(path.parent().unwrap(), &path, || control.check())?;
    let mut file = match fs::File::open(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let result = (|| {
        let mut raw = Vec::with_capacity(metadata.len() as usize);
        let mut buffer = [0; 8192];
        while raw.len() < 65537 {
            control.check()?;
            let request = buffer.len().min(65537 - raw.len());
            let n = file.read(&mut buffer[..request])?;
            raw.extend_from_slice(&buffer[..n]);
            control.check()?;
            if n == 0 {
                break;
            }
        }
        if raw.len() > 65536 {
            return Err(invalid("archive journal encoding"));
        }
        decode(&raw, &root, &path, control)
    })();
    storage::finish_file(file, result, control).map(Some)
}
#[cfg(test)]
mod tests;
