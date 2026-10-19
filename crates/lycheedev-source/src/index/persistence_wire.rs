//! Versioned private Postcard persistence with one borrowed-row admission path.
//! Full consumers own admitted rows once; summary consumers validate the same
//! fields without constructing and dropping String/Value owners per row.
//! The 128 MiB limit is the actual encoded file size, not a second JSON pass.
use super::{
    AssetRecord, Document, Index, SourceSummary, Symbol,
    cache_projection::{CollectionLength, Counted},
    facts_cache::Locator,
    model::{DiagnosticSample, Samples},
    symbol_projection::{BuiltSymbols, SymbolsWire},
};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence::BlobRef, selection::valid_digest,
};
use postcard::de_flavors::Flavor as _;
use serde::{
    Deserialize, Serialize,
    de::{DeserializeSeed, Error, SeqAccess, Visitor},
    ser::{SerializeSeq, SerializeTuple},
};
use std::{cell::RefCell, collections::BTreeSet, fmt, marker::PhantomData};

pub(crate) const SCHEMA: &str = "lycheedev.source-index.rust.v7";
const MAGIC: &[u8; 8] = b"LYCIDX01";
const META_MAGIC: &[u8; 8] = b"LYCIMD01";
const HEADER: usize = 44;
pub(crate) const MAXIMUM: usize = 128 << 20;
const DOCUMENTS: usize = 100_000;
const ASSETS: usize = 200_000;
const SYMBOLS: usize = 2_000_000;
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.source_integrity", message, 4)
}
fn budget() -> ToolkitError {
    ToolkitError::new(
        "codebase.source_budget",
        "Source index ownership or file budget exceeded",
        3,
    )
}

/// Bounded geometric input growth, including the one-byte oversize sentinel.
/// Physical cache size and retained allocation capacity are checked separately.
pub(crate) fn append_input(raw: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let maximum = MAXIMUM + 1;
    let length = raw
        .len()
        .checked_add(bytes.len())
        .filter(|n| *n <= maximum)
        .ok_or_else(|| invalid("Cached index content exceeds file limit"))?;
    if length > raw.capacity() {
        let capacity = length.max(raw.capacity().saturating_mul(2)).min(maximum);
        raw.try_reserve_exact(capacity - raw.len())
            .map_err(|_| budget())?;
        if raw.capacity() > maximum {
            return Err(budget());
        }
    }
    raw.extend_from_slice(bytes);
    Ok(())
}

/// A separate construction bound derived from the existing row limits plus
/// the maximum bytes available for strings. It does not reuse the unrelated
/// 256 MiB original-source allowance or silently add a smaller source limit.
fn maximum_owned() -> Result<usize> {
    [
        (DOCUMENTS, size_of::<Document>()),
        (ASSETS, size_of::<AssetRecord>()),
        (SYMBOLS, size_of::<Symbol>()),
    ]
    .into_iter()
    .try_fold(
        MAXIMUM + size_of::<SourceSummary>() + 6144,
        |total, (count, size)| {
            count
                .checked_mul(size)
                .and_then(|bytes| total.checked_add(bytes))
                .ok_or_else(budget)
        },
    )
}
pub(super) struct Context<'a> {
    control: &'a OperationControl,
    failure: &'a RefCell<Option<ToolkitError>>,
    remaining: usize,
    payload: usize,
    declarations: usize,
    relationships: usize,
}
impl Context<'_> {
    fn stop<E: Error>(&self, error: ToolkitError) -> E {
        *self.failure.borrow_mut() = Some(error);
        E::custom("Source index admission stopped")
    }
    fn check<E: Error>(&self) -> std::result::Result<(), E> {
        check_control(self.control).map_err(|e| self.stop(e))
    }
    fn admit<E: Error>(&mut self, bytes: usize) -> std::result::Result<(), E> {
        self.check()?;
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| self.stop::<E>(budget()))?;
        Ok(())
    }
    fn text<E: Error>(&mut self, value: &str) -> std::result::Result<String, E> {
        self.admit(value.len())?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(value.len())
            .map_err(|_| self.stop::<E>(budget()))?;
        self.admit(owned.capacity() - value.len())?;
        let mut start = 0;
        while start < value.len() {
            self.check::<E>()?;
            let mut end = start.saturating_add(65536).min(value.len());
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            owned.push_str(&value[start..end]);
            start = end;
        }
        self.check()?;
        Ok(owned)
    }
}

