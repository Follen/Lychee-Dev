//! Fixed Go 3.2.0 archive.go dependency consumers. Historical payloads are
//! inspected only through their original typed representations and byte caps.
use super::{BlobRef, CaptureRef, Provenance, bounded, names_capture, validate_capture};
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    metadata::{Document, TransactionReader},
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{borrow::Cow, fmt, path::Path};

/// A signed Go JSON integer decoded from its original number token.
/// RawValue preserves -0 and rejects floating/exponent spellings rather than
/// rounding them. It also prevents JSON objects masquerading as serde's
/// internal arbitrary-precision number representation from becoming numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoSignedInteger(pub i64);
impl<'de> Deserialize<'de> for GoSignedInteger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        let token = raw.get();
        let digits = token.strip_prefix('-').unwrap_or(token);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(serde::de::Error::custom(
                "expected a signed Go integer token",
            ));
        }
        token
            .parse::<i64>()
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Permissive embedded capture decoding for fixed-Go historical payloads.
/// This is a wire representation, not authenticated evidence. Reading a
/// checkpoint still requires validating its outer manifest and dependencies.
/// Repeated objects merge in input order through `deserialize_in_place`.
#[derive(Default)]
pub struct GoCaptureDecode(EmbeddedCapture);
impl GoCaptureDecode {
    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn data_build(&self) -> &str {
        &self.0.provenance.data_build
    }
}
impl<'de> Deserialize<'de> for GoCaptureDecode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        Self::deserialize_in_place(deserializer, &mut value)?;
        Ok(value)
    }
    fn deserialize_in_place<D: Deserializer<'de>>(
        deserializer: D,
        place: &mut Self,
    ) -> std::result::Result<(), D::Error> {
        ObjectSeed(&mut place.0).deserialize(deserializer)
    }
}
impl Serialize for GoCaptureDecode {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            schema: &'a str,
            id: &'a str,
            blob: &'a EmbeddedBlob,
            media_type: &'a str,
            provenance: &'a EmbeddedProvenance,
            complete: bool,
            truncated: bool,
            captured_at: &'a str,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            dependencies: &'a [String],
        }
        Wire {
            schema: &self.0.schema,
            id: &self.0.id,
            blob: &self.0.blob,
            media_type: &self.0.media_type,
            provenance: &self.0.provenance,
            complete: self.0.complete,
            truncated: self.0.truncated,
            captured_at: self.0.captured_at.canonical.as_deref().ok_or_else(|| {
                <S::Error as serde::ser::Error>::custom(
                    "capture timestamp offset cannot be marshaled by Go",
                )
            })?,
            dependencies: self.0.dependencies.values(),
        }
        .serialize(serializer)
    }
}
/// Normalize only JSON string tokens with Go's historical reader rules.
/// Callers must bound input bytes and account for at most 3x scratch allocation.
/// Original bytes and capture identities must never be replaced by this view.
pub fn normalize_go_json_strings<'a>(
    raw: &'a [u8],
    control: &OperationControl,
) -> Result<Cow<'a, [u8]>> {
    normalize_go_strings(raw, control)
}

pub(super) fn deserialize_dependencies<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

