//! One ordered Facts-to-Symbol mapping for retained and write-only consumers.
//! Summary rebuilds retain the original Facts/raw owners for cohort repair, but
//! never construct a second corpus-sized set of owned Symbol strings. Their
//! complete Index wire still includes every symbol, with at most one temporary
//! 36-byte declaration ID live while a borrowed row is serialized.
use super::{
    Document, Facts, SourceSummary, Symbol, SymbolIdentity,
    cache_projection::{CollectionLength, Counted},
    persistence_wire::{Collection, SymbolWire},
};
use lycheedev_core::{Result, ToolkitError, selection::SourcePin};
use serde::{Serialize, ser::SerializeSeq};
use std::borrow::Cow;

/// Only the two existing Symbol consumers select a cold representation. The
/// decoder's generic collection contract remains unchanged for every row type.
pub(super) trait RebuildSymbols: Collection<Symbol> {
    type Built: BuiltSymbols;
    fn finish_built(rows: Self::Built) -> Self;
}
impl RebuildSymbols for Vec<Symbol> {
    type Built = Vec<Symbol>;
    fn finish_built(rows: Self::Built) -> Self {
        rows
    }
}
impl RebuildSymbols for Counted<Symbol> {
    type Built = DeferredSymbols;
    fn finish_built(rows: Self::Built) -> Self {
        Self::from_count(rows.count)
    }
}

pub(super) trait BuiltSymbols: Default + CollectionLength {
    fn document(&mut self, pin: &SourcePin, path: &str, facts: &Facts) -> Result<()>;
    fn serialize<S: serde::Serializer>(
        &self,
        documents: &[Document],
        summary: &SourceSummary,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>;
}
impl BuiltSymbols for Vec<Symbol> {
    fn document(&mut self, pin: &SourcePin, path: &str, facts: &Facts) -> Result<()> {
        visit(&pin.repository, &pin.exact_commit, path, facts, |row| {
            #[cfg(test)]
            OWNED_ROWS.with(|count| {
                if let Some(value) = count.get() {
                    count.set(Some(value + 1));
                }
            });
            self.push(own(row));
            Ok(())
        })
    }
    fn serialize<S: serde::Serializer>(
        &self,
        _: &[Document],
        _: &SourceSummary,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter().map(SymbolWire))
    }
}

#[derive(Default)]
pub(super) struct DeferredSymbols {
    count: usize,
}
impl CollectionLength for DeferredSymbols {
    fn len(&self) -> usize {
        self.count
    }
}
impl BuiltSymbols for DeferredSymbols {
    fn document(&mut self, _: &SourcePin, _: &str, facts: &Facts) -> Result<()> {
        // Counting is not an extra ownership or file allowance. The unchanged
        // encoder enforces its 2M-row/128MiB limits before exposing a cache.
        for count in [
            facts.declarations.len(),
            facts.relationships.len(),
            facts.loads.len(),
            facts.headers.len(),
        ] {
            self.count = self.count.checked_add(count).ok_or_else(|| {
                ToolkitError::new("codebase.source_budget", "Source symbol count overflow", 3)
            })?;
        }
        Ok(())
    }
    fn serialize<S: serde::Serializer>(
        &self,
        documents: &[Document],
        summary: &SourceSummary,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.count))?;
        let mut emitted = 0usize;
        for document in documents {
            visit::<S::Error>(
                &summary.repository,
                &summary.commit,
                &document.path,
                &document.facts,
                |row| {
                    if emitted == self.count {
                        return Err(serde::ser::Error::custom("Source symbol count differs"));
                    }
                    #[cfg(test)]
                    let _active_id = ActiveId::new(&row.id);
                    sequence.serialize_element(&SymbolWire(&row))?;
                    emitted += 1;
                    Ok(())
                },
            )?;
        }
        if emitted != self.count {
            return Err(serde::ser::Error::custom("Source symbol count differs"));
        }
        sequence.end()
    }
}

pub(super) struct SymbolsWire<'a, S> {
    pub rows: &'a S,
    pub documents: &'a [Document],
    pub summary: &'a SourceSummary,
}
impl<S: BuiltSymbols> Serialize for SymbolsWire<'_, S> {
    fn serialize<X: serde::Serializer>(
        &self,
        serializer: X,
    ) -> std::result::Result<X::Ok, X::Error> {
        self.rows
            .serialize(self.documents, self.summary, serializer)
    }
}

