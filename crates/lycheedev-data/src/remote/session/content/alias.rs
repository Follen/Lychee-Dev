//! Fixed decoded-v1 identity. Every candidate hashes its current encoded source;
//! the most recent authenticated body is shared only within this scoped request.
use super::*;
use crate::remote::{
    cache::{
        Store, pure_expected,
        wire::{CachedBlob, Object, ObjectSeed, folded, scalar},
    },
    files::Encoded,
};
use md5::{Digest, Md5};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};
use sha2::Sha256;

fn integrity() -> ToolkitError {
    fault("vault.blob_integrity", 4, "Decoded cache identity differs")
}
#[derive(Default, Serialize)]
struct Source {
    kind: String,
    state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    sha256: String,
}
impl Object for Source {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "kind" => scalar(&mut map, &mut self.kind)?,
                "state" => scalar(&mut map, &mut self.state)?,
                "commit" => scalar(&mut map, &mut self.commit)?,
                "sha256" => scalar(&mut map, &mut self.sha256)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl From<&keys::KeySource> for Source {
    fn from(value: &keys::KeySource) -> Self {
        Self {
            kind: value.kind.clone(),
            state: value.state.clone(),
            commit: value.commit.clone(),
            sha256: value.sha256.clone(),
        }
    }
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    content_key: String,
    blob: CachedBlob,
    keys: Source,
}
impl Object for Entry {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "contentkey" => scalar(&mut map, &mut self.content_key)?,
                "blob" => map.next_value_seed(ObjectSeed(&mut self.blob))?,
                "keys" => map.next_value_seed(ObjectSeed(&mut self.keys))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(deserializer)?;
        Ok(value)
    }
}
fn read_entry(store: &Store<'_>, key: &str) -> Result<Option<Entry>> {
    store
        .read(key)
        .map(|found| found.map(|(entry, _)| entry))
        .map_err(|error| {
            if pure_expected(&error, "records.remote_identity") {
                integrity()
            } else {
                error
            }
        })
}

