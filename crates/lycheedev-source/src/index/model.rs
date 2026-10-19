//! The producer's persistent Source domain. These types also decode borrowed
//! Postcard rows: the cache does not maintain a second copied field schema.
//! Runtime I/O observations remain a small public-result object, never wire.
use lycheedev_core::{evidence::BlobRef, selection::SourcePin};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::{Value, json};
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRecord<T = String, L = BlobRef> {
    pub path: T,
    pub content_hash: T,
    #[serde(with = "coordinate")]
    pub bytes: usize,
    pub extension: T,
    pub mime: T,
    pub format: T,
    pub width: u32,
    pub height: u32,
    // Metadata-only assets deliberately retain an explicit public null.
    pub local: Option<L>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSample<T = String> {
    pub path: T,
    #[serde(with = "coordinate")]
    pub line: usize,
    pub message: T,
}

/// A sample has a producer-defined maximum of twenty rows. An inline bounded
/// container lets the borrowed decoder check an untrusted count before any
/// collection allocation, including the summary-only path.
#[derive(Clone, Debug)]
pub struct Samples<T = String> {
    rows: [Option<DiagnosticSample<T>>; 20],
    len: usize,
}
impl<T> Default for Samples<T> {
    fn default() -> Self {
        Self {
            rows: std::array::from_fn(|_| None),
            len: 0,
        }
    }
}
impl<T> Samples<T> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[cfg(test)]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut DiagnosticSample<T>> {
        self.rows.iter_mut().flatten()
    }
    pub fn iter(&self) -> impl Iterator<Item = &DiagnosticSample<T>> {
        self.rows.iter().flatten()
    }
    pub(super) fn push(&mut self, value: DiagnosticSample<T>) -> bool {
        if self.len == self.rows.len() {
            return false;
        }
        self.rows[self.len] = Some(value);
        self.len += 1;
        true
    }
    pub(super) fn into_rows(self) -> impl Iterator<Item = DiagnosticSample<T>> {
        self.rows.into_iter().flatten()
    }
}
impl<T: Serialize> Serialize for Samples<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.len))?;
        for row in self.iter() {
            rows.serialize_element(row)?;
        }
        rows.end()
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Samples<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SampleVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for SampleVisitor<T> {
            type Value = Samples<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most twenty diagnostic samples")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                if seq.size_hint().is_some_and(|n| n > 20) {
                    return Err(A::Error::custom("too many diagnostic samples"));
                }
                let mut result = Samples::default();
                while let Some(row) = seq.next_element()? {
                    if !result.push(row) {
                        return Err(A::Error::custom("too many diagnostic samples"));
                    }
                }
                Ok(result)
            }
        }
        deserializer.deserialize_seq(SampleVisitor(std::marker::PhantomData))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(deserialize = "T: Deserialize<'de>")
)]
pub struct SourceSummary<T = String> {
    pub schema: T,
    pub storage: T,
    pub repository: T,
    pub product: T,
    pub commit: T,
    pub parser: T,
    #[serde(with = "coordinate")]
    pub documents: usize,
    #[serde(with = "coordinate")]
    pub ast_files: usize,
    #[serde(with = "coordinate")]
    pub skipped_documents: usize,
    #[serde(with = "coordinate")]
    pub declarations: usize,
    #[serde(with = "coordinate")]
    pub relationships: usize,
    #[serde(with = "coordinate")]
    pub assets: usize,
    #[serde(with = "coordinate")]
    pub diagnostics: usize,
    pub complete: bool,
    #[serde(default)]
    pub diagnostic_sample: Option<Samples<T>>,
    // Public JSON can include this request's observations. Cache serialization
    // and decoding always omit it, so a cache cannot supply fresh I/O claims.
    #[serde(default, deserialize_with = "runtime_io")]
    pub io: Value,
}
impl<T: Serialize> Serialize for SourceSummary<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Exhaustive destructuring makes adding a domain field a compile-time
        // wire/public review obligation rather than silently dropping it.
        let Self {
            schema,
            storage,
            repository,
            product,
            commit,
            parser,
            documents,
            ast_files,
            skipped_documents,
            declarations,
            relationships,
            assets,
            diagnostics,
            complete,
            diagnostic_sample,
            io,
        } = self;
        if !serializer.is_human_readable() {
            return (
                schema,
                storage,
                repository,
                product,
                commit,
                parser,
                *documents as u64,
                *ast_files as u64,
                *skipped_documents as u64,
                *declarations as u64,
                *relationships as u64,
                *assets as u64,
                *diagnostics as u64,
                complete,
                diagnostic_sample,
                (),
            )
                .serialize(serializer);
        }
        let mut map = serializer.serialize_map(Some(
            14 + usize::from(diagnostic_sample.is_some()) + usize::from(!io.is_null()),
        ))?;
        map.serialize_entry("schema", schema)?;
        map.serialize_entry("storage", storage)?;
        map.serialize_entry("repository", repository)?;
        map.serialize_entry("product", product)?;
        map.serialize_entry("commit", commit)?;
        map.serialize_entry("parser", parser)?;
        map.serialize_entry("documents", documents)?;
        map.serialize_entry("astFiles", ast_files)?;
        map.serialize_entry("skippedDocuments", skipped_documents)?;
        map.serialize_entry("declarations", declarations)?;
        map.serialize_entry("relationships", relationships)?;
        map.serialize_entry("assets", assets)?;
        map.serialize_entry("diagnostics", diagnostics)?;
        map.serialize_entry("complete", complete)?;
        if let Some(sample) = diagnostic_sample {
            map.serialize_entry("diagnosticSample", sample)?;
        }
        if !io.is_null() {
            map.serialize_entry("io", io)?;
        }
        map.end()
    }
}
impl SourceSummary {
    pub(super) fn empty(pin: &SourcePin) -> Self {
        Self {
            schema: "lycheedev.source-map.v2".into(),
            storage: "file-cache".into(),
            repository: pin.repository.clone(),
            product: pin.product.clone(),
            commit: pin.exact_commit.clone(),
            parser: pin.parser_revision.clone(),
            documents: 0,
            ast_files: 0,
            skipped_documents: 0,
            declarations: 0,
            relationships: 0,
            assets: 0,
            diagnostics: 0,
            complete: true,
            diagnostic_sample: None,
            io: Value::Null,
        }
    }
    pub fn coverage(&self, documents: usize) -> Value {
        let mut value = json!({"analyzedDocuments":documents,"skippedDocuments":self.skipped_documents,"diagnostics":self.diagnostics,"complete":self.complete,"io":self.io});
        if let Some(sample) = &self.diagnostic_sample {
            value["diagnosticSample"] = json!(sample);
        }
        value
    }
}
impl<T: AsRef<str>> SourceSummary<T> {
    pub(crate) fn matches(&self, pin: &SourcePin) -> bool {
        self.repository.as_ref() == pin.repository
            && self.product.as_ref() == pin.product
            && self.commit.as_ref() == pin.exact_commit
            && self.parser.as_ref() == pin.parser_revision
    }
}