pub(super) trait Row: Sized {
    type Borrowed<'de>: Deserialize<'de>;
    const MAX: usize;
    const MIN: usize;
    fn validate<'de>(row: &Self::Borrowed<'de>, context: &mut Context<'_>) -> Result<()>;
    fn own<'de, E: Error>(
        row: Self::Borrowed<'de>,
        context: &mut Context<'_>,
    ) -> std::result::Result<Self, E>;
}
fn valid_path(path: &str) -> Result<()> {
    if crate::git::source_path(path) {
        Ok(())
    } else {
        Err(invalid("Invalid cached source path"))
    }
}
impl Row for Document {
    type Borrowed<'de> = Document<&'de str>;
    const MAX: usize = DOCUMENTS;
    const MIN: usize = 4;
    fn validate(row: &Document<&str>, _: &mut Context<'_>) -> Result<()> {
        valid_path(row.path)?;
        if !valid_digest(row.sha256, 64) || row.bytes > crate::git::MAX_SOURCE_BYTES {
            return Err(invalid("Invalid cached document identity or byte limit"));
        }
        if let Some(locator) = &row.facts_locator {
            if locator.offset < 8
                || locator.bytes < 76
                || locator
                    .offset
                    .checked_add(68)
                    .and_then(|n| n.checked_add(u64::from(locator.bytes)))
                    .is_none_or(|n| n > 32 << 20)
            {
                return Err(invalid("Invalid cached Facts locator range"));
            }
        }
        Ok(())
    }
    fn own<E: Error>(row: Document<&str>, ctx: &mut Context<'_>) -> std::result::Result<Self, E> {
        let Document {
            path,
            text: _,
            sha256,
            bytes,
            facts: _,
            facts_locator,
        } = row;
        Ok(Document {
            path: ctx.text(path)?,
            text: None,
            sha256: ctx.text(sha256)?,
            bytes,
            facts: Default::default(),
            facts_locator,
        })
    }
}
impl Row for AssetRecord {
    // Core owns BlobRef's public schema. Its two fixed fields are projected as
    // a borrowed tuple, not duplicated in a second Source blob domain type.
    type Borrowed<'de> = AssetRecord<&'de str, (&'de str, u64)>;
    const MAX: usize = ASSETS;
    const MIN: usize = 9;
    fn validate(row: &Self::Borrowed<'_>, _: &mut Context<'_>) -> Result<()> {
        valid_path(row.path)?;
        match &row.local {
            Some((hash, bytes))
                if valid_digest(hash, 64)
                    && *hash == row.content_hash
                    && *bytes == row.bytes as u64
                    && row.bytes <= crate::assets::MAX_BYTES =>
            {
                Ok(())
            }
            None if row.content_hash.is_empty() && row.bytes > crate::assets::MAX_BYTES => Ok(()),
            _ => Err(invalid("Invalid cached asset local reference")),
        }
    }
    fn own<E: Error>(
        row: Self::Borrowed<'_>,
        ctx: &mut Context<'_>,
    ) -> std::result::Result<Self, E> {
        let AssetRecord {
            path,
            content_hash,
            bytes,
            extension,
            mime,
            format,
            width,
            height,
            local,
        } = row;
        Ok(AssetRecord {
            path: ctx.text(path)?,
            content_hash: ctx.text(content_hash)?,
            bytes,
            extension: ctx.text(extension)?,
            mime: ctx.text(mime)?,
            format: ctx.text(format)?,
            width,
            height,
            local: match local {
                Some((hash, bytes)) => Some(BlobRef {
                    sha256: ctx.text(hash)?,
                    bytes,
                }),
                None => None,
            },
        })
    }
}
impl Row for Symbol {
    type Borrowed<'de> = Symbol<&'de str>;
    const MAX: usize = SYMBOLS;
    const MIN: usize = 12;
    fn validate(row: &Symbol<&str>, ctx: &mut Context<'_>) -> Result<()> {
        validate_symbol(row)?;
        match row.kind {
            "declaration" => ctx.declarations += 1,
            "relationship" => ctx.relationships += 1,
            "load" | "header" => (),
            _ => return Err(invalid("Unknown cached source symbol kind")),
        }
        Ok(())
    }
    fn own<E: Error>(row: Symbol<&str>, ctx: &mut Context<'_>) -> std::result::Result<Self, E> {
        let Symbol {
            id,
            kind,
            name,
            target,
            category,
            confidence,
            path,
            line,
            end_line,
            signature,
            scope,
            ordinal,
        } = row;
        Ok(Symbol {
            id: ctx.text(id)?,
            kind: ctx.text(kind)?,
            name: ctx.text(name)?,
            target: ctx.text(target)?,
            category: ctx.text(category)?,
            confidence: ctx.text(confidence)?,
            path: ctx.text(path)?,
            line,
            end_line,
            signature: ctx.text(signature)?,
            scope: ctx.text(scope)?,
            ordinal,
        })
    }
}

pub(super) trait Collection<T: Row>: CollectionLength + From<Vec<T>> {
    fn reserve<E: Error>(count: usize, context: &mut Context<'_>) -> std::result::Result<Self, E>;
    fn push<'de, E: Error>(
        &mut self,
        row: T::Borrowed<'de>,
        context: &mut Context<'_>,
    ) -> std::result::Result<(), E>;
}
impl<T: Row> Collection<T> for Vec<T> {
    fn reserve<E: Error>(count: usize, ctx: &mut Context<'_>) -> std::result::Result<Self, E> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .ok_or_else(|| ctx.stop::<E>(budget()))?;
        ctx.admit(bytes)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| ctx.stop::<E>(budget()))?;
        let capacity = result
            .capacity()
            .checked_mul(size_of::<T>())
            .ok_or_else(|| ctx.stop::<E>(budget()))?;
        ctx.admit(capacity - bytes)?;
        Ok(result)
    }
    fn push<'de, E: Error>(
        &mut self,
        row: T::Borrowed<'de>,
        ctx: &mut Context<'_>,
    ) -> std::result::Result<(), E> {
        Vec::push(self, T::own::<E>(row, ctx)?);
        Ok(())
    }
}
impl<T: Row> Collection<T> for Counted<T> {
    fn reserve<E: Error>(_: usize, ctx: &mut Context<'_>) -> std::result::Result<Self, E> {
        ctx.check()?;
        Ok(Self::default())
    }
    fn push<'de, E: Error>(
        &mut self,
        _: T::Borrowed<'de>,
        ctx: &mut Context<'_>,
    ) -> std::result::Result<(), E> {
        ctx.check()?;
        self.increment();
        Ok(())
    }
}
struct RowsSeed<'a, 'b, C, T>(&'a mut Context<'b>, PhantomData<(C, T)>);
impl<'de, C: Collection<T>, T: Row> DeserializeSeed<'de> for RowsSeed<'_, '_, C, T> {
    type Value = C;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<C, D::Error> {
        struct Rows<'a, 'b, C, T>(RowsSeed<'a, 'b, C, T>);
        impl<'de, C: Collection<T>, T: Row> Visitor<'de> for Rows<'_, '_, C, T> {
            type Value = C;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded Source index sequence")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<C, A::Error> {
                let ctx = self.0.0;
                ctx.check()?;
                let count = seq
                    .size_hint()
                    .ok_or_else(|| A::Error::custom("missing Source index sequence length"))?;
                if count > T::MAX || count.checked_mul(T::MIN).is_none_or(|n| n > ctx.payload) {
                    return Err(ctx.stop(invalid("Invalid cached Source row count")));
                }
                let mut result = C::reserve::<A::Error>(count, ctx)?;
                for _ in 0..count {
                    ctx.check()?;
                    let row = seq
                        .next_element::<T::Borrowed<'de>>()?
                        .ok_or_else(|| A::Error::custom("missing cached Source row"))?;
                    T::validate(&row, ctx).map_err(|error| ctx.stop::<A::Error>(error))?;
                    result.push::<A::Error>(row, ctx)?;
                }
                Ok(result)
            }
        }
        deserializer.deserialize_seq(Rows(self))
    }
}
fn own_summary<E: Error>(
    row: SourceSummary<&str>,
    ctx: &mut Context<'_>,
) -> std::result::Result<SourceSummary, E> {
    let SourceSummary {
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
        io: _,
    } = row;
    let sample = if let Some(sample) = diagnostic_sample {
        let mut owned = Samples::default();
        for DiagnosticSample {
            path,
            line,
            message,
        } in sample.into_rows()
        {
            owned.push(DiagnosticSample {
                path: ctx.text(path)?,
                line,
                message: ctx.text(message)?,
            });
        }
        Some(owned)
    } else {
        None
    };
    Ok(SourceSummary {
        schema: ctx.text(schema)?,
        storage: ctx.text(storage)?,
        repository: ctx.text(repository)?,
        product: ctx.text(product)?,
        commit: ctx.text(commit)?,
        parser: ctx.text(parser)?,
        documents,
        ast_files,
        skipped_documents,
        declarations,
        relationships,
        assets,
        diagnostics,
        complete,
        diagnostic_sample: sample,
        io: serde_json::Value::Null,
    })
}
fn validate_summary<T: AsRef<str>>(
    summary: &SourceSummary<T>,
    identity: &str,
    control: &OperationControl,
) -> Result<()> {
    let sample = summary.diagnostic_sample.as_ref().map_or(0, Samples::len);
    if super::fingerprint_fields_checked(
        summary.repository.as_ref(),
        summary.product.as_ref(),
        summary.commit.as_ref(),
        summary.parser.as_ref(),
        || check_control(control),
    )? != identity
        || summary.schema.as_ref() != "lycheedev.source-map.v2"
        || summary.storage.as_ref() != "file-cache"
        || summary.documents > DOCUMENTS
        || summary.assets > ASSETS
        || summary.ast_files > summary.documents
        || summary.declarations > SYMBOLS
        || summary.relationships > SYMBOLS
        || summary.complete != (summary.skipped_documents == 0 && summary.diagnostics == 0)
        || sample != summary.diagnostics.min(20)
        || summary.diagnostic_sample.is_some() != (summary.diagnostics != 0)
    {
        return Err(invalid("Invalid cached Source summary identity or counts"));
    }
    if let Some(sample) = &summary.diagnostic_sample {
        for row in sample.iter() {
            valid_path(row.path.as_ref())?;
        }
    }
    Ok(())
}
struct GapsSeed<'a, 'b>(&'a mut Context<'b>);
impl<'de> DeserializeSeed<'de> for GapsSeed<'_, '_> {
    type Value = BTreeSet<String>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        impl<'de> Visitor<'de> for GapsSeed<'_, '_> {
            type Value = BTreeSet<String>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("ordered compatibility categories")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let count = seq
                    .size_hint()
                    .filter(|n| *n <= 6)
                    .ok_or_else(|| A::Error::custom("invalid compatibility category count"))?;
                let mut result = BTreeSet::new();
                let mut previous: Option<&str> = None;
                for _ in 0..count {
                    self.0.check()?;
                    let row: &str = seq
                        .next_element()?
                        .ok_or_else(|| A::Error::custom("missing compatibility category"))?;
                    if ![
                        "api",
                        "event",
                        "frame-type",
                        "interface",
                        "mixin",
                        "template",
                    ]
                    .contains(&row)
                        || previous.is_some_and(|p| p >= row)
                    {
                        return Err(A::Error::custom(
                            "unknown, duplicate or unordered compatibility category",
                        ));
                    }
                    previous = Some(row);
                    result.insert(self.0.text::<A::Error>(row)?);
                }
                Ok(result)
            }
        }
        deserializer.deserialize_seq(self)
    }
}
struct IndexSeed<'a, 'b, D, A, S> {
    ctx: &'a mut Context<'b>,
    identity: &'a str,
    metadata: bool,
    types: PhantomData<(D, A, S)>,
}
impl<'de, D: Collection<Document>, A: Collection<AssetRecord>, S: Collection<Symbol>>
    DeserializeSeed<'de> for IndexSeed<'_, '_, D, A, S>
{
    type Value = Index<D, A, S>;
    fn deserialize<X: serde::Deserializer<'de>>(
        self,
        deserializer: X,
    ) -> std::result::Result<Self::Value, X::Error> {
        impl<'de, D: Collection<Document>, A: Collection<AssetRecord>, S: Collection<Symbol>>
            Visitor<'de> for IndexSeed<'_, '_, D, A, S>
        {
            type Value = Index<D, A, S>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a complete current Source index")
            }
            fn visit_seq<Q: SeqAccess<'de>>(
                self,
                mut seq: Q,
            ) -> std::result::Result<Self::Value, Q::Error> {
                let ctx = self.ctx;
                ctx.check()?;
                let summary: SourceSummary<&str> = seq
                    .next_element()?
                    .ok_or_else(|| Q::Error::custom("missing index summary"))?;
                validate_summary(&summary, self.identity, ctx.control)
                    .map_err(|error| ctx.stop::<Q::Error>(error))?;
                let summary = own_summary::<Q::Error>(summary, ctx)?;
                let documents: D = seq
                    .next_element_seed(RowsSeed::<D, Document>(ctx, PhantomData))?
                    .ok_or_else(|| Q::Error::custom("missing index documents"))?;
                let assets: A = seq
                    .next_element_seed(RowsSeed::<A, AssetRecord>(ctx, PhantomData))?
                    .ok_or_else(|| Q::Error::custom("missing index assets"))?;
                let symbols: S = if self.metadata {
                    S::reserve::<Q::Error>(0, ctx)?
                } else {
                    seq.next_element_seed(RowsSeed::<S, Symbol>(ctx, PhantomData))?
                        .ok_or_else(|| Q::Error::custom("missing index symbols"))?
                };
                let compatibility_gaps = seq
                    .next_element_seed(GapsSeed(ctx))?
                    .ok_or_else(|| Q::Error::custom("missing compatibility categories"))?;
                if documents.len() != summary.documents
                    || assets.len() != summary.assets
                    || (!self.metadata
                        && (ctx.declarations != summary.declarations
                            || ctx.relationships != summary.relationships))
                {
                    return Err(Q::Error::custom(
                        "Source index collection counts differ from summary",
                    ));
                }
                ctx.check()?;
                Ok(Index {
                    documents,
                    assets,
                    symbols,
                    summary,
                    compatibility_gaps,
                    home: Default::default(),
                })
            }
        }
        deserializer.deserialize_tuple(if self.metadata { 4 } else { 5 }, self)
    }
}