pub(super) fn key(
    files: &Files<'_>,
    source: &mut Encoded<'_, '_>,
    content_key: &str,
    keys: &keys::Context,
    stats: &mut Stats,
    budget: &Budget,
) -> Result<Option<String>> {
    if keys.source.kind == "provided" {
        return Ok(None);
    }
    files.store.check()?;
    let _scratch = budget.reserve_retained(65536)?;
    let mut digest = Sha256::new();
    digest.update(format!(
        "decoded-v1\0{content_key}\0{}\0{}\0",
        keys.source.kind, keys.source.sha256
    ));
    stats.content_hash_calls += 1;
    source.rewind()?;
    let mut buffer = [0u8; 65536];
    loop {
        files.store.check()?;
        let n = source.read(&mut buffer).map_err(blte::stream_io_error)?;
        stats.content_hash_bytes += n as u64;
        files.store.check()?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(Some(format!(
        "decoded-v1/{}",
        casc::hex(&digest.finalize())
    )))
}

#[derive(Default)]
pub(in crate::remote::session) struct Memo {
    latest: Option<(evidence::BlobRef, Arc<SharedContent>)>,
}
impl Memo {
    fn verified(
        &mut self,
        store: &Store<'_>,
        reference: &evidence::BlobRef,
        stats: &mut Stats,
        budget: &Arc<Budget>,
        optional: bool,
    ) -> Result<Option<Arc<SharedContent>>> {
        store.check()?;
        if let Some((previous, content)) = &self.latest
            && previous.sha256 == reference.sha256
            && previous.bytes == reference.bytes
        {
            return Ok(Some(content.clone()));
        }
        let size = usize::try_from(reference.bytes).map_err(|_| integrity())?;
        // Fixed Go readVerifiedBlob charges logical retained once per new body.
        // The separate real allocation remains refundable with all Arc owners.
        budget.charge_split(Cost::retained(size), Cost::default())?;
        // Admit Arc/Content bookkeeping and retained reference strings with
        // conservative fixed slack; body bytes remain the exact payload size.
        let allocation = budget.reserve_retained(size.checked_add(256).ok_or_else(integrity)?)?;
        let _io_scratch = budget.reserve_retained(65536)?;
        let raw = if optional {
            store.read_blob_optional(reference, size.max(1))?
        } else {
            Some(store.read_blob(reference, size.max(1))?)
        };
        let Some(raw) = raw else {
            return Ok(None);
        };
        stats.blob_reads += 1;
        stats.blob_read_bytes += raw.len() as u64;
        let content = Arc::new(SharedContent {
            value: blte::Content {
                bytes: raw,
                missing: Vec::new(),
            },
            _allocation: allocation.into_owned(budget)?,
        });
        store.check()?;
        self.latest = Some((reference.clone(), content.clone()));
        Ok(Some(content))
    }
    pub(super) fn read(
        &mut self,
        store: &Store<'_>,
        key: Option<&str>,
        record: &EncodingRecord,
        keys: &mut keys::Context,
        stats: &mut Stats,
        budget: &Arc<Budget>,
    ) -> Result<Option<(evidence::BlobRef, Arc<SharedContent>)>> {
        let Some(key) = key else {
            return Ok(None);
        };
        let Some(entry) = read_entry(store, key)? else {
            return Ok(None);
        };
        if entry.content_key != record.content_key
            || entry.blob.bytes < 0
            || entry.blob.bytes as u64 != record.decoded_bytes
            || entry.keys.kind != keys.source.kind
            || entry.keys.sha256 != keys.source.sha256
        {
            return Err(integrity());
        }
        if entry.keys.kind == "public" && entry.keys.state == "loaded" {
            match keys.ensure_public() {
                Ok(()) => {}
                Err(error)
                    if keys.source.state != "loaded"
                        && pure_expected(&error, "container.key_unavailable") =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
        if !lycheedev_core::selection::valid_digest(&entry.blob.sha256, 64) {
            return Err(integrity());
        }
        let reference = entry.blob.into_core()?;
        let Some(content) = self.verified(store, &reference, stats, budget, true)? else {
            return Ok(None);
        };
        let mut digest = Md5::new();
        for block in content.bytes.chunks(65536) {
            store.check()?;
            digest.update(block);
        }
        store.check()?;
        if casc::hex(&digest.finalize()) != record.content_key {
            return Err(integrity());
        }
        Ok(Some((reference, content)))
    }
    pub(super) fn save(
        &mut self,
        store: &Store<'_>,
        key: Option<&str>,
        record: &EncodingRecord,
        reference: &evidence::BlobRef,
        keys: &mut keys::Context,
        stats: &mut Stats,
        budget: &Arc<Budget>,
    ) -> Result<()> {
        let Some(key) = key else {
            return Ok(());
        };
        let entry = Entry {
            content_key: record.content_key.clone(),
            blob: CachedBlob::try_from(reference.clone())?,
            keys: Source::from(&keys.source),
        };
        // Go marshals before reading only existence; it does not re-validate an
        // already present alias here. A concurrent create alone triggers reread.
        let _ = serde_json::to_vec(&entry)?;
        if store.generation(key)?.is_some() {
            return Ok(());
        }
        self.after_create(
            store,
            key,
            record,
            keys,
            stats,
            budget,
            store.put(key, &entry, None),
        )
    }
    fn after_create(
        &mut self,
        store: &Store<'_>,
        key: &str,
        record: &EncodingRecord,
        keys: &mut keys::Context,
        stats: &mut Stats,
        budget: &Arc<Budget>,
        result: Result<()>,
    ) -> Result<()> {
        match result {
            Ok(()) => Ok(()),
            Err(error) if pure_expected(&error, "vault.generation_conflict") => self
                .read(store, Some(key), record, keys, stats, budget)
                .map(|_| ()),
            Err(error) => Err(error),
        }
    }
    pub(super) fn published(
        &mut self,
        store: &Store<'_>,
        reference: &evidence::BlobRef,
        stats: &mut Stats,
        budget: &Arc<Budget>,
    ) -> Result<Arc<SharedContent>> {
        self.verified(store, reference, stats, budget, false)
            .map(|body| body.expect("required read cannot return absent body"))
    }
}
#[cfg(test)]
mod tests;
