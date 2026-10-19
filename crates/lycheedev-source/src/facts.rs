// SPDX-License-Identifier: MIT
// Source fact contracts ported from the MIT wowdoc-derived Lychee Dev scanner.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(crate) mod worker;
use worker::{ScratchFrame, WorkerGuard};

/// The public parser mode belongs to the immutable source path. Generated Lua
/// shares the complete ordinary observations and appends its literal API facts;
/// adding a worker format must not silently disable generated observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParseMode {
    LuaOrdinary,
    LuaGenerated,
    Xml,
    Toc,
}
impl ParseMode {
    pub(crate) fn for_path(path: &str) -> Self {
        let suffix = path
            .as_bytes()
            .get(path.len().saturating_sub(4)..)
            .unwrap_or_default();
        if suffix.eq_ignore_ascii_case(b".lua") {
            if path.contains("/Blizzard_APIDocumentationGenerated/") {
                Self::LuaGenerated
            } else {
                Self::LuaOrdinary
            }
        } else if suffix.eq_ignore_ascii_case(b".toc") {
            Self::Toc
        } else {
            Self::Xml
        }
    }
    pub(crate) fn parse(self, raw: &[u8]) -> Facts {
        match self {
            Self::LuaOrdinary => lua(raw, false),
            Self::LuaGenerated => lua(raw, true),
            Self::Xml => xml(raw),
            Self::Toc => toc(raw),
        }
    }
}
/// Pure, optional preparation with one shared document allowance. Admission or
/// cancellation failure abandons every partial fact; the ordered caller keeps
/// the full original parser and selects the public result at its original turn.
pub(crate) fn for_worker(
    raw: &[u8],
    mode: ParseMode,
    control: &lycheedev_core::control::OperationControl,
    max_owned_bytes: usize,
) -> Option<Facts> {
    if raw.len() > 64 << 10 || std::str::from_utf8(raw).is_err() {
        return None;
    }
    let guard = WorkerGuard::new(control, max_owned_bytes);
    if !guard.check() {
        return None;
    }
    let facts = match mode {
        ParseMode::LuaOrdinary => crate::lua_tree::facts_with_worker_guard(raw, &guard),
        ParseMode::Toc => toc_with_guard(raw, Some(&guard)),
        ParseMode::Xml => {
            let mut lines = XmlLines::new(raw);
            xml_guarded(raw, |offset| lines.at(offset), Some(&guard))
        }
        ParseMode::LuaGenerated => {
            crate::lua_tree::generated_facts_with_worker_guard(raw, true, &guard)
        }
    };
    // Linear scanner Drop checks have completed before this final boundary.
    // Never publish a precomputed Some after an exit check abandoned its guard.
    guard.check().then_some(facts).flatten()
}

pub(crate) mod binary;