pub(super) fn references_capture(
    home: &Path,
    document: &Document,
    reader: &TransactionReader<'_>,
    target: &CaptureRef,
    control: &OperationControl,
) -> Result<bool> {
    control.check()?;
    if names_capture(&document.value, &target.id) {
        return Ok(true);
    }
    let Some(id) = document.key.strip_prefix("capture/") else {
        return Ok(false);
    };
    // Go reads a permissive projection before deciding whether this is a
    // known payload consumer. Unknown extension fields never invalidate an
    // unrelated capture; only selected consumers undergo manifest identity.
    // Read under the SAME write transaction. A Value map sorts names and
    // collapses repeated fields, which can reverse Go's last-field-wins rules
    // and hide a real payload consumer. Raw tokens also preserve time escapes.
    let manifest = reader.read_raw(&document.key)?;
    let needle = format!("\"{}\"", target.id);
    let locator = format!("\"capture:{}\"", target.id);
    if manifest
        .windows(needle.len())
        .any(|bytes| bytes == needle.as_bytes())
        || manifest
            .windows(locator.len())
            .any(|bytes| bytes == locator.as_bytes())
    {
        return Ok(true);
    }
    let manifest = normalize_go_strings(&manifest, control)?;
    let reference: EmbeddedCapture = serde_json::from_slice(&manifest)?;
    if reference.media_type != "application/json"
        || reference.provenance.kind != "asset-demux-manifest"
            && !reference.dependencies.values().is_empty()
    {
        return Ok(false);
    }
    let maximum = match (
        reference.provenance.kind.as_str(),
        target.provenance.kind.as_str(),
    ) {
        ("hotfix-scan", "hotfix-records") => 4 << 20,
        ("hotfix-records", "hotfix-cache")
            if reference.provenance.locator.starts_with("dbcache:sha256:") =>
        {
            32 << 20
        }
        ("asset-export-manifest", "asset-image") => 1 << 20,
        ("asset-demux-manifest", "asset" | "asset-demux-source" | "asset-demux-frame") => 16 << 20,
        _ => return Ok(false),
    };
    let reference = reference.into_capture()?;
    validate_capture(&reference, id)?;
    let raw = bounded::read_internal(
        home,
        &serde_json::to_value(&reference.blob)?,
        maximum,
        Some(control),
    )?;
    control.check()?;
    let normalized = normalize_go_strings(&raw, control)?;
    let raw = normalized.as_ref();
    let found = match reference.provenance.kind.as_str() {
        "hotfix-scan" => {
            let scan: Scan = serde_json::from_slice(raw)?;
            scan.pages.values().iter().any(|page| page == &target.id)
        }
        "hotfix-records" => {
            let page: Page = serde_json::from_slice(raw)?;
            page.snapshot == reference.provenance.snapshot
                && page.source.schema == "lycheedev.capture.v1"
                && page.source.id == target.id
        }
        "asset-export-manifest" => {
            let manifest: Export = serde_json::from_slice(raw)?;
            manifest.schema == "lycheedev.asset-export.v1"
                && manifest.artifact.is_some_and(|artifact| {
                    artifact.schema == "lycheedev.capture.v1" && artifact.id == target.id
                })
        }
        "asset-demux-manifest" => {
            let manifest: Demux = serde_json::from_slice(raw)?;
            manifest.schema == "lycheedev.asset-demux.v1"
                && if target.provenance.kind == "asset-demux-frame" {
                    manifest
                        .frames
                        .values()
                        .iter()
                        .any(|frame| frame.capture == target.id)
                } else {
                    manifest.source == target.id
                }
        }
        _ => unreachable!("known typed payload"),
    };
    control.check()?;
    Ok(found)
}