// Delegate all framing/varints/UTF-8 to Postcard. This flavor only checks the
// caller's control between fields and every 4096 individually consumed bytes.
struct Input<'a> {
    inner: postcard::de_flavors::Slice<'a>,
    control: &'a OperationControl,
    failure: &'a RefCell<Option<ToolkitError>>,
    popped: usize,
}
impl Input<'_> {
    fn check(&self) -> postcard::Result<()> {
        check_control(self.control).map_err(|error| {
            *self.failure.borrow_mut() = Some(error);
            postcard::Error::DeserializeBadEncoding
        })
    }
}
impl<'de> postcard::de_flavors::Flavor<'de> for Input<'de> {
    type Remainder = &'de [u8];
    type Source = &'de [u8];
    fn pop(&mut self) -> postcard::Result<u8> {
        if self.popped % 4096 == 0 {
            self.check()?;
        }
        self.popped += 1;
        self.inner.pop()
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
    fn try_take_n(&mut self, count: usize) -> postcard::Result<&'de [u8]> {
        self.check()?;
        self.inner.try_take_n(count)
    }
    fn finalize(self) -> postcard::Result<Self::Remainder> {
        #[cfg(test)]
        CANCEL_AT_FINALIZE.with(|slot| {
            if let Some(token) = slot.borrow().as_ref() {
                token.cancel();
            }
        });
        self.check()?;
        self.inner.finalize()
    }
}
fn identity_bytes(text: &str) -> Result<[u8; 32]> {
    if !valid_digest(text, 64) {
        return Err(invalid("Invalid index fingerprint"));
    }
    let mut raw = [0; 32];
    for (pair, out) in text.as_bytes().chunks_exact(2).zip(&mut raw) {
        let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        *out = digit(pair[0]) * 16 + digit(pair[1]);
    }
    Ok(raw)
}
pub(super) fn decode<D: Collection<Document>, A: Collection<AssetRecord>, S: Collection<Symbol>>(
    raw: &[u8],
    identity: &str,
    control: &OperationControl,
) -> Result<Index<D, A, S>> {
    decode_mode(raw, identity, control, false).map(|(index, _)| index)
}
pub(crate) fn decode_metadata(
    raw: &[u8],
    identity: &str,
    control: &OperationControl,
) -> Result<(Index, usize)> {
    decode_mode(raw, identity, control, true)
}
fn decode_mode<D: Collection<Document>, A: Collection<AssetRecord>, S: Collection<Symbol>>(
    raw: &[u8],
    identity: &str,
    control: &OperationControl,
    metadata: bool,
) -> Result<(Index<D, A, S>, usize)> {
    decode_with_owned(raw, identity, control, metadata, maximum_owned()?)
}
fn decode_with_owned<D: Collection<Document>, A: Collection<AssetRecord>, S: Collection<Symbol>>(
    raw: &[u8],
    identity: &str,
    control: &OperationControl,
    metadata: bool,
    owned: usize,
) -> Result<(Index<D, A, S>, usize)> {
    check_control(control)?;
    let magic = if metadata { META_MAGIC } else { MAGIC };
    if raw.len() < HEADER
        || raw.len() > MAXIMUM
        || raw.get(..8) != Some(magic.as_slice())
        || u32::from_le_bytes(raw[8..12].try_into().expect("bounded header")) as usize
            != raw.len() - HEADER
        || raw[12..HEADER] != identity_bytes(identity)?
    {
        return Err(invalid("Invalid Source binary index frame"));
    }
    let failure = RefCell::new(None);
    let payload = &raw[HEADER..];
    let mut ctx = Context {
        control,
        failure: &failure,
        remaining: owned
            .checked_sub(size_of::<SourceSummary>() + 6144)
            .ok_or_else(budget)?,
        payload: payload.len(),
        declarations: 0,
        relationships: 0,
    };
    let mut decoder = postcard::Deserializer::from_flavor(Input {
        inner: postcard::de_flavors::Slice::new(payload),
        control,
        failure: &failure,
        popped: 0,
    });
    let result = IndexSeed {
        ctx: &mut ctx,
        identity,
        metadata,
        types: PhantomData,
    }
    .deserialize(&mut decoder);
    // A consumed malformed field or admission failure is already terminal.
    // Do not observe a later finalization cancellation and replace that error.
    // Input owns no file/process resources and needs no success finalization.
    let index = match result {
        Ok(index) => index,
        Err(_) => {
            drop(decoder);
            return Err(failure
                .into_inner()
                .unwrap_or_else(|| invalid("Invalid Source binary index payload")));
        }
    };
    // Consume the borrowed remainder before extracting the recorded failure.
    let remainder_is_empty = matches!(decoder.finalize(), Ok([]));
    let remaining = ctx.remaining;
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    if !remainder_is_empty {
        return Err(invalid("Source binary index has trailing bytes"));
    }
    check_control(control)?;
    Ok((index, remaining))
}
pub(crate) fn validate_symbol(row: &Symbol<&str>) -> Result<()> {
    valid_path(row.path)?;
    if ["declaration", "relationship", "load", "header"].contains(&row.kind) {
        Ok(())
    } else {
        Err(invalid("Unknown cached source symbol kind"))
    }
}