fn runtime_io<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    if deserializer.is_human_readable() {
        Value::deserialize(deserializer)
    } else {
        <()>::deserialize(deserializer)?;
        Ok(Value::Null)
    }
}

/// Serde's platform-independent wire is explicitly u64, including 32-bit
/// decoders. Public JSON keeps the existing unsigned integer representation.
pub(super) mod coordinate {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(value: &usize, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(u64::try_from(*value).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
        usize::try_from(u64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
pub(crate) fn test_summary(value: Value) -> SourceSummary {
    let pin = SourcePin {
        repository: "fixture".into(),
        product: "retail".into(),
        exact_commit: "a".repeat(40),
        parser_revision: "source-v1".into(),
        requested_ref: "fixture".into(),
    };
    let mut merged = serde_json::to_value(SourceSummary::empty(&pin)).unwrap();
    let fields = value.as_object().expect("summary fixture object");
    for (key, value) in fields {
        merged[key] = value.clone();
    }
    let io = merged
        .as_object_mut()
        .unwrap()
        .remove("io")
        .unwrap_or(Value::Null);
    let mut summary: SourceSummary = serde_json::from_value(merged).unwrap();
    summary.io = io;
    summary
}
#[cfg(test)]
pub(crate) fn test_asset(value: Value) -> AssetRecord {
    let mut merged = json!({"path":"","contentHash":"","bytes":0,"extension":"","mime":"application/octet-stream","format":"","width":0,"height":0,"local":null});
    for (key, value) in value.as_object().expect("asset fixture object") {
        merged[key] = value.clone();
    }
    serde_json::from_value(merged).unwrap()
}