// Go's encoding/json accepts unknown fields, case-insensitive names, null
// scalar/struct fields and repeated fields in input order (nested objects
// merge). Preserve those read semantics without interpreting arbitrary fields
// as references or rewriting any historical bytes.
trait GoObject: Default {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error>;
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            let key = fold_field(&key);
            self.field(&key, &mut map)?;
        }
        Ok(())
    }
}
struct ObjectSeed<'a, T>(&'a mut T);
impl<'de, T: GoObject> DeserializeSeed<'de> for ObjectSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        struct ObjectVisitor<'a, T>(&'a mut T);
        impl<'de, T: GoObject> Visitor<'de> for ObjectVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
                self.0.fields(map)
            }
        }
        deserializer.deserialize_any(ObjectVisitor(self.0))
    }
}
struct OptionalObjectSeed<'a, T>(&'a mut Option<T>);
impl<'de, T: GoObject> DeserializeSeed<'de> for OptionalObjectSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        struct OptionalVisitor<'a, T>(&'a mut Option<T>);
        impl<'de, T: GoObject> Visitor<'de> for OptionalVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = None;
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
                self.0.get_or_insert_with(T::default).fields(map)
            }
        }
        deserializer.deserialize_any(OptionalVisitor(self.0))
    }
}
// encoding/json reuses slice backing elements across repeated array fields.
// Keep their prior zero/null-sensitive values even when an intermediate array
// shortens the logical length; null and [] discard that backing storage.
struct GoSlice<T> {
    elements: Vec<T>,
    length: usize,
}
impl<T> Default for GoSlice<T> {
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            length: 0,
        }
    }
}
impl<T> GoSlice<T> {
    fn values(&self) -> &[T] {
        &self.elements[..self.length]
    }
}
trait GoElement: Default {
    fn decode<'de, D: Deserializer<'de>>(
        &mut self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error>;
}
impl GoElement for String {
    fn decode<'de, D: Deserializer<'de>>(
        &mut self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        if let Some(value) = Option::<String>::deserialize(deserializer)? {
            *self = value;
        }
        Ok(())
    }
}
impl GoElement for Frame {
    fn decode<'de, D: Deserializer<'de>>(
        &mut self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        ObjectSeed(self).deserialize(deserializer)
    }
}
struct ElementSeed<'a, T>(&'a mut T);
impl<'de, T: GoElement> DeserializeSeed<'de> for ElementSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        self.0.decode(deserializer)
    }
}
struct SliceSeed<'a, T>(&'a mut GoSlice<T>);
impl<'de, T: GoElement> DeserializeSeed<'de> for SliceSeed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        struct SliceVisitor<'a, T>(&'a mut GoSlice<T>);
        impl<'de, T: GoElement> Visitor<'de> for SliceVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an array or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = GoSlice::default();
                Ok(())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
                let mut length = 0;
                loop {
                    if length == self.0.elements.len() {
                        self.0.elements.push(T::default());
                    }
                    if seq
                        .next_element_seed(ElementSeed(&mut self.0.elements[length]))?
                        .is_none()
                    {
                        break;
                    }
                    length += 1;
                }
                self.0.length = length;
                if length == 0 {
                    self.0.elements.clear();
                }
                Ok(())
            }
        }
        deserializer.deserialize_any(SliceVisitor(self.0))
    }
}
fn scalar<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    value: &mut T,
) -> std::result::Result<(), A::Error> {
    if let Some(next) = map.next_value::<Option<T>>()? {
        *value = next;
    }
    Ok(())
}
fn ignore<'de, A: MapAccess<'de>>(map: &mut A) -> std::result::Result<(), A::Error> {
    map.next_value::<IgnoredAny>().map(|_| ())
}
macro_rules! deserialize_object {
    ($($ty:ty),+ $(,)?) => {$ (
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
                let mut value = Self::default();
                ObjectSeed(&mut value).deserialize(deserializer)?;
                Ok(value)
            }
        }
    )+};
}
#[derive(Default)]
struct Scan {
    pages: GoSlice<String>,
}
impl GoObject for Scan {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        if key == "pages" {
            map.next_value_seed(SliceSeed(&mut self.pages))
        } else {
            ignore(map)
        }
    }
}
#[derive(Default)]
struct Page {
    snapshot: String,
    source: EmbeddedCapture,
}
impl GoObject for Page {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "snapshot" => scalar(map, &mut self.snapshot),
            "source" => map.next_value_seed(ObjectSeed(&mut self.source)),
            _ => ignore(map),
        }
    }
}
#[derive(Default)]
struct Export {
    schema: String,
    artifact: Option<EmbeddedCapture>,
}
impl GoObject for Export {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(map, &mut self.schema),
            "artifactcapture" => map.next_value_seed(OptionalObjectSeed(&mut self.artifact)),
            _ => ignore(map),
        }
    }
}
#[derive(Default)]
struct Demux {
    schema: String,
    source: String,
    frames: GoSlice<Frame>,
}
impl GoObject for Demux {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(map, &mut self.schema),
            "sourcecapture" => scalar(map, &mut self.source),
            "frames" => map.next_value_seed(SliceSeed(&mut self.frames)),
            _ => ignore(map),
        }
    }
}
#[derive(Default)]
struct Frame {
    capture: String,
}
impl GoObject for Frame {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        if key == "capture" {
            scalar(map, &mut self.capture)
        } else {
            ignore(map)
        }
    }
}
struct EmbeddedCapture {
    schema: String,
    id: String,
    blob: EmbeddedBlob,
    media_type: String,
    provenance: EmbeddedProvenance,
    complete: bool,
    truncated: bool,
    captured_at: GoTimestamp,
    dependencies: GoSlice<String>,
}
impl Default for EmbeddedCapture {
    fn default() -> Self {
        Self {
            schema: String::new(),
            id: String::new(),
            blob: EmbeddedBlob::default(),
            media_type: String::new(),
            provenance: EmbeddedProvenance::default(),
            complete: false,
            truncated: false,
            captured_at: GoTimestamp::default(),
            dependencies: GoSlice::default(),
        }
    }
}
impl EmbeddedCapture {
    fn into_capture(self) -> Result<CaptureRef> {
        let mut dependencies = self.dependencies.elements;
        dependencies.truncate(self.dependencies.length);
        Ok(CaptureRef {
            schema: self.schema,
            id: self.id,
            blob: BlobRef {
                sha256: self.blob.sha256,
                bytes: self
                    .blob
                    .bytes
                    .try_into()
                    .map_err(|_| ToolkitError::invalid("capture blob length is negative"))?,
            },
            media_type: self.media_type,
            provenance: self.provenance.into_provenance(),
            complete: self.complete,
            truncated: self.truncated,
            captured_at: self.captured_at.canonical.ok_or_else(|| {
                ToolkitError::invalid("capture timestamp offset cannot be marshaled by Go")
            })?,
            dependencies,
        })
    }
}
impl GoObject for EmbeddedCapture {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(map, &mut self.schema),
            "id" => scalar(map, &mut self.id),
            "blob" => map.next_value_seed(ObjectSeed(&mut self.blob)),
            "provenance" => map.next_value_seed(ObjectSeed(&mut self.provenance)),
            "mediatype" => scalar(map, &mut self.media_type),
            "complete" => scalar(map, &mut self.complete),
            "truncated" => scalar(map, &mut self.truncated),
            "capturedat" => scalar(map, &mut self.captured_at),
            "dependencies" => map.next_value_seed(SliceSeed(&mut self.dependencies)),
            _ => ignore(map),
        }
    }
}
#[derive(Default, Serialize)]
struct EmbeddedBlob {
    sha256: String,
    bytes: i64,
}
impl GoObject for EmbeddedBlob {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "sha256" => scalar(map, &mut self.sha256),
            "bytes" => {
                if let Some(next) = map.next_value::<Option<GoSignedInteger>>()? {
                    self.bytes = next.0;
                }
                Ok(())
            }
            _ => ignore(map),
        }
    }
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedProvenance {
    kind: String,
    locator: String,
    snapshot: String,
    operation_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    source_commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    base_snapshot: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    base_source_commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    data_build: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    session: String,
}
impl EmbeddedProvenance {
    fn into_provenance(self) -> Provenance {
        Provenance {
            kind: self.kind,
            locator: self.locator,
            snapshot: self.snapshot,
            operation_id: self.operation_id,
            source_commit: self.source_commit,
            base_snapshot: self.base_snapshot,
            base_source_commit: self.base_source_commit,
            data_build: self.data_build,
            session: self.session,
        }
    }
}
impl GoObject for EmbeddedProvenance {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "kind" => scalar(map, &mut self.kind),
            "locator" => scalar(map, &mut self.locator),
            "snapshot" => scalar(map, &mut self.snapshot),
            "operationid" => scalar(map, &mut self.operation_id),
            "sourcecommit" => scalar(map, &mut self.source_commit),
            "basesnapshot" => scalar(map, &mut self.base_snapshot),
            "basesourcecommit" => scalar(map, &mut self.base_source_commit),
            "databuild" => scalar(map, &mut self.data_build),
            "session" => scalar(map, &mut self.session),
            _ => ignore(map),
        }
    }
}
deserialize_object!(Scan, Page, Export, Demux, Frame, EmbeddedCapture);

