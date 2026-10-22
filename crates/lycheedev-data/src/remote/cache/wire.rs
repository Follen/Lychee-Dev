//! Typed current cache records. Referenced bytes are authenticated on consumption.
pub(crate) use lycheedev_core::evidence::BlobRef;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[derive(Clone, Debug)]
pub(crate) struct ObservationTime(String);
impl Default for ObservationTime {
    fn default() -> Self {
        Self("0001-01-01T00:00:00Z".into())
    }
}
impl ObservationTime {
    pub fn parse(raw: &str) -> Option<Self> {
        let time = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
        Some(Self(
            time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
        ))
    }
    pub fn canonical(&self) -> Option<&str> {
        Some(&self.0)
    }
    pub fn is_zero(&self) -> bool {
        chrono::DateTime::parse_from_rfc3339(&self.0).is_ok_and(|time| {
            time.timestamp() == -62135596800 && time.timestamp_subsec_nanos() == 0
        })
    }
}
impl Serialize for ObservationTime {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for ObservationTime {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom("invalid RFC3339 observation time"))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Observation {
    pub product_code: String,
    pub region: String,
    pub observed_at: ObservationTime,
    pub versions: BlobRef,
    pub distribution: BlobRef,
}
