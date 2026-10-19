//! Static-table derivation identity. A snapshot label is not enough: source
//! bytes, selected definition, locale and missing-key coverage remain distinct.
use crate::{capture, db2::Prepared, fault, resource::Budget};
use lycheedev_core::{Result, selection::DataPin};
use serde::Serialize;
use serde_json::Value;

/// Deliberately borrowed from the already authenticated Prepared owner. No key
/// bytes or mutable runtime/cache counters are copied into an identity.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StaticIdentity<'a> {
    schema: &'static str,
    source_kind: &'static str,
    data: &'a DataPin,
    table: &'a crate::definitions::TableIdentity,
    definition: &'a crate::schema::Definition,
    definition_manifest: &'a Value,
    definition_object: &'a Value,
    layout: &'a crate::table::Layout,
    source: SourceIdentity<'a>,
    missing: &'a [crate::blte::Missing],
    unavailable_partitions: &'a std::collections::BTreeSet<usize>,
    complete: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceIdentity<'a> {
    origin: &'a Value,
    content_key: &'a Value,
    payload_encoding_key: &'a Value,
    content: &'a Value,
    partial_content: &'a Value,
    key_source: &'a Value,
}
impl<'a> StaticIdentity<'a> {
    pub(crate) fn from_prepared(prepared: &'a Prepared) -> Result<Self> {
        let data = prepared.pin.data.as_ref().ok_or_else(|| {
            fault(
                "records.data_pin_required",
                4,
                "A typed generation requires an exact data pin",
            )
        })?;
        let reading = &prepared.file.reading;
        let complete = reading["contentVerified"] == true
            && prepared.file.bytes.missing().is_empty()
            && prepared.availability.skipped.is_empty();
        // A complete payload must keep the provider's authenticated content
        // digest. Partial spans use their own digest and cannot impersonate it.
        let content = if complete {
            &reading["content"]
        } else {
            &reading["partialContent"]
        };
        if !lycheedev_core::selection::valid_digest(content["sha256"].as_str().unwrap_or(""), 64)
            || content["bytes"].as_u64() != Some(prepared.file.bytes.len() as u64)
            || prepared.bundle.commit != data.definition_commit
            || prepared.layout.table_hash != prepared.bundle.identity.hash
        {
            return Err(fault(
                "records.generation_identity",
                4,
                "Prepared source identity is incomplete or inconsistent",
            ));
        }
        Ok(Self {
            schema: "lycheedev.data-static-generation-identity.v1",
            source_kind: "static",
            data,
            table: &prepared.bundle.identity,
            definition: &prepared.definition,
            definition_manifest: &prepared.bundle.manifest,
            definition_object: &prepared.bundle.definition,
            layout: &prepared.layout,
            source: SourceIdentity {
                origin: &reading["source"],
                content_key: &reading["entry"]["contentKey"],
                payload_encoding_key: &reading["payloadEncodingKey"],
                content: &reading["content"],
                partial_content: &reading["partialContent"],
                key_source: &reading["keySource"],
            },
            missing: prepared.file.bytes.missing(),
            unavailable_partitions: &prepared.availability.skipped,
            complete,
        })
    }
    pub(crate) fn digest(&self, budget: &Budget) -> Result<String> {
        capture::json_sha256(self, budget, serde_json::ser::CompactFormatter)
    }
    pub(crate) fn complete(&self) -> bool {
        self.complete
    }
    pub(crate) fn generation_digest(
        &self,
        typed_schema_sha256: &str,
        representation: &str,
        derivation_version: &str,
        budget: &Budget,
    ) -> Result<String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct GenerationIdentity<'a, 'source> {
            schema: &'static str,
            source: &'a StaticIdentity<'source>,
            typed_schema_sha256: &'a str,
            representation: &'a str,
            derivation_version: &'a str,
        }
        capture::json_sha256(
            &GenerationIdentity {
                schema: "lycheedev.data-generation-identity.v1",
                source: self,
                typed_schema_sha256,
                representation,
                derivation_version,
            },
            budget,
            serde_json::ser::CompactFormatter,
        )
    }
}