// The Go 1.27.1 time.Time JSON reader uses the RFC3339 layout fallback, not
// chrono's general parser. That fallback also accepts a one-digit hour,
// comma fractions, and offsets with hour=24 or minute=60. JSON escape bytes
// are NOT unescaped by Time.UnmarshalJSON (go.dev/issue/47353).
pub struct GoTimestamp {
    canonical: Option<String>,
    zero: bool,
}
impl GoTimestamp {
    /// Go JSON marshal form, absent when a parsed timezone cannot be marshalled.
    pub fn canonical(&self) -> Option<&str> {
        self.canonical.as_deref()
    }

    /// Whether the represented UTC instant is Go's zero time.
    pub fn is_zero(&self) -> bool {
        self.zero
    }
}

impl Default for GoTimestamp {
    fn default() -> Self {
        Self {
            canonical: Some("0001-01-01T00:00:00Z".into()),
            zero: true,
        }
    }
}
impl<'de> Deserialize<'de> for GoTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        parse_go_timestamp(&value)
            .ok_or_else(|| serde::de::Error::custom("invalid Go capture timestamp"))
    }
}
/// Parse the raw interior bytes of a Go time.Time JSON string token without JSON-unescaping them.
/// Callers must preserve the original token: JSON escape sequences are not
/// accepted by Go's time reader. Parse success does not imply marshal success.
pub fn parse_go_timestamp(value: &str) -> Option<GoTimestamp> {
    fn number(raw: &[u8]) -> Option<u32> {
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(raw.iter().fold(0, |n, ch| n * 10 + u32::from(*ch - b'0')))
    }
    let raw = value.as_bytes();
    if raw.len() < 19
        || raw.get(4) != Some(&b'-')
        || raw.get(7) != Some(&b'-')
        || raw.get(10) != Some(&b'T')
    {
        return None;
    }
    let year = number(&raw[..4])?;
    let month = number(&raw[5..7])?;
    let day = number(&raw[8..10])?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days {
        return None;
    }
    let hour_length = if raw.get(12) == Some(&b':') { 1 } else { 2 };
    let colon = 11 + hour_length;
    if raw.get(colon) != Some(&b':') || raw.get(colon + 3) != Some(&b':') {
        return None;
    }
    let hour = number(raw.get(11..colon)?)?;
    let minute = number(raw.get(colon + 1..colon + 3)?)?;
    let second = number(raw.get(colon + 4..colon + 6)?)?;
    if hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    let mut at = colon + 6;
    let mut fraction = "";
    if matches!(raw.get(at), Some(b'.' | b',')) {
        at += 1;
        let start = at;
        while raw.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return None;
        }
        fraction = std::str::from_utf8(&raw[start..at.min(start + 9)])
            .ok()?
            .trim_end_matches('0');
    }
    let zone = raw.get(at..)?;
    let mut offset = 0;
    if zone != b"Z" {
        if zone.len() != 6 || !matches!(zone[0], b'+' | b'-') || zone[3] != b':' {
            return None;
        }
        let hour = number(&zone[1..3])?;
        let minute = number(&zone[4..6])?;
        if hour > 24 || minute > 60 {
            return None;
        }
        offset = hour * 60 + minute;
    }
    let signed_minutes = if zone.first() == Some(&b'-') {
        -(i64::from(offset))
    } else {
        i64::from(offset)
    };
    let local = chrono::NaiveDate::from_ymd_opt(year as i32, month, day)?
        .and_hms_opt(hour, minute, second)?;
    let zero = fraction.is_empty()
        && local.checked_sub_signed(chrono::Duration::minutes(signed_minutes))
            == chrono::NaiveDate::from_ymd_opt(1, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0));
    // Go can parse a >=24h offset but refuses to marshal it into an identity.
    if offset >= 24 * 60 {
        return Some(GoTimestamp {
            canonical: None,
            zero,
        });
    }
    let mut canonical = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if !fraction.is_empty() {
        canonical.push('.');
        canonical.push_str(fraction);
    }
    if offset == 0 {
        canonical.push('Z');
    } else {
        canonical.push(char::from(zone[0]));
        canonical.push_str(&format!("{:02}:{:02}", offset / 60, offset % 60));
    }
    Some(GoTimestamp {
        canonical: Some(canonical),
        zero,
    })
}

