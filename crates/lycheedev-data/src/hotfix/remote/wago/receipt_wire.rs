//! Receipt decoding uses the fixed Go historical object rules; only the caller
//! authenticates identity and consumes the referenced body.
use super::*;
use crate::remote::cache::wire::{CachedBlob, Object, ObjectSeed, folded, scalar};
use serde::{
    Deserialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};
use serde_json::value::RawValue;
impl Object for Identity {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "provider" => scalar(&mut map, &mut self.provider)?,
                "parser" => scalar(&mut map, &mut self.parser)?,
                "product" => scalar(&mut map, &mut self.product)?,
                "fullbuild" => scalar(&mut map, &mut self.full_build)?,
                "region" => scalar(&mut map, &mut self.region)?,
                "locale" => scalar(&mut map, &mut self.locale)?,
                "search" => scalar(&mut map, &mut self.search)?,
                "page" => signed(&mut map, &mut self.page)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
fn signed<'de, A: MapAccess<'de>>(
    map: &mut A,
    place: &mut i64,
) -> std::result::Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<evidence::GoSignedInteger>>()? {
        *place = value.0;
    }
    Ok(())
}
struct Wire {
    receipt: Receipt,
    blob: CachedBlob,
}
impl Default for Wire {
    fn default() -> Self {
        Self {
            receipt: Receipt {
                schema: String::new(),
                identity: Identity::default(),
                request_url: String::new(),
                response: Value::Null,
                captured_at: "0001-01-01T00:00:00Z".into(),
                current_page: 0,
                last_page: 0,
                per_page: 0,
                total: 0,
                next_page_url: String::new(),
            },
            blob: CachedBlob::default(),
        }
    }
}
impl Object for Wire {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "schema" => scalar(&mut map, &mut self.receipt.schema)?,
                "identity" => map.next_value_seed(ObjectSeed(&mut self.receipt.identity))?,
                "requesturl" => scalar(&mut map, &mut self.receipt.request_url)?,
                "response" => map.next_value_seed(ObjectSeed(&mut self.blob))?,
                "capturedat" => {
                    if let Some(raw) = map.next_value::<Option<Box<RawValue>>>()? {
                        let interior = raw
                            .get()
                            .strip_prefix('"')
                            .and_then(|s| s.strip_suffix('"'));
                        let (raw, parsed) = interior
                            .and_then(|s| evidence::parse_go_timestamp(s).map(|p| (s, p)))
                            .ok_or_else(|| {
                                serde::de::Error::custom("invalid Go receipt timestamp")
                            })?;
                        self.receipt.captured_at = parsed.canonical().unwrap_or(raw).to_owned();
                    }
                }
                "currentpage" => signed(&mut map, &mut self.receipt.current_page)?,
                "lastpage" => signed(&mut map, &mut self.receipt.last_page)?,
                "perpage" => signed(&mut map, &mut self.receipt.per_page)?,
                "total" => signed(&mut map, &mut self.receipt.total)?,
                "nextpageurl" => scalar(&mut map, &mut self.receipt.next_page_url)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
pub(super) fn decode(raw: &[u8], inv: &Invocation, budget: &Budget) -> Result<Receipt> {
    check_scope(inv, budget)?;
    let grammar = lycheedev_core::raw_json::validate_with_check(raw, || check_scope(inv, budget))?;
    if grammar.is_err() {
        return finish_scope(
            Err(error("cache_corrupt", 4, "invalid receipt JSON")),
            inv,
            budget,
        );
    }
    let _scratch = budget.reserve_retained(raw.len().checked_mul(3).ok_or_else(limit)?)?;
    let view =
        evidence::normalize_go_json_strings_with_check(raw, &inv.control, || budget.check())?;
    let mut wire = Wire::default();
    let mut reader = serde_json::Deserializer::from_slice(&view);
    let result = ObjectSeed(&mut wire)
        .deserialize(&mut reader)
        .and_then(|()| reader.end())
        .map_err(|e| error("cache_corrupt", 4, e.to_string()));
    finish_scope(result, inv, budget)?;
    wire.receipt.response = serde_json::to_value(&wire.blob)?;
    Ok(wire.receipt)
}
