//! Static-table derivation identity. A snapshot label is not enough: source
//! bytes, selected definition, locale and missing-key coverage remain distinct.
use crate::{capture, db2::Prepared, fault, resource::Budget};
use lycheedev_core::{Result, selection::DataPin};
use serde::Serialize;
use serde_json::Value;

/// Exact borrowed inputs shared by explicit preparation and ordinary SQL.
#[derive(Clone, Copy)]
pub(crate) struct StaticSource<'a> {
    pub data: &'a DataPin,
    pub bundle: &'a crate::definitions::Bundle,
    pub definition: &'a crate::schema::Definition,
    pub layout: &'a crate::table::Layout,
    pub reading: &'a Value,
    pub missing: &'a [crate::blte::Missing],
    pub unavailable_partitions: &'a std::collections::BTreeSet<usize>,
    pub bytes: usize,
}

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
        Self::from_source(StaticSource {
            data,
            bundle: &prepared.bundle,
            definition: &prepared.definition,
            layout: &prepared.layout,
            reading: &prepared.file.reading,
            missing: prepared.file.bytes.missing(),
            unavailable_partitions: &prepared.availability.skipped,
            bytes: prepared.file.bytes.len(),
        })
    }
    pub(crate) fn from_source(source: StaticSource<'a>) -> Result<Self> {
        let StaticSource {
            data,
            bundle,
            definition,
            layout,
            reading,
            missing,
            unavailable_partitions,
            bytes,
        } = source;
        let complete = reading["contentVerified"] == true
            && missing.is_empty()
            && unavailable_partitions.is_empty();
        let content = if complete {
            &reading["content"]
        } else {
            &reading["partialContent"]
        };
        if !lycheedev_core::selection::valid_digest(content["sha256"].as_str().unwrap_or(""), 64)
            || content["bytes"].as_u64() != Some(bytes as u64)
            || bundle.commit != data.definition_commit
            || layout.table_hash != bundle.identity.hash
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
            table: &bundle.identity,
            definition,
            definition_manifest: &bundle.manifest,
            definition_object: &bundle.definition,
            layout,
            source: SourceIdentity {
                origin: &reading["source"],
                content_key: &reading["entry"]["contentKey"],
                payload_encoding_key: &reading["payloadEncodingKey"],
                content: &reading["content"],
                partial_content: &reading["partialContent"],
                key_source: &reading["keySource"],
            },
            missing,
            unavailable_partitions,
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