fn fold_field(key: &str) -> String {
    key.chars()
        .map(|ch| match ch {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}

// Normalize only JSON string tokens for serde's typed consumer. Original
// blobs and byte limits are untouched. Go replaces each invalid UTF-8 byte
// and each unpaired UTF-16 escape with U+FFFD; malformed JSON syntax stays an
// error. The output is bounded by three times the original producer cap.
fn normalize_go_strings<'a>(raw: &'a [u8], control: &OperationControl) -> Result<Cow<'a, [u8]>> {
    normalize_go_strings_with_time_key(raw, control, "capturedat")
}
fn normalize_go_strings_with_time_key<'a>(
    raw: &'a [u8],
    control: &OperationControl,
    time_key: &str,
) -> Result<Cow<'a, [u8]>> {
    normalize_go_strings_checked(raw, time_key, &mut || control.check())
}
fn needs_string_normalization(raw: &[u8], check: &mut impl FnMut() -> Result<()>) -> Result<bool> {
    let mut at = 0;
    while at < raw.len() {
        check()?;
        let mut end = at.saturating_add(65536).min(raw.len());
        match std::str::from_utf8(&raw[at..end]) {
            Ok(_) => {}
            Err(e) if e.error_len().is_none() && end < raw.len() => end = at + e.valid_up_to(),
            Err(_) => return Ok(true),
        }
        // Include the next byte so an escape crossing a scan boundary counts.
        if raw[at..end.saturating_add(1).min(raw.len())]
            .windows(2)
            .any(|pair| pair == b"\\u")
        {
            return Ok(true);
        }
        at = end;
    }
    check()?;
    Ok(false)
}
fn normalize_go_strings_checked<'a>(
    raw: &'a [u8],
    time_key: &str,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Cow<'a, [u8]>> {
    check()?;
    // Normal producer JSON needs no second body allocation. Only Unicode
    // escapes can hide a valid timestamp character from the raw Go reader;
    // the other JSON escapes produce characters the time grammar rejects.
    let needs_normalization = needs_string_normalization(raw, check)?;
    check()?;
    if !needs_normalization {
        return Ok(Cow::Borrowed(raw));
    }
    fn hex4(raw: &[u8]) -> Option<u16> {
        if raw.len() < 4 {
            return None;
        }
        raw[..4].iter().try_fold(0u16, |n, ch| {
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => return None,
            };
            Some((n << 4) | u16::from(digit))
        })
    }
    fn skip_space(
        raw: &[u8],
        mut at: usize,
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<usize> {
        let mut next = at.saturating_add(65536);
        while raw
            .get(at)
            .is_some_and(|ch| matches!(ch, b' ' | b'\t' | b'\r' | b'\n'))
        {
            if at >= next {
                check()?;
                next = at.saturating_add(65536);
            }
            at += 1;
        }
        Ok(at)
    }
    let maximum = raw
        .len()
        .checked_mul(3)
        .ok_or_else(|| ToolkitError::invalid("legacy JSON normalization exceeds limit"))?;
    let mut out = Vec::new();
    out.try_reserve_exact(maximum)
        .map_err(|_| ToolkitError::invalid("legacy JSON normalization allocation failed"))?;
    let mut at = 0;
    let mut timestamp_at = None;
    let mut next_check = 0;
    while at < raw.len() {
        if at >= next_check {
            check()?;
            next_check = at.saturating_add(65536);
        }
        if raw[at] != b'"' {
            out.push(raw[at]);
            at += 1;
            continue;
        }
        let start = at;
        let output_start = out.len();
        let is_timestamp = timestamp_at == Some(at);
        let mut had_escape = false;
        out.push(b'"');
        at += 1;
        while at < raw.len() && raw[at] != b'"' {
            if at >= next_check {
                check()?;
                next_check = at.saturating_add(65536);
            }
            if raw[at] == b'\\' {
                had_escape = true;
                let Some(next) = raw.get(at + 1) else {
                    return Err(ToolkitError::invalid("unterminated JSON string"));
                };
                if *next == b'u' {
                    let code = hex4(raw.get(at + 2..).unwrap_or_default())
                        .ok_or_else(|| ToolkitError::invalid("invalid JSON unicode escape"))?;
                    if (0xd800..=0xdbff).contains(&code) {
                        let low = raw
                            .get(at + 6..at + 8)
                            .filter(|pair| *pair == b"\\u")
                            .and_then(|_| hex4(raw.get(at + 8..).unwrap_or_default()));
                        if low.is_some_and(|code| (0xdc00..=0xdfff).contains(&code)) {
                            out.extend_from_slice(&raw[at..at + 12]);
                            at += 12;
                            continue;
                        }
                        out.extend_from_slice(b"\\ufffd");
                        at += 6;
                        continue;
                    }
                    if (0xdc00..=0xdfff).contains(&code) {
                        out.extend_from_slice(b"\\ufffd");
                    } else {
                        out.extend_from_slice(&raw[at..at + 6]);
                    }
                    at += 6;
                    continue;
                }
                if !matches!(next, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') {
                    return Err(ToolkitError::invalid("invalid JSON string escape"));
                }
                out.extend_from_slice(&raw[at..at + 2]);
                at += 2;
                continue;
            }
            if raw[at] >= 0x80 {
                let width = match raw[at] {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => 1,
                };
                if width > 1
                    && raw
                        .get(at..at + width)
                        .is_some_and(|bytes| std::str::from_utf8(bytes).is_ok())
                {
                    out.extend_from_slice(&raw[at..at + width]);
                    at += width;
                } else {
                    out.extend_from_slice("\u{fffd}".as_bytes());
                    at += 1;
                }
                continue;
            }
            if raw[at] < 0x20 {
                return Err(ToolkitError::invalid("unescaped JSON string control byte"));
            }
            out.push(raw[at]);
            at += 1;
        }
        if at == raw.len() {
            return Err(ToolkitError::invalid("unterminated JSON string"));
        }
        out.push(b'"');
        at += 1;
        if is_timestamp && had_escape {
            // The actual Time.UnmarshalJSON sees the original backslash bytes,
            // so no JSON-escaped timestamp can become valid by normalization.
            out.truncate(output_start);
            out.extend_from_slice(b"\"!\"");
        }
        let next = skip_space(raw, at, check)?;
        if raw.get(next) == Some(&b':') && at - start <= 64 {
            let key: String = serde_json::from_slice(&out[output_start..])?;
            if fold_field(&key) == time_key {
                timestamp_at = Some(skip_space(raw, next + 1, check)?);
            }
        }
        debug_assert!(out.len() <= maximum);
    }
    check()?;
    Ok(Cow::Owned(out))
}
#[cfg(test)]
mod compatibility_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn go_timestamp_canonical_identity_uses_layout_fallback_normalization() {
        for (raw, expected) in [
            (
                "2026-10-14T2:00:00,123456789012Z",
                Some("2026-10-14T02:00:00.123456789Z"),
            ),
            (
                "2026-10-14T22:00:00.0000000001+00:60",
                Some("2026-10-14T22:00:00+01:00"),
            ),
            ("0000-02-29T00:00:00-00:00", Some("0000-02-29T00:00:00Z")),
            ("2026-10-14T22:00:00-24:60", None),
        ] {
            assert_eq!(
                parse_go_timestamp(raw).unwrap().canonical.as_deref(),
                expected
            );
        }
        for raw in [
            "2026-10-14 22:00:00Z",
            "2026-10-14T22:00:00UTC",
            "2026-10-14T22:00:00+0100",
            "2026-10-14T22:00:00+25:00",
            "2026-10-14T22:00:00+00:61",
        ] {
            assert!(parse_go_timestamp(raw).is_none(), "{raw}");
        }
    }

    #[test]
    fn go_json_replacements_preserve_rune_boundaries_and_raw_timestamp_rules() {
        let mut raw = br#"{"values":["\ud800","\udfff","\ud83d\ude00","\\ud800",""#.to_vec();
        raw.extend_from_slice(&[0xf0, 0x90, 0x80, b'A', 0xed, 0xa0, 0x80]);
        raw.extend_from_slice(br#""],"capturedAt":"2026-10-14\u005422:00:00Z"}"#);
        let normalized = normalize_go_strings(&raw, &OperationControl::default()).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&normalized).unwrap(),
            json!({"values":["\u{fffd}","\u{fffd}","😀","\\ud800","\u{fffd}\u{fffd}\u{fffd}A\u{fffd}\u{fffd}\u{fffd}"],"capturedAt":"!"})
        );
        let ordinary = br#"{"pages":["CAP-exact"],"unknown":"valid UTF8"}"#;
        assert!(matches!(
            normalize_go_strings(ordinary, &OperationControl::default()).unwrap(),
            Cow::Borrowed(_)
        ));
    }
}