pub(crate) fn own_symbols<'a>(
    rows: impl Iterator<Item = Result<Symbol<&'a str>>>,
    count: usize,
    remaining: usize,
    control: &OperationControl,
) -> Result<Vec<Symbol>> {
    if count > SYMBOLS {
        return Err(budget());
    }
    let failure = RefCell::new(None);
    let mut ctx = Context {
        control,
        failure: &failure,
        remaining,
        payload: 0,
        declarations: 0,
        relationships: 0,
    };
    let converted: std::result::Result<_, postcard::Error> = (|| {
        let mut output =
            <Vec<Symbol> as Collection<Symbol>>::reserve::<postcard::Error>(count, &mut ctx)?;
        for row in rows {
            if output.len() == count {
                return Err(ctx.stop::<postcard::Error>(invalid("Selected symbol count differs")));
            }
            ctx.check::<postcard::Error>()?;
            let row = row.map_err(|error| ctx.stop::<postcard::Error>(error))?;
            output.push(Symbol::own::<postcard::Error>(row, &mut ctx)?);
        }
        if output.len() != count {
            return Err(ctx.stop::<postcard::Error>(invalid("Selected symbol count differs")));
        }
        Ok(output)
    })();
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    converted.map_err(|_| invalid("Invalid selected Source symbol"))
}

