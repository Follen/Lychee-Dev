//! Typed presentation preserves RawValue and explicit RawJsonBytes until final encoding.
use super::PublicFormatter;
use crate::{Result, ToolkitError};
use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter};
use std::{
    cell::Cell,
    io::{self, Write},
};

mod checked;
#[cfg(test)]
mod tests;

/// Write one compact public JSON record, including explicit RawJsonBytes,
/// without a Value conversion or frame buffer. RawValue fields preserve
/// duplicate keys, number/string lexemes and
/// literal HTML/JavaScript separators, with insignificant whitespace removed,
/// matching the fixed Go encoder's SetEscapeHTML(false) RawMessage handling.
/// Typed strings retain the ordinary public escaping and typed finite floats
/// use their actual f32/f64 provenance; unknown Number tokens are not retyped.
///
/// The producer must provide a stable, deterministic Serialize implementation
/// across two passes, including field/key order and any omission decisions.
/// Its schema/byte representation must already match the Go result schema;
/// this writer does not sort arbitrary maps or change serde field attributes.
/// Validation serializes to a sink first; a failure there writes no bytes.
/// The second pass writes directly, then appends exactly one newline. Actual
/// output failures remain external.io and can leave a partial record; this is
/// not an OS-atomic write. A stateful/custom serializer that changes between
/// passes can also fail after output begins. No frame-size limit is invented.
/// Both passes and raw validation cost CPU; performance remains unmeasured.
/// This is a presentation API, never a journal/cache/artifact encoder.
pub fn write_serializable_line<T: ?Sized + Serialize>(
    out: &mut impl Write,
    value: &T,
) -> Result<()> {
    encode(out, value, CompactFormatter, false)
}

/// As write_serializable_line, using the fixed Go text format's two-space
/// indentation, including correctly nested RawValue objects and arrays.
pub fn write_pretty_serializable_line<T: ?Sized + Serialize>(
    out: &mut impl Write,
    value: &T,
) -> Result<()> {
    encode(out, value, PrettyFormatter::with_indent(b"  "), true)
}

fn encode<T: ?Sized + Serialize, F: Formatter + Clone>(
    out: &mut impl Write,
    value: &T,
    structure: F,
    pretty: bool,
) -> Result<()> {
    let preflight_raw = Cell::new(false);
    checked::Value(value, &preflight_raw).serialize(
        &mut serde_json::Serializer::with_formatter(
            io::sink(),
            TypedFormatter::new(structure.clone(), pretty, &preflight_raw),
        ),
    )?;
    // Formatter errors also use serde_json's IO error carrier. Only errors
    // originating in the real writer are output IO failures, not malformed
    // raw JSON emitted by an unstable custom Serialize implementation.
    let mut output = Output {
        inner: out,
        error: None,
    };
    let raw = Cell::new(false);
    let result =
        checked::Value(value, &raw).serialize(&mut serde_json::Serializer::with_formatter(
            &mut output,
            TypedFormatter::new(structure, pretty, &raw),
        ));
    if let Err(error) = result {
        return Err(match output.error {
            Some(kind) => ToolkitError::io(io::Error::new(kind, error)),
            None => ToolkitError::from(error),
        });
    }
    output.write_all(b"\n")?;
    Ok(())
}

struct Output<'a, W> {
    inner: &'a mut W,
    error: Option<io::ErrorKind>,
}
impl<W: Write> Write for Output<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.inner.write(bytes);
        match &result {
            Err(error) if error.kind() != io::ErrorKind::Interrupted => {
                self.error = Some(error.kind());
            }
            // write_all turns a zero write into WriteZero outside this method.
            Ok(0) if !bytes.is_empty() => self.error = Some(io::ErrorKind::WriteZero),
            _ => (),
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// Go1.27.1 encoding/json/scanner.go maxNestingDepth; not a new report-byte cap.
// RawMessage is validated independently by appendCompact. Pretty output is
// additionally checked as one whole document by appendIndent.
const GO_MAX_DEPTH: usize = crate::raw_json::MAX_DEPTH;
struct TypedFormatter<'a, F> {
    raw: &'a Cell<bool>,
    public: PublicFormatter<F>,
    depth: usize,
    pretty: bool,
}
impl<'a, F: Formatter> TypedFormatter<'a, F> {
    fn new(structure: F, pretty: bool, raw: &'a Cell<bool>) -> Self {
        Self {
            raw,
            public: PublicFormatter(structure),
            depth: 0,
            pretty,
        }
    }
    fn enter(&mut self) -> io::Result<()> {
        if self.pretty && self.depth >= GO_MAX_DEPTH {
            return Err(invalid("public JSON exceeds Go nesting depth"));
        }
        self.depth += 1;
        Ok(())
    }
}
fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
impl<F: Formatter> Formatter for TypedFormatter<'_, F> {
    fn write_byte_array<W: ?Sized + Write>(&mut self, w: &mut W, bytes: &[u8]) -> io::Result<()> {
        if !self.raw.get() {
            self.begin_array(w)?;
            for (index, byte) in bytes.iter().enumerate() {
                self.begin_array_value(w, index == 0)?;
                self.write_u8(w, *byte)?;
                self.end_array_value(w)?;
            }
            return self.end_array(w);
        }
        let base = if self.pretty { self.depth } else { 0 };
        crate::raw_json::validate_at(bytes, base).map_err(invalid)?;
        format_raw(&mut self.public, w, bytes)
    }
    fn write_number_str<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        value: &str,
    ) -> io::Result<()> {
        if value.trim() != value
            || !value
                .as_bytes()
                .first()
                .is_some_and(|b| matches!(b, b'-' | b'0'..=b'9'))
        {
            return Err(invalid("invalid public JSON number token"));
        }
        // Number's arbitrary_precision marker must not become an unchecked
        // raw-document escape. Borrow validation never rounds its token.
        let _: &serde_json::value::RawValue = serde_json::from_str(value).map_err(invalid)?;
        writer.write_all(value.as_bytes())
    }
    fn write_f64<W: ?Sized + Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        self.public.write_f64(writer, value)
    }
    fn write_f32<W: ?Sized + Write>(&mut self, writer: &mut W, value: f32) -> io::Result<()> {
        if !value.is_finite() {
            return Err(invalid("public binary32 number is not finite"));
        }
        let absolute = value.abs();
        if absolute != 0.0 && !(1e-6f32..1e21f32).contains(&absolute) {
            let text = format!("{value:e}");
            let (mantissa, exponent) = text.split_once('e').expect("scientific float");
            writer.write_all(mantissa.as_bytes())?;
            writer.write_all(b"e")?;
            if !exponent.starts_with('-') {
                writer.write_all(b"+")?;
            }
            writer.write_all(exponent.as_bytes())
        } else {
            write!(writer, "{value}")
        }
    }
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        value: &str,
    ) -> io::Result<()> {
        self.public.write_string_fragment(w, value)
    }
    fn write_raw_fragment<W: ?Sized + Write>(&mut self, w: &mut W, raw: &str) -> io::Result<()> {
        let base = if self.pretty { self.depth } else { 0 };
        validate_raw(raw, base)?;
        format_raw(&mut self.public, w, raw.as_bytes())
    }
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.enter()?;
        self.public.begin_array(w)
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth -= 1;
        self.public.end_array(w)
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.public.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.public.end_array_value(w)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.enter()?;
        self.public.begin_object(w)
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth -= 1;
        self.public.end_object(w)
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.public.begin_object_key(w, first)
    }
    fn end_object_key<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.public.end_object_key(w)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.public.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.public.end_object_value(w)
    }
}

