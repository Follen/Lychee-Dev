//! Exact fixed-Go AssetDemuxManifest field order, omission, float provenance,
//! MarshalIndent whitespace and capture HTML escaping. This is artifact JSON.
use super::*;
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::ser::{Formatter, PrettyFormatter};
use std::io::{self, Write};
#[derive(Clone, Copy)]
enum Kind {
    Manifest,
    Source,
    Frame,
    Limits,
    Pin,
    Blob,
}
struct Ordered<'a>(&'a Value, Kind);
struct Frames<'a>(&'a Value);
impl Serialize for Frames<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let values = self
            .0
            .as_array()
            .ok_or_else(|| serde::ser::Error::custom("frames array"))?;
        let mut seq = s.serialize_seq(Some(values.len()))?;
        for value in values {
            seq.serialize_element(&Ordered(value, Kind::Frame))?;
        }
        seq.end()
    }
}
impl Serialize for Ordered<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let fields: &[&str] = match self.1 {
            Kind::Manifest => &[
                "schema",
                "snapshot",
                "input",
                "source",
                "output",
                "width",
                "height",
                "frameRate",
                "frameCount",
                "enumeratedFrames",
                "enumeratedBytes",
                "frames",
                "complete",
                "truncated",
                "truncationReason",
                "warnings",
                "limits",
                "sourceCapture",
            ],
            Kind::Source => &[
                "kind",
                "input",
                "path",
                "fileDataID",
                "snapshot",
                "pin",
                "contentKey",
                "encodingKey",
                "bytes",
                "sha256",
            ],
            Kind::Frame => &[
                "index",
                "type",
                "timestamp",
                "duration",
                "size",
                "chunk",
                "offset",
                "output",
                "sha256",
                "blob",
                "capture",
                "subFrames",
            ],
            Kind::Limits => &[
                "maxInputBytes",
                "maxFrames",
                "maxFrameBytes",
                "maxOutputBytes",
            ],
            Kind::Pin => &[
                "product",
                "region",
                "fullBuild",
                "buildConfig",
                "cdnConfig",
                "language",
                "definitionCommit",
            ],
            Kind::Blob => &["sha256", "bytes"],
        };
        let mut map = s.serialize_map(None)?;
        for &name in fields {
            let Some(value) = self.0.get(name) else {
                continue;
            };
            match (self.1, name) {
                (Kind::Manifest, "source") => {
                    map.serialize_entry(name, &Ordered(value, Kind::Source))?
                }
                (Kind::Manifest, "frames") => map.serialize_entry(name, &Frames(value))?,
                (Kind::Manifest, "limits") => {
                    map.serialize_entry(name, &Ordered(value, Kind::Limits))?
                }
                (Kind::Source, "pin") => map.serialize_entry(name, &Ordered(value, Kind::Pin))?,
                (Kind::Frame, "blob") => map.serialize_entry(name, &Ordered(value, Kind::Blob))?,
                (Kind::Manifest, "frameRate") | (Kind::Frame, "timestamp" | "duration") => {
                    let value = value
                        .as_f64()
                        .ok_or_else(|| serde::ser::Error::custom("typed finite float"))?;
                    map.serialize_entry(name, &value)?;
                }
                _ => map.serialize_entry(name, value)?,
            }
        }
        map.end()
    }
}
struct GoPretty(PrettyFormatter<'static>);
macro_rules! structure {
    ($($method:ident $(,$arg:ident:$ty:ty)?);*$(;)?)=>{$(
        fn $method<W:?Sized+Write>(&mut self,w:&mut W $(,$arg:$ty)?)->io::Result<()> {
            self.0.$method(w $(,$arg)?)
        }
    )*};
}
impl Formatter for GoPretty {
    structure! {begin_array;end_array;begin_array_value,first:bool;end_array_value;
    begin_object;end_object;begin_object_key,first:bool;end_object_key;
    begin_object_value;end_object_value;}
    fn write_string_fragment<W: ?Sized + Write>(&mut self, w: &mut W, s: &str) -> io::Result<()> {
        crate::capture::GoCaptureFormatter.write_string_fragment(w, s)
    }
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, n: f64) -> io::Result<()> {
        // Reuse the established Go binary64 formatter, never parse/round a
        // Number of unknown provenance. Its sole added LF is removed here.
        let mut token = Vec::with_capacity(32);
        lycheedev_core::public_json::write_line(
            &mut token,
            &json!(n),
            &lycheedev_core::public_json::NumberLayout::Binary64,
        )
        .map_err(|error| io::Error::other(error.message))?;
        let newline = token.pop();
        debug_assert_eq!(newline, Some(b'\n'));
        w.write_all(&token)
    }
}
struct Body<'a, 'b> {
    raw: Vec<u8>,
    meter: &'a Meter<'b>,
    error: Option<ToolkitError>,
}
impl Write for Body<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        use evidence::CaptureAccounting;
        let result = (|| {
            self.meter.check()?;
            if bytes.len() > (16usize << 20).saturating_sub(self.raw.len()) {
                return Err(fault("records.metadata_limit", 3));
            }
            self.meter.admit_work(bytes.len())?;
            if self.raw.capacity() - self.raw.len() < bytes.len() {
                let needed = self.raw.len() + bytes.len();
                let target = needed
                    .max(self.raw.capacity().saturating_mul(2))
                    .max(4096)
                    .min(16usize << 20);
                // Reallocation may coexist with the previous buffer. This
                // safety-only charge admits the entire replacement first.
                self.meter.admit_allocation(target)?;
                self.raw
                    .try_reserve_exact(target - self.raw.len())
                    .map_err(|_| fault("records.metadata_limit", 3))?;
            }
            self.raw.extend_from_slice(bytes);
            Ok(bytes.len())
        })();
        result.map_err(|error| {
            self.error = Some(error);
            io::Error::other("demux manifest encoding failed")
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &Value, meter: &Meter<'_>) -> Result<Vec<u8>> {
    let mut body = Body {
        raw: Vec::new(),
        meter,
        error: None,
    };
    let result =
        Ordered(value, Kind::Manifest).serialize(&mut serde_json::Serializer::with_formatter(
            &mut body,
            GoPretty(PrettyFormatter::with_indent(b"  ")),
        ));
    if let Err(error) = result {
        return Err(body.error.unwrap_or_else(|| error.into()));
    }
    evidence::CaptureAccounting::check(meter)?;
    Ok(body.raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn fixed_typed_manifest_order_float_and_html_escaping_without_terminal_lf() {
        let inv = Invocation {
            command: "asset demux".into(),
            positional: None,
            flags: BTreeMap::new(),
            home: std::path::PathBuf::new(),
            cwd: std::path::PathBuf::new(),
            control: Default::default(),
        };
        let meter = Meter {
            inv: &inv,
            budget: None,
        };
        let value = json!({"schema":"s","input":"<&> ","source":{"kind":"file","input":"f","bytes":0,"sha256":"abc"},"output":"out","width":0,"height":0,"frameRate":25.0,"frameCount":0,"enumeratedFrames":0,"enumeratedBytes":0,"frames":[],"complete":true,"truncated":false,"limits":{"maxInputBytes":12,"maxFrames":1,"maxFrameBytes":2,"maxOutputBytes":3},"sourceCapture":"id"});
        let expected = "{\n  \"schema\": \"s\",\n  \"input\": \"\\u003c\\u0026\\u003e\\u2028\",\n  \"source\": {\n    \"kind\": \"file\",\n    \"input\": \"f\",\n    \"bytes\": 0,\n    \"sha256\": \"abc\"\n  },\n  \"output\": \"out\",\n  \"width\": 0,\n  \"height\": 0,\n  \"frameRate\": 25,\n  \"frameCount\": 0,\n  \"enumeratedFrames\": 0,\n  \"enumeratedBytes\": 0,\n  \"frames\": [],\n  \"complete\": true,\n  \"truncated\": false,\n  \"limits\": {\n    \"maxInputBytes\": 12,\n    \"maxFrames\": 1,\n    \"maxFrameBytes\": 2,\n    \"maxOutputBytes\": 3\n  },\n  \"sourceCapture\": \"id\"\n}";
        assert_eq!(encode(&value, &meter).unwrap(), expected.as_bytes());
    }
}