struct IndexWire<'a, S>(&'a Index<Vec<Document>, Vec<AssetRecord>, S>, bool);
impl<C: BuiltSymbols> Serialize for IndexWire<'_, C> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let Index {
            documents,
            assets,
            symbols,
            summary,
            compatibility_gaps,
            home: _,
        } = self.0;
        let mut fields = serializer.serialize_tuple(if self.1 { 4 } else { 5 })?;
        fields.serialize_element(summary)?;
        fields.serialize_element(&DocumentsWire(documents))?;
        fields.serialize_element(assets)?;
        if !self.1 {
            fields.serialize_element(&SymbolsWire {
                rows: symbols,
                documents,
                summary,
            })?;
        }
        fields.serialize_element(compatibility_gaps)?;
        fields.end()
    }
}
struct DocumentsWire<'a>(&'a [Document]);
impl Serialize for DocumentsWire<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(DocumentWire))
    }
}
struct DocumentWire<'a>(&'a Document);
impl Serialize for DocumentWire<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let Document {
            path,
            text: _,
            sha256,
            bytes,
            facts: _,
            facts_locator,
        } = self.0;
        (
            path,
            sha256,
            *bytes as u64,
            facts_locator.as_ref().map(LocatorWire),
        )
            .serialize(serializer)
    }
}
struct LocatorWire<'a>(&'a Locator);
impl Serialize for LocatorWire<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let Locator {
            group,
            offset,
            bytes,
            sha256,
        } = self.0;
        (DigestWire(group), offset, bytes, DigestWire(sha256)).serialize(serializer)
    }
}
struct DigestWire<'a>(&'a [u8; 32]);
impl Serialize for DigestWire<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = [0; 64];
        for (byte, pair) in self.0.iter().zip(encoded.chunks_exact_mut(2)) {
            pair[0] = HEX[(byte >> 4) as usize];
            pair[1] = HEX[(byte & 15) as usize];
        }
        serializer.serialize_str(std::str::from_utf8(&encoded).expect("hex ASCII"))
    }
}
pub(super) struct SymbolWire<'a, T = String>(pub &'a Symbol<T>);
impl<T: AsRef<str>> Serialize for SymbolWire<'_, T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let Symbol {
            id,
            kind,
            name,
            target,
            category,
            confidence,
            path,
            line,
            end_line,
            signature,
            scope,
            ordinal,
        } = self.0;
        (
            id.as_ref(),
            kind.as_ref(),
            name.as_ref(),
            target.as_ref(),
            category.as_ref(),
            confidence.as_ref(),
            path.as_ref(),
            *line as u64,
            *end_line as u64,
            signature.as_ref(),
            scope.as_ref(),
            *ordinal as u64,
        )
            .serialize(serializer)
    }
}
struct Output<'a> {
    raw: Vec<u8>,
    since_check: usize,
    control: &'a OperationControl,
    failure: &'a RefCell<Option<ToolkitError>>,
}
impl postcard::ser_flavors::Flavor for Output<'_> {
    type Output = Vec<u8>;
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        self.try_extend(&[byte])
    }
    fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
        let fail = |error| {
            *self.failure.borrow_mut() = Some(error);
            postcard::Error::SerializeBufferFull
        };
        if self.since_check.saturating_add(bytes.len()) >= 65536 {
            check_control(self.control).map_err(fail)?;
            self.since_check = 0;
        }
        let length = self
            .raw
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAXIMUM)
            .ok_or_else(|| fail(budget()))?;
        if length > self.raw.capacity() {
            let capacity = length
                .max(self.raw.capacity().saturating_mul(2))
                .min(MAXIMUM);
            self.raw
                .try_reserve_exact(capacity - self.raw.len())
                .map_err(|_| fail(budget()))?;
            if self.raw.capacity() > MAXIMUM {
                return Err(fail(budget()));
            }
        }
        if bytes.len() >= 65536 {
            for chunk in bytes.chunks(65536) {
                check_control(self.control).map_err(fail)?;
                self.raw.extend_from_slice(chunk);
            }
            self.since_check = 0;
        } else {
            self.raw.extend_from_slice(bytes);
            self.since_check += bytes.len();
        }
        #[cfg(test)]
        super::symbol_projection::observe_output_capacity(self.raw.capacity());
        Ok(())
    }
    fn finalize(self) -> postcard::Result<Vec<u8>> {
        Ok(self.raw)
    }
}
pub(super) fn encode<S: BuiltSymbols>(
    index: &Index<Vec<Document>, Vec<AssetRecord>, S>,
    identity: &str,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    encode_mode(index, identity, control, false)
}
pub(crate) fn encode_metadata(
    index: &Index,
    identity: &str,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    encode_mode(index, identity, control, true)
}
fn encode_mode<S: BuiltSymbols>(
    index: &Index<Vec<Document>, Vec<AssetRecord>, S>,
    identity: &str,
    control: &OperationControl,
    metadata: bool,
) -> Result<Vec<u8>> {
    check_control(control)?;
    validate_summary(&index.summary, identity, control)?;
    if index.documents.len() != index.summary.documents
        || index.assets.len() != index.summary.assets
        || index.symbols.len() > SYMBOLS
    {
        return Err(budget());
    }
    let failure = RefCell::new(None);
    let output = Output {
        raw: vec![0; HEADER],
        since_check: 0,
        control,
        failure: &failure,
    };
    let encoded = postcard::serialize_with_flavor(&IndexWire(index, metadata), output);
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let mut raw = encoded.map_err(|_| invalid("Cannot encode Source binary index"))?;
    let length = u32::try_from(raw.len() - HEADER).map_err(|_| budget())?;
    raw[..8].copy_from_slice(if metadata { META_MAGIC } else { MAGIC });
    raw[8..12].copy_from_slice(&length.to_le_bytes());
    raw[12..HEADER].copy_from_slice(&identity_bytes(identity)?);
    check_control(control)?;
    Ok(raw)
}