fn validate_raw(raw: &str, base: usize) -> io::Result<()> {
    // Bound the validator's nesting scratch before invoking serde's iterative
    // RawValue validator (which deliberately has no 128-level Value limit).
    let mut depth = base;
    let mut string = false;
    let mut escaped = false;
    for byte in raw.bytes() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'[' | b'{' => {
                    if depth >= GO_MAX_DEPTH {
                        return Err(invalid("raw public JSON exceeds Go nesting depth"));
                    }
                    depth += 1;
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => (),
            }
        }
    }
    // Borrow only. No Value tree, number conversion or report-sized copy.
    let _: &serde_json::value::RawValue = serde_json::from_str(raw).map_err(invalid)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Frame {
    Array { first: bool },
    Object { first: bool, key: bool },
}
impl Frame {
    fn begin<F: Formatter, W: ?Sized + Write>(&self, f: &mut F, w: &mut W) -> io::Result<()> {
        match *self {
            Self::Array { first } => f.begin_array_value(w, first),
            Self::Object { first, key: true } => f.begin_object_key(w, first),
            Self::Object { key: false, .. } => f.begin_object_value(w),
        }
    }
    fn end<F: Formatter, W: ?Sized + Write>(&mut self, f: &mut F, w: &mut W) -> io::Result<()> {
        match self {
            Self::Array { first } => {
                *first = false;
                f.end_array_value(w)
            }
            Self::Object { key, first } => {
                if *key {
                    *key = false;
                    f.end_object_key(w)
                } else {
                    *key = true;
                    *first = false;
                    f.end_object_value(w)
                }
            }
        }
    }
}
fn format_raw<F: Formatter, W: ?Sized + Write>(
    f: &mut F,
    w: &mut W,
    bytes: &[u8],
) -> io::Result<()> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | b',' | b':') {
            // Validation has already established legal token boundaries.
            at += 1;
            continue;
        }
        if matches!(byte, b']' | b'}') {
            frames.pop();
            if byte == b']' {
                f.end_array(w)?;
            } else {
                f.end_object(w)?;
            }
            if let Some(parent) = frames.last_mut() {
                parent.end(f, w)?;
            }
            at += 1;
            continue;
        }
        if let Some(parent) = frames.last() {
            parent.begin(f, w)?;
        }
        match byte {
            b'[' => {
                f.begin_array(w)?;
                frames.push(Frame::Array { first: true });
                at += 1;
            }
            b'{' => {
                f.begin_object(w)?;
                frames.push(Frame::Object {
                    first: true,
                    key: true,
                });
                at += 1;
            }
            _ => {
                let start = at;
                if byte == b'"' {
                    at += 1;
                    while at < bytes.len() {
                        match bytes[at] {
                            b'\\' => at += 2,
                            b'"' => {
                                at += 1;
                                break;
                            }
                            _ => at += 1,
                        }
                    }
                } else {
                    while at < bytes.len()
                        && !matches!(
                            bytes[at],
                            b' ' | b'\n' | b'\r' | b'\t' | b',' | b':' | b']' | b'}'
                        )
                    {
                        at += 1;
                    }
                }
                // Go appendCompact(false) does not re-encode raw strings:
                // preserve escapes, surrogate spellings and literal U+2028/29.
                w.write_all(&bytes[start..at])?;
                if let Some(parent) = frames.last_mut() {
                    parent.end(f, w)?;
                }
            }
        }
    }
    Ok(())
}
