//! Derived decoded reuse never substitutes another selected encoded source.
//! Hash all current encoded bytes, then rehash cached blob and check whole CKey.
use crate::{
    blte, casc, fault, keys,
    metadata_format::EncodingRecord,
    resource::{Budget, Cost},
};
use lycheedev_core::{Result, evidence, metadata::DataCacheMetadata};
#[cfg(test)]
use lycheedev_core::storage;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::path::Path;
mod streaming;
pub(crate) use streaming::{
    read_content_controlled, read_content_in_session, read_content_with_reference_controlled,
    read_content_with_reference_in_session, read_verified_content,
    read_verified_content_in_session,
};
#[derive(Default, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub hits: u64,
    pub reused_bytes: u64,
    pub content_hash_calls: u64,
    pub content_hash_bytes: u64,
    pub content_decode_calls: u64,
    pub extracted_bytes: u64,
    pub blob_reads: u64,
    pub blob_read_bytes: u64,
    pub fragment_loads: u64,
    pub block_cache_hits: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    content_key: String,
    blob: Value,
    keys: keys::KeySource,
}
fn integrity() -> lycheedev_core::ToolkitError {
    fault("vault.blob_integrity", 4, "Decoded cache identity differs")
}
