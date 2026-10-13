//! Full JSON validation followed by borrowed, bounded LSP projection. The frame
//! remains raw: locations after the first1000, unused initialization extensions
//! and notification payloads never become arbitrary serde_json::Value trees.
use lycheedev_core::{Result as ToolkitResult, ToolkitError};
use serde::{
    Serialize,
    de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor},
};
use std::{fmt, io::Write};

pub(crate) const FRAME_BYTES: usize = 8 << 20;
pub(crate) const LOCATIONS: usize = 1000;
const HOVER_BYTES: usize = 16384;
// Windows extended file paths have at most32768 UTF-16 units. UTF-8 uses at
// most3 bytes per unit and a file URI can percent-encode each byte threefold.
// Source members have already been materialized as ordinary Windows files.
const URI_BYTES: usize = 32768 * 9 + 1024;

#[derive(Default)]
struct Shape {
    string: bool,
    object: bool,
    id: bool,
    method_string: bool,
    preserve: bool,
}
enum Key {
    Id,
    Method,
    Other,
    SerdePrivate,
    Number,
}
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Key, E> {
                Ok(match value {
                    "id" => Key::Id,
                    "method" => Key::Method,
                    "$serde_json::private::Number" => Key::Number,
                    name if name.starts_with("$serde_json::") => Key::SerdePrivate,
                    _ => Key::Other,
                })
            }
        }
        de.deserialize_identifier(KeyVisitor)
    }
}

// Match the pinned arbitrary_precision Value decoder's special FIRST map key.
// This also handles the synthetic map used by serde_json for large numbers.
// NumberFromString is private to serde_json; its public Number parser and the
// same string visitor preserve its validation without retaining object trees.
struct NumberString;
impl<'de> Deserialize<'de> for NumberString {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct NumberVisitor;
        impl Visitor<'_> for NumberVisitor {
            type Value = NumberString;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("string containing a number")
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<NumberString, E> {
                let _: serde_json::Number = value.parse().map_err(E::custom)?;
                Ok(NumberString)
            }
        }
        de.deserialize_str(NumberVisitor)
    }
}
impl<'de> Deserialize<'de> for Shape {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct ShapeVisitor;
        impl<'de> Visitor<'de> for ShapeVisitor {
            type Value = Shape;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any valid JSON value")
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Shape, E> {
                Ok(Shape {
                    string: true,
                    ..Shape::default()
                })
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Shape, E> {
                Ok(Shape::default())
            }
            fn visit_some<D: Deserializer<'de>>(self, de: D) -> Result<Shape, D::Error> {
                Shape::deserialize(de)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Shape, S::Error> {
                let mut shape = Shape::default();
                while let Some(child) = seq.next_element::<Shape>()? {
                    shape.preserve |= child.preserve;
                }
                Ok(shape)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Shape, M::Error> {
                let mut shape = Shape {
                    object: true,
                    ..Shape::default()
                };
                let mut first = true;
                while let Some(key) = map.next_key::<Key>()? {
                    if first && matches!(key, Key::Number) {
                        map.next_value::<NumberString>()?;
                        // Do not consume extra entries: the exact Value visitor
                        // also returns here and serde_json rejects a remaining
                        // entry at the map terminator.
                        return Ok(Shape {
                            preserve: true,
                            ..Shape::default()
                        });
                    }
                    first = false;
                    let child = map.next_value::<Shape>()?;
                    shape.preserve |= child.preserve;
                    match key {
                        Key::Id => shape.id = true,
                        // Value's map insertion uses the last duplicate value.
                        Key::Method => shape.method_string = child.string,
                        Key::SerdePrivate | Key::Number => shape.preserve = true,
                        Key::Other => {}
                    }
                }
                Ok(shape)
            }
        }
        // Unlike IgnoredAny, recursively visiting every value preserves the
        // Value decoder's JSON depth, UTF-8, string and number validation.
        // Serde may allocate scratch for escaped strings/numbers; this avoids
        // a retained Value tree, not every possible heap allocation.
        de.deserialize_any(ShapeVisitor)
    }
}

#[cfg(test)]
pub(crate) fn ignored_notification(raw: &[u8]) -> serde_json::Result<bool> {
    let shape = validate(raw)?;
    Ok(shape.object && shape.method_string && !shape.id && !shape.preserve)
}

fn validate(raw: &[u8]) -> serde_json::Result<Shape> {
    let mut de = serde_json::Deserializer::from_slice(raw);
    let shape = Shape::deserialize(&mut de)?;
    de.end()?;
    Ok(shape)
}

pub(crate) struct Frame {
    raw: Vec<u8>,
    object: bool,
    ignored: bool,
}
impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("bytes", &self.raw.len())
            .field("object", &self.object)
            .finish()
    }
}
impl Frame {
    pub(crate) fn parse(raw: Vec<u8>) -> ToolkitResult<Self> {
        let shape = validate(&raw)?;
        Ok(Self {
            raw,
            object: shape.object,
            // Private-number semantics are now fully checked above. Valid
            // unused notifications need no legacy DOM fallback, even when a
            // decimal uses serde's synthetic private-number map internally.
            ignored: shape.object && shape.method_string && !shape.id,
        })
    }
    pub(crate) fn ignored(&self) -> bool {
        self.ignored
    }
    pub(crate) fn field(&self, name: &str) -> Option<&[u8]> {
        self.object.then(|| field(&self.raw, name)).flatten()
    }
    pub(crate) fn result(&self) -> &[u8] {
        self.field("result").unwrap_or(b"null")
    }
    pub(crate) fn id_matches(&self, wanted: u64) -> ToolkitResult<bool> {
        let Some(id) = self.field("id") else {
            return Ok(false);
        };
        let id = trim(id);
        if id.first() == Some(&b'{') {
            let mut fields = Fields::new(id);
            if let Some((key, value)) = fields.next()
                && key_is(key, "$serde_json::private::Number")
            {
                struct NumberID(u64);
                impl Visitor<'_> for NumberID {
                    type Value = bool;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("string containing a number")
                    }
                    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<bool, E> {
                        let value = value.trim();
                        Ok(value.len() <= 20
                            && value
                                .parse::<serde_json::Number>()
                                .is_ok_and(|n| n == serde_json::Number::from(self.0)))
                    }
                }
                let mut de = serde_json::Deserializer::from_slice(value);
                return Ok(de.deserialize_str(NumberID(wanted))?);
            }
            return Ok(false);
        }
        // An ordinary large number, string, array or object cannot equal the
        // small monotonically assigned integer ID. Never decode its subtree.
        Ok(id.len() <= 20
            && serde_json::from_slice::<serde_json::Value>(id).is_ok_and(|value| value == wanted))
    }
}

