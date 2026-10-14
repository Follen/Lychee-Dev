//! Go encoding/json's typed checkpoint reader: ordered duplicate fields,
//! folded names, scalar null no-ops and nested struct merge. This accepts
//! historical wire spelling without changing immutable capture bytes.
use super::{Request, RequestFilter, State};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

trait Object {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error>;
}
struct ObjectSeed<'a, T>(&'a mut T);
impl<'de, T: Object> DeserializeSeed<'de> for ObjectSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        struct ObjectVisitor<'a, T>(&'a mut T);
        impl<'de, T: Object> Visitor<'de> for ObjectVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while let Some(key) = map.next_key::<String>()? {
                    let key: String = key
                        .chars()
                        .map(|ch| match ch {
                            '\u{017f}' => 's',
                            '\u{212a}' => 'k',
                            _ => ch.to_ascii_lowercase(),
                        })
                        .collect();
                    self.0.field(&key, &mut map)?;
                }
                Ok(())
            }
        }
        deserializer.deserialize_any(ObjectVisitor(self.0))
    }
}
struct InPlaceSeed<'a, T>(&'a mut T);
impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for InPlaceSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        T::deserialize_in_place(deserializer, self.0)
    }
}
fn scalar<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    value: &mut T,
) -> Result<(), A::Error> {
    if let Some(next) = map.next_value::<Option<T>>()? {
        *value = next;
    }
    Ok(())
}
fn signed<'de, A: MapAccess<'de>>(map: &mut A, value: &mut i64) -> Result<(), A::Error> {
    if let Some(next) = map.next_value::<Option<lycheedev_core::evidence::GoSignedInteger>>()? {
        *value = next.0;
    }
    Ok(())
}
fn ignore<'de, A: MapAccess<'de>>(map: &mut A) -> Result<(), A::Error> {
    map.next_value::<IgnoredAny>().map(|_| ())
}
impl Object for State {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "snapshot" => scalar(map, &mut self.snapshot),
            "source" => map.next_value_seed(InPlaceSeed(&mut self.source)),
            "query" => map.next_value_seed(ObjectSeed(&mut self.query)),
            "pages" => map.next_value_seed(InPlaceSeed(&mut self.pages)),
            "matched" => scalar(map, &mut self.matched),
            "returned" => scalar(map, &mut self.returned),
            "decoded" => scalar(map, &mut self.decoded),
            "nopayload" => scalar(map, &mut self.no_payload),
            "notvalid" => scalar(map, &mut self.not_valid),
            "raw" => scalar(map, &mut self.raw),
            "nextindex" => {
                self.next_index = map.next_value()?;
                Ok(())
            }
            "complete" => scalar(map, &mut self.complete),
            "truncated" => scalar(map, &mut self.truncated),
            _ => ignore(map),
        }
    }
}
impl Object for Request {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "file" => scalar(map, &mut self.file),
            "from" => scalar(map, &mut self.from),
            "table" => scalar(map, &mut self.table),
            "offline" => scalar(map, &mut self.offline),
            "filter" => map.next_value_seed(ObjectSeed(&mut self.filter)),
            "maxbytes" => signed(map, &mut self.max_bytes),
            _ => ignore(map),
        }
    }
}
impl Object for RequestFilter {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "build" => scalar(map, &mut self.build),
            "tablehash" => {
                self.table_hash = map.next_value()?;
                Ok(())
            }
            "recordid" => {
                self.record_id = map.next_value()?;
                Ok(())
            }
            "push" => {
                self.push = map
                    .next_value::<Option<lycheedev_core::evidence::GoSignedInteger>>()?
                    .map(|value| {
                        i32::try_from(value.0).map_err(<A::Error as serde::de::Error>::custom)
                    })
                    .transpose()?;
                Ok(())
            }
            "region" => {
                self.region = map.next_value()?;
                Ok(())
            }
            "status" => {
                self.status = map.next_value()?;
                Ok(())
            }
            "latest" => scalar(map, &mut self.latest),
            "afterindex" => {
                self.after_index = map.next_value()?;
                Ok(())
            }
            "limit" => signed(map, &mut self.limit),
            _ => ignore(map),
        }
    }
}
macro_rules! deserialize_object {
    ($($ty:ty),+ $(,)?) => {$ (
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let mut value = Self::default();
                ObjectSeed(&mut value).deserialize(deserializer)?;
                Ok(value)
            }
        }
    )+};
}
deserialize_object!(State, Request);

// encoding/json reuses old slice backing slots when a duplicate array is
// shorter, so a later null string element retains that slot's old value.
// Both null and [] discard the backing, but marshal as null and [] respectively.
#[derive(Default)]
pub(super) struct GoStrings {
    items: Vec<String>,
    length: Option<usize>,
}
impl GoStrings {
    pub(super) fn empty() -> Self {
        Self {
            items: Vec::new(),
            length: Some(0),
        }
    }
    pub(super) fn values(&self) -> &[String] {
        &self.items[..self.length.unwrap_or(0)]
    }
    pub(super) fn len(&self) -> usize {
        self.length.unwrap_or(0)
    }
    pub(super) fn push(&mut self, value: String) {
        let length = self.len();
        if length == self.items.len() {
            self.items.push(value);
        } else {
            self.items[length] = value;
        }
        self.length = Some(length + 1);
    }
}
impl Serialize for GoStrings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.length {
            Some(_) => self.values().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}
impl<'de> Deserialize<'de> for GoStrings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = Self::default();
        Self::deserialize_in_place(deserializer, &mut value)?;
        Ok(value)
    }
    fn deserialize_in_place<D: Deserializer<'de>>(
        deserializer: D,
        place: &mut Self,
    ) -> Result<(), D::Error> {
        struct StringsVisitor<'a>(&'a mut GoStrings);
        impl<'de> Visitor<'de> for StringsVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                *self.0 = GoStrings::default();
                Ok(())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
                let mut length = 0;
                while let Some(value) = seq.next_element::<Option<String>>()? {
                    if length == self.0.items.len() {
                        self.0.items.push(String::new());
                    }
                    if let Some(value) = value {
                        self.0.items[length] = value;
                    }
                    length += 1;
                }
                self.0.length = Some(length);
                if length == 0 {
                    self.0.items.clear();
                }
                Ok(())
            }
        }
        deserializer.deserialize_any(StringsVisitor(place))
    }
}
