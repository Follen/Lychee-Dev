//! Historical Go typed JSON: duplicate objects merge, scalar null is a no-op,
//! slices replace (null clears), signed integers retain their original tokens.
//! Blob references are authenticated only when the cache actually consumes them.
use super::*;
use crate::remote::cache::wire::{CachedBlob, Object, ObjectSeed, folded, scalar};
use lycheedev_core::evidence::GoSignedInteger;
use serde::{
    Deserializer, Serializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};
use serde_json::value::RawValue;

#[derive(Default)]
pub(super) struct CachedDocument {
    pub schema: String,
    pub provenance: CachedProvenance,
}
pub(super) struct CachedProvenance {
    pub kind: String,
    pub files: Vec<CachedFile>,
    pub fetched_at: String,
    pub entry_count: i64,
    pub rejected: i64,
    pub total_bytes: i64,
    pub cache: String,
}
impl Default for CachedProvenance {
    fn default() -> Self {
        Self {
            kind: String::new(),
            files: Vec::new(),
            fetched_at: "0001-01-01T00:00:00Z".into(),
            entry_count: 0,
            rejected: 0,
            total_bytes: 0,
            cache: String::new(),
        }
    }
}
#[derive(Default)]
pub(super) struct CachedFile {
    pub component: String,
    pub url: String,
    pub bytes: i64,
    pub sha256: String,
    pub blob: CachedBlob,
}
fn signed<'de, A: MapAccess<'de>>(
    map: &mut A,
    place: &mut i64,
) -> std::result::Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<GoSignedInteger>>()? {
        *place = value.0;
    }
    Ok(())
}
impl Object for CachedDocument {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "schema" => scalar(&mut map, &mut self.schema)?,
                "provenance" => map.next_value_seed(ObjectSeed(&mut self.provenance))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl Object for CachedProvenance {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "kind" => scalar(&mut map, &mut self.kind)?,
                "files" => {
                    self.files = map
                        .next_value::<Option<Vec<CachedFile>>>()?
                        .unwrap_or_default()
                }
                "fetchedat" => {
                    if let Some(raw) = map.next_value::<Option<Box<RawValue>>>()? {
                        let interior = raw
                            .get()
                            .strip_prefix('"')
                            .and_then(|s| s.strip_suffix('"'));
                        let raw = interior
                            .filter(|s| evidence::parse_go_timestamp(s).is_some())
                            .ok_or_else(|| {
                                serde::de::Error::custom("invalid Go listfile timestamp")
                            })?;
                        self.fetched_at = raw.to_owned();
                    }
                }
                "entrycount" => signed(&mut map, &mut self.entry_count)?,
                "rejected" => signed(&mut map, &mut self.rejected)?,
                "totalbytes" => signed(&mut map, &mut self.total_bytes)?,
                "cache" => scalar(&mut map, &mut self.cache)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl Object for CachedFile {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "component" => scalar(&mut map, &mut self.component)?,
                "url" => scalar(&mut map, &mut self.url)?,
                "bytes" => signed(&mut map, &mut self.bytes)?,
                "sha256" => scalar(&mut map, &mut self.sha256)?,
                "blob" => map.next_value_seed(ObjectSeed(&mut self.blob))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
macro_rules! decode_object {($($ty:ty),*) => {$ (
    impl<'de> Deserialize<'de> for $ty {
        fn deserialize<D:Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error> {
            let mut value=Self::default(); ObjectSeed(&mut value).deserialize(d)?; Ok(value)
        }
    }
)*};}
decode_object!(CachedDocument, CachedProvenance, CachedFile);
pub(super) fn decode(raw: &[u8], control: &OperationControl) -> Result<CachedDocument> {
    let grammar = lycheedev_core::raw_json::validate_with_check(raw, || control.check())?;
    if grammar.is_err() {
        return finish_stop(Err(format("invalid cached JSON")), control.check());
    }
    // Bound remains the existing one-MiB Metadata row. Only a parsing view is
    // normalized; raw time tokens, stored bytes and blob identities stay intact.
    let view = evidence::normalize_go_json_strings(raw, control)?;
    let parsed =
        serde_json::from_slice(&view).map_err(|_| format("invalid cached listfile document"));
    finish_stop(parsed, control.check())
}
impl CachedProvenance {
    pub(super) fn into_public(self) -> Result<Provenance> {
        let files = self
            .files
            .into_iter()
            .map(|file| {
                Ok(SourceFile {
                    component: file.component,
                    url: file.url,
                    bytes: file
                        .bytes
                        .try_into()
                        .map_err(|_| format("cached payload length changed"))?,
                    sha256: file.sha256,
                    blob: evidence::BlobRef {
                        sha256: file.blob.sha256,
                        bytes: file
                            .blob
                            .bytes
                            .try_into()
                            .map_err(|_| format("cached blob length changed"))?,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Provenance {
            kind: self.kind,
            files,
            fetched_at: self.fetched_at,
            entry_count: self
                .entry_count
                .try_into()
                .map_err(|_| format("cached entry count changed"))?,
            rejected: self
                .rejected
                .try_into()
                .map_err(|_| format("cached rejection count changed"))?,
            total_bytes: self
                .total_bytes
                .try_into()
                .map_err(|_| format("cached total changed"))?,
            cache: self.cache,
        })
    }
}
pub(super) fn serialize_time<S: Serializer>(
    value: &str,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    let time = evidence::parse_go_timestamp(value)
        .ok_or_else(|| serde::ser::Error::custom("invalid Go listfile timestamp"))?;
    s.serialize_str(
        time.canonical()
            .ok_or_else(|| serde::ser::Error::custom("Go time timezone outside RFC3339 range"))?,
    )
}