fn trim(raw: &[u8]) -> &[u8] {
    let start = raw
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(raw.len());
    let end = raw
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &raw[start..end]
}
fn white(raw: &[u8], mut pos: usize) -> usize {
    while raw.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    pos
}
fn string_end(raw: &[u8], mut pos: usize) -> usize {
    pos += 1;
    while pos < raw.len() {
        match raw[pos] {
            b'\\' => pos += 2,
            b'"' => return pos + 1,
            _ => pos += 1,
        }
    }
    raw.len()
}
// Only called on fully validated JSON. No second grammar, heap or recursion
// is introduced: strings shield delimiters and nesting is counted in place.
fn value_end(raw: &[u8], start: usize) -> usize {
    if raw[start] == b'"' {
        return string_end(raw, start);
    }
    if matches!(raw[start], b'[' | b'{') {
        let mut depth = 0usize;
        let mut pos = start;
        while pos < raw.len() {
            match raw[pos] {
                b'"' => {
                    pos = string_end(raw, pos);
                    continue;
                }
                b'[' | b'{' => depth += 1,
                b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return pos + 1;
                    }
                }
                _ => {}
            }
            pos += 1;
        }
        return raw.len();
    }
    let mut pos = start;
    while raw
        .get(pos)
        .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b',' | b']' | b'}'))
    {
        pos += 1;
    }
    pos
}
struct Fields<'a> {
    raw: &'a [u8],
    pos: usize,
}
impl<'a> Fields<'a> {
    fn new(raw: &'a [u8]) -> Self {
        Self {
            raw: trim(raw),
            pos: 1,
        }
    }
}
impl<'a> Iterator for Fields<'a> {
    type Item = (&'a [u8], &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        self.pos = white(self.raw, self.pos);
        if self.raw.get(self.pos) != Some(&b'"') {
            return None;
        }
        let start = self.pos;
        let end = string_end(self.raw, start);
        let key = &self.raw[start..end];
        self.pos = white(self.raw, end) + 1; // validated colon
        self.pos = white(self.raw, self.pos);
        let start = self.pos;
        let end = value_end(self.raw, start);
        self.pos = white(self.raw, end);
        if self.raw.get(self.pos) == Some(&b',') {
            self.pos += 1;
        }
        Some((key, &self.raw[start..end]))
    }
}
struct Elements<'a> {
    raw: &'a [u8],
    pos: usize,
}
impl<'a> Elements<'a> {
    fn new(raw: &'a [u8]) -> Self {
        Self {
            raw: trim(raw),
            pos: 1,
        }
    }
}
impl<'a> Iterator for Elements<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        self.pos = white(self.raw, self.pos);
        if self.raw.get(self.pos).is_none_or(|b| *b == b']') {
            return None;
        }
        let start = self.pos;
        let end = value_end(self.raw, start);
        self.pos = white(self.raw, end);
        if self.raw.get(self.pos) == Some(&b',') {
            self.pos += 1;
        }
        Some(&self.raw[start..end])
    }
}
fn key_is(raw: &[u8], name: &str) -> bool {
    if raw.len() > name.len() * 6 + 2 {
        return false;
    }
    if !raw.contains(&b'\\') {
        return &raw[1..raw.len() - 1] == name.as_bytes();
    }
    read_string(raw, name.len()).is_ok_and(|decoded| decoded == name)
}
pub(crate) fn string_is(raw: &[u8], name: &str) -> bool {
    let raw = trim(raw);
    raw.first() == Some(&b'"') && key_is(raw, name)
}
pub(crate) fn is_string(raw: &[u8]) -> bool {
    trim(raw).first() == Some(&b'"')
}
fn field<'a>(raw: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    let raw = trim(raw);
    if raw.first() != Some(&b'{') {
        return None;
    }
    let mut answer = None;
    for (index, (key, value)) in Fields::new(raw).enumerate() {
        if index == 0 && key_is(key, "$serde_json::private::Number") {
            return None;
        }
        if key_is(key, wanted) {
            answer = Some(value);
        }
    }
    answer
}
fn budget(message: &str) -> ToolkitError {
    ToolkitError::new("luals.input_budget", message, 3)
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("luals.check_failed", message, 5)
}

