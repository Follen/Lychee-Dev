//! Fixed Go 3.2.0 pin reader for historical consumers. No disk opens, pin
//! publication, resolution, or inferred identities occur in this module.
//! Oracle: internal/selection/{pins,products}.go at 3057a966.
use super::{Timestamp, decode_record};
use crate::go_mailbox_v1::admission::{RecordSeed, WireRecord, ignored, scalar};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, selection, storage};
use serde::{
    Serialize,
    de::{DeserializeSeed, MapAccess, Visitor},
};

#[derive(Debug, Clone, Default, Serialize)]
struct PinWire {
    schema: String,
    id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    parent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<selection::SourcePin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<selection::DataPin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changes: Option<ChangeWire>,
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeWire {
    provider: String,
    product: String,
    full_build: String,
    region: String,
    scope: String,
    fetched_at: Timestamp,
    sha256: String,
}

struct OptionalRecordSeed<'a, T>(&'a mut Option<T>);
impl<'de, T: WireRecord + Default> DeserializeSeed<'de> for OptionalRecordSeed<'_, T> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct OptionalVisitor<'a, T>(&'a mut Option<T>);
        impl<'de, T: WireRecord + Default> Visitor<'de> for OptionalVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = None;
                Ok(())
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                self.visit_none()
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<(), D::Error> {
                RecordSeed(self.0.get_or_insert_with(T::default)).deserialize(d)
            }
        }
        d.deserialize_option(OptionalVisitor(self.0))
    }
}
impl WireRecord for PinWire {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "id" => scalar(&mut self.id, map),
            "parent" => scalar(&mut self.parent, map),
            "source" => map.next_value_seed(OptionalRecordSeed(&mut self.source)),
            "data" => map.next_value_seed(OptionalRecordSeed(&mut self.data)),
            "changes" => map.next_value_seed(OptionalRecordSeed(&mut self.changes)),
            _ => ignored(map),
        }
    }
}
impl WireRecord for selection::SourcePin {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "repository" => scalar(&mut self.repository, map),
            "product" => scalar(&mut self.product, map),
            "requestedref" => scalar(&mut self.requested_ref, map),
            "exactcommit" => scalar(&mut self.exact_commit, map),
            "parserrevision" => scalar(&mut self.parser_revision, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for selection::DataPin {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "product" => scalar(&mut self.product, map),
            "region" => scalar(&mut self.region, map),
            "fullbuild" => scalar(&mut self.full_build, map),
            "buildconfig" => scalar(&mut self.build_config, map),
            "cdnconfig" => scalar(&mut self.cdn_config, map),
            "language" => scalar(&mut self.language, map),
            "definitioncommit" => scalar(&mut self.definition_commit, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for ChangeWire {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "provider" => scalar(&mut self.provider, map),
            "product" => scalar(&mut self.product, map),
            "fullbuild" => scalar(&mut self.full_build, map),
            "region" => scalar(&mut self.region, map),
            "scope" => scalar(&mut self.scope, map),
            "fetchedat" => scalar(&mut self.fetched_at, map),
            "sha256" => scalar(&mut self.sha256, map),
            _ => ignored(map),
        }
    }
}

fn failure(reason: impl Into<String>) -> ToolkitError {
    ToolkitError::new("command.failed", reason, 5)
}
fn build(value: &str) -> bool {
    let mut count = 0;
    for part in value.split('.') {
        count += 1;
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || part.parse::<u32>().is_err()
        {
            return false;
        }
    }
    count == 4
}
fn validate(pin: &PinWire) -> Result<()> {
    if pin.source.is_none() && pin.data.is_none() && pin.changes.is_none() {
        return Err(failure("selection.empty"));
    }
    if let Some(s) = &pin.source {
        if s.repository.is_empty()
            || s.product.is_empty()
            || s.parser_revision.is_empty()
            || !selection::valid_digest(&s.exact_commit, 40)
        {
            return Err(failure("selection.unresolved_source"));
        }
    }
    if let Some(d) = &pin.data {
        if d.product.is_empty()
            || d.region.is_empty()
            || d.language.is_empty()
            || !build(&d.full_build)
            || !selection::valid_digest(&d.build_config, 32)
            || !selection::valid_digest(&d.cdn_config, 32)
            || !selection::valid_digest(&d.definition_commit, 40)
        {
            return Err(failure("selection.unresolved_data"));
        }
    }
    if let Some(c) = &pin.changes {
        if c.provider.is_empty()
            || c.product.is_empty()
            || c.region.is_empty()
            || c.scope.is_empty()
            || !build(&c.full_build)
            || c.fetched_at.is_zero()
            || !selection::valid_digest(&c.sha256, 64)
        {
            return Err(failure("selection.unresolved_changes"));
        }
    }
    let data_product = pin.data.as_ref().map(|d| d.product.as_str()).unwrap_or("");
    let changes_product = pin
        .changes
        .as_ref()
        .map(|c| c.product.as_str())
        .unwrap_or("");
    if !data_product.is_empty() && !changes_product.is_empty() && data_product != changes_product {
        return Err(failure(format!(
            "selection.product_mismatch: {data_product} / {changes_product}"
        )));
    }
    let product = if changes_product.is_empty() {
        data_product
    } else {
        changes_product
    };
    if let Some(source) = &pin.source {
        if !product.is_empty() && !selection::source_supports_product(source, product) {
            return Err(failure(format!(
                "selection.product_mismatch: {} / {product}",
                source.product
            )));
        }
    }
    Ok(())
}
fn identity(pin: &PinWire) -> Result<String> {
    let mut unsigned = pin.clone();
    unsigned.id.clear();
    let bytes = storage::identity_json(&unsigned).map_err(|error| failure(error.message))?;
    Ok(format!("PIN-{}", storage::sha256(&bytes)))
}

/// Called after an original-budget raw metadata read. Read-only and bounded;
/// the returned core DTO is safe to use only because the wire hash and typed
/// constraints have already been checked here. Core's ordinary serde reader
/// is deliberately not invoked and cannot silently replace this path.
pub(super) fn read_pin(
    raw: &[u8],
    id: &str,
    control: &OperationControl,
) -> Result<selection::PinnedSet> {
    control.check()?;
    let pin: PinWire = decode_record(raw, control)?;
    control.check()?;
    if pin.id != id
        || pin.schema != "lycheedev.selection.v1"
        || !pin
            .id
            .strip_prefix("PIN-")
            .is_some_and(|digest| selection::valid_digest(digest, 64))
    {
        return Err(failure("selection.invalid_pin"));
    }
    validate(&pin)?;
    if identity(&pin)? != pin.id {
        return Err(failure("selection.pin_integrity"));
    }
    control.check()?;
    let changes = pin
        .changes
        .map(|c| -> Result<selection::ChangePin> {
            Ok(selection::ChangePin {
                provider: c.provider,
                product: c.product,
                full_build: c.full_build,
                region: c.region,
                scope: c.scope,
                // Successful identity_json above proves this value is Some.
                fetched_at: c
                    .fetched_at
                    .canonical
                    .ok_or_else(|| failure("selection.pin_integrity"))?,
                sha256: c.sha256,
            })
        })
        .transpose()?;
    Ok(selection::PinnedSet {
        schema: pin.schema,
        id: pin.id,
        spec: selection::SelectionSpec {
            parent: pin.parent,
            source: pin.source,
            data: pin.data,
            changes,
        },
    })
}

#[cfg(test)]
#[path = "pins_tests.rs"]
mod tests;
