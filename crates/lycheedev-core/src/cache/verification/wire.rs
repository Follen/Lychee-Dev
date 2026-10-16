use super::*;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;
pub(super) fn error(message: impl fmt::Display) -> ToolkitError {
    ToolkitError::new(
        "vault.cache_protection",
        format!("vault.cache_protection: {message}"),
        4,
    )
}
fn field(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
#[derive(Default)]
pub(super) struct Owner {
    kind: String,
    owner: String,
}
impl Owner {
    pub(super) fn validate(&self) -> Result<()> {
        if !["pinned", "evidence", "operation"].contains(&self.kind.as_str()) {
            return Err(error(format!("unknown kind {:?}", self.kind)));
        }
        if self.owner.is_empty()
            || self.owner.len() > 128
            || self.owner.chars().any(|c| c < ' ' || c == '\u{7f}')
        {
            return Err(error("invalid owner"));
        }
        Ok(())
    }
}
struct OwnerSeed<'a>(&'a mut Owner);
impl<'de> DeserializeSeed<'de> for OwnerSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct Fields<'a>(&'a mut Owner);
        impl<'de> Visitor<'de> for Fields<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a cache owner")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> std::result::Result<(), M::Error> {
                while let Some(key) = m.next_key::<String>()? {
                    let into = match field(&key).as_str() {
                        "kind" => &mut self.0.kind,
                        "owner" => &mut self.0.owner,
                        "retainedat" => {
                            let raw = m.next_value::<Box<RawValue>>()?;
                            if raw.get() != "null" {
                                let token = raw
                                    .get()
                                    .strip_prefix('"')
                                    .and_then(|v| v.strip_suffix('"'))
                                    .ok_or_else(|| {
                                        serde::de::Error::custom(
                                            "Time.UnmarshalJSON: input is not a JSON string",
                                        )
                                    })?;
                                crate::evidence::parse_go_timestamp(token).ok_or_else(|| {
                                    serde::de::Error::custom("invalid Go cache timestamp")
                                })?;
                            }
                            continue;
                        }
                        _ => {
                            m.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    if let Some(value) = m.next_value::<Option<String>>()? {
                        *into = value;
                    }
                }
                Ok(())
            }
        }
        d.deserialize_any(Fields(self.0))
    }
}
/// Go resets the slice length but reuses its element backing across duplicate
/// owners fields; a later null element can retain an earlier owner structure.
#[derive(Default)]
pub(super) struct Owners {
    elements: Vec<Owner>,
    length: usize,
}
impl Owners {
    pub(super) fn iter(&self) -> impl Iterator<Item = &Owner> {
        self.elements[..self.length].iter()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.length == 0
    }
}
struct OwnersSeed<'a>(&'a mut Owners);
impl<'de> DeserializeSeed<'de> for OwnersSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct Items<'a>(&'a mut Owners);
        impl<'de> Visitor<'de> for Items<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("cache owners")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = Owners::default();
                Ok(())
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> std::result::Result<(), S::Error> {
                let mut length = 0;
                loop {
                    if self.0.elements.len() == length {
                        self.0.elements.push(Owner::default());
                    }
                    if seq
                        .next_element_seed(OwnerSeed(&mut self.0.elements[length]))?
                        .is_none()
                    {
                        break;
                    }
                    length += 1;
                }
                if length == 0 {
                    *self.0 = Owners::default()
                } else {
                    self.0.length = length
                }
                Ok(())
            }
        }
        d.deserialize_any(Items(self.0))
    }
}
#[derive(Default)]
pub(super) struct Retain {
    pub sha256: String,
    pub owners: Owners,
}
impl<'de> Deserialize<'de> for Retain {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Retain;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a cache retention marker")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Retain, E> {
                Ok(Retain::default())
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Retain, M::Error> {
                let mut value = Retain::default();
                while let Some(key) = m.next_key::<String>()? {
                    match field(&key).as_str() {
                        "sha256" => {
                            if let Some(v) = m.next_value::<Option<String>>()? {
                                value.sha256 = v
                            }
                        }
                        "owners" => m.next_value_seed(OwnersSeed(&mut value.owners))?,
                        _ => {
                            m.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(value)
            }
        }
        d.deserialize_any(Fields)
    }
}