// Preserve the Go signed wire representation through manifest authentication.
// The blob reader, not the manifest decoder, rejects a negative byte length.
pub(super) fn decode_historical_capture(
    raw: &[u8],
    id: &str,
    control: &OperationControl,
) -> Result<(CaptureRef, i64)> {
    control.check()?;
    if !crate::raw_json::is_valid(raw) {
        return Err(ToolkitError::invalid("invalid capture JSON"));
    }
    let normalized = normalize_go_strings(raw, control)?;
    let mut value: EmbeddedCapture = serde_json::from_slice(&normalized)?;
    control.check()?;
    if value.schema != "lycheedev.capture.v1" || value.id != id || value.complete && value.truncated
    {
        return Err(ToolkitError::new(
            "command.failed",
            "evidence.invalid_manifest",
            5,
        ));
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Identity<'a> {
        schema: &'a str,
        id: &'a str,
        blob: &'a EmbeddedBlob,
        media_type: &'a str,
        provenance: &'a EmbeddedProvenance,
        complete: bool,
        truncated: bool,
        captured_at: &'a str,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        dependencies: &'a [String],
    }
    let captured_at = value.captured_at.canonical.as_deref().ok_or_else(|| {
        ToolkitError::invalid("capture timestamp offset cannot be marshaled by Go")
    })?;
    let identity = Identity {
        schema: &value.schema,
        id: "",
        blob: &value.blob,
        media_type: &value.media_type,
        provenance: &value.provenance,
        complete: value.complete,
        truncated: value.truncated,
        captured_at,
        dependencies: &value.dependencies.elements[..value.dependencies.length],
    };
    let bytes = crate::storage::identity_json(&identity)?;
    if format!("CAP-{}", crate::storage::sha256(&bytes)) != id {
        return Err(ToolkitError::new(
            "command.failed",
            "evidence.manifest_integrity",
            5,
        ));
    }
    let signed_bytes = value.blob.bytes;
    // A negative length remains carried separately until FetchCapture's blob
    // boundary; no unsigned projection is returned to its public consumer.
    value.blob.bytes = signed_bytes.max(0);
    let capture = value.into_capture()?;
    control.check()?;
    Ok((capture, signed_bytes))
}

