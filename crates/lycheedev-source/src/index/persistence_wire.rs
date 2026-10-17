//! The existing index bytes, serialized from borrowed fields instead of a
//! second full JSON DOM. This is private persistence order, not a change to the
//! public Index/Symbol serde contracts. Arrays keep their original order and
//! duplicates; safely constructed Value subtrees keep their number and null
//! semantics. Current index producers use integers, strings, booleans and null;
//! hidden unchecked Number constructors are outside that producer contract.
use super::{Document, Index, Symbol, facts_cache::Locator};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};

pub(super) fn encode(index: &Index) -> serde_json::Result<Vec<u8>> {
    // Keep the complete encoded Vec and the original downstream 128 MiB
    // publisher check. No streaming publication, early cap, or partial object.
    serde_json::to_vec(&IndexWire(index))
}

struct IndexWire<'a>(&'a Index);
impl Serialize for IndexWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let index = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("assets", &index.assets)?;
        map.serialize_entry("compatibilityGaps", &index.compatibility_gaps)?;
        map.serialize_entry("documents", &DocumentsWire(&index.documents))?;
        // build_uncached always creates an object; preserve the old required
        // object contract rather than silently accepting an invalid summary.
        map.serialize_entry("summary", &SummaryWire(index.summary.as_object().unwrap()))?;
        map.serialize_entry("symbols", &SymbolsWire(&index.symbols))?;
        map.end()
    }
}

struct SummaryWire<'a>(&'a Map<String, Value>);
impl Serialize for SummaryWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer
            .serialize_map(Some(self.0.len() - usize::from(self.0.contains_key("io"))))?;
        for (key, value) in self.0 {
            if key != "io" {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

struct DocumentsWire<'a>(&'a [Document]);
impl Serialize for DocumentsWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(DocumentWire))
    }
}
struct DocumentWire<'a>(&'a Document);
impl Serialize for DocumentWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let document = self.0;
        let mut map =
            serializer.serialize_map(Some(3 + usize::from(document.facts_locator.is_some())))?;
        map.serialize_entry("bytes", &document.bytes)?;
        if let Some(locator) = &document.facts_locator {
            map.serialize_entry("factsLocator", &LocatorWire(locator))?;
        }
        map.serialize_entry("path", &document.path)?;
        map.serialize_entry("sha256", &document.sha256)?;
        map.end()
    }
}

struct LocatorWire<'a>(&'a Locator);
impl Serialize for LocatorWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let locator = self.0;
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("bytes", &locator.bytes)?;
        map.serialize_entry("group", &DigestWire(&locator.group))?;
        map.serialize_entry("offset", &locator.offset)?;
        map.serialize_entry("sha256", &DigestWire(&locator.sha256))?;
        map.end()
    }
}
struct DigestWire<'a>(&'a [u8; 32]);
impl Serialize for DigestWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = [0; 64];
        for (byte, pair) in self.0.iter().zip(encoded.chunks_exact_mut(2)) {
            pair[0] = HEX[(byte >> 4) as usize];
            pair[1] = HEX[(byte & 15) as usize];
        }
        serializer.serialize_str(std::str::from_utf8(&encoded).expect("hex digits are ASCII"))
    }
}

struct SymbolsWire<'a>(&'a [Symbol]);
impl Serialize for SymbolsWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(SymbolWire))
    }
}
struct SymbolWire<'a>(&'a Symbol);
impl Serialize for SymbolWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let symbol = self.0;
        let optional = usize::from(!symbol.id.is_empty())
            + usize::from(symbol.ordinal != 0)
            + usize::from(!symbol.scope.is_empty())
            + usize::from(!symbol.signature.is_empty())
            + usize::from(!symbol.target.is_empty());
        let mut map = serializer.serialize_map(Some(7 + optional))?;
        map.serialize_entry("category", &symbol.category)?;
        map.serialize_entry("confidence", &symbol.confidence)?;
        map.serialize_entry("endLine", &symbol.end_line)?;
        if !symbol.id.is_empty() {
            map.serialize_entry("id", &symbol.id)?;
        }
        map.serialize_entry("kind", &symbol.kind)?;
        map.serialize_entry("line", &symbol.line)?;
        map.serialize_entry("name", &symbol.name)?;
        if symbol.ordinal != 0 {
            map.serialize_entry("ordinal", &symbol.ordinal)?;
        }
        map.serialize_entry("path", &symbol.path)?;
        if !symbol.scope.is_empty() {
            map.serialize_entry("scope", &symbol.scope)?;
        }
        if !symbol.signature.is_empty() {
            map.serialize_entry("signature", &symbol.signature)?;
        }
        if !symbol.target.is_empty() {
            map.serialize_entry("target", &symbol.target)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