fn read_string(raw: &[u8], maximum: usize) -> ToolkitResult<String> {
    struct Text(usize);
    impl Visitor<'_> for Text {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded string")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            if value.len() > self.0 {
                return Err(E::custom("LSP string byte budget"));
            }
            let mut owned = String::new();
            owned
                .try_reserve_exact(value.len())
                .map_err(|_| E::custom("LSP string allocation budget"))?;
            owned.push_str(value);
            Ok(owned)
        }
    }
    let mut de = serde_json::Deserializer::from_slice(raw);
    de.deserialize_str(Text(maximum)).map_err(|e| {
        if e.is_data() {
            budget("LSP string byte or allocation budget")
        } else {
            e.into()
        }
    })
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Position {
    pub line: u64,
    pub character: u64,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Range {
    pub start: Position,
    pub end: Position,
}
#[derive(Debug)]
pub(crate) struct RawLocation {
    pub uri: String,
    pub range: Range,
}
#[derive(Debug)]
pub(crate) struct ParsedLocations {
    pub items: Vec<RawLocation>,
    pub truncated: bool,
}
fn coordinate(raw: Option<&[u8]>) -> ToolkitResult<i64> {
    match raw {
        None | Some(b"null") => Ok(0),
        Some(raw) => {
            serde_json::from_slice::<i64>(raw).map_err(|_| invalid("Invalid LSP location range"))
        }
    }
}
fn range(raw: &[u8]) -> ToolkitResult<[(i64, i64); 2]> {
    if trim(raw) != b"null" && trim(raw).first() != Some(&b'{') {
        return Err(invalid("Invalid LSP location range"));
    }
    let position = |name: &str| -> ToolkitResult<(i64, i64)> {
        let raw = field(raw, name).unwrap_or(b"null");
        if trim(raw) != b"null" && trim(raw).first() != Some(&b'{') {
            return Err(invalid("Invalid LSP location range"));
        }
        Ok((
            coordinate(field(raw, "line"))?,
            coordinate(field(raw, "character"))?,
        ))
    };
    Ok([position("start")?, position("end")?])
}
fn checked_range(raw: [(i64, i64); 2]) -> ToolkitResult<Range> {
    if raw.iter().any(|(line, char)| {
        *line < 0 || *char < 0 || *line > i32::MAX as i64 || *char > i32::MAX as i64
    }) || raw[1] < raw[0]
    {
        return Err(invalid("Invalid LSP location range"));
    }
    Ok(Range {
        start: Position {
            line: raw[0].0 as u64,
            character: raw[0].1 as u64,
        },
        end: Position {
            line: raw[1].0 as u64,
            character: raw[1].1 as u64,
        },
    })
}
fn location(raw: &[u8]) -> ToolkitResult<RawLocation> {
    if trim(raw).first() != Some(&b'{') {
        return Err(invalid("Invalid LSP location"));
    }
    let text = |name| -> ToolkitResult<String> {
        match field(raw, name) {
            None | Some(b"null") => Ok(String::new()),
            Some(raw) if is_string(raw) => read_string(raw, URI_BYTES),
            _ => Err(invalid("Invalid LSP location")),
        }
    };
    // Go decodes all defined fields, then selects URI/range as a PAIR. A null
    // or empty primary URI selects targetURI+targetRange; a wrong type fails.
    // Missing/null coordinates retain Go's zero defaults before range checks.
    let uri = text("uri")?;
    let target_uri = text("targetUri")?;
    let primary = range(field(raw, "range").unwrap_or(b"null"))?;
    let target = range(field(raw, "targetRange").unwrap_or(b"null"))?;
    let (uri, range) = if uri.is_empty() {
        (target_uri, target)
    } else {
        (uri, primary)
    };
    Ok(RawLocation {
        uri,
        range: checked_range(range)?,
    })
}
pub(crate) fn locations(raw: &[u8]) -> ToolkitResult<ParsedLocations> {
    let raw = trim(raw);
    let mut items = Vec::new();
    if raw == b"null" {
        return Ok(ParsedLocations {
            items,
            truncated: false,
        });
    }
    if raw.first() != Some(&b'[') {
        items
            .try_reserve_exact(1)
            .map_err(|_| budget("LSP location allocation budget"))?;
        items.push(location(raw)?);
        return Ok(ParsedLocations {
            items,
            truncated: false,
        });
    }
    let mut elements = Elements::new(raw);
    while items.len() < LOCATIONS {
        let Some(raw) = elements.next() else {
            return Ok(ParsedLocations {
                items,
                truncated: false,
            });
        };
        if items.is_empty() {
            items
                .try_reserve_exact(LOCATIONS)
                .map_err(|_| budget("LSP location allocation budget"))?;
        }
        items.push(location(raw)?);
    }
    // Full JSON/UTF-8/private-number validation already covered the tail. As
    // before, its location schema is not examined after truncation is proven.
    Ok(ParsedLocations {
        items,
        truncated: elements.next().is_some(),
    })
}

#[derive(Default)]
struct Hover {
    text: String,
    parts: usize,
    truncated: bool,
}
impl Hover {
    fn append(&mut self, plain: &str) -> std::result::Result<(), &'static str> {
        if self.truncated {
            return Ok(());
        }
        let room = HOVER_BYTES.saturating_sub(self.text.len());
        let mut end = plain.len().min(room);
        while !plain.is_char_boundary(end) {
            end -= 1;
        }
        self.text
            .try_reserve_exact(end)
            .map_err(|_| "LSP hover allocation budget")?;
        self.text.push_str(&plain[..end]);
        self.truncated |= end < plain.len();
        Ok(())
    }
    fn string(&mut self, raw: &[u8]) -> ToolkitResult<bool> {
        struct Prefix<'a>(&'a mut Hover);
        impl Visitor<'_> for Prefix<'_> {
            type Value = bool;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("hover text")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<bool, E> {
                if value.is_empty() {
                    return Ok(false);
                }
                if self.0.parts > 0 {
                    self.0.append("\n").map_err(E::custom)?;
                }
                self.0.parts += 1;
                self.0.append(value).map_err(E::custom)?;
                Ok(true)
            }
        }
        let mut de = serde_json::Deserializer::from_slice(raw);
        de.deserialize_str(Prefix(self)).map_err(|e| {
            if e.is_data() {
                budget("LSP hover allocation budget")
            } else {
                e.into()
            }
        })
    }
    fn visit(&mut self, raw: &[u8], depth: usize) -> ToolkitResult<()> {
        if depth > 64 {
            return Err(budget("Hover nesting budget"));
        }
        let raw = trim(raw);
        if raw == b"null" {
            return Ok(());
        }
        if is_string(raw) {
            self.string(raw)?;
            return Ok(());
        }
        if raw.first() == Some(&b'[') {
            for item in Elements::new(raw) {
                self.visit(item, depth + 1)?;
            }
            return Ok(());
        }
        if let Some(value) = field(raw, "value").filter(|raw| is_string(raw))
            && self.string(value)?
        {
            return Ok(());
        }
        Err(invalid("Unsupported hover contents"))
    }
}
pub(crate) fn hover(raw: &[u8]) -> ToolkitResult<(String, bool)> {
    if trim(raw) == b"null" {
        return Ok((String::new(), false));
    }
    let contents = field(raw, "contents").ok_or_else(|| invalid("Hover lacks contents"))?;
    let mut output = Hover::default();
    output.visit(contents, 0)?;
    Ok((output.text, output.truncated))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Capabilities {
    pub definition: bool,
    pub references: bool,
    pub hover: bool,
}
pub(crate) fn capabilities(raw: &[u8]) -> ToolkitResult<Capabilities> {
    let raw = field(raw, "capabilities").unwrap_or(b"null");
    if let Some(encoding) = field(raw, "positionEncoding").filter(|raw| is_string(raw))
        && !string_is(encoding, "utf-16")
    {
        return Err(ToolkitError::new(
            "luals.unsupported",
            "LuaLS position encoding is not UTF-16",
            3,
        ));
    }
    let enabled =
        |name| field(raw, name).is_some_and(|raw| !matches!(trim(raw), b"null" | b"false"));
    Ok(Capabilities {
        definition: enabled("definitionProvider"),
        references: enabled("referencesProvider"),
        hover: enabled("hoverProvider"),
    })
}

pub(crate) fn error_message(raw: &[u8]) -> ToolkitResult<String> {
    // Frozen Go rpcMessage.Error and requestContext retain code/message and
    // render "server error <code>: <message>"; unused data never becomes DOM.
    if trim(raw).first() != Some(&b'{') {
        return Err(invalid("Invalid LSP server error"));
    }
    let code = match field(raw, "code") {
        None | Some(b"null") => 0i64,
        Some(raw) => {
            serde_json::from_slice(raw).map_err(|_| invalid("Invalid LSP server error code"))?
        }
    };
    let message = match field(raw, "message") {
        None | Some(b"null") => String::new(),
        Some(raw) if is_string(raw) => read_string(raw, FRAME_BYTES)?,
        _ => return Err(invalid("Invalid LSP server error message")),
    };
    let code = code.to_string();
    let length = 15usize
        .checked_add(code.len())
        .and_then(|n| n.checked_add(message.len()))
        .filter(|n| *n <= FRAME_BYTES)
        .ok_or_else(|| budget("LSP error text byte budget"))?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|_| budget("LSP error text allocation budget"))?;
    text.push_str("server error ");
    text.push_str(&code);
    text.push_str(": ");
    text.push_str(&message);
    Ok(text)
}