// The single field declaration defines the public JSON type and its private
// positional wire view. Public serde attributes never apply to the wire: in
// particular, empty optional strings must still occupy their binary position.
macro_rules! fact_contract {
    (
        $(#[$attribute:meta])*
        pub struct $name:ident {
            $( $(#[$field_attribute:meta])* pub $field:ident: $kind:ty, )*
        }
        $(skipped { $( $(#[$skip_attribute:meta])* $visibility:vis $skip:ident: $skip_kind:ty, )* })?
    ) => {
        $(#[$attribute])*
        pub struct $name {
            $( $(#[$field_attribute])* pub $field: $kind, )*
            $( $( $(#[$skip_attribute])* $visibility $skip: $skip_kind, )* )?
        }
        impl binary::Wire for $name {
            const MIN_BYTES: usize = 0 $(+ <$kind as binary::Wire>::MIN_BYTES)*;
            fn encode<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeTuple;
                let fields = [$(stringify!($field)),*];
                let mut tuple = serializer.serialize_tuple(fields.len())?;
                $(tuple.serialize_element(&binary::WireRef(&self.$field))?;)*
                tuple.end()
            }
            fn decode<'de, D: serde::Deserializer<'de>>(
                deserializer: D,
                context: &mut binary::DecodeContext<'_>,
            ) -> Result<Self, D::Error> {
                struct Row<'a, 'b>(&'a mut binary::DecodeContext<'b>);
                impl<'de> serde::de::Visitor<'de> for Row<'_, '_> {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str(concat!("complete binary ", stringify!($name)))
                    }
                    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<$name, A::Error> {
                        self.0.check::<A::Error>()?;
                        Ok($name {
                            $($field: sequence.next_element_seed(binary::Seed::<$kind>::new(self.0))?
                                .ok_or_else(|| serde::de::Error::custom(concat!("missing ", stringify!($field))))?,)*
                            $($( $skip: Default::default(), )*)?
                        })
                    }
                }
                let fields = [$(stringify!($field)),*];
                deserializer.deserialize_tuple(fields.len(), Row(context))
            }
        }
    };
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    pub name: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    pub line: usize,
    pub end_line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
}
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from: String,
    pub to: String,
    pub category: String,
    pub confidence: String,
    pub line: usize,
}
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadReference {
    pub path: String,
    pub line: usize,
    pub kind: String,
}
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderValue {
    pub key: String,
    pub value: String,
    pub line: usize,
}
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxNote {
    pub message: String,
    pub line: usize,
}
}

fact_contract! {
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Facts {
    pub declarations: Vec<Declaration>,
    pub relationships: Vec<Relationship>,
    pub loads: Vec<LoadReference>,
    pub headers: Vec<HeaderValue>,
    pub diagnostics: Vec<SyntaxNote>,
}
    skipped {
    /// Only fresh parser observations retain pre-JSON target identity. None
    /// after deserialization means that raw identity cannot be authenticated.
    #[serde(skip)]
    pub(crate) raw_relationship_targets: Option<BTreeMap<usize, Vec<u8>>>,
}
}

impl Facts {
    pub(crate) fn fresh() -> Self {
        Self {
            raw_relationship_targets: Some(BTreeMap::new()),
            ..Self::default()
        }
    }
    pub(crate) fn relationship_target_bytes(&self, ordinal: usize) -> Option<&[u8]> {
        let edge = self.relationships.get(ordinal)?;
        let originals = self.raw_relationship_targets.as_ref()?;
        Some(
            originals
                .get(&ordinal)
                .map_or(edge.to.as_bytes(), Vec::as_slice),
        )
    }
}
pub fn lua(raw: &[u8], generated: bool) -> Facts {
    match crate::lua_tree::SyntaxDocument::new_public(raw) {
        Ok(d) => d.facts(generated),
        Err(e) => {
            let mut f = Facts::fresh();
            f.diagnostics.push(SyntaxNote {
                message: e.to_string(),
                line: 0,
            });
            f
        }
    }
}
pub(crate) const LUA_WORKER_SCRATCH_BYTES: usize = 32 << 20;
/// Keep the original Lua regression entry pointed at the production dispatcher.
#[cfg(test)]
pub(crate) fn lua_for_worker(
    raw: &[u8],
    control: &lycheedev_core::control::OperationControl,
    max_owned_bytes: usize,
) -> Option<Facts> {
    for_worker(raw, ParseMode::LuaOrdinary, control, max_owned_bytes)
}
pub fn toc(raw: &[u8]) -> Facts {
    toc_with_guard(raw, None).expect("unrestricted TOC facts")
}
fn toc_with_guard(raw: &[u8], guard: Option<&WorkerGuard<'_>>) -> Option<Facts> {
    let mut f = Facts::fresh();
    let text = String::from_utf8_lossy(raw.strip_prefix(&[239, 187, 191]).unwrap_or(raw));
    for (i, line) in text.split('\n').enumerate() {
        if !worker::check(guard) {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix("##") {
            if let Some((key, value)) = header.trim().split_once(':') {
                let (key, value) = (key.trim(), value.trim());
                if !worker::charge(guard, key.len().saturating_add(value.len()))
                    || !worker::prepare_push(guard, &mut f.headers)
                {
                    return None;
                }
                f.headers.push(HeaderValue {
                    key: key.into(),
                    value: value.into(),
                    line: i + 1,
                });
            }
        } else if !line.starts_with('#') {
            if !worker::charge(guard, line.len().saturating_add(3))
                || !worker::prepare_push(guard, &mut f.loads)
            {
                return None;
            }
            f.loads.push(LoadReference {
                path: line.into(),
                line: i + 1,
                kind: "toc".into(),
            });
        }
    }
    worker::check(guard).then_some(f)
}

/// LF positions are queried in parser order. Keep the exact prefix definition
/// for a backwards query as well, without adding an assumption to parsing.
struct XmlLines<'a> {
    raw: &'a [u8],
    cursor: usize,
    line: usize,
    #[cfg(test)]
    scanned: usize,
}
impl<'a> XmlLines<'a> {
    fn new(raw: &'a [u8]) -> Self {
        Self {
            raw,
            cursor: 0,
            line: 1,
            #[cfg(test)]
            scanned: 0,
        }
    }
    fn at(&mut self, offset: usize) -> usize {
        if offset < self.cursor {
            #[cfg(test)]
            {
                self.scanned += offset;
            }
            return 1 + self.raw[..offset]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count();
        }
        #[cfg(test)]
        {
            self.scanned += offset - self.cursor;
        }
        self.line += self.raw[self.cursor..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        self.cursor = offset;
        self.line
    }
}

// The public Go scanner consumes XML tokens, not the native UI profile. Native
// xml_program/xml_inline retain their authenticated catalog, all script kinds,
// segmented source maps and stricter profile checks at their own entrypoints.
pub fn xml(raw: &[u8]) -> Facts {
    let mut lines = XmlLines::new(raw);
    xml_with_line_counter(raw, |offset| lines.at(offset))
}
// encoding/xml normalizes physical CR/CRLF before expanding entities. It
// retains tabs/LF and character-reference CRs, including inside attributes.
pub(crate) fn xml_attribute(
    attribute: &quick_xml::events::attributes::Attribute<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> std::result::Result<String, String> {
    xml_attribute_guarded(attribute, decoder, None)
}
fn xml_attribute_guarded(
    attribute: &quick_xml::events::attributes::Attribute<'_>,
    decoder: quick_xml::encoding::Decoder,
    guard: Option<&WorkerGuard<'_>>,
) -> std::result::Result<String, String> {
    let decoded = decoder
        .decode(attribute.value.as_ref())
        .map_err(|_| "invalid UTF-8".to_owned())?;
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let mut value = String::new();
    let mut rest = normalized.as_str();
    while let Some(at) = rest.find('&') {
        if !worker::check(guard) {
            return Err(String::new());
        }
        if rest[..at].contains('<') {
            return Err("unescaped < inside quoted string".into());
        }
        value.push_str(&rest[..at]);
        let entity = &rest[at..];
        let end = xml_reference_end(entity.as_bytes());
        if entity.as_bytes().get(end) != Some(&b';') {
            return Err(format!(
                "invalid character entity {} (no semicolon)",
                &entity[..end]
            ));
        }
        value.push_str(&xml_entity_value_guarded(&entity[1..end], guard)?);
        rest = &entity[end + 1..];
    }
    if rest.contains('<') {
        return Err("unescaped < inside quoted string".into());
    }
    value.push_str(rest);
    xml_characters_guarded(&value, guard)?;
    Ok(value)
}
pub(crate) fn xml_entity(reference: &str) -> std::result::Result<String, String> {
    let value = xml_entity_value(reference)?;
    xml_characters(&value)?;
    Ok(value)
}
fn xml_entity_value(reference: &str) -> std::result::Result<String, String> {
    xml_entity_value_guarded(reference, None)
}
fn xml_entity_value_guarded(
    reference: &str,
    guard: Option<&WorkerGuard<'_>>,
) -> std::result::Result<String, String> {
    let mut scan = worker::ByteScan::new(guard);
    let value = match reference {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => reference.strip_prefix('#').and_then(|number| {
            let (number, radix) = number
                .strip_prefix('x')
                .map_or((number, 10), |number| (number, 16));
            if number.is_empty() {
                return None;
            }
            let mut value = 0u32;
            for ch in number.chars() {
                if !scan.bytes(ch.len_utf8()) {
                    return None;
                }
                let digit = ch.to_digit(radix)?;
                value = value.checked_mul(radix)?.checked_add(digit)?;
            }
            (value <= 0x10ffff).then(|| char::from_u32(value).unwrap_or('\u{fffd}'))
        }),
    };
    // Cancellation inside a long numeric reference must be observed before
    // allocating its potentially source-sized diagnostic, as well as success.
    if !scan.check() {
        return Err(String::new());
    }
    let value = value.ok_or_else(|| format!("invalid character entity &{reference};"))?;
    Ok(value.to_string())
}
pub(crate) fn xml_characters(text: &str) -> std::result::Result<(), String> {
    xml_characters_guarded(text, None)
}
fn xml_characters_guarded(
    text: &str,
    guard: Option<&WorkerGuard<'_>>,
) -> std::result::Result<(), String> {
    let mut scan = worker::ByteScan::new(guard);
    for ch in text.chars() {
        if !scan.bytes(ch.len_utf8()) {
            return Err(String::new());
        }
        if !matches!(ch,'\t'|'\n'|'\r'|' '..='\u{d7ff}'|'\u{e000}'..='\u{fffd}'|'\u{10000}'..='\u{10ffff}')
        {
            return Err(format!("illegal character code U+{:04X}", ch as u32));
        }
    }
    Ok(())
}
fn is_public_handler(tag: &str) -> bool {
    tag.len() > 2 && tag.starts_with("On")
}
fn append_inline(
    facts: &mut Facts,
    script: &str,
    script_line: usize,
    owner: &str,
    tag: &str,
    guard: Option<&WorkerGuard<'_>>,
) -> Option<()> {
    if script.trim().is_empty() {
        return worker::check(guard).then_some(());
    }
    let mut inner = if let Some(guard) = guard {
        crate::lua_tree::facts_with_worker_guard(script.as_bytes(), guard)?
    } else {
        lua(script.as_bytes(), false)
    };
    let offset = script_line.saturating_sub(1);
    for declaration in &mut inner.declarations {
        if !worker::check(guard) {
            return None;
        }
        declaration.line += offset;
        // Go adds the offset even to the zero LastLine of assignments.
        declaration.end_line += offset;
    }
    for relation in &mut inner.relationships {
        if !worker::check(guard) {
            return None;
        }
        relation.line += offset;
        if relation.from == "<file>" {
            let from = format!("{owner}:{tag}");
            if !worker::charge(guard, from.capacity()) {
                return None;
            }
            relation.from = from;
        }
    }
    for diagnostic in &mut inner.diagnostics {
        if !worker::check(guard) {
            return None;
        }
        diagnostic.line = if diagnostic.line > 0 {
            diagnostic.line + offset
        } else {
            script_line
        };
    }
    // Keep token order and raw target ownership. Inner strings/Vec payloads
    // move, but destination vector/map growth is additionally admitted while
    // the original buffers still coexist. No handler gets a fresh Facts cap.
    let offset = facts.relationships.len();
    match (
        &mut facts.raw_relationship_targets,
        inner.raw_relationship_targets.take(),
    ) {
        (Some(targets), Some(inner)) => {
            for (ordinal, raw) in inner {
                if guard.is_some_and(|guard| !guard.raw_target(targets.len(), 0)) {
                    return None;
                }
                targets.insert(offset + ordinal, raw);
            }
        }
        _ => facts.raw_relationship_targets = None,
    }
    for declaration in inner.declarations {
        if !worker::prepare_push(guard, &mut facts.declarations) {
            return None;
        }
        facts.declarations.push(declaration);
    }
    for relationship in inner.relationships {
        if !worker::prepare_push(guard, &mut facts.relationships) {
            return None;
        }
        facts.relationships.push(relationship);
    }
    for diagnostic in inner.diagnostics {
        if !worker::prepare_push(guard, &mut facts.diagnostics) {
            return None;
        }
        facts.diagnostics.push(diagnostic);
    }
    worker::check(guard).then_some(())
}

fn xml_error_message(error: quick_xml::Error) -> String {
    use quick_xml::errors::{IllFormedError, SyntaxError};
    match error {
        quick_xml::Error::Syntax(SyntaxError::UnclosedCData) => {
            "unexpected EOF in CDATA section".into()
        }
        quick_xml::Error::Syntax(_) => "unexpected EOF".into(),
        quick_xml::Error::IllFormed(IllFormedError::MismatchedEndTag { expected, found }) => {
            let (expected_prefix, expected) = xml_name(&expected);
            let (found_prefix, found) = xml_name(&found);
            if expected == found && expected_prefix != found_prefix {
                format!(
                    "element <{expected}> in space {expected_prefix} closed by </{found}> in space {}",
                    if found_prefix.is_empty() {
                        "\"\""
                    } else {
                        found_prefix
                    }
                )
            } else {
                format!("element <{expected}> closed by </{found}>")
            }
        }
        quick_xml::Error::IllFormed(IllFormedError::UnmatchedEndTag(found)) => format!(
            "unexpected end element </{}>",
            found.rsplit(':').next().unwrap_or(&found)
        ),
        quick_xml::Error::IllFormed(IllFormedError::DoubleHyphenInComment) => {
            "invalid sequence \"--\" not allowed in comments".into()
        }
        other => other.to_string(),
    }
}
fn xml_attribute_error(error: quick_xml::events::attributes::AttrError) -> String {
    use quick_xml::events::attributes::AttrError;
    match error {
        AttrError::ExpectedEq(_) => "attribute name without = in element".into(),
        AttrError::ExpectedValue(_) | AttrError::UnquotedValue(_) => {
            "unquoted or missing attribute value in element".into()
        }
        AttrError::ExpectedQuote(_, _) => "unexpected EOF".into(),
        other => other.to_string(),
    }
}
// encoding/xml compares the literal namespace prefixes before translation.
// Empty prefixes/suffixes are retained as part of the local name.
fn xml_name(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() => (prefix, local),
        _ => ("", name),
    }
}
// The fixed Go decoder rejects these lexical failures before it compares end
// tags or returns a token to scanMarkup. quick-xml either accepts the bytes or
// reports a later error. Return the Go consumed offset, not the event's end.
// Keep this separate from the native XML profile: that profile has its own
// grammar and is not the public Go scanner contract.
fn public_xml_token_failure(raw: &[u8]) -> Option<(String, usize)> {
    if raw.starts_with(b"<!-") {
        return match raw.get(3) {
            None => Some(("unexpected EOF".into(), raw.len())),
            Some(b'-') => None,
            Some(_) => Some(("invalid sequence <!- not part of <!--".into(), 4)),
        };
    }
    if raw.starts_with(b"<![") {
        for (index, expected) in b"CDATA[".iter().enumerate() {
            match raw.get(3 + index) {
                None => return Some(("unexpected EOF".into(), raw.len())),
                Some(actual) if actual != expected => {
                    return Some(("invalid <![ sequence".into(), 4 + index));
                }
                Some(_) => {}
            }
        }
        return None;
    }
    if !raw.starts_with(b"</") {
        return None;
    }
    // Go readName consumes every name byte before validating the name, and
    // unreads the delimiter. An unterminated name therefore reports EOF even
    // when its first character would otherwise be invalid.
    let mut end = 2;
    while raw.get(end).is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b':' | b'.' | b'-') || *byte >= 128
    }) {
        end += 1;
    }
    // Go's Unicode name tables differ from the native XML 1.0 Fifth Edition
    // profile. Do not silently impose that profile on this public path. Those
    // names retain the existing parser until independently compared.
    if !raw[2..end].is_ascii() {
        return None;
    }
    if end == raw.len() {
        return Some(("unexpected EOF".into(), end));
    }
    if end == 2 {
        return Some(("expected element name after </".into(), end));
    }
    let name = std::str::from_utf8(&raw[2..end]).expect("ASCII end name");
    if !raw[2].is_ascii_alphabetic() && !matches!(raw[2], b'_' | b':') {
        return Some((format!("invalid XML name: {name}"), end));
    }
    if name.bytes().filter(|byte| *byte == b':').count() > 1 {
        return Some(("expected element name after </".into(), end));
    }
    while raw
        .get(end)
        .is_some_and(|byte| matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        end += 1;
    }
    match raw.get(end) {
        None => Some(("unexpected EOF".into(), end)),
        Some(b'>') => None,
        Some(_) => Some((
            format!("invalid characters between </{} and >", xml_name(name).1),
            end + 1,
        )),
    }
}
fn xml_reference_end(raw: &[u8]) -> usize {
    xml_reference_end_scanned(raw, &mut worker::ByteScan::new(None))
}
fn xml_reference_end_scanned(raw: &[u8], scan: &mut worker::ByteScan<'_>) -> usize {
    if !scan.byte() {
        return 0;
    }
    let mut at = 1; // The initial & belongs to the error token.
    if raw.get(at) == Some(&b'#') {
        if !scan.byte() {
            return at;
        }
        at += 1;
        let hex = raw.get(at) == Some(&b'x');
        if hex {
            if !scan.byte() {
                return at;
            }
            at += 1;
        }
        while raw.get(at).is_some_and(|byte| {
            if hex {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            }
        }) {
            if !scan.byte() {
                return at;
            }
            at += 1;
        }
    } else {
        while raw.get(at).is_some_and(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(*byte, b'_' | b':' | b'.' | b'-')
                || *byte >= 128
        }) {
            if !scan.byte() {
                return at;
            }
            at += 1;
        }
    }
    at
}
// A Go CharData token contains adjacent text and references. Validate the
// entire token once, preserving first-failure precedence and the scanner's
// consumed-byte position (which can precede quick-xml's final event cursor).
fn xml_text_failure(raw: &[u8], guard: Option<&WorkerGuard<'_>>) -> Option<(String, usize)> {
    let mut scan = worker::ByteScan::new(guard);
    let end = scan.find(raw, b"<").unwrap_or(raw.len());
    if !scan.check() {
        return None;
    }
    let text = &raw[..end];
    let mut expanded = Vec::new();
    let mut at = 0;
    let mut copied = 0;
    while at < text.len() {
        if !scan.byte() {
            return None;
        }
        if text[at..].starts_with(b"]]>") {
            return Some(("unescaped ]]> not in CDATA section".into(), at + 3));
        }
        if text[at] == b'&' {
            if !append_scanned(&mut expanded, &text[copied..at], &mut scan) {
                return None;
            }
            let finish = at + xml_reference_end_scanned(&text[at..], &mut scan);
            if !scan.check() {
                return None;
            }
            if text.get(finish) != Some(&b';') {
                return Some((
                    if finish == raw.len() {
                        "unexpected EOF".into()
                    } else {
                        format!(
                            "invalid character entity {} (no semicolon)",
                            String::from_utf8_lossy(&text[at..finish])
                        )
                    },
                    finish,
                ));
            }
            let reference = std::str::from_utf8(&text[at + 1..finish]).ok()?;
            match xml_entity_value_guarded(reference, guard) {
                Ok(value) => {
                    if !append_scanned(&mut expanded, value.as_bytes(), &mut scan) {
                        return None;
                    }
                }
                Err(error) => return Some((error, finish + 1)),
            }
            if !scan.byte() {
                return None;
            }
            at = finish + 1;
            copied = at;
        } else {
            at += 1;
        }
    }
    let value = if copied == 0 {
        std::borrow::Cow::Borrowed(text)
    } else {
        if !append_scanned(&mut expanded, &text[copied..], &mut scan) {
            return None;
        }
        std::borrow::Cow::Owned(expanded)
    };
    let value = std::str::from_utf8(&value).ok()?;
    xml_characters_guarded(value, guard)
        .err()
        .map(|error| (error, end))
}
fn append_scanned(output: &mut Vec<u8>, bytes: &[u8], scan: &mut worker::ByteScan<'_>) -> bool {
    for chunk in bytes.chunks(255) {
        // Expansion/copy work shares the parent scan's counter. Check again
        // immediately before a possible Vec growth, regardless of block state.
        if !scan.bytes(chunk.len()) || !scan.check() {
            return false;
        }
        output.extend_from_slice(chunk);
    }
    true
}
fn xml_attribute_failure_offset(raw: &[u8]) -> usize {
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'<' {
            return at + 1;
        }
        if raw[at] == b'&' {
            let end = at + xml_reference_end(&raw[at..]);
            if raw.get(end) != Some(&b';') {
                return end;
            }
            if std::str::from_utf8(&raw[at + 1..end])
                .is_ok_and(|reference| xml_entity_value(reference).is_err())
            {
                return end + 1;
            }
            at = end + 1;
        } else {
            at += 1;
        }
    }
    raw.len() + 1 // Character validity is checked after the closing quote.
}
// Match encoding/xml's deliberately permissive procInst attribute lookup,
// rather than imposing quick-xml's XML-declaration schema.
fn xml_declaration(content: &[u8]) -> std::result::Result<(), String> {
    fn parameter<'a>(content: &'a str, key: &str) -> &'a str {
        let needle = format!("{key}=");
        let mut rest = content;
        while let Some(at) = rest.find(&needle) {
            rest = &rest[at + needle.len()..];
            let Some(quote) = rest.as_bytes().first().copied() else {
                return "";
            };
            rest = &rest[rest.chars().next().expect("nonempty parameter").len_utf8()..];
            if matches!(quote, b'\'' | b'"') {
                return rest.find(quote as char).map_or("", |end| &rest[..end]);
            }
        }
        ""
    }
    let content = String::from_utf8_lossy(content);
    let version = parameter(&content, "version");
    if !version.is_empty() && version != "1.0" {
        return Err(format!(
            "xml: unsupported version {version:?}; only version 1.0 is supported"
        ));
    }
    let encoding = parameter(&content, "encoding");
    if !encoding.is_empty() && !encoding.eq_ignore_ascii_case("utf-8") {
        return Err(format!(
            "xml: encoding {encoding:?} declared but Decoder.CharsetReader is nil"
        ));
    }
    Ok(())
}
// Go ignores arbitrary <!directives>, with nested brackets and quoted >,
// without installing declared entities. Quick-xml only admits DOCTYPE. Mask
// those tokens to same-width ignored PIs; positions and source bytes stay raw.
fn public_xml_grammar<'a>(
    raw: &'a [u8],
    guard: Option<&WorkerGuard<'_>>,
) -> std::borrow::Cow<'a, [u8]> {
    let mut scan = worker::ByteScan::new(guard);
    let mut changed: Option<Vec<u8>> = None;
    let mut at = 0;
    while at < raw.len() {
        if !scan.byte() {
            return std::borrow::Cow::Borrowed(raw);
        }
        if raw[at..].starts_with(b"<!--")
            || raw[at..].starts_with(b"<![CDATA[")
            || raw[at..].starts_with(b"<?")
        {
            let end: &[u8] = if raw[at..].starts_with(b"<!--") {
                b"-->"
            } else if raw[at..].starts_with(b"<?") {
                b"?>"
            } else {
                b"]]>"
            };
            let Some(length) = scan.find(&raw[at..], end) else {
                break;
            };
            if !scan.bytes(end.len() - 1) {
                return std::borrow::Cow::Borrowed(raw);
            }
            at += length + end.len();
            continue;
        }
        if raw[at..].starts_with(b"<!") && !matches!(raw.get(at + 2), Some(b'-' | b'[')) {
            let start = at;
            if !scan.bytes(2) {
                return std::borrow::Cow::Borrowed(raw);
            }
            at += 3;
            let mut depth = 0usize;
            let mut quote = 0;
            let mut complete = false;
            while at < raw.len() {
                if !scan.byte() {
                    return std::borrow::Cow::Borrowed(raw);
                }
                let byte = raw[at];
                if quote != 0 {
                    if byte == quote {
                        quote = 0;
                    }
                } else if matches!(byte, b'\'' | b'"') {
                    quote = byte;
                } else if raw[at..].starts_with(b"<!--") {
                    let Some(end) = scan.find(&raw[at + 4..], b"-->") else {
                        break;
                    };
                    if !scan.bytes(5) {
                        return std::borrow::Cow::Borrowed(raw);
                    }
                    at += end + 7;
                    continue;
                } else if byte == b'<' {
                    depth += 1;
                } else if byte == b'>' {
                    if depth == 0 {
                        at += 1;
                        complete = true;
                        break;
                    }
                    depth -= 1;
                }
                at += 1;
            }
            if !complete {
                break;
            }
            if !scan.check() {
                return std::borrow::Cow::Borrowed(raw);
            }
            let bytes = changed.get_or_insert_with(|| raw.to_vec());
            for byte in &mut bytes[start + 1..at - 1] {
                if !scan.byte() {
                    return std::borrow::Cow::Borrowed(raw);
                }
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
            bytes[start + 1] = b'?';
            bytes[at - 2] = b'?';
            continue;
        }
        if raw[at] == b'<' {
            at += 1;
            let mut quote = 0;
            while at < raw.len() {
                if !scan.byte() {
                    return std::borrow::Cow::Borrowed(raw);
                }
                let byte = raw[at];
                at += 1;
                if quote != 0 {
                    if byte == quote {
                        quote = 0;
                    }
                } else if matches!(byte, b'\'' | b'"') {
                    quote = byte;
                } else if byte == b'>' {
                    break;
                }
            }
        } else {
            at += 1;
        }
    }
    changed
        .map(std::borrow::Cow::Owned)
        .unwrap_or(std::borrow::Cow::Borrowed(raw))
}
fn xml_with_line_counter(raw: &[u8], line_at: impl FnMut(usize) -> usize) -> Facts {
    xml_guarded(raw, line_at, None).expect("unrestricted XML facts")
}
/// Only these five attributes affect public Facts. Every attribute is still
/// decoded and validated in input order, and duplicate local names keep the
/// last value exactly as the former BTreeMap did.
#[derive(Default)]
struct XmlAttributes {
    values: [Option<String>; 5],
}
impl XmlAttributes {
    const NAMES: [&'static str; 5] = ["name", "file", "inherits", "function", "method"];
    fn insert(&mut self, name: &str, value: String) {
        if let Some(index) = Self::NAMES
            .iter()
            .position(|key| name.eq_ignore_ascii_case(key))
        {
            self.values[index] = Some(value);
        }
    }
    fn get(&self, name: &str) -> Option<&String> {
        let index = Self::NAMES.iter().position(|key| name == *key)?;
        self.values[index].as_ref()
    }
}
struct XmlFrame<'a> {
    tag: String,
    owner: String,
    // Live inherited owner copies count against the same scratch as Lua frames.
    _strings: Option<ScratchFrame<'a>>,
}
fn xml_guarded(
    raw: &[u8],
    mut line_at: impl FnMut(usize) -> usize,
    guard: Option<&WorkerGuard<'_>>,
) -> Option<Facts> {
    use quick_xml::{Reader, events::Event};
    let grammar = public_xml_grammar(raw, guard);
    if !worker::check(guard) {
        return None;
    }
    let mut reader = Reader::from_reader(grammar.as_ref());
    reader.config_mut().check_comments = true;
    let mut facts = Facts::fresh();
    let mut stack = Vec::<XmlFrame<'_>>::new();
    let mut script = String::new();
    let mut script_line = 0;
    let mut character_data_line = None;
    let prefix = if raw.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    loop {
        if !worker::check(guard) {
            return None;
        }
        // The whole input bounds decoded attribute/text/error temporaries.
        // Keep this frame live during inline Lua: its existing scratch and
        // retained XML stack/script must fit together, never independent caps.
        let _event_scratch = if let Some(guard) = guard {
            Some(guard.frame(raw.len())?)
        } else {
            None
        };
        let at =
            crate::xml_inline::source_map::raw_cursor(raw.len(), prefix, reader.buffer_position())
                .expect("slice reader original cursor");
        let line = line_at(at.min(raw.len()));
        let event = reader.read_event();
        let empty = matches!(&event, Ok(Event::Empty(_)));
        let is_character_data = matches!(&event, Ok(Event::Text(_) | Event::GeneralRef(_)))
            || raw.get(at) == Some(&b'&');
        let first_character_data = is_character_data && character_data_line.is_none();
        let diagnostic_line = if is_character_data {
            *character_data_line.get_or_insert(line)
        } else {
            character_data_line = None;
            line
        };
        let mut error_offset = None;
        let result: std::result::Result<(), String> = (|| {
            if first_character_data
                && let Some((message, consumed)) = xml_text_failure(&raw[at..], guard)
            {
                error_offset = Some(at + consumed);
                return Err(message);
            }
            if !worker::check(guard) {
                return Err(String::new());
            }
            if let Some((message, consumed)) = public_xml_token_failure(&raw[at..]) {
                error_offset = Some(at + consumed);
                return Err(message);
            }
            if raw.get(at..at + 4) == Some(b"<!--") {
                if let Some(bad) = raw[at + 4..]
                    .windows(3)
                    .position(|bytes| bytes.starts_with(b"--"))
                {
                    if raw[at + 4 + bad + 2] != b'>' {
                        error_offset = Some(at + 4 + bad + 3);
                        return Err("invalid sequence \"--\" not allowed in comments".into());
                    }
                }
            }
            match event.map_err(xml_error_message)? {
                Event::Start(element) | Event::Empty(element) => {
                    let qualified = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                    let tag = xml_name(&qualified).1.to_owned();
                    let mut attrs = XmlAttributes::default();
                    for attribute in element.attributes().with_checks(false) {
                        if !worker::check(guard) {
                            return Err(String::new());
                        }
                        let attribute = attribute.map_err(xml_attribute_error)?;
                        let value = xml_attribute_guarded(&attribute, reader.decoder(), guard)
                            .map_err(|error| {
                                if let Some(value_at) = (attribute.value.as_ptr() as usize)
                                    .checked_sub(grammar.as_ptr() as usize)
                                    .filter(|offset| *offset <= raw.len())
                                {
                                    error_offset = Some(
                                        value_at
                                            + xml_attribute_failure_offset(
                                                attribute.value.as_ref(),
                                            ),
                                    );
                                }
                                error
                            })?;
                        attrs.insert(
                            xml_name(&String::from_utf8_lossy(attribute.key.as_ref())).1,
                            value,
                        );
                    }
                    let mut owner = attrs.get("name").cloned().unwrap_or_default();
                    if !owner.is_empty() {
                        let category = format!("xml-{tag}");
                        if !worker::charge(guard, owner.len().saturating_add(category.capacity()))
                            || !worker::prepare_push(guard, &mut facts.declarations)
                        {
                            return Err(String::new());
                        }
                        facts.declarations.push(Declaration {
                            name: owner.clone(),
                            category,
                            scope: String::new(),
                            line,
                            end_line: line,
                            signature: String::new(),
                        });
                    } else if let Some(parent) = stack.last() {
                        owner = parent.owner.clone();
                    }
                    if let Some(file) = attrs.get("file").filter(|file| !file.is_empty()) {
                        let kind = match tag.to_ascii_lowercase().as_str() {
                            "script" => "script",
                            "include" => "include",
                            _ => "",
                        };
                        if !kind.is_empty() {
                            if !worker::charge(guard, file.len().saturating_add(kind.len()))
                                || !worker::prepare_push(guard, &mut facts.loads)
                            {
                                return Err(String::new());
                            }
                            facts.loads.push(LoadReference {
                                path: file.clone(),
                                line,
                                kind: kind.into(),
                            });
                        }
                    }
                    for base in attrs
                        .get("inherits")
                        .into_iter()
                        .flat_map(|s| s.split(','))
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        if !worker::charge(
                            guard,
                            owner.len().saturating_add(base.len()).saturating_add(17),
                        ) || !worker::prepare_push(guard, &mut facts.relationships)
                        {
                            return Err(String::new());
                        }
                        facts.relationships.push(Relationship {
                            from: owner.clone(),
                            to: base.into(),
                            category: "xml-inherits".into(),
                            confidence: "exact".into(),
                            line,
                        });
                    }
                    for attr in ["function", "method"] {
                        if let Some(target) = attrs.get(attr).filter(|target| !target.is_empty()) {
                            let category = format!("xml-{attr}");
                            if !worker::charge(
                                guard,
                                owner
                                    .len()
                                    .saturating_add(target.len())
                                    .saturating_add(category.capacity())
                                    .saturating_add(5),
                            ) || !worker::prepare_push(guard, &mut facts.relationships)
                            {
                                return Err(String::new());
                            }
                            facts.relationships.push(Relationship {
                                from: owner.clone(),
                                to: target.clone(),
                                category,
                                confidence: "exact".into(),
                                line,
                            });
                        }
                    }
                    if is_public_handler(&tag) {
                        script.clear();
                        script_line = 0;
                    }
                    if !empty {
                        if !worker::prepare_scratch_push(guard, &mut stack) {
                            return Err(String::new());
                        }
                        let strings = if let Some(guard) = guard {
                            Some(
                                guard
                                    .reserve_scratch(
                                        tag.capacity().saturating_add(owner.capacity()),
                                    )
                                    .ok_or_else(String::new)?,
                            )
                        } else {
                            None
                        };
                        stack.push(XmlFrame {
                            tag,
                            owner,
                            _strings: strings,
                        });
                    }
                }
                Event::End(element) => {
                    let qualified = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                    let tag = xml_name(&qualified).1.to_owned();
                    if is_public_handler(&tag) {
                        let owner = stack.last().map_or("", |frame| frame.owner.as_str());
                        append_inline(&mut facts, &script, script_line, owner, &tag, guard)
                            .ok_or_else(String::new)?;
                    }
                    stack.pop();
                }
                Event::Text(text) => {
                    let text = text.xml10_content().map_err(|error| error.to_string())?;
                    xml_characters_guarded(&text, guard)?;
                    if text.contains("]]>") {
                        return Err("unescaped ]]> not in CDATA section".into());
                    }
                    if stack
                        .last()
                        .is_some_and(|frame| is_public_handler(&frame.tag))
                    {
                        if script_line == 0 {
                            script_line = line;
                        }
                        if !worker::append_scratch(guard, &mut script, &text) {
                            return Err(String::new());
                        }
                    }
                }
                Event::CData(text) => {
                    let text = text.xml10_content().map_err(|error| error.to_string())?;
                    xml_characters_guarded(&text, guard)?;
                    if stack
                        .last()
                        .is_some_and(|frame| is_public_handler(&frame.tag))
                    {
                        if script_line == 0 {
                            script_line = line;
                        }
                        if !worker::append_scratch(guard, &mut script, &text) {
                            return Err(String::new());
                        }
                    }
                }
                Event::GeneralRef(reference) => {
                    let reference = reference.decode().map_err(|error| error.to_string())?;
                    let decoded = xml_entity(&reference)?;
                    if stack
                        .last()
                        .is_some_and(|frame| is_public_handler(&frame.tag))
                    {
                        if script_line == 0 {
                            script_line = line;
                        }
                        if !worker::append_scratch(guard, &mut script, &decoded) {
                            return Err(String::new());
                        }
                    }
                }
                Event::Eof => {
                    if !stack.is_empty() {
                        return Err("unexpected EOF".into());
                    }
                }
                Event::Decl(declaration) => xml_declaration(declaration.as_ref())?,
                Event::DocType(_) | Event::PI(_) | Event::Comment(_) => {}
            }
            Ok(())
        })();
        // Resource/cancellation abandonment is never serialized as an XML
        // diagnostic, even if it happened while validating an earlier token.
        if !worker::check(guard) {
            return None;
        }
        if let Err(message) = result {
            let after = crate::xml_inline::source_map::raw_cursor(
                raw.len(),
                prefix,
                reader.buffer_position(),
            )
            .unwrap_or(raw.len());
            let error_line = line_at(error_offset.unwrap_or(after).min(raw.len()));
            let message = if message.starts_with("xml: ") {
                message
            } else {
                format!("XML syntax error on line {error_line}: {message}")
            };
            if !worker::charge(guard, message.capacity())
                || !worker::prepare_push(guard, &mut facts.diagnostics)
            {
                return None;
            }
            facts.diagnostics.push(SyntaxNote {
                message,
                line: diagnostic_line,
            });
            break;
        }
        if reader.buffer_position() as usize + prefix >= raw.len() {
            if !stack.is_empty() {
                let line = line_at(raw.len());
                let message = format!("XML syntax error on line {line}: unexpected EOF");
                if !worker::charge(guard, message.capacity())
                    || !worker::prepare_push(guard, &mut facts.diagnostics)
                {
                    return None;
                }
                facts.diagnostics.push(SyntaxNote { message, line });
            }
            break;
        }
    }
    worker::check(guard).then_some(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_go_malformed_observations() {
        fixed_go_facts_observations(
            include_str!("../../../tests/rust-parity/source-facts-malformed/cases.json"),
            "LYCHEEDEV_SOURCE_FACTS_GO_REPORT",
        );
    }
    #[test]
    fn fixed_go_xml_diagnostic_observations() {
        fixed_go_facts_observations(
            include_str!("../../../tests/rust-parity/source-facts-xml-diagnostics/cases.json"),
            "LYCHEEDEV_SOURCE_FACTS_XML_GO_REPORT",
        );
    }
    #[test]
    fn fixed_go_generated_observations() {
        fixed_go_facts_observations(
            include_str!("../../../tests/rust-parity/source-facts-generated/cases.json"),
            "LYCHEEDEV_SOURCE_FACTS_GENERATED_GO_REPORT",
        );
    }
    fn fixed_go_facts_observations(fixture: &str, report_variable: &str) {
        use sha2::{Digest, Sha256};
        let input: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let mut observations = Vec::new();
        for case in input["cases"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let raw = case["source"].as_str().unwrap().as_bytes();
            let facts = if path.ends_with(".lua") {
                lua(raw, path.contains("/Blizzard_APIDocumentationGenerated/"))
            } else {
                assert!(path.ends_with(".xml"));
                xml(raw)
            };
            let row =
                serde_json::json!({"id":case["id"], "path":path, "facts":facts, "error":null});
            println!("SOURCE_FACT_ORACLE_JSON {row}");
            observations.push(row);
        }
        if let Some(path) = std::env::var_os(report_variable) {
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                report["schema"],
                "lycheedev.source-facts-malformed-observations.v1"
            );
            assert_eq!(report["oracleCommit"], input["oracleCommit"]);
            assert_eq!(report["goVersion"], "go1.27.1");
            assert_eq!(
                report["casesSHA256"],
                format!("{:x}", Sha256::digest(fixture.as_bytes()))
            );
            assert_eq!(
                report["observations"],
                serde_json::json!(observations),
                "Full public Facts/error differential; no fields or order omitted"
            );
        } else {
            println!(
                "SOURCE_FACT_ORACLE_NOT_COMPARED: no sealed fixed-Go report supplied; observations only"
            );
        }
    }
    #[test]
    fn public_xml_bang_markers_keep_first_consumed_failure() {
        for (raw, expected, consumed) in [
            ("<!-", "unexpected EOF", 3),
            ("<!-x>", "invalid sequence <!- not part of <!--", 4),
            ("<!-\n-->", "invalid sequence <!- not part of <!--", 4),
            ("<![", "unexpected EOF", 3),
            ("<![CDA", "unexpected EOF", 6),
            ("<![CDATx[x]]>", "invalid <![ sequence", 8),
            ("<![CD\nATA[x]]>", "invalid <![ sequence", 6),
        ] {
            assert_eq!(
                public_xml_token_failure(raw.as_bytes()),
                Some((expected.into(), consumed)),
                "{raw:?}"
            );
        }
        for raw in ["<!-- valid -->", "<![CDATA[valid]]>", "<!anything>"] {
            assert_eq!(public_xml_token_failure(raw.as_bytes()), None, "{raw:?}");
        }
    }
    #[test]
    fn public_xml_end_syntax_precedes_mismatch_and_preserves_go_tolerance() {
        for (raw, expected, consumed) in [
            ("</>", "expected element name after </", 2),
            ("</\nUi>", "expected element name after </", 2),
            ("</", "unexpected EOF", 2),
            ("</1Ui", "unexpected EOF", 5),
            ("</1Ui>", "invalid XML name: 1Ui", 5),
            ("</-Ui>", "invalid XML name: -Ui", 5),
            ("</a:b:c>", "expected element name after </", 7),
            ("</Ui junk>", "invalid characters between </Ui and >", 6),
            ("</x:Ui/>", "invalid characters between </Ui and >", 7),
            ("</Ui \n", "unexpected EOF", 6),
        ] {
            assert_eq!(
                public_xml_token_failure(raw.as_bytes()),
                Some((expected.into(), consumed)),
                "{raw:?}"
            );
        }
        for raw in ["</Ui>", "</Ui \n >", "</:Ui>", "</Ui:>", "</x:Ui>"] {
            assert_eq!(public_xml_token_failure(raw.as_bytes()), None, "{raw:?}");
        }
        // A different native XML-name profile must not get substituted here.
        assert_eq!(public_xml_token_failure("</界 junk>".as_bytes()), None);
    }
    #[test]
    fn public_xml_lexical_failures_keep_prior_facts_and_distinct_diagnostic_lines() {
        for (suffix, message, message_line) in [
            ("<!-\nx>", "invalid sequence <!- not part of <!--", 4),
            ("<![CD\nATA[x]]>", "invalid <![ sequence", 4),
            ("</Ui\n junk>", "invalid characters between </Ui and >", 4),
            ("</\nUi>", "expected element name after </", 3),
        ] {
            let raw = format!(
                "<Ui>\n<Frame name=\"Before\" inherits=\"Base\"/><Include file=\"Earlier.xml\"/>\n{suffix}\n<Frame name=\"After\"/>"
            );
            let actual = serde_json::to_value(xml(raw.as_bytes())).unwrap();
            assert_eq!(
                actual,
                serde_json::json!({
                    "declarations":[{"name":"Before","category":"xml-Frame","line":2,"endLine":2}],
                    "relationships":[{"from":"Before","to":"Base","category":"xml-inherits","confidence":"exact","line":2}],
                    "loads":[{"path":"Earlier.xml","line":2,"kind":"include"}],
                    "headers":[],
                    "diagnostics":[{"message":format!("XML syntax error on line {message_line}: {message}"),"line":3}]
                }),
                "{raw:?}"
            );
        }
    }
    #[test]
    fn public_xml_declaration_errors_are_plain_and_keep_token_start_line() {
        for (raw, expected, line) in [
            (
                "<?xml version=\"1.1\"?><Root/>",
                "xml: unsupported version \"1.1\"; only version 1.0 is supported",
                1,
            ),
            (
                "<Root/>\n<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>",
                "xml: encoding \"ISO-8859-1\" declared but Decoder.CharsetReader is nil",
                2,
            ),
        ] {
            assert_eq!(
                serde_json::to_value(xml(raw.as_bytes()).diagnostics).unwrap(),
                serde_json::json!([{"message":expected,"line":line}])
            );
        }
        for raw in [
            "<?xml encoding=\"Utf-8\"?><Root/>",
            "<?xml version = \"1.1\"?><Root/>",
        ] {
            assert!(xml(raw.as_bytes()).diagnostics.is_empty(), "{raw}");
        }
    }
    #[test]
    fn public_xml_entity_and_comment_errors_keep_consumed_position() {
        for (raw, message, line) in [
            (
                "<Ui>\ntext &unknown\n</Ui>",
                "XML syntax error on line 2: invalid character entity &unknown (no semicolon)",
                1,
            ),
            (
                "<Ui>&unknown",
                "XML syntax error on line 1: unexpected EOF",
                1,
            ),
            (
                "<Ui>&#xZ;</Ui>",
                "XML syntax error on line 1: invalid character entity &#x (no semicolon)",
                1,
            ),
            (
                "<Ui><Frame name=\"&bad\"\n/></Ui>",
                "XML syntax error on line 1: invalid character entity &bad (no semicolon)",
                1,
            ),
            (
                "<Ui><!--bad--x\nend--></Ui>",
                "XML syntax error on line 1: invalid sequence \"--\" not allowed in comments",
                1,
            ),
            (
                "<Ui>&#0;&unknown;</Ui>",
                "XML syntax error on line 1: invalid character entity &unknown;",
                1,
            ),
        ] {
            assert_eq!(
                serde_json::to_value(xml(raw.as_bytes()).diagnostics).unwrap(),
                serde_json::json!([{"message":message,"line":line}]),
                "{raw}"
            );
        }
    }
    #[test]
    fn public_xml_namespace_prefixes_and_directives_follow_go_tokens() {
        let mismatched =
            xml(br#"<Ui><a:Frame xmlns:a="urn:x" xmlns:b="urn:x" name="F"></b:Frame></Ui>"#);
        assert_eq!(mismatched.declarations[0].name, "F");
        assert_eq!(
            serde_json::to_value(mismatched.diagnostics).unwrap(),
            serde_json::json!([{"message":"XML syntax error on line 1: element <Frame> in space a closed by </Frame> in space b", "line":1}])
        );
        let undefined = xml(br#"<Ui><x:Frame name="F"/><:Frame name="G"/></Ui>"#);
        assert!(undefined.diagnostics.is_empty());
        assert_eq!(undefined.declarations[0].category, "xml-Frame");
        assert_eq!(undefined.declarations[1].category, "xml-:Frame");
        let raw =
            br#"<!DOCTYPE Ui [<!ENTITY ignored "a>b">]><Ui><OnLoad>R<!anything>un()</OnLoad></Ui>"#;
        let facts = xml(raw);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(facts.relationships[0].to, "Run");
        let not_expanded = xml(br#"<!ENTITY ignored "text"><Ui>&ignored;</Ui>"#);
        assert_eq!(
            not_expanded.diagnostics[0].message,
            "XML syntax error on line 1: invalid character entity &ignored;"
        );
    }
    #[test]
    fn never_evaluates_and_keeps_scopes() {
        let f=lua(b"-- function fake() end\nlocal function one()\n frame:RegisterEvent(\"PLAYER_LOGIN\")\n C_Test.Do()\nend\nlocal two = function() one() end\n",false);
        assert!(f.diagnostics.is_empty());
        assert_eq!(f.declarations.len(), 2);
        let round_trip: Facts = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(round_trip.declarations.len(), 2);
        assert_eq!(f.declarations[0].name, "one");
        assert!(
            f.relationships
                .iter()
                .any(|r| r.from == "one" && r.to == "PLAYER_LOGIN" && r.confidence == "exact")
        );
        assert!(
            f.relationships
                .iter()
                .any(|r| r.from == "two" && r.to == "one")
        );
    }
    #[test]
    fn fresh_literal_targets_keep_raw_identity_without_changing_public_json() {
        let raw = r#"C_X["\255"](); frame:RegisterEvent("\254"); C_X["�"]()"#.as_bytes();
        let fresh = lua(raw, false);
        assert!(fresh.diagnostics.is_empty());
        assert_eq!(fresh.relationship_target_bytes(0).unwrap(), b"C_X.\xff");
        assert_eq!(
            fresh.relationship_target_bytes(1).unwrap(),
            b"frame:RegisterEvent"
        );
        assert_eq!(fresh.relationship_target_bytes(2).unwrap(), b"\xfe");
        assert_eq!(
            fresh.relationship_target_bytes(3).unwrap(),
            "C_X.�".as_bytes()
        );
        assert_eq!(fresh.raw_relationship_targets.as_ref().unwrap().len(), 2);
        let public = serde_json::to_value(&fresh).unwrap();
        assert_eq!(public.as_object().unwrap().len(), 5);
        assert_eq!(
            public["relationships"][0]["to"],
            public["relationships"][3]["to"]
        );
        let cached: Facts = serde_json::from_value(public.clone()).unwrap();
        assert!(cached.raw_relationship_targets.is_none());
        assert!(cached.relationship_target_bytes(0).is_none());
        assert!(cached.relationship_target_bytes(1).is_none());
        assert_eq!(serde_json::to_value(&cached).unwrap(), public);
        let reparsed = lua(raw, false);
        assert_eq!(
            reparsed.relationship_target_bytes(0),
            fresh.relationship_target_bytes(0)
        );
        assert_eq!(serde_json::to_value(&reparsed).unwrap(), public);
    }
    #[test]
    fn inline_raw_targets_follow_appended_relationship_ordinals() {
        let raw = br#"<Ui><Frame name="F" inherits="T"><Scripts><OnLoad>C_X["\255"](); frame:RegisterEvent("\254")</OnLoad></Scripts></Frame></Ui>"#;
        let facts = xml(raw);
        assert!(facts.diagnostics.is_empty());
        assert_eq!(facts.relationship_target_bytes(0).unwrap(), b"T");
        assert_eq!(facts.relationship_target_bytes(1).unwrap(), b"C_X.\xff");
        assert_eq!(
            facts.relationship_target_bytes(2).unwrap(),
            b"frame:RegisterEvent"
        );
        assert_eq!(facts.relationship_target_bytes(3).unwrap(), b"\xfe");
        assert_eq!(
            facts
                .raw_relationship_targets
                .as_ref()
                .unwrap()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [1, 3]
        );
    }
    #[test]
    fn rejected_program_clears_raw_targets_but_empty_fresh_identity_is_known() {
        let facts = lua(br#"C_X["\255"](); function Broken("#, false);
        assert!(!facts.diagnostics.is_empty());
        assert!(facts.relationships.is_empty());
        assert!(facts.raw_relationship_targets.as_ref().unwrap().is_empty());
        assert!(
            lua(b"", false)
                .raw_relationship_targets
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(
            xml(b"<Root/>")
                .raw_relationship_targets
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(
            toc(b"## Interface: 120100\n")
                .raw_relationship_targets
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(Facts::default().raw_relationship_targets.is_none());
    }
    #[test]
    fn descriptors_are_literal_metadata() {
        let f=lua(br#"local api={Name="Test", Type="System", Namespace="C_Test", Functions={{Name="Do",Type="Function",Arguments={{Name="id",Type="number",Nilable=true}},Returns={{Name="ok",Type="bool"}}}}}"#,true);
        assert!(f.diagnostics.is_empty());
        let d = f
            .declarations
            .iter()
            .find(|d| d.name == "C_Test.Do")
            .unwrap();
        assert_eq!(d.signature, "C_Test.Do(id?: number) -> ok: bool");
        assert_eq!(f.declarations.len(), 2);
    }
    #[test]
    fn malformed_never_emits_declarations() {
        let f = lua(b"function X( broken", false);
        assert!(!f.diagnostics.is_empty());
        assert!(f.declarations.is_empty());
    }
    #[test]
    fn xml_structural_rows_use_original_tags_after_bom_and_whitespace() {
        for bom in ["", "\u{feff}"] {
            for newline in ["\n", "\r\n"] {
                let text = format!(
                    "{bom}<Ui>{newline}  <Frame name=\"First\"/>{newline}  <Frame name=\"Second\"><Scripts>{newline}    <OnShow method=\"Run\"/>{newline}    <OnHide function=\"Stop\"/>{newline}  </Scripts></Frame>{newline}</Ui>"
                );
                let facts = xml(text.as_bytes());
                assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
                assert_eq!(
                    facts
                        .declarations
                        .iter()
                        .map(|declaration| (declaration.name.as_str(), declaration.line))
                        .collect::<Vec<_>>(),
                    [("First", 2), ("Second", 3)]
                );
                assert!(
                    facts
                        .relationships
                        .iter()
                        .any(|relation| relation.from == "Second"
                            && relation.to == "Run"
                            && relation.line == 4)
                );
                assert!(
                    facts
                        .relationships
                        .iter()
                        .any(|relation| relation.from == "Second"
                            && relation.to == "Stop"
                            && relation.line == 5)
                );
                let program =
                    crate::xml_program::extract_production("Frames.xml", text.as_bytes()).unwrap();
                for node in program
                    .occurrences
                    .iter()
                    .filter(|node| node.tag == "Frame")
                {
                    assert_eq!(
                        &text.as_bytes()[node.raw.start..node.raw.start + 6],
                        b"<Frame"
                    );
                    let declaration = facts
                        .declarations
                        .iter()
                        .find(|declaration| declaration.name == node.attributes["name"].value)
                        .unwrap();
                    assert_eq!(
                        declaration.line,
                        1 + text.as_bytes()[..node.raw.start]
                            .iter()
                            .filter(|byte| **byte == b'\n')
                            .count()
                    );
                }
            }
        }
    }
    #[test]
    fn public_inline_lines_follow_go_decoded_lines_native_keeps_source_map() {
        let raw = "\u{feff}<Ui>\r\n<Frame name=\"Demo\"><Scripts><OnLoad>&#10;local function Inline()&#10; return 1&#10;end&#10;Inline()</OnLoad></Scripts></Frame>\r\n<Script>&#10;function Loaded()&#10; Inline()&#10;end</Script>\r\n</Ui>";
        let facts = xml(raw.as_bytes());
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        let local = facts
            .declarations
            .iter()
            .find(|declaration| declaration.name == "Inline")
            .unwrap();
        assert_eq!((local.line, local.end_line), (3, 5));
        assert!(
            !facts
                .declarations
                .iter()
                .any(|declaration| declaration.name == "Loaded")
        );
        assert_eq!(facts.relationships.len(), 1);
        assert_eq!(
            (
                facts.relationships[0].from.as_str(),
                facts.relationships[0].to.as_str(),
                facts.relationships[0].line
            ),
            ("Demo:OnLoad", "Inline", 6)
        );
        let scripts = crate::xml_inline::extract("Native.xml", raw.as_bytes()).unwrap();
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].original_body_line(2).unwrap(), 2);
        assert_eq!(scripts[1].original_body_line(2).unwrap(), 3);
    }
    #[test]
    fn xml_loads_and_inline_scope() {
        let f=xml(br#"<Ui><Frame name="Demo" inherits="A, B"><Scripts><OnLoad>self:RegisterEvent("PLAYER_LOGIN")</OnLoad></Scripts></Frame><Script FiLe="Demo.lua"/></Ui>"#);
        assert!(f.diagnostics.is_empty());
        assert_eq!(f.loads[0].path, "Demo.lua");
        assert!(
            f.relationships
                .iter()
                .any(|r| r.from == "Demo:OnLoad" && r.to == "PLAYER_LOGIN")
        );
    }
    #[test]
    fn xml_linear_lines_match_legacy_prefix_all_fact_fields() {
        let documents = [
            "<Ui>\n <Frame name=\"Demo\" inherits=\"A, B\"><Scripts>\n<OnLoad>\nlocal function F() return 1 end\nF()\nself:RegisterEvent(&quot;READY&quot;)\n</OnLoad>\n<OnShow method=\"Show\"/><OnHide function=\"Hide\"/>\n</Scripts></Frame>\n<Script FiLe=\"Other.lua\"/><Include file=\"Other.xml\"/>\n</Ui>",
            "<?xml version=\"1.0\"?>\n<!-- note\n -->\n<Ui><Frame name=\"Entity\"><Scripts><OnLoad>&#10;local x=1&#10;<![CDATA[x = x + 1]]></OnLoad></Scripts></Frame></Ui>",
            "<Ui>\n<Frame name=\"Bad &unknown;\"/>\n</Ui>",
            "<Ui>\n<Frame name=\"First\" name=\"Again\"/>\n</Ui>",
            "<Ui>\n<Frame name=\"Unclosed\">\n</Ui>",
            "<Ui>\n<Frame name=\"Truncated\"",
            "<Ui>\n<Frame name=\"Before\"/>\n<Broken attr=no_quote/>\n</Ui>",
            "<Ui>\n<Frame name=\"Unicode界🙂\"/>\n</Ui>",
            "",
        ];
        for bom in ["", "\u{feff}"] {
            for newline in ["\n", "\r\n", "\r"] {
                for document in documents {
                    let text = format!("{bom}{}", document.replace('\n', newline));
                    let raw = text.as_bytes();
                    let mut lines = XmlLines::new(raw);
                    let actual = xml_with_line_counter(raw, |at| lines.at(at));
                    let expected = xml_with_line_counter(raw, |at| {
                        1 + raw[..at].iter().filter(|byte| **byte == b'\n').count()
                    });
                    assert_eq!(
                        serde_json::to_value(actual).unwrap(),
                        serde_json::to_value(expected).unwrap(),
                        "{text:?}"
                    );
                    assert!(
                        lines.scanned <= raw.len(),
                        "ordered events rescanned source: {text:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn xml_line_cursor_preserves_repeated_and_backwards_prefix_queries() {
        let raw = b"\xef\xbb\xbf<Ui>\r\n\n<Frame/>\r</Ui>";
        let mut lines = XmlLines::new(raw);
        for at in 0..=raw.len() {
            let expected = 1 + raw[..at].iter().filter(|byte| **byte == b'\n').count();
            assert_eq!(lines.at(at), expected);
            assert_eq!(lines.at(at), expected);
        }
        assert_eq!(lines.scanned, raw.len());
        for at in (0..=raw.len()).rev() {
            assert_eq!(
                lines.at(at),
                1 + raw[..at].iter().filter(|byte| **byte == b'\n').count()
            );
        }
    }
    #[test]
    fn public_xml_does_not_require_native_profile_or_catalog() {
        let raw = br#"<?xml version="1.0"?><!DOCTYPE Root><?tool test?><Root xmlns:x="urn:test"><x:Frame name="Demo"><OnLoad>Run()</OnLoad></x:Frame></Root>"#;
        let before = crate::xml_program::catalog_verifications();
        let facts = xml(raw);
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(facts.declarations[0].category, "xml-Frame");
        assert_eq!(facts.relationships[0].from, "Demo:OnLoad");
        assert_eq!(facts.relationships[0].to, "Run");
        assert!(crate::xml_program::extract_production("Native.xml", raw).is_err());
    }
    #[test]
    fn public_xml_inline_facts_are_interleaved_and_empty_attributes_ignored() {
        let raw = br#"<Ui>
<Frame name="First"><Scripts><OnLoad>function Inline() end
Run()</OnLoad><PreClick>Ignored()</PreClick><OnShow function="" method="Shown"/></Scripts></Frame>
<Frame name="Last" inherits="Base"/><Script file=""/><Include file="next.xml"/>
<Script>IgnoredTopLevel()</Script><Binding>IgnoredBinding()</Binding>
</Ui>"#;
        let facts = xml(raw);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|d| (d.name.as_str(), d.line, d.end_line))
                .collect::<Vec<_>>(),
            [("First", 2, 2), ("Inline", 2, 2), ("Last", 4, 4)]
        );
        assert_eq!(
            serde_json::to_value(&facts.relationships).unwrap(),
            serde_json::json!([
                {"from":"First:OnLoad","to":"Run","category":"call","confidence":"inferred","line":3},
                {"from":"First","to":"Shown","category":"xml-method","confidence":"exact","line":3},
                {"from":"Last","to":"Base","category":"xml-inherits","confidence":"exact","line":4}
            ])
        );
        assert_eq!(
            serde_json::to_value(&facts.loads).unwrap(),
            serde_json::json!([{"path":"next.xml","line":4,"kind":"include"}])
        );
    }
    #[test]
    fn public_xml_character_data_concatenates_entities_cdata_and_comment_gaps() {
        let facts=xml(br#"<Ui><Frame name="Demo"><OnLoad>R<![CDATA[u]]><!-- omitted -->n(); frame:RegisterEvent(&quot;READY&quot;)</OnLoad></Frame></Ui>"#);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(
            facts
                .relationships
                .iter()
                .map(|r| r.to.as_str())
                .collect::<Vec<_>>(),
            ["Run", "frame:RegisterEvent", "READY"]
        );
        assert!(xml(b"").diagnostics.is_empty());
        assert!(xml(b"<Root/> <Another/>").diagnostics.is_empty());
        assert!(!xml(b"<Ui><Frame>").diagnostics.is_empty());
        assert!(!xml(b"<Ui>&unknown;</Ui>").diagnostics.is_empty());
    }

    #[test]
    fn public_xml_attribute_newlines_are_normalized_before_entities() {
        let facts = xml(b"<Ui><Frame name=\"A\r\nB\tC&#13;D&#10;E\"/></Ui>");
        assert!(facts.diagnostics.is_empty());
        assert_eq!(facts.declarations[0].name, "A\nB\tC\rD\nE");
        assert!(
            !xml(b"<Ui><Frame name=\"&#0;\"/></Ui>")
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn public_xml_error_keeps_go_prior_rows_and_token_diagnostic_position() {
        let facts = xml(b"<Ui>\n<Frame name=\"Before\">\n</Ui>");
        assert_eq!(facts.declarations.len(), 1);
        assert_eq!(
            serde_json::to_value(&facts.diagnostics).unwrap(),
            serde_json::json!([{"message":"XML syntax error on line 3: element <Frame> closed by </Ui>","line":3}])
        );
        let eof = xml(b"<Ui>\n<Frame>");
        assert_eq!(
            serde_json::to_value(eof.diagnostics).unwrap(),
            serde_json::json!([{"message":"XML syntax error on line 2: unexpected EOF","line":2}])
        );
    }
}

#[cfg(test)]
#[path = "facts/worker_tests.rs"]
mod worker_tests;
