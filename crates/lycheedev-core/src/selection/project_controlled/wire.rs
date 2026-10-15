//! Go project JSON: 64 KiB is admitted by the caller; duplicate member names
//! are rejected before typed decoding, with the fixed depth 16 convention.
//! Preserve original raw time tokens and input-order struct merges.
use super::*;
use serde::{
    Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, fmt};

struct Object(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a project object")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Object, M::Error> {
                let mut fields = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    fields.push((key, map.next_value()?));
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}
fn object(raw: &str) -> Result<Object> {
    if raw.trim() == "null" {
        return Ok(Object(Vec::new()));
    }
    serde_json::from_str(raw).map_err(|_| format())
}
fn field(key: &str) -> String {
    // Exactly the two non-ASCII simple folds that can match ASCII JSON fields.
    key.chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
fn lower(key: &str) -> String {
    // Go strings.ToLower is scalar/simple; Rust's char iterator is scalar too.
    // The only lowercase expansion is U+0130; Go retains its first scalar i.
    key.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}
fn string<C: FnMut() -> Result<()>>(
    raw: &str,
    into: &mut String,
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    if raw.trim() == "null" {
        return Ok(());
    }
    scope.check()?;
    let normalized = crate::evidence::normalize_go_json_strings(raw.as_bytes(), scope.control)?;
    let value = serde_json::from_slice::<String>(&normalized).map_err(|_| format());
    let value = scope.finish(value, [])?;
    *into = value;
    Ok(())
}
pub(super) fn validate<C: FnMut() -> Result<()>>(
    raw: &[u8],
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    fn value<C: FnMut() -> Result<()>>(
        raw: &str,
        depth: usize,
        scope: &mut Scope<'_, C>,
    ) -> Result<()> {
        scope.check()?;
        if depth > 16 {
            return Err(format());
        }
        let raw = raw.trim();
        match raw.as_bytes().first() {
            Some(b'{') => {
                let mut seen = BTreeSet::new();
                for (key, raw) in object(raw)?.0 {
                    if !seen.insert(lower(&key)) {
                        return Err(format());
                    }
                    value(raw.get(), depth + 1, scope)?;
                }
            }
            Some(b'[') => {
                let values: Vec<Box<RawValue>> = serde_json::from_str(raw).map_err(|_| format())?;
                for raw in values {
                    value(raw.get(), depth + 1, scope)?;
                }
            }
            Some(b'"') => {
                string(raw, &mut String::new(), scope)?;
            }
            Some(b'n' | b't' | b'f') => {
                let _: Value = serde_json::from_str(raw).map_err(|_| format())?;
            }
            _ => {
                let number: f64 = serde_json::from_str(raw).map_err(|_| format())?;
                if !number.is_finite() {
                    return Err(format());
                }
            }
        }
        scope.check()
    }
    scope.check()?;
    if !crate::raw_json::is_valid(raw) {
        return Err(format());
    }
    value(std::str::from_utf8(raw).map_err(|_| format())?, 0, scope)
}
pub(super) fn project<C: FnMut() -> Result<()>>(
    raw: &[u8],
    scope: &mut Scope<'_, C>,
) -> Result<String> {
    let (mut schema, mut product) = (String::new(), String::new());
    for (key, value) in object(std::str::from_utf8(raw).map_err(|_| format())?)?.0 {
        match field(&key).as_str() {
            "schema" => string(value.get(), &mut schema, scope)?,
            "product" => string(value.get(), &mut product, scope)?,
            _ => return Err(format()),
        }
    }
    if schema != "lycheedev.project.v1" || !valid_product(&product) {
        return Err(format());
    }
    scope.check()?;
    Ok(product)
}
pub(super) fn source<C: FnMut() -> Result<()>>(
    raw: &str,
    pin: &mut SourcePin,
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        let into = match field(&key).as_str() {
            "repository" => &mut pin.repository,
            "product" => &mut pin.product,
            "requestedref" => &mut pin.requested_ref,
            "exactcommit" => &mut pin.exact_commit,
            "parserrevision" => &mut pin.parser_revision,
            _ => return Err(format()),
        };
        string(value.get(), into, scope)?;
    }
    Ok(())
}
fn data<C: FnMut() -> Result<()>>(
    raw: &str,
    pin: &mut DataPin,
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        let into = match field(&key).as_str() {
            "product" => &mut pin.product,
            "region" => &mut pin.region,
            "fullbuild" => &mut pin.full_build,
            "buildconfig" => &mut pin.build_config,
            "cdnconfig" => &mut pin.cdn_config,
            "language" => &mut pin.language,
            "definitioncommit" => &mut pin.definition_commit,
            _ => return Err(format()),
        };
        string(value.get(), into, scope)?;
    }
    Ok(())
}
pub(super) fn changes<C: FnMut() -> Result<()>>(
    raw: &str,
    pin: &mut ChangePin,
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        if field(&key) == "fetchedat" {
            let raw = value.get();
            if raw == "null" {
                continue;
            }
            let raw = raw
                .strip_prefix('"')
                .and_then(|raw| raw.strip_suffix('"'))
                .ok_or_else(format)?;
            let time = crate::evidence::parse_go_timestamp(raw).ok_or_else(format)?;
            if time.is_zero() {
                pin.fetched_at.clear();
            } else {
                pin.fetched_at = time.canonical().ok_or_else(format)?.to_owned();
            }
            continue;
        }
        let into = match field(&key).as_str() {
            "provider" => &mut pin.provider,
            "product" => &mut pin.product,
            "region" => &mut pin.region,
            "fullbuild" => &mut pin.full_build,
            "scope" => &mut pin.scope,
            "sha256" => &mut pin.sha256,
            _ => return Err(format()),
        };
        string(value.get(), into, scope)?;
    }
    Ok(())
}
fn selection<C: FnMut() -> Result<()>>(
    raw: &str,
    pin: &mut PinnedSet,
    scope: &mut Scope<'_, C>,
) -> Result<()> {
    for (key, value) in object(raw)?.0 {
        scope.check()?;
        let raw = value.get();
        match field(&key).as_str() {
            "schema" => string(raw, &mut pin.schema, scope)?,
            "id" => string(raw, &mut pin.id, scope)?,
            "parent" => string(raw, &mut pin.spec.parent, scope)?,
            "source" => {
                if raw == "null" {
                    pin.spec.source = None;
                } else {
                    source(raw, pin.spec.source.get_or_insert_default(), scope)?;
                }
            }
            "data" => {
                if raw == "null" {
                    pin.spec.data = None;
                } else {
                    data(raw, pin.spec.data.get_or_insert_default(), scope)?;
                }
            }
            "changes" => {
                if raw == "null" {
                    pin.spec.changes = None;
                } else {
                    changes(raw, pin.spec.changes.get_or_insert_default(), scope)?;
                }
            }
            _ => return Err(format()),
        }
    }
    Ok(())
}
pub(super) fn lock<C: FnMut() -> Result<()>>(
    raw: &[u8],
    scope: &mut Scope<'_, C>,
) -> Result<PinnedSet> {
    let mut schema = String::new();
    let mut pin = PinnedSet {
        schema: String::new(),
        id: String::new(),
        spec: SelectionSpec::default(),
    };
    for (key, value) in object(std::str::from_utf8(raw).map_err(|_| format())?)?.0 {
        match field(&key).as_str() {
            "schema" => string(value.get(), &mut schema, scope)?,
            "selection" => selection(value.get(), &mut pin, scope)?,
            _ => return Err(format()),
        }
    }
    if schema != "lycheedev.project-lock.v1" {
        return Err(format());
    }
    // Existing general pin APIs retain their historical validator. Project
    // decoding applies Go's digit-only build spelling before that validator.
    for build in pin
        .spec
        .data
        .iter()
        .map(|p| p.full_build.as_str())
        .chain(pin.spec.changes.iter().map(|p| p.full_build.as_str()))
    {
        if !build
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(format());
        }
    }
    let id = pin.id.clone();
    validate_loaded_pin(pin, &id).map_err(|cause| {
        ToolkitError::new(
            "selection.project_format",
            format!("selection.project_format: {}", cause.message),
            4,
        )
    })
}