pub(crate) fn serialize(value: &impl Serialize, maximum: usize) -> ToolkitResult<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            let needed = self
                .bytes
                .len()
                .checked_add(input.len())
                .filter(|n| *n <= self.maximum)
                .ok_or_else(|| std::io::Error::other("LSP JSON byte budget"))?;
            if needed > self.bytes.capacity() {
                let target = needed
                    .max(self.bytes.capacity().max(1024).saturating_mul(2))
                    .min(self.maximum);
                self.bytes
                    .try_reserve_exact(target - self.bytes.len())
                    .map_err(|_| std::io::Error::other("LSP JSON allocation budget"))?;
            }
            self.bytes.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut out, value)
        .map_err(|_| budget("LSP JSON byte or allocation budget"))?;
    Ok(out.bytes)
}
pub(crate) fn serialized_bytes(value: &impl Serialize, maximum: usize) -> ToolkitResult<usize> {
    struct Count {
        bytes: usize,
        maximum: usize,
    }
    impl Write for Count {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            self.bytes = self
                .bytes
                .checked_add(input.len())
                .filter(|n| *n <= self.maximum)
                .ok_or_else(|| std::io::Error::other("LSP result byte budget"))?;
            Ok(input.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count { bytes: 0, maximum };
    serde_json::to_writer(&mut count, value).map_err(|_| budget("LSP result byte budget"))?;
    Ok(count.bytes)
}

// Query descriptors are the only caller-supplied Value trees copied into a
// typed result. Account them BEFORE clone, including spare Vec capacity and
// the pinned Rust1.99 BTreeMap nodes (11 slots, non-root minimum5 entries).
// This is deliberately not applied to response tails that will be truncated.
pub(crate) fn value_bytes(value: &serde_json::Value, maximum: usize) -> ToolkitResult<usize> {
    fn visit(
        value: &serde_json::Value,
        depth: usize,
        total: &mut usize,
        maximum: usize,
    ) -> ToolkitResult<()> {
        if depth > 128 {
            return Err(budget("LSP query descriptor depth budget"));
        }
        let mut add = |bytes: usize| -> ToolkitResult<()> {
            *total = total
                .checked_add(bytes)
                .filter(|n| *n <= maximum)
                .ok_or_else(|| budget("LSP query descriptor allocation budget"))?;
            Ok(())
        };
        add(std::mem::size_of::<serde_json::Value>())?;
        match value {
            serde_json::Value::String(text) => add(text.capacity())?,
            serde_json::Value::Number(number) => add(serialized_bytes(number, maximum)?)?,
            serde_json::Value::Array(values) => {
                add(values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<serde_json::Value>()))?;
                for child in values {
                    visit(child, depth + 1, total, maximum)?;
                }
            }
            serde_json::Value::Object(values) => {
                let nodes = if values.is_empty() {
                    0
                } else {
                    1 + (values.len() - 1) / 5
                };
                let node_bytes = (12
                    * (std::mem::size_of::<String>() + std::mem::size_of::<serde_json::Value>())
                    + 256)
                    .next_power_of_two();
                add(nodes.saturating_mul(node_bytes))?;
                for (key, child) in values {
                    *total = total
                        .checked_add(key.capacity())
                        .filter(|n| *n <= maximum)
                        .ok_or_else(|| budget("LSP query descriptor allocation budget"))?;
                    visit(child, depth + 1, total, maximum)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut total = 0;
    visit(value, 0, &mut total, maximum)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn legacy_ignored(value: &Value) -> bool {
        value["method"].as_str().is_some() && value.get("id").is_none()
    }
    fn compare(raw: &[u8]) {
        let legacy = serde_json::from_slice::<Value>(raw);
        match ignored_notification(raw) {
            Ok(true) => assert!(legacy_ignored(&legacy.expect("routing hid invalid JSON"))),
            // Preserved frames are still passed to the exact legacy decoder.
            Ok(false) => {}
            Err(error) => assert_eq!(error.to_string(), legacy.unwrap_err().to_string()),
        }
    }

    #[test]
    fn notifications_keep_duplicate_and_escaped_key_semantics() {
        for raw in [
            r#"{"method":"$/progress","params":{"items":[1,true,null,{"text":"x\\y"}]}}"#,
            r#"{"m\u0065thod":"$/progress"}"#,
            r#"{"method":null,"method":"$/progress"}"#,
        ] {
            compare(raw.as_bytes());
            assert!(ignored_notification(raw.as_bytes()).unwrap());
        }
        for raw in [
            r#"{"id":null,"method":"workspace/configuration"}"#,
            r#"{"\u0069d":7,"method":"window/workDoneProgress/create"}"#,
            r#"{"id":1,"id":null,"method":"client/registerCapability"}"#,
            r#"{"method":"$/progress","method":false}"#,
            r#"{"id":3,"result":[{"uri":"file:///x","range":{}}]}"#,
            r#"{"method":"progress","params":{"$serde_json::private::Number":"123"}}"#,
            r#"[null,{"method":"progress"}]"#,
        ] {
            compare(raw.as_bytes());
            assert!(!ignored_notification(raw.as_bytes()).unwrap());
        }
    }

    #[test]
    fn discarded_notifications_still_validate_all_json() {
        let mut cases: Vec<Vec<u8>> = [
            r#"{"method":"progress"} garbage"#,
            r#"{"method":"progress","params":[1,]}"#,
            r#"{"method":"progress","params":{"x":true,}}"#,
            r#"{"method":"progress","params":"\uD800"}"#,
            r#"{"method":"progress","params":01}"#,
            r#"{"method":"progress","params":1e+}"#,
            r#"{"method":"progress","params":NaN}"#,
            r#"{"method":"progress","params":1e400}"#,
            r#"{"method":"progress","params":184467440737095516160}"#,
            r#"{"method":"progress","params":-184467440737095516160}"#,
            r#"{"method":"progress","params":{"$serde_json::private::Number":"bad"}}"#,
            r#"{"$serde_json::private::Number":"1","method":"progress"}"#,
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
        cases.push(b"{\"method\":\"progress\",\"params\":\"\xff\"}".to_vec());
        for depth in [125, 126, 127, 128, 129, 256] {
            cases.push(
                format!(
                    "{{\"method\":\"progress\",\"params\":{}0{}}}",
                    "[".repeat(depth),
                    "]".repeat(depth)
                )
                .into_bytes(),
            );
        }
        for raw in cases {
            compare(&raw);
        }
    }

    #[test]
    fn large_discarded_tree_does_not_need_value_containers() {
        let raw = format!(
            "{{\"method\":\"progress\",\"params\":[{}]}}",
            "{\"a\":[null,true,\"x\"]},"
                .repeat(20000)
                .trim_end_matches(',')
        );
        assert!(ignored_notification(raw.as_bytes()).unwrap());
        compare(raw.as_bytes());
    }

    fn peer_frame(result: &str) -> Frame {
        let raw = format!("{{\"id\":1,\"result\":{result}}}").into_bytes();
        assert!(raw.len() <= FRAME_BYTES);
        Frame::parse(raw).unwrap()
    }
    fn loc() -> String {
        r#"{"uri":"file:///D:/scope/界.lua","range":{"start":{"line":2,"character":3},"end":{"line":4,"character":5}}}"#.into()
    }
    #[test]
    fn raw_projection_preserves_last_duplicate_and_escaped_keys() {
        let raw = format!(
            r#"{{"id":9,"\u0069d":1,"result":false,"re\u0073ult":[{}],"unknown":{{"x":[null,true,{{"quoted":"[,]{{}}"}}]}}}}"#,
            loc()
        );
        let frame = Frame::parse(raw.into_bytes()).unwrap();
        assert!(frame.id_matches(1).unwrap());
        assert!(!frame.id_matches(9).unwrap());
        let projected = locations(frame.result()).unwrap();
        assert_eq!(projected.items.len(), 1);
        assert_eq!(projected.items[0].uri, "file:///D:/scope/界.lua");
        assert_eq!(
            projected.items[0].range.start,
            Position {
                line: 2,
                character: 3
            }
        );
        assert!(!projected.truncated);
    }
    #[test]
    fn large_legal_tail_preserves_first1000_and_truncation_without_dom() {
        let prefix = std::iter::repeat_n(loc(), LOCATIONS)
            .collect::<Vec<_>>()
            .join(",");
        // A Value decoder creates roughly900000 BTreeMap leaves here. That
        // discarded location tail is valid JSON but is never business output.
        let tail = "{\"\":0},".repeat(900000);
        let result = format!("[{prefix},{{\"unused\":[{}]}}]", tail.trim_end_matches(','));
        let frame = peer_frame(&result);
        let projected = locations(frame.result()).unwrap();
        assert_eq!(projected.items.len(), LOCATIONS);
        assert!(projected.truncated);
        assert_eq!(
            projected.items.last().unwrap().range.end,
            Position {
                line: 4,
                character: 5
            }
        );
    }
    #[test]
    fn small_location_prefix_matches_legacy_and_rejects_bad_first_item() {
        let result = format!("[{},{}]", loc(), loc());
        let legacy: Value = serde_json::from_str(&result).unwrap();
        let frame = peer_frame(&result);
        let projected = locations(frame.result()).unwrap();
        for (old, new) in legacy.as_array().unwrap().iter().zip(projected.items) {
            assert_eq!(old["uri"], new.uri);
            assert_eq!(old["range"], serde_json::to_value(new.range).unwrap());
        }
        let frame = peer_frame(
            r#"[{"uri":"file:///D:/scope/a.lua","range":{"start":{"line":-1,"character":0},"end":{"line":2,"character":0}}}]"#,
        );
        assert_eq!(
            locations(frame.result()).unwrap_err().code,
            "luals.check_failed"
        );
    }
    #[test]
    fn location_links_select_uri_and_range_together_with_go_zero_defaults() {
        for primary in [r#""uri":"","#, r#""uri":null,"#, ""] {
            let frame = peer_frame(&format!(
                "{{{primary}\"range\":{{\"start\":{{\"line\":1}},\"end\":{{\"line\":2}}}},\"targetUri\":\"file:///D:/scope/link.lua\",\"targetRange\":{{\"start\":{{\"line\":8}},\"end\":{{\"line\":9}}}}}}"
            ));
            let result = locations(frame.result()).unwrap();
            assert_eq!(result.items[0].uri, "file:///D:/scope/link.lua");
            assert_eq!(
                result.items[0].range.start,
                Position {
                    line: 8,
                    character: 0
                }
            );
            assert_eq!(
                result.items[0].range.end,
                Position {
                    line: 9,
                    character: 0
                }
            );
        }
        let frame = peer_frame(
            r#"{"uri":"file:///D:/scope/ordinary.lua","targetUri":"file:///D:/scope/link.lua","targetRange":{"start":{"line":8},"end":{"line":9}}}"#,
        );
        assert_eq!(
            locations(frame.result()).unwrap().items[0].range,
            Range {
                start: Position {
                    line: 0,
                    character: 0
                },
                end: Position {
                    line: 0,
                    character: 0
                }
            }
        );
        for bad in ["true", "1", "[]", "{}"] {
            let frame = peer_frame(&format!(
                "{{\"uri\":{bad},\"targetUri\":\"file:///D:/scope/link.lua\"}}"
            ));
            assert_eq!(
                locations(frame.result()).unwrap_err().code,
                "luals.check_failed"
            );
        }
    }
    #[test]
    fn unused_tail_still_checks_utf8_private_number_and_json() {
        let prefix = std::iter::repeat_n(loc(), LOCATIONS)
            .collect::<Vec<_>>()
            .join(",");
        for tail in [
            r#""\uD800""#,
            r#"{"$serde_json::private::Number":"bad"}"#,
            "[1,]",
        ] {
            let raw = format!("{{\"id\":1,\"result\":[{prefix},{tail}]}}");
            let legacy = serde_json::from_str::<Value>(&raw).unwrap_err();
            assert_eq!(
                Frame::parse(raw.into_bytes()).unwrap_err().message,
                legacy.to_string()
            );
        }
    }
    #[test]
    fn huge_hover_keeps_utf8_prefix_and_validates_late_contents() {
        let text = format!("{}中{}", "a".repeat(16383), "x".repeat(6 << 20));
        let frame =
            peer_frame(&serde_json::to_string(&serde_json::json!({"contents":text})).unwrap());
        let (text, cut) = hover(frame.result()).unwrap();
        assert_eq!(text, "a".repeat(16383));
        assert!(cut);
        let frame = peer_frame(
            &serde_json::to_string(
                &serde_json::json!({"contents":["x".repeat(20000), {"bad":null}]}),
            )
            .unwrap(),
        );
        assert_eq!(
            hover(frame.result()).unwrap_err().message,
            "Unsupported hover contents"
        );
        let frame =
            peer_frame(r#"{"contents":["first",{"language":"lua","value":"second"},"",null]}"#);
        assert_eq!(
            hover(frame.result()).unwrap(),
            ("first\nsecond".into(), false)
        );
    }
    #[test]
    fn initialization_unknown_extensions_do_not_build_value_trees() {
        let tail = "{\"\":0},".repeat(900000);
        let result = format!(
            "{{\"capabilities\":{{\"definitionProvider\":{{}},\"referencesProvider\":true,\"referencesProvider\":false,\"hoverProvider\":true,\"positionEncoding\":\"utf-16\",\"unused\":[{}]}}}}",
            tail.trim_end_matches(',')
        );
        let frame = peer_frame(&result);
        let caps = capabilities(frame.result()).unwrap();
        assert!(caps.definition && caps.hover && !caps.references);
    }
    #[test]
    fn private_numbers_are_validated_before_ignored_notifications() {
        for raw in [
            r#"{"method":"progress","params":{"n":1.25}}"#,
            r#"{"method":"progress","params":{"$serde_json::private::Number":"1e400"}}"#,
        ] {
            let old: Value = serde_json::from_str(raw).unwrap();
            assert!(legacy_ignored(&old));
            assert!(Frame::parse(raw.as_bytes().to_vec()).unwrap().ignored());
        }
        for raw in [
            r#"{"method":"progress","params":{"$serde_json::private::Number":false}}"#,
            r#"{"method":"progress","params":{"$serde_json::private::Number":"bad"}}"#,
            r#"{"$serde_json::private::Number":"1","method":"progress"}"#,
        ] {
            let old = serde_json::from_str::<Value>(raw).unwrap_err();
            assert_eq!(
                Frame::parse(raw.as_bytes().to_vec()).unwrap_err().message,
                old.to_string()
            );
        }
    }
    #[test]
    fn output_admission_is_exact_and_counting_does_not_build_a_buffer() {
        let value = serde_json::json!({"text":"a\n中", "values":[1,true,null]});
        let old = serde_json::to_vec(&value).unwrap();
        assert_eq!(serialize(&value, old.len()).unwrap(), old);
        assert_eq!(serialized_bytes(&value, old.len()).unwrap(), old.len());
        assert_eq!(
            serialize(&value, old.len() - 1).unwrap_err().code,
            "luals.input_budget"
        );
        assert_eq!(
            serialized_bytes(&value, old.len() - 1).unwrap_err().code,
            "luals.input_budget"
        );
    }
    #[test]
    fn server_error_projection_keeps_frozen_go_fields_and_last_duplicates() {
        let frame = peer_frame("null");
        assert!(frame.id_matches(1).unwrap());
        for (raw, expected) in [
            (
                r#"{"code":-32603,"message":"not enough memory","data":[{"unused":true}]}"#,
                "server error -32603: not enough memory",
            ),
            (
                r#"{"code":1,"code":-32603,"message":"first","message":"末\n尾"}"#,
                "server error -32603: 末\n尾",
            ),
            (r#"{"code":null,"message":null}"#, "server error 0: "),
            (r#"{}"#, "server error 0: "),
        ] {
            let raw = Frame::parse(format!("{{\"id\":1,\"error\":{raw}}}").into_bytes()).unwrap();
            assert_eq!(
                error_message(raw.field("error").unwrap()).unwrap(),
                expected
            );
        }
        for raw in [
            r#"{"code":"-1","message":"x"}"#,
            r#"{"code":1.5,"message":"x"}"#,
            r#"{"code":1,"message":false}"#,
            "[]",
        ] {
            assert_eq!(
                error_message(raw.as_bytes()).unwrap_err().code,
                "luals.check_failed"
            );
        }
    }
}