fn check_control(control: &OperationControl) -> Result<()> {
    #[cfg(test)]
    CANCEL_AT_STEP.with(|slot| {
        if let Some((left, token)) = slot.borrow_mut().as_mut() {
            *left = left.saturating_sub(1);
            if *left == 0 {
                token.cancel();
            }
        }
    });
    control.check()
}
#[cfg(test)]
std::thread_local! {
    static CANCEL_AT_STEP: RefCell<Option<(usize, lycheedev_core::control::TrackedCancellation)>> = const { RefCell::new(None) };
    static CANCEL_AT_FINALIZE: RefCell<Option<lycheedev_core::control::TrackedCancellation>> = const { RefCell::new(None) };
}
#[cfg(test)]
pub(super) struct CancelAtStep;
#[cfg(test)]
impl CancelAtStep {
    pub(super) fn new(step: usize, control: &OperationControl) -> Self {
        CANCEL_AT_STEP.with(|slot| {
            assert!(
                slot.borrow_mut()
                    .replace((step, control.tracked_cancel_handle().unwrap()))
                    .is_none()
            )
        });
        Self
    }
}
#[cfg(test)]
impl Drop for CancelAtStep {
    fn drop(&mut self) {
        CANCEL_AT_STEP.with(|slot| *slot.borrow_mut() = None);
    }
}
#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
mod export_tests;
