//! Read-only Go JSON representations. References are authenticated on consumption.
use lycheedev_core::evidence::{self, BlobRef, GoSignedInteger};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;

#[derive(Clone, Debug)]
pub(crate) struct ObservationTime {
    canonical: Option<String>,
    zero: bool,
}
impl Default for ObservationTime {
    fn default() -> Self {
        Self {
            canonical: Some("0001-01-01T00:00:00Z".into()),
            zero: true,
        }
    }
}
impl ObservationTime {
    pub fn parse(raw_interior: &str) -> Option<Self> {
        let parsed = evidence::parse_go_timestamp(raw_interior)?;
        Some(Self {
            canonical: parsed.canonical().map(str::to_owned),
            zero: parsed.is_zero(),
        })
    }
    pub fn canonical(&self) -> Option<&str> {
        self.canonical.as_deref()
    }
    pub fn is_zero(&self) -> bool {
        self.zero
    }
}
impl Serialize for ObservationTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Go accepts offset >=24h on input but fails later at json.Marshal.
        serializer.serialize_str(
            self.canonical().ok_or_else(|| {
                serde::ser::Error::custom("Go time timezone outside RFC3339 range")
            })?,
        )
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct CachedBlob {
    pub sha256: String,
    pub bytes: i64,
}
impl TryFrom<BlobRef> for CachedBlob {
    type Error = lycheedev_core::ToolkitError;
    fn try_from(value: BlobRef) -> Result<Self, Self::Error> {
        Ok(Self {
            sha256: value.sha256,
            bytes: value.bytes.try_into().map_err(|_| super::identity())?,
        })
    }
}
impl CachedBlob {
    pub fn into_core(self) -> lycheedev_core::Result<BlobRef> {
        Ok(BlobRef {
            sha256: self.sha256,
            bytes: self.bytes.try_into().map_err(|_| super::identity())?,
        })
    }
}

/// Scalar null is a no-op; repeated struct fields merge into the existing value.
pub(crate) trait Object: Default {
    fn fields<'de, A: MapAccess<'de>>(&mut self, map: A) -> Result<(), A::Error>;
}
pub(crate) struct ObjectSeed<'a, T>(pub &'a mut T);
impl<'de, T: Object> DeserializeSeed<'de> for ObjectSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct Map<'a, T>(&'a mut T);
        impl<'de, T: Object> Visitor<'de> for Map<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<(), A::Error> {
                self.0.fields(map)
            }
        }
        d.deserialize_any(Map(self.0))
    }
}
pub(crate) fn folded(key: &str) -> String {
    // encoding/json SimpleFold matches these two non-ASCII runes to ASCII fields.
    key.chars()
        .map(|ch| match ch {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}
pub(crate) fn scalar<'de, T: Deserialize<'de>, A: MapAccess<'de>>(
    map: &mut A,
    place: &mut T,
) -> Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<T>>()? {
        *place = value;
    }
    Ok(())
}
impl Object for CachedBlob {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "sha256" => scalar(&mut map, &mut self.sha256)?,
                "bytes" => {
                    if let Some(value) = map.next_value::<Option<GoSignedInteger>>()? {
                        self.bytes = value.0;
                    }
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for CachedBlob {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Observation {
    pub product_code: String,
    pub region: String,
    pub observed_at: ObservationTime,
    pub versions: CachedBlob,
    pub distribution: CachedBlob,
}
impl Object for Observation {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "productcode" => scalar(&mut map, &mut self.product_code)?,
                "region" => scalar(&mut map, &mut self.region)?,
                "observedat" => {
                    if let Some(raw) = map.next_value::<Option<Box<RawValue>>>()? {
                        let interior = raw
                            .get()
                            .strip_prefix('"')
                            .and_then(|s| s.strip_suffix('"'));
                        self.observed_at =
                            interior.and_then(ObservationTime::parse).ok_or_else(|| {
                                serde::de::Error::custom("invalid Go observation time")
                            })?;
                    }
                }
                "versions" => map.next_value_seed(ObjectSeed(&mut self.versions))?,
                "distribution" => map.next_value_seed(ObjectSeed(&mut self.distribution))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Observation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