fn own(row: Symbol<Cow<'_, str>>) -> Symbol {
    Symbol {
        id: row.id.into_owned(),
        kind: row.kind.into_owned(),
        name: row.name.into_owned(),
        target: row.target.into_owned(),
        category: row.category.into_owned(),
        confidence: row.confidence.into_owned(),
        path: row.path.into_owned(),
        line: row.line,
        end_line: row.end_line,
        signature: row.signature.into_owned(),
        scope: row.scope.into_owned(),
        ordinal: row.ordinal,
    }
}

/// Original document order and the four within-document arrays are preserved,
/// including duplicate rows. Only declaration IDs own memory in this view;
/// their exact commit-bound formula is shared with the public identity helper.
fn visit<E>(
    repository: &str,
    commit: &str,
    path: &str,
    facts: &Facts,
    mut emit: impl FnMut(Symbol<Cow<'_, str>>) -> std::result::Result<(), E>,
) -> std::result::Result<(), E> {
    use Cow::{Borrowed, Owned};
    if !facts.declarations.is_empty() {
        let identity = SymbolIdentity::for_fields(repository, commit, path);
        for (ordinal, d) in facts.declarations.iter().enumerate() {
            let mut row = Symbol {
                id: Borrowed(""),
                kind: Borrowed("declaration"),
                name: Borrowed(&d.name),
                target: Borrowed(""),
                category: Borrowed(&d.category),
                confidence: Borrowed("exact"),
                path: Borrowed(path),
                line: d.line,
                end_line: d.end_line,
                signature: Borrowed(&d.signature),
                scope: Borrowed(&d.scope),
                ordinal,
            };
            row.id = Owned(identity.stable_id(&row));
            emit(row)?;
        }
    }
    for r in &facts.relationships {
        emit(Symbol {
            id: Borrowed(""),
            kind: Borrowed("relationship"),
            name: Borrowed(&r.from),
            target: Borrowed(&r.to),
            category: Borrowed(&r.category),
            confidence: Borrowed(&r.confidence),
            path: Borrowed(path),
            line: r.line,
            end_line: r.line,
            signature: Borrowed(""),
            scope: Borrowed(""),
            ordinal: 0,
        })?;
    }
    for load in &facts.loads {
        emit(Symbol {
            id: Borrowed(""),
            kind: Borrowed("load"),
            name: Borrowed(path),
            target: Borrowed(&load.path),
            category: Borrowed(&load.kind),
            confidence: Borrowed("exact"),
            path: Borrowed(path),
            line: load.line,
            end_line: load.line,
            signature: Borrowed(""),
            scope: Borrowed(""),
            ordinal: 0,
        })?;
    }
    for header in &facts.headers {
        emit(Symbol {
            id: Borrowed(""),
            kind: Borrowed("header"),
            name: Borrowed(&header.key),
            target: Borrowed(&header.value),
            category: Borrowed("toc-field"),
            confidence: Borrowed("exact"),
            path: Borrowed(path),
            line: header.line,
            end_line: header.line,
            signature: Borrowed(""),
            scope: Borrowed(""),
            ordinal: 0,
        })?;
    }
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static OWNED_ROWS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static ACTIVE_ID_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SERIALIZATION_PEAK: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
struct ActiveId;
#[cfg(test)]
impl ActiveId {
    fn new(id: &Cow<'_, str>) -> Self {
        let bytes = match id {
            Cow::Owned(value) => value.capacity(),
            Cow::Borrowed(_) => 0,
        };
        ACTIVE_ID_BYTES.with(|slot| assert_eq!(slot.replace(bytes), 0));
        Self
    }
}
#[cfg(test)]
impl Drop for ActiveId {
    fn drop(&mut self) {
        ACTIVE_ID_BYTES.with(|slot| slot.set(0));
    }
}
#[cfg(test)]
pub(super) fn observe_output_capacity(capacity: usize) {
    let id = ACTIVE_ID_BYTES.with(|slot| slot.get());
    if id != 0 {
        SERIALIZATION_PEAK.with(|slot| {
            if slot
                .get()
                .is_some_and(|(old_id, old_capacity)| id + capacity > old_id + old_capacity)
            {
                slot.set(Some((id, capacity)));
            }
        });
    }
}

#[cfg(test)]
mod tests;
