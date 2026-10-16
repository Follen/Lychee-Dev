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
    match crate::lua_tree::SyntaxDocument::new(raw) {
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

/// One index invocation may reuse the immutable embedded catalog, including an
/// authentication failure. Each document still runs its complete extraction and
/// produces its own original diagnostic; a cache hit never initializes this.
#[derive(Default)]
pub(crate) struct XmlFactsContext {
    catalog: Option<std::result::Result<crate::xml_program::Catalog, String>>,
}
impl XmlFactsContext {
    fn extract(&mut self, raw: &[u8]) -> std::result::Result<crate::xml_program::XmlProgram, String> {
        match self.catalog.get_or_insert_with(crate::xml_program::catalog) {
            Ok(catalog) => crate::xml_program::extract_production_with_catalog("@index.xml", raw, catalog),
            Err(error) => Err(error.clone()),
        }
    }
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
        Self { raw, cursor: 0, line: 1, #[cfg(test)] scanned: 0 }
    }
    fn at(&mut self, offset: usize) -> usize {
        if offset < self.cursor {
            #[cfg(test)]
            { self.scanned += offset; }
            return 1 + self.raw[..offset].iter().filter(|byte| **byte == b'\n').count();
        }
        #[cfg(test)]
        { self.scanned += offset - self.cursor; }
        self.line += self.raw[self.cursor..offset].iter().filter(|byte| **byte == b'\n').count();
        self.cursor = offset;
        self.line
    }
}

pub fn xml(raw: &[u8]) -> Facts {
    xml_with_context(raw, &mut XmlFactsContext::default())
}
pub(crate) fn xml_with_context(raw: &[u8], context: &mut XmlFactsContext) -> Facts {
    let mut lines = XmlLines::new(raw);
    xml_with_line_counter(raw, context, |offset| lines.at(offset))
}
fn xml_with_line_counter(
    raw: &[u8],
    context: &mut XmlFactsContext,
    mut line_at: impl FnMut(usize) -> usize,
) -> Facts {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_reader(raw);
    let mut f = Facts::default();
    let mut stack = Vec::<(String, String)>::new();
    let prefix=if raw.starts_with(b"\xef\xbb\xbf"){3}else{0};
    loop {
        let at = crate::xml_inline::source_map::raw_cursor(raw.len(),prefix,reader.buffer_position()).expect("slice reader original cursor");
        let n = line_at(at.min(raw.len()));
        let event=reader.read_event();
        let empty=matches!(&event,Ok(Event::Empty(_)));
        match event {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag_start=crate::xml_inline::source_map::borrowed_span(raw,e.as_ref()).expect("slice reader original element bytes").start-1;
                let n=line_at(tag_start);
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let mut attrs = BTreeMap::new();
                let mut bad = false;
                for a in e.attributes() {
                    match a {
                        Ok(a) => match a.decode_and_unescape_value(reader.decoder()) {
                            Ok(v) => {
                                attrs.insert(
                                    String::from_utf8_lossy(a.key.local_name().as_ref())
                                        .to_ascii_lowercase(),
                                    v.into_owned(),
                                );
                            }
                            Err(error) => {
                                f.diagnostics.push(SyntaxNote {
                                    message: error.to_string(),
                                    line: n,
                                });
                                bad = true;
                            }
                        },
                        Err(error) => {
                            f.diagnostics.push(SyntaxNote {
                                message: error.to_string(),
                                line: n,
                            });
                            bad = true;
                        }
                    }
                }
                if bad {
                    break;
                }
                let mut owner = attrs.get("name").cloned().unwrap_or_default();
                if !owner.is_empty() {
                    f.declarations.push(Declaration {
                        name: owner.clone(),
                        category: format!("xml-{tag}"),
                        scope: String::new(),
                        line: n,
                        end_line: n,
                        signature: String::new(),
                    });
                } else if let Some((_, parent)) = stack.last() {
                    owner = parent.clone();
                }
                if let Some(file) = attrs.get("file") {
                    let kind = match tag.to_ascii_lowercase().as_str() {
                        "script" => "script",
                        "include" => "include",
                        _ => "",
                    };
                    if !kind.is_empty() {
                        f.loads.push(LoadReference {
                            path: file.clone(),
                            line: n,
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
                    f.relationships.push(Relationship {
                        from: owner.clone(),
                        to: base.into(),
                        category: "xml-inherits".into(),
                        confidence: "exact".into(),
                        line: n,
                    });
                }
                for attr in ["function", "method"] {
                    if let Some(target) = attrs.get(attr) {
                        f.relationships.push(Relationship {
                            from: owner.clone(),
                            to: target.clone(),
                            category: format!("xml-{attr}"),
                            confidence: "exact".into(),
                            line: n,
                        });
                    }
                }
                if !empty {
                    stack.push((tag, owner));
                }
            }
            Ok(Event::End(_)) => { stack.pop(); }
            Ok(Event::Eof) => break,
            Err(e) => {
                f.diagnostics.push(SyntaxNote {
                    message: e.to_string(),
                    line: n,
                });
                break;
            }
            _ => {}
        }
    }
    // Structural XML attributes above remain candidates. Inline source facts
    // use the production decoder and its original-byte source map.
    let mapped=(||->lycheedev_core::Result<()> {
        let program=context.extract(raw).map_err(|error|lycheedev_core::ToolkitError::new("codebase.xml_invalid",error,4))?;
        let scripts=crate::xml_inline::scripts(&program,raw)?;
        for (ordinal,script) in scripts.iter().enumerate(){
            let mut owner=String::new();
            let mut node=program.occurrences.iter().find(|node|node.script==Some(ordinal));
            while let Some(current)=node{
                if let Some(name)=current.attributes.get("name"){owner=name.value.clone();break;}
                node=current.parent.map(|parent|&program.occurrences[parent]);
            }
            let mut inner=lua(script.body_text().as_bytes(),false);
            for declaration in &mut inner.declarations{declaration.line=script.original_body_line(declaration.line)?;declaration.end_line=script.original_body_line(declaration.end_line)?;}
            for relation in &mut inner.relationships{
                relation.line=script.original_body_line(relation.line)?;
                if relation.from=="<file>"{relation.from=format!("{owner}:{}",script.handler);}
            }
            for diagnostic in &mut inner.diagnostics{if diagnostic.line>0{diagnostic.line=script.original_body_line(diagnostic.line)?;}}
            f.declarations.extend(inner.declarations);f.relationships.extend(inner.relationships);f.diagnostics.extend(inner.diagnostics);
        }
        Ok(())
    })();
    if let Err(error)=mapped{f.diagnostics.push(SyntaxNote{message:error.message,line:0});}
    f
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
    fn xml_structural_rows_use_original_tags_after_bom_and_whitespace(){
        for bom in ["","\u{feff}"]{for newline in ["\n","\r\n"]{
            let text=format!("{bom}<Ui>{newline}  <Frame name=\"First\"/>{newline}  <Frame name=\"Second\"><Scripts>{newline}    <OnShow method=\"Run\"/>{newline}    <OnHide function=\"Stop\"/>{newline}  </Scripts></Frame>{newline}</Ui>");
            let facts=xml(text.as_bytes());assert!(facts.diagnostics.is_empty(),"{:?}",facts.diagnostics);
            assert_eq!(facts.declarations.iter().map(|declaration|(declaration.name.as_str(),declaration.line)).collect::<Vec<_>>(),[("First",2),("Second",3)]);
            assert!(facts.relationships.iter().any(|relation|relation.from=="Second"&&relation.to=="Run"&&relation.line==4));
            assert!(facts.relationships.iter().any(|relation|relation.from=="Second"&&relation.to=="Stop"&&relation.line==5));
            let program=crate::xml_program::extract_production("Frames.xml",text.as_bytes()).unwrap();
            for node in program.occurrences.iter().filter(|node|node.tag=="Frame"){
                assert_eq!(&text.as_bytes()[node.raw.start..node.raw.start+6],b"<Frame");
                let declaration=facts.declarations.iter().find(|declaration|declaration.name==node.attributes["name"].value).unwrap();
                assert_eq!(declaration.line,1+text.as_bytes()[..node.raw.start].iter().filter(|byte|**byte==b'\n').count());
            }
        }}
    }
    #[test]
    fn inline_entity_newlines_preserve_original_declaration_and_relation_rows(){
        let raw="\u{feff}<Ui>\r\n<Frame name=\"Demo\"><Scripts><OnLoad>&#10;local function Inline()&#10; return 1&#10;end&#10;Inline()</OnLoad></Scripts></Frame>\r\n<Script>&#10;function Loaded()&#10; Inline()&#10;end</Script>\r\n</Ui>";
        let facts=xml(raw.as_bytes());assert!(facts.diagnostics.is_empty(),"{:?}",facts.diagnostics);
        let local=facts.declarations.iter().find(|declaration|declaration.name=="Inline").unwrap();assert_eq!((local.line,local.end_line),(2,2));
        let loaded=facts.declarations.iter().find(|declaration|declaration.name=="Loaded").unwrap();assert_eq!((loaded.line,loaded.end_line),(3,3));
        assert!(facts.relationships.iter().any(|relation|relation.from=="Demo:OnLoad"&&relation.to=="Inline"&&relation.line==2));
        assert!(facts.relationships.iter().any(|relation|relation.from=="Loaded"&&relation.to=="Inline"&&relation.line==3));
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
                    let actual = xml_with_line_counter(raw, &mut XmlFactsContext::default(), |at| lines.at(at));
                    let expected = xml_with_line_counter(raw, &mut XmlFactsContext::default(), |at| {
                        1 + raw[..at].iter().filter(|byte| **byte == b'\n').count()
                    });
                    assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap(), "{text:?}");
                    assert!(lines.scanned <= raw.len(), "ordered events rescanned source: {text:?}");
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
            assert_eq!(lines.at(at), 1 + raw[..at].iter().filter(|byte| **byte == b'\n').count());
        }
    }
    #[test]
    fn xml_catalog_failure_keeps_each_documents_original_facts_and_gap() {
        let failure = "xml.companion_identity_mismatch".to_owned();
        let mut shared = XmlFactsContext { catalog: Some(Err(failure.clone())) };
        for raw in [b"<Ui>\n<Frame name=\"First\"/>\n</Ui>".as_slice(), b"<Ui>\n<Frame name=\"Second\"/>\n</Ui>".as_slice()] {
            let actual = xml_with_context(raw, &mut shared);
            let expected = xml_with_line_counter(raw, &mut XmlFactsContext { catalog: Some(Err(failure.clone())) }, |at| {
                1 + raw[..at].iter().filter(|byte| **byte == b'\n').count()
            });
            assert_eq!(serde_json::to_value(&actual).unwrap(), serde_json::to_value(expected).unwrap());
            assert_eq!(actual.declarations.len(), 1);
            assert_eq!(actual.diagnostics.len(), 1);
            assert_eq!(actual.diagnostics[0].message, failure);
            assert_eq!(actual.diagnostics[0].line, 0);
        }
    }
    #[test]
    fn shared_catalog_keeps_each_xml_validation_and_all_fact_fields() {
        let mut shared = XmlFactsContext::default();
        for raw in [
            b"<Ui><Frame name=\"First\"/></Ui>".as_slice(),
            b"<Ui><Frame name=\"Broken\"></Ui>".as_slice(),
            b"<Ui><Frame name=\"Last\"><Scripts><OnLoad>self:Show()</OnLoad></Scripts></Frame></Ui>".as_slice(),
        ] {
            let expected = xml(raw);
            let before = crate::xml_program::catalog_verifications();
            let initialized = shared.catalog.is_some();
            let actual = xml_with_context(raw, &mut shared);
            assert_eq!(crate::xml_program::catalog_verifications(), before + usize::from(!initialized));
            assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap());
        }
    }
}
