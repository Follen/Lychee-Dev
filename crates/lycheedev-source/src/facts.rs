// SPDX-License-Identifier: MIT
// Source fact contracts ported from the MIT wowdoc-derived Lychee Dev scanner.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from: String,
    pub to: String,
    pub category: String,
    pub confidence: String,
    pub line: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadReference {
    pub path: String,
    pub line: usize,
    pub kind: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderValue {
    pub key: String,
    pub value: String,
    pub line: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxNote {
    pub message: String,
    pub line: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Facts {
    pub declarations: Vec<Declaration>,
    pub relationships: Vec<Relationship>,
    pub loads: Vec<LoadReference>,
    pub headers: Vec<HeaderValue>,
    pub diagnostics: Vec<SyntaxNote>,
}
pub fn lua(raw: &[u8], generated: bool) -> Facts {
    match crate::lua_tree::SyntaxDocument::new_public(raw) {
        Ok(d) => d.facts(generated),
        Err(e) => {
            let mut f = Facts::default();
            f.diagnostics.push(SyntaxNote {
                message: e.to_string(),
                line: 0,
            });
            f
        }
    }
}
pub fn toc(raw: &[u8]) -> Facts {
    let mut f = Facts::default();
    let text = String::from_utf8_lossy(raw.strip_prefix(&[239, 187, 191]).unwrap_or(raw));
    for (i, line) in text.split('\n').enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix("##") {
            if let Some((key, value)) = header.trim().split_once(':') {
                f.headers.push(HeaderValue {
                    key: key.trim().into(),
                    value: value.trim().into(),
                    line: i + 1,
                });
            }
        } else if !line.starts_with('#') {
            f.loads.push(LoadReference {
                path: line.into(),
                line: i + 1,
                kind: "toc".into(),
            });
        }
    }
    f
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
    let decoded = decoder
        .decode(attribute.value.as_ref())
        .map_err(|_| "invalid UTF-8".to_owned())?;
    if decoded.contains('<') {
        return Err("unescaped < inside quoted string".into());
    }
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let mut value = String::new();
    let mut rest = normalized.as_str();
    while let Some(at) = rest.find('&') {
        value.push_str(&rest[..at]);
        let entity = &rest[at..];
        let end = entity
            .find(';')
            .ok_or_else(|| format!("invalid character entity {entity} (no semicolon)"))?;
        value.push_str(&xml_entity(&entity[1..end])?);
        rest = &entity[end + 1..];
    }
    value.push_str(rest);
    xml_characters(&value)?;
    Ok(value)
}
pub(crate) fn xml_entity(reference: &str) -> std::result::Result<String, String> {
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
            if number.is_empty() || !number.chars().all(|ch| ch.is_digit(radix)) {
                return None;
            }
            u32::from_str_radix(number, radix)
                .ok()
                .filter(|number| *number <= 0x10ffff)
                .map(|number| char::from_u32(number).unwrap_or('\u{fffd}'))
        }),
    }
    .ok_or_else(|| format!("invalid character entity &{reference};"))?;
    let value = value.to_string();
    xml_characters(&value)?;
    Ok(value)
}
pub(crate) fn xml_characters(text: &str) -> std::result::Result<(), String> {
    for ch in text.chars() {
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
fn append_inline(facts: &mut Facts, script: &str, script_line: usize, owner: &str, tag: &str) {
    if script.trim().is_empty() {
        return;
    }
    let mut inner = lua(script.as_bytes(), false);
    let offset = script_line.saturating_sub(1);
    for declaration in &mut inner.declarations {
        declaration.line += offset;
        // Go adds the offset even to the zero LastLine of assignments.
        declaration.end_line += offset;
    }
    for relation in &mut inner.relationships {
        relation.line += offset;
        if relation.from == "<file>" {
            relation.from = format!("{owner}:{tag}");
        }
    }
    for diagnostic in &mut inner.diagnostics {
        diagnostic.line = if diagnostic.line > 0 {
            diagnostic.line + offset
        } else {
            script_line
        };
    }
    facts.declarations.extend(inner.declarations);
    facts.relationships.extend(inner.relationships);
    facts.diagnostics.extend(inner.diagnostics);
}
fn xml_error_message(error: quick_xml::Error) -> String {
    use quick_xml::errors::{IllFormedError, SyntaxError};
    match error {
        quick_xml::Error::Syntax(SyntaxError::UnclosedCData) => {
            "unexpected EOF in CDATA section".into()
        }
        quick_xml::Error::Syntax(_) => "unexpected EOF".into(),
        quick_xml::Error::IllFormed(IllFormedError::MismatchedEndTag { expected, found }) => {
            format!(
                "element <{}> closed by </{}>",
                expected.rsplit(':').next().unwrap_or(&expected),
                found.rsplit(':').next().unwrap_or(&found)
            )
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
fn xml_with_line_counter(raw: &[u8], mut line_at: impl FnMut(usize) -> usize) -> Facts {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_reader(raw);
    reader.config_mut().check_comments = true;
    let mut facts = Facts::default();
    let mut stack = Vec::<(String, String)>::new();
    let mut script = String::new();
    let mut script_line = 0;
    let mut character_data_line = None;
    let prefix = if raw.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    loop {
        let at =
            crate::xml_inline::source_map::raw_cursor(raw.len(), prefix, reader.buffer_position())
                .expect("slice reader original cursor");
        let line = line_at(at.min(raw.len()));
        let event = reader.read_event();
        let empty = matches!(&event, Ok(Event::Empty(_)));
        // quick-xml exposes references separately; Go returns one CharData
        // token for the entire adjacent text/reference sequence.
        let diagnostic_line = if matches!(&event, Ok(Event::Text(_) | Event::GeneralRef(_))) {
            *character_data_line.get_or_insert(line)
        } else {
            character_data_line = None;
            line
        };
        let result: std::result::Result<(), String> = (|| {
            match event.map_err(xml_error_message)? {
                Event::Start(element) | Event::Empty(element) => {
                    let tag = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                    let mut attrs = BTreeMap::new();
                    for attribute in element.attributes().with_checks(false) {
                        let attribute = attribute.map_err(xml_attribute_error)?;
                        let value = xml_attribute(&attribute, reader.decoder())?;
                        attrs.insert(
                            String::from_utf8_lossy(attribute.key.local_name().as_ref())
                                .to_ascii_lowercase(),
                            value,
                        );
                    }
                    let mut owner = attrs.get("name").cloned().unwrap_or_default();
                    if !owner.is_empty() {
                        facts.declarations.push(Declaration {
                            name: owner.clone(),
                            category: format!("xml-{tag}"),
                            scope: String::new(),
                            line,
                            end_line: line,
                            signature: String::new(),
                        });
                    } else if let Some((_, parent)) = stack.last() {
                        owner = parent.clone();
                    }
                    if let Some(file) = attrs.get("file").filter(|file| !file.is_empty()) {
                        let kind = match tag.to_ascii_lowercase().as_str() {
                            "script" => "script",
                            "include" => "include",
                            _ => "",
                        };
                        if !kind.is_empty() {
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
                            facts.relationships.push(Relationship {
                                from: owner.clone(),
                                to: target.clone(),
                                category: format!("xml-{attr}"),
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
                        stack.push((tag, owner));
                    }
                }
                Event::End(element) => {
                    let tag = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                    if is_public_handler(&tag) {
                        let owner = stack.last().map_or("", |(_, owner)| owner.as_str());
                        append_inline(&mut facts, &script, script_line, owner, &tag);
                    }
                    stack.pop();
                }
                Event::Text(text) => {
                    let text = text.xml10_content().map_err(|error| error.to_string())?;
                    xml_characters(&text)?;
                    if text.contains("]]>") {
                        return Err("unescaped ]]> not in CDATA section".into());
                    }
                    if stack.last().is_some_and(|(tag, _)| is_public_handler(tag)) {
                        if script_line == 0 {
                            script_line = line;
                        }
                        script.push_str(&text);
                    }
                }
                Event::CData(text) => {
                    let text = text.xml10_content().map_err(|error| error.to_string())?;
                    xml_characters(&text)?;
                    if stack.last().is_some_and(|(tag, _)| is_public_handler(tag)) {
                        if script_line == 0 {
                            script_line = line;
                        }
                        script.push_str(&text);
                    }
                }
                Event::GeneralRef(reference) => {
                    let reference = reference.decode().map_err(|error| error.to_string())?;
                    let decoded = xml_entity(&reference)?;
                    if stack.last().is_some_and(|(tag, _)| is_public_handler(tag)) {
                        if script_line == 0 {
                            script_line = line;
                        }
                        script.push_str(&decoded);
                    }
                }
                Event::Eof => {
                    if !stack.is_empty() {
                        return Err("unexpected EOF".into());
                    }
                }
                // Go accepts XML declarations, directives, processing
                // instructions and comments without imposing a UI schema.
                Event::Decl(_) | Event::DocType(_) | Event::PI(_) | Event::Comment(_) => {}
            }
            Ok(())
        })();
        if let Err(message) = result {
            let after = crate::xml_inline::source_map::raw_cursor(
                raw.len(),
                prefix,
                reader.buffer_position(),
            )
            .unwrap_or(raw.len());
            let error_line = line_at(after.min(raw.len()));
            facts.diagnostics.push(SyntaxNote {
                message: format!("XML syntax error on line {error_line}: {message}"),
                line: diagnostic_line,
            });
            break;
        }
        if reader.buffer_position() as usize + prefix >= raw.len() {
            if !stack.is_empty() {
                let line = line_at(raw.len());
                facts.diagnostics.push(SyntaxNote {
                    message: format!("XML syntax error on line {line}: unexpected EOF"),
                    line,
                });
            }
            break;
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
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