// Shared foundation admission uses the same Go scalar/null/string/time decoder
// as capture manifests; no separate historical-workspace policy is introduced.
pub(crate) fn decode_workspace_identity(raw: &[u8]) -> Result<(serde_json::Value, bool)> {
    let control = OperationControl::default();
    decode_workspace_identity_with_check(raw, || control.check())
}
pub(crate) fn decode_workspace_identity_with_check(
    raw: &[u8],
    mut check: impl FnMut() -> Result<()>,
) -> Result<(serde_json::Value, bool)> {
    #[derive(Default)]
    struct Identity {
        schema: String,
        workspace_id: String,
        created_at: GoTimestamp,
        original_time: String,
    }
    impl GoObject for Identity {
        fn field<'de, A: MapAccess<'de>>(
            &mut self,
            key: &str,
            map: &mut A,
        ) -> std::result::Result<(), A::Error> {
            match key {
                "schema" => scalar(map, &mut self.schema),
                "workspaceid" => scalar(map, &mut self.workspace_id),
                "createdat" => {
                    if let Some(value) = map.next_value::<Option<String>>()? {
                        self.created_at = parse_go_timestamp(&value).ok_or_else(|| {
                            serde::de::Error::custom("invalid Go workspace timestamp")
                        })?;
                        self.original_time = value;
                    }
                    Ok(())
                }
                _ => ignore(map),
            }
        }
    }
    if crate::raw_json::validate_with_check(raw, &mut check)?.is_err() {
        return Err(ToolkitError::invalid("invalid workspace JSON"));
    }
    let normalized = normalize_go_strings_checked(raw, "createdat", &mut check)?;
    check()?;
    let mut identity = Identity::default();
    if normalized.len() <= 65536 {
        let mut decoder = serde_json::Deserializer::from_slice(&normalized);
        ObjectSeed(&mut identity).deserialize(&mut decoder)?;
        decoder.end()?;
    } else {
        struct Reader<'a, C> {
            raw: &'a [u8],
            at: usize,
            next: usize,
            check: C,
            stopped: Option<ToolkitError>,
        }
        impl<C: FnMut() -> Result<()>> std::io::Read for Reader<'_, C> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.at >= self.next {
                    if let Err(e) = (self.check)() {
                        self.stopped = Some(e.clone());
                        return Err(std::io::Error::other(e));
                    }
                    self.next = self.at.saturating_add(65536);
                }
                let n = out.len().min(self.raw.len() - self.at).min(65536);
                out[..n].copy_from_slice(&self.raw[self.at..self.at + n]);
                self.at += n;
                Ok(n)
            }
        }
        let mut reader = Reader {
            raw: &normalized,
            at: 0,
            next: 0,
            check: &mut check,
            stopped: None,
        };
        let result = {
            let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
            ObjectSeed(&mut identity)
                .deserialize(&mut decoder)
                .and_then(|_| decoder.end())
        };
        if let Some(error) = reader.stopped {
            return Err(error);
        }
        result?;
    }

    Ok((
        serde_json::json!({"schema":identity.schema,"workspaceId":identity.workspace_id,"createdAt":identity.created_at.canonical.unwrap_or(identity.original_time)}),
        identity.created_at.zero,
    ))
}
#[cfg(test)]
mod workspace_control_tests;
