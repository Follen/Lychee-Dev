//! Typed Go SelectionSpec decoding. Unlike project files, specs allow repeated
//! fields and merge non-null pointer objects; unknown fields are rejected.
use super::*;
use serde::{
    Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;
struct Object(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a selection object")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Object, E> {
                Ok(Object(Vec::new()))
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Object, M::Error> {
                let mut fields = Vec::new();
                while let Some(key) = m.next_key()? {
                    fields.push((key, m.next_value()?));
                }
                Ok(Object(fields))
            }
        }
        d.deserialize_any(Fields)
    }
}
fn object(raw: &str) -> Result<Object> {
    serde_json::from_str(raw).map_err(|e| failed(e.to_string()))
}
fn field(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
fn unknown(key: &str) -> ToolkitError {
    failed(format!("json: unknown field {key:?}"))
}
fn string(raw: &str, into: &mut String) -> Result<()> {
    if raw != "null" {
        *into = serde_json::from_str(raw).map_err(|e| failed(e.to_string()))?;
    }
    Ok(())
}
fn source(raw: &str, pin: &mut SourcePin, control: &OperationControl, strict: bool) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        control.check()?;
        let field = match field(&key).as_str() {
            "repository" => &mut pin.repository,
            "product" => &mut pin.product,
            "requestedref" => &mut pin.requested_ref,
            "exactcommit" => &mut pin.exact_commit,
            "parserrevision" => &mut pin.parser_revision,
            _ if strict => return Err(unknown(&key)),
            _ => continue,
        };
        string(value.get(), field)?;
    }
    Ok(())
}
fn data(raw: &str, pin: &mut DataPin, control: &OperationControl, strict: bool) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        control.check()?;
        let field = match field(&key).as_str() {
            "product" => &mut pin.product,
            "region" => &mut pin.region,
            "fullbuild" => &mut pin.full_build,
            "buildconfig" => &mut pin.build_config,
            "cdnconfig" => &mut pin.cdn_config,
            "language" => &mut pin.language,
            "definitioncommit" => &mut pin.definition_commit,
            _ if strict => return Err(unknown(&key)),
            _ => continue,
        };
        string(value.get(), field)?;
    }
    Ok(())
}
fn changes(
    raw: &str,
    pin: &mut ChangePin,
    unmarshal_only: &mut bool,
    control: &OperationControl,
    strict: bool,
) -> Result<Option<ToolkitError>> {
    let fields = match object(raw) {
        Ok(fields) => fields,
        Err(error) => return Ok(Some(error)),
    };
    let mut first = None;
    for (key, value) in fields.0 {
        control.check()?;
        if field(&key) == "fetchedat" {
            let raw = value.get();
            if raw == "null" {
                continue;
            }
            let raw = raw
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .ok_or_else(|| failed("Time.UnmarshalJSON: input is not a JSON string"))?;
            let time = crate::evidence::parse_go_timestamp(raw)
                .ok_or_else(|| failed("invalid Go selection timestamp"))?;
            *unmarshal_only = time.canonical().is_none();
            pin.fetched_at = if time.is_zero() {
                String::new()
            } else {
                time.canonical().unwrap_or(raw).to_owned()
            };
            continue;
        }
        let field = match field(&key).as_str() {
            "provider" => &mut pin.provider,
            "product" => &mut pin.product,
            "fullbuild" => &mut pin.full_build,
            "region" => &mut pin.region,
            "scope" => &mut pin.scope,
            "sha256" => &mut pin.sha256,
            _ => {
                if strict {
                    first.get_or_insert_with(|| unknown(&key));
                }
                continue;
            }
        };
        if let Err(error) = string(value.get(), field) {
            first.get_or_insert(error);
        }
    }
    Ok(first)
}
pub(super) struct Decoded {
    pub spec: SelectionSpec,
    pub time_unmarshal_only: bool,
    schema: String,
    id: String,
}
pub(super) fn decode(raw: &[u8], control: &OperationControl) -> Result<Decoded> {
    decode_inner(raw, control, true)
}
pub(super) fn stored(raw: &[u8], control: &OperationControl) -> Result<(PinnedSet, bool)> {
    let decoded = decode_inner(raw, control, false)?;
    Ok((
        PinnedSet {
            schema: decoded.schema,
            id: decoded.id,
            spec: decoded.spec,
        },
        decoded.time_unmarshal_only,
    ))
}
fn decode_inner(raw: &[u8], control: &OperationControl, strict: bool) -> Result<Decoded> {
    control.check()?;
    if raw
        .iter()
        .all(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
    {
        return Err(failed("EOF"));
    }
    let end = crate::raw_json::first_value_end(raw).map_err(|e| failed(e.to_string()))?;
    let normalized = crate::evidence::normalize_go_json_strings(&raw[..end], control)?;
    let raw_text = std::str::from_utf8(&normalized).map_err(|e| failed(e.to_string()))?;
    let mut decoded = Decoded {
        spec: SelectionSpec::default(),
        time_unmarshal_only: false,
        schema: String::new(),
        id: String::new(),
    };
    let mut first = None;
    for (key, value) in object(raw_text)?.0 {
        control.check()?;
        let raw = value.get();
        let result = match field(&key).as_str() {
            "parent" => string(raw, &mut decoded.spec.parent),
            "source" => {
                if raw == "null" {
                    decoded.spec.source = None;
                    Ok(())
                } else {
                    source(
                        raw,
                        decoded.spec.source.get_or_insert_default(),
                        control,
                        strict,
                    )
                }
            }
            "data" => {
                if raw == "null" {
                    decoded.spec.data = None;
                    Ok(())
                } else {
                    data(
                        raw,
                        decoded.spec.data.get_or_insert_default(),
                        control,
                        strict,
                    )
                }
            }
            "changes" => {
                if raw == "null" {
                    decoded.spec.changes = None;
                    decoded.time_unmarshal_only = false;
                    Ok(())
                } else {
                    // Time.UnmarshalJSON errors are immediate in Go, whereas
                    // ordinary unknown/type errors are saved until struct end.
                    match changes(
                        raw,
                        decoded.spec.changes.get_or_insert_default(),
                        &mut decoded.time_unmarshal_only,
                        control,
                        strict,
                    )? {
                        Some(error) => Err(error),
                        None => Ok(()),
                    }
                }
            }
            "schema" if !strict => string(raw, &mut decoded.schema),
            "id" if !strict => string(raw, &mut decoded.id),
            _ if strict => Err(unknown(&key)),
            _ => Ok(()),
        };
        if let Err(error) = result {
            first.get_or_insert(error);
        }
    }
    if let Some(error) = first {
        return Err(error);
    }
    if end != raw.len() {
        return Err(failed("selection.trailing_spec_data"));
    }
    control.check()?;
    Ok(decoded)
}
