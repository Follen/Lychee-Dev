//! Ordered XML program extraction shared by production Source and the v8 probe.
//! Schema ancestry is extraction provenance, never Lua nominal inheritance.
use std::{collections::{BTreeMap,BTreeSet},time::Instant};
#[cfg(feature="emmy-prototype")]
use emmylua_parser::{LuaAstNode,LuaAstToken,LuaCallExpr,LuaLanguageLevel,LuaNameToken,LuaParseErrorKind,LuaParser,ParserConfig};
use quick_xml::{Reader,events::{BytesStart,Event}};
use serde::Serialize;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use crate::xml_inline::source_map::{Span,Origin,MappedText,xml_char,borrowed_span,raw_cursor};
type Result<T> = std::result::Result<T,String>;
fn hash(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn err<T>(message:&str)->Result<T>{Err(message.to_owned())}
pub const COMMIT:&str="31c7f7b9cc79e56c986b365c06a6afbcf3c9177b";
pub const ENGINE:&str="08b0873962e83bb8307e9a81052e3f41578fbc78";
pub const XML_RULE_REVISION:&str="xml-map-v8";
pub const XSD:&[u8]=include_bytes!("../../../tests/rust-source-engine/xml-retail-UI.xsd");
pub const COMPANION:&str=include_str!("../../../tests/rust-source-engine/xml-companion-manifest.json");
pub const MAX_FILE:usize=8<<20;
pub const MAX_DECODED:usize=16<<20;
pub const MAX_SEGMENTS:usize=200_000;
pub const MAX_OCCURRENCES:usize=20_000;
pub const MAX_SCRIPTS:usize=4096;
pub const MAX_DEPTH:usize=128;
pub const MAX_OUTPUT:usize=8<<20;
pub const MAX_SYNTAX_FACTS:usize=20_000;
// XML 1.0 Fifth Edition productions [3], [4], [4a], [5]. These are explicit
// Unicode ranges, not Unicode-category/ASCII approximations. The separate
// prefixed-element profile remains unchanged; namespace attributes are names.
fn xml_space(ch:char)->bool { matches!(ch,' '|'\t'|'\r'|'\n') }
fn xml_name_start(ch:char)->bool {
    matches!(ch, ':'|'A'..='Z'|'_'|'a'..='z'|'\u{c0}'..='\u{d6}'|'\u{d8}'..='\u{f6}'
        |'\u{f8}'..='\u{2ff}'|'\u{370}'..='\u{37d}'|'\u{37f}'..='\u{1fff}'
        |'\u{200c}'..='\u{200d}'|'\u{2070}'..='\u{218f}'|'\u{2c00}'..='\u{2fef}'
        |'\u{3001}'..='\u{d7ff}'|'\u{f900}'..='\u{fdcf}'|'\u{fdf0}'..='\u{fffd}'
        |'\u{10000}'..='\u{effff}')
}
fn xml_name_char(ch:char)->bool {
    xml_name_start(ch) || matches!(ch,'-'|'.'|'0'..='9'|'\u{b7}'|'\u{300}'..='\u{36f}'|'\u{203f}'..='\u{2040}')
}
pub fn xml_name(name:&str)->bool {
    let mut chars=name.chars();
    chars.next().is_some_and(xml_name_start) && chars.all(xml_name_char)
}

#[derive(Clone, Debug, Serialize)]
pub struct AttributeFact { pub value: String, pub raw: Span }

#[derive(Debug, Serialize)]
pub struct Occurrence {
    pub ordinal: usize,
    pub kind: String,
    pub tag: String,
    pub raw: Span,
    pub parent: Option<usize>,
    pub attributes: BTreeMap<String, AttributeFact>,
    pub script: Option<usize>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub handler_catalog: Option<Value>,
    #[serde(rename="sourceText",skip_serializing_if="Option::is_none")]
    pub source_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct ProfileDiagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub occurrence: usize,
    pub raw: Span,
    pub validation_state: &'static str,
    pub native_runtime_behavior: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ScriptSource {
    pub id: String,
    pub kind: String,
    pub tag: String,
    pub element: Span,
    #[serde(skip_serializing_if="Option::is_none")]
    pub binding_body_raw: Option<Span>,
    pub decoded: MappedText,
    pub compiler: MappedText,
    pub syntax: Value,
    pub implicit_parameters: &'static str,
}

impl ScriptSource {
    /// Fixed luac 5.1.5 -p - may consume these exact bytes. This method never launches it.
    pub fn compiler_input(&self) -> &[u8] { self.compiler.text.as_bytes() }

    /// luac reports lines, not known columns. Keep all source segments on the line.
    pub fn compiler_line(&self, line: usize) -> Result<Value> {
        if line == 0 { return err("xml.invalid_compiler_line"); }
        // Lua 5.1.5 llex.c inclinenumber: CR and LF each advance a line;
        // adjacent opposite newline bytes (CRLF or LFCR) form one advance.
        // Scan actual compiler bytes; entity-produced CR must not be rewritten.
        let bytes=self.compiler.text.as_bytes();
        let next_line=|start:usize|->Option<usize> {
            let newline=start+bytes[start..].iter().position(|byte|matches!(*byte,b'\r'|b'\n'))?;
            let mut end=newline+1;
            if bytes.get(end).is_some_and(|byte|matches!(*byte,b'\r'|b'\n') && *byte!=bytes[newline]) {
                end+=1;
            }
            Some(end)
        };
        let mut start = 0;
        for _ in 1..line {
            start=next_line(start).ok_or("xml.compiler_line_outside_input")?;
        }
        let end=next_line(start).unwrap_or(bytes.len());
        if start == end {
            return Ok(json!({"columnKnown":false,"compiledLine":line,"accuracy":"synthetic-anchor","anchor":self.element}));
        }
        Ok(json!({"columnKnown":false,"compiledLine":line,"sourceMap":self.compiler.project(Span::new(start,end))?}))
    }
}

#[derive(Debug, Serialize)]
pub struct XmlProgram {
    #[serde(rename="xmlRuleRevision")]
    pub rule_revision: &'static str,
    #[serde(rename="sourceProfile")]
    pub source_profile: &'static str,
    pub path: String,
    pub raw_sha256: String,
    pub raw_bytes: usize,
    pub snapshot_identity: String,
    pub occurrences: Vec<Occurrence>,
    pub scripts: Vec<ScriptSource>,
    pub diagnostics: Vec<ProfileDiagnostic>,
    pub coverage: Value,
}

impl XmlProgram {
    pub fn compiler_inputs(&self) -> BTreeMap<String, Vec<u8>> {
        self.scripts.iter().enumerate().map(|(index, script)|
            (format!("xml-script-{index:04}.lua"), script.compiler_input().to_vec())).collect()
    }
}

pub const SCRIPT_CATALOG_TYPES:[&str;4]=["ScriptsType","ActorScriptsType","AnimScriptsType","AnimGroupScriptsType"];

struct SchemaType {
    raw:Span,
    base:Option<(String,Span)>,
    scripts:Option<(String,Span)>,
    references:Vec<(String,Span)>,
    object_attributes:bool,
}
struct SchemaElement { raw:Span, schema_type:Option<String>, substitution_group:Option<String> }
pub struct Catalog {
    handlers:BTreeSet<String>,
    handler_declarations:BTreeMap<String,BTreeMap<String,Span>>,
    types:BTreeMap<String,SchemaType>,
    elements:BTreeMap<String,SchemaElement>,
    identity:String,
    schema_sha256:String,
}

fn schema_local_name(name:&str)->&str { name.rsplit(':').next().unwrap_or(name) }

impl Catalog {
    /// XML construction vocabulary only. These schema edges do not establish
    /// Lua native classes, methods, or nominal subtype relationships.
    pub fn object_elements(&self)->BTreeSet<String>{
        self.elements.iter().filter(|(_,element)|{
            let mut current=element.schema_type.as_deref();let mut seen=BTreeSet::new();
            while let Some(name)=current{
                if !seen.insert(name){break;}
                let Some(kind)=self.types.get(name)else{break;};
                if kind.object_attributes{return true;}
                current=kind.base.as_ref().map(|(name,_)|name.as_str());
            }
            false
        }).map(|(name,_)|name.clone()).collect()
    }
    fn scripts_substitution_path(&self, reference:&str)->Option<Vec<String>> {
        let mut name="Scripts";
        let mut path=Vec::new();
        for _ in 0..MAX_DEPTH {
            if path.iter().any(|visited|visited==name) { return None; }
            path.push(name.to_owned());
            if name==reference { return Some(path); }
            name=self.elements.get(name)?.substitution_group.as_deref()?;
        }
        None
    }

    // Provenance only: a known catalog name is an extraction candidate, not a
    // claim that this document is XSD-valid. Unknown/mismatched owners stay visible.
    fn handler_provenance(&self, owner:&str, handler:&str)->Value {
        let owner_element=self.elements.get(owner);
        let owner_type=owner_element.and_then(|element|element.schema_type.as_deref());
        let mut current=owner_type;
        let mut visited=BTreeSet::new();
        let mut inheritance=Vec::new();
        let mut container:Option<Value>=None;
        let mut selected_type:Option<&str>=None;
        for _ in 0..MAX_DEPTH {
            let Some(type_name)=current else {break};
            if !visited.insert(type_name) {break;}
            let Some(schema_type)=self.types.get(type_name) else {break};
            inheritance.push(json!({"type":type_name,"declaration":schema_type.raw,
                "base":schema_type.base.as_ref().map(|(name,_)|name),
                "extensionDeclaration":schema_type.base.as_ref().map(|(_,span)|span)}));
            if let Some((scripts_type,span))=&schema_type.scripts {
                selected_type=Some(scripts_type);
                container=Some(json!({"catalogType":scripts_type,"declaredByType":type_name,
                    "declaration":span,"via":"local_element","substitutionPath":[]}));
                break;
            }
            for (reference,span) in &schema_type.references {
                if let Some(substitutions)=self.scripts_substitution_path(reference)
                    && let Some(scripts)=self.elements.get("Scripts")
                    && let Some(scripts_type)=scripts.schema_type.as_deref()
                {
                    selected_type=Some(scripts_type);
                    container=Some(json!({"catalogType":scripts_type,"declaredByType":type_name,
                        "declaration":scripts.raw,"via":"element_reference_substitution",
                        "referenceDeclaration":span,"substitutionPath":substitutions}));
                    break;
                }
            }
            if selected_type.is_some() {break;}
            current=schema_type.base.as_ref().map(|(base,_)|base.as_str());
        }
        let declarations:Vec<_>=self.handler_declarations.iter().filter_map(|(kind,names)|names.get(handler)
            .map(|span|json!({"catalogType":kind,"declaration":span}))).collect();
        let matched=selected_type.and_then(|kind|self.handler_declarations.get(kind)).and_then(|names|names.get(handler));
        let resolution=if owner_type.is_none(){"unknown_owner_element"}
            else if selected_type.is_none(){"owner_scripts_unresolved"}
            else if matched.is_none(){"known_owner_mismatch"}else{"matched"};
        json!({"schemaSHA256":self.schema_sha256,"handler":handler,"handlerDeclarations":declarations,
            "ownerElement":owner,"ownerElementDeclaration":owner_element.map(|element|element.raw),
            "ownerType":owner_type,"inheritance":inheritance,"scriptContainer":container,
            "matchedDeclaration":matched,"ownerResolution":resolution,
            "schemaValidation":"not_performed","implicitParameters":"not_injected"})
    }
}

#[cfg(test)]
thread_local! { static CATALOG_VERIFICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn catalog_verifications()->usize {CATALOG_VERIFICATIONS.with(|value|value.get())}

pub fn catalog() -> Result<Catalog> {
    #[cfg(test)]
    CATALOG_VERIFICATIONS.with(|value|value.set(value.get()+1));
    let identity: Value = serde_json::from_str(COMPANION).map_err(|e| e.to_string())?;
    if identity["commit"] != COMMIT || identity["bytes"] != XSD.len()
        || identity["sha256"] != hash(XSD) || identity["originalDocumentCount"] != 4036
    { return err("xml.companion_identity_mismatch"); }
    let mut reader = Reader::from_reader(XSD);
    let mut catalog=Catalog { handlers:BTreeSet::new(),handler_declarations:BTreeMap::new(),
        types:BTreeMap::new(),elements:BTreeMap::new(),identity:hash(COMPANION.as_bytes()),schema_sha256:hash(XSD) };
    let mut elements=Vec::<String>::new();
    let mut type_stack=Vec::<Option<String>>::new();
    let prefix=if XSD.starts_with(b"\xef\xbb\xbf"){3}else{0};
    loop {
        let event=reader.read_event().map_err(|e|e.to_string())?;
        let empty=matches!(&event,Event::Empty(_));
        match event {
            Event::Start(e)|Event::Empty(e)=>{
                let tag=String::from_utf8(e.local_name().as_ref().to_vec()).map_err(|e|e.to_string())?;
                let mut attrs=BTreeMap::new();
                for a in e.attributes() {
                    let a=a.map_err(|e|e.to_string())?;
                    attrs.insert(String::from_utf8(a.key.as_ref().to_vec()).map_err(|e|e.to_string())?,
                        a.unescape_value().map_err(|e|e.to_string())?.into_owned());
                }
                let start=borrowed_span(XSD,e.as_ref())?.start.checked_sub(1).ok_or("xml.schema_tag_position")?;
                let span=Span::new(start,raw_cursor(XSD.len(),prefix,reader.buffer_position())?);
                let owner=type_stack.last().and_then(|name|name.as_deref());
                if tag=="complexType" {
                    let name=attrs.get("name").cloned();
                    if let Some(name)=&name {
                        catalog.types.insert(name.clone(),SchemaType {raw:span,base:None,scripts:None,references:Vec::new(),object_attributes:false});
                    }
                    if !empty {type_stack.push(name);}
                } else if tag=="attribute" && attrs.get("name").is_some_and(|name|matches!(name.as_str(),"virtual"|"mixin"|"intrinsic")) {
                    if let Some(kind)=owner.and_then(|owner|catalog.types.get_mut(owner)){kind.object_attributes=true;}
                } else if tag=="extension" {
                    if let Some(owner)=owner
                        && let Some(base)=attrs.get("base")
                        && let Some(kind)=catalog.types.get_mut(owner)
                    {kind.base=Some((schema_local_name(base).into(),span));}
                } else if tag=="element" {
                    let name=attrs.get("name");
                    let target=attrs.get("type").map(|name|schema_local_name(name));
                    if elements.len()==1 && elements[0]=="schema"
                        && let Some(name)=name
                    {
                        catalog.elements.insert(name.clone(),SchemaElement {raw:span,schema_type:target.map(str::to_owned),
                            substitution_group:attrs.get("substitutionGroup").map(|name|schema_local_name(name).to_owned())});
                    }
                    if let Some(owner)=owner {
                        if SCRIPT_CATALOG_TYPES.contains(&owner) && target==Some("ScriptType") {
                            let name=name.ok_or("xml.schema_missing_handler_name")?.clone();
                            catalog.handlers.insert(name.clone());
                            catalog.handler_declarations.entry(owner.into()).or_default().insert(name,span);
                        }
                        if let Some(kind)=catalog.types.get_mut(owner) {
                            if name.is_some_and(|name|name=="Scripts")
                                && let Some(target)=target
                            {kind.scripts=Some((target.into(),span));}
                            if let Some(reference)=attrs.get("ref") {
                                kind.references.push((schema_local_name(reference).into(),span));
                            }
                        }
                    }
                }
                if !empty {elements.push(tag);}
            }
            Event::End(e)=>{
                if e.local_name().as_ref()==b"complexType" {type_stack.pop();}
                elements.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !SCRIPT_CATALOG_TYPES.iter().all(|kind|catalog.handler_declarations.contains_key(*kind))
        || !["OnEvent","PreClick","PostClick","OnModelLoading","OnPlay","OnPause","OnStop","OnFinished","OnLoop"]
            .iter().all(|name|catalog.handlers.contains(*name))
    {
        return err("xml.schema_handler_set_incomplete");
    }
    Ok(catalog)
}

// XML S, Eq and quoted-value framing for a complete event, including its tail.
// quick-xml's attribute/getter paths do not enforce every separator or visit
// every declaration field. Scan borrowed bytes without repairing the input.
fn quoted_fields(bytes:&[u8],prefix:usize,mut accept:impl FnMut(&[u8],&[u8])->Result<()>)->Result<()> {
    if prefix>bytes.len() { return err("xml.field_prefix_outside_event"); }
    let is_space=|byte:u8|matches!(byte,b' '|b'\t'|b'\r'|b'\n');
    let mut at=prefix;
    while at<bytes.len() {
        let separator=at;
        while bytes.get(at).is_some_and(|b|is_space(*b)) {at+=1;}
        if at==separator { return err("xml.attribute_missing_separator"); }
        if at==bytes.len() {break;}
        let start=at;
        while bytes.get(at).is_some_and(|b|!is_space(*b) && *b!=b'=') {at+=1;}
        let name=&bytes[start..at];
        if name.is_empty() { return err("xml.empty_attribute_name"); }
        while bytes.get(at).is_some_and(|b|is_space(*b)) {at+=1;}
        if bytes.get(at)!=Some(&b'=') { return err("xml.attribute_missing_equals"); }
        at+=1;
        while bytes.get(at).is_some_and(|b|is_space(*b)) {at+=1;}
        let quote=*bytes.get(at).ok_or("xml.attribute_missing_quote")?;
        if !matches!(quote,b'\''|b'"') { return err("xml.attribute_missing_quote"); }
        at+=1;
        let value_start=at;
        while bytes.get(at).is_some_and(|b|*b!=quote) {at+=1;}
        if at==bytes.len() { return err("xml.attribute_unclosed_value"); }
        accept(name,&bytes[value_start..at])?;
        at+=1;
    }
    Ok(())
}

fn declaration_fields(raw:&[u8],span:Span)->Result<()> {
    let bytes=raw.get(span.start..span.end).ok_or("xml.declaration_outside_input")?;
    if !bytes.starts_with(b"<?xml") || !bytes.ends_with(b"?>") || bytes.len()<7 {
        return err("xml.unclosed_or_invalid_declaration");
    }
    let mut phase=0;
    quoted_fields(&bytes[2..bytes.len()-2],3,|name,value| {
        match name {
            b"version" if phase==0=>{
                if value!=b"1.0" {return err("xml.unsupported_version");}
                phase=1;
            }
            b"encoding" if phase==1=>{
                if !value.eq_ignore_ascii_case(b"utf-8") {return err("xml.encoding_not_utf8");}
                phase=2;
            }
            b"standalone" if matches!(phase,1|2)=>{
                if !matches!(value,b"yes"|b"no") {return err("xml.invalid_standalone");}
                phase=3;
            }
            _=>return err("xml.invalid_declaration_field_or_order"),
        }
        Ok(())
    })?;
    if phase==0 {return err("xml.declaration_missing_version");}
    Ok(())
}

fn attributes(raw: &[u8], e: &BytesStart<'_>) -> Result<BTreeMap<String, AttributeFact>> {
    let mut out = BTreeMap::new();
    quoted_fields(e.as_ref(),e.name().as_ref().len(),|name,value| {
        let name = std::str::from_utf8(name).map_err(|_| "xml.attribute_encoding")?.to_owned();
        if !xml_name(&name) { return err("xml.invalid_attribute_name"); }
        if out.contains_key(&name) {return err("xml.duplicate_attribute");}
        let span = borrowed_span(raw,value)?;
        if value.contains(&b'<') { return err("xml.raw_lt_in_attribute"); }
        // XML attribute normalization applies to physical whitespace before references
        // are expanded; an explicit &#10; must remain a newline.
        let physical = std::str::from_utf8(value).map_err(|_| "xml.attribute_encoding")?
            .replace("\r\n", " ").replace(['\r', '\n', '\t'], " ");
        let value = quick_xml::escape::unescape(&physical).map_err(|e| format!("xml.attribute_entity: {e}"))?.into_owned();
        if !value.chars().all(xml_char) { return err("xml.invalid_attribute_character"); }
        out.insert(name, AttributeFact { value, raw: span });
        Ok(())
    })?;
    Ok(out)
}

struct Element { name: String, occurrence: Option<usize> }
struct Body { kind: String, tag: String, start: usize, content_start: usize, occurrence: usize, decoded: MappedText }

pub struct ExtractionLimits { pub scripts:usize, pub decoded:usize, pub maps:usize, pub occurrences:usize, pub facts:usize }
impl Default for ExtractionLimits {
    fn default()->Self { Self { scripts:MAX_SCRIPTS,decoded:MAX_DECODED,maps:2*MAX_SEGMENTS+2*MAX_SCRIPTS,
        occurrences:MAX_OCCURRENCES,facts:MAX_SYNTAX_FACTS*MAX_SCRIPTS } }
}

fn push_occurrence(program: &mut XmlProgram, kind: &str, tag: &str, raw: Span,
    parent: Option<usize>, attributes: BTreeMap<String, AttributeFact>) -> Result<usize>
{
    if program.occurrences.len() >= MAX_OCCURRENCES { return err("xml.occurrence_budget"); }
    let ordinal = program.occurrences.len();
    program.occurrences.push(Occurrence { ordinal, kind: kind.into(), tag: tag.into(), raw,
        parent, attributes, script: None,handler_catalog:None,source_text:None });
    Ok(ordinal)
}

fn finish_body(program: &mut XmlProgram, body: Body, end: usize, close: Span, limits:&ExtractionLimits) -> Result<()> {
    // Empty bindings remain declarations, with no fabricated executable body.
    if body.kind=="binding" && body.decoded.text.is_empty() { return Ok(()); }
    if program.scripts.len() >= MAX_SCRIPTS { return err("xml.script_budget"); }
    if program.scripts.iter().map(|s| s.decoded.text.len()).sum::<usize>() + body.decoded.text.len() > MAX_DECODED {
        return err("xml.aggregate_decoded_budget");
    }
    if program.scripts.iter().map(|s| s.decoded.segments.len()).sum::<usize>() + body.decoded.segments.len() > MAX_SEGMENTS {
        return err("xml.aggregate_segment_budget");
    }
    let element = Span::new(body.start,end);
    let mut compiler = MappedText::default();
    if matches!(body.kind.as_str(),"handler"|"binding") {
        // A syntax-only closure envelope; no unsupported self/event/button parameter catalog.
        compiler.append("return function(...)\n", None, Origin::Synthetic, Some(Span::new(body.start,body.start)))?;
    }
    compiler.extend(&body.decoded)?;
    if matches!(body.kind.as_str(),"handler"|"binding") {
        compiler.append("\nend\n", None, Origin::Synthetic, Some(close))?;
    }
    let used_decoded=program.scripts.iter().map(|s|s.decoded.text.len()).sum::<usize>();
    let used_maps=program.scripts.iter().map(|s|s.decoded.segments.len()+s.compiler.segments.len()).sum::<usize>();
    let used_facts=program.scripts.iter().map(|s|s.syntax["names"].as_array().map_or(0,Vec::len)
        +s.syntax["calls"].as_array().map_or(0,Vec::len)).sum::<usize>();
    if program.scripts.len()>=limits.scripts || used_decoded+body.decoded.text.len()>limits.decoded
        || used_maps+body.decoded.segments.len()+compiler.segments.len()>limits.maps {
        return err("xml.remaining_program_budget");
    }
    let index = program.scripts.len();
    let id = hash(format!("{}\0{}\0{}\0{}", program.snapshot_identity, body.tag, element.start, element.end).as_bytes());
    let syntax = syntax_facts(&compiler,limits.facts.saturating_sub(used_facts))?;
    program.occurrences[body.occurrence].script = Some(index);
    let is_binding=body.kind=="binding";
    program.scripts.push(ScriptSource { id, kind: body.kind, tag: body.tag, element,
        binding_body_raw:is_binding.then_some(Span::new(body.content_start,close.start)),
        decoded: body.decoded, compiler, syntax,
        implicit_parameters: if is_binding {"not_injected: InputCommandCallback documents keystate, but has no proven XML Binding signature edge"}
            else {"not_injected: pinned UI.xsd has handler names but no parameter catalog"} });
    Ok(())
}

#[cfg(feature="emmy-prototype")]
fn syntax_facts(source: &MappedText, remaining_facts:usize) -> Result<Value> {
    let tree = LuaParser::parse(&source.text, ParserConfig::with_level(LuaLanguageLevel::Lua51));
    let errors = tree.get_errors().iter().filter(|error| error.kind == LuaParseErrorKind::SyntaxError)
        .map(|error| {
            let start = u32::from(error.range.start()) as usize;
            let end = u32::from(error.range.end()) as usize;
            let mapping = if start < end { source.project(Span::new(start,end))? }
                else { json!({"accuracy":"zero_length_parser_position","offset":start}) };
            Ok(json!({"message":error.message,"bytes":Span::new(start,end),"sourceMap":mapping}))
        }).collect::<Result<Vec<_>>>()?;
    if !errors.is_empty() {
        return Ok(json!({"state":"invalid","errors":errors,"names":[],"calls":[],"bindingFactsExported":false}));
    }
    let mut names = Vec::new();
    for item in tree.get_red_root().descendants_with_tokens() {
        let Some(token) = item.into_token().and_then(LuaNameToken::cast) else { continue; };
        let range = token.get_range();
        if names.len() >= remaining_facts { return err("xml.remaining_program_budget"); }
        if names.len() >= MAX_SYNTAX_FACTS { return err("xml.syntax_fact_budget"); }
        names.push(json!({"name":token.get_name_text(),"sourceMap":source.project(Span::new(
            u32::from(range.start()) as usize,u32::from(range.end()) as usize))?}));
    }
    let calls = tree.get_chunk_node().descendants::<LuaCallExpr>().enumerate().map(|(index,call)| {
        if index + names.len() >= remaining_facts { return err("xml.remaining_program_budget"); }
        if index + names.len() >= MAX_SYNTAX_FACTS { return err("xml.syntax_fact_budget"); }
        let range = call.get_range();
        Ok(json!({"kind":"syntax_call_expression","sourceMap":source.project(Span::new(
            u32::from(range.start()) as usize,u32::from(range.end()) as usize))?,"binding":"not_analyzed"}))
    }).collect::<Result<Vec<_>>>()?;
    Ok(json!({"state":"parsed","errors":[],"names":names,"calls":calls,
        "bindingFactsExported":false,"syntaxAuthority":"luac-5.1.5 required; Emmy raw Lua51 label gap remains"}))
}

#[cfg(not(feature="emmy-prototype"))]
fn syntax_facts(_: &MappedText, _:usize)->Result<Value>{Ok(json!({"state":"not_checked","names":[],"calls":[],"bindingFactsExported":false}))}

/// One transactional parse: malformed XML returns Err and no partial program.
pub fn extract(path: &str, raw: &[u8]) -> Result<XmlProgram> {
    extract_with_deadline(path,raw,&catalog()?,None,&ExtractionLimits::default())
}

pub fn extract_production(path: &str, raw: &[u8]) -> Result<XmlProgram> {
    extract_production_with_catalog(path,raw,&catalog()?)
}

/// The caller may borrow one verified immutable catalog for a single preparation.
/// The production profile, limits and complete per-document evidence are unchanged.
pub(crate) fn extract_production_with_catalog(path:&str,raw:&[u8],catalog:&Catalog)->Result<XmlProgram> {
    extract_profile(path,raw,catalog,None,&ExtractionLimits::default(),true)
}

pub fn extract_with_deadline(path: &str, raw: &[u8], catalog: &Catalog, deadline: Option<Instant>, limits:&ExtractionLimits) -> Result<XmlProgram> {
    extract_profile(path,raw,catalog,deadline,limits,false)
}

fn extract_profile(path: &str, raw: &[u8], catalog: &Catalog, deadline: Option<Instant>, limits:&ExtractionLimits, preserve_unknown_handlers:bool) -> Result<XmlProgram> {
    if raw.len() > MAX_FILE || path.len() > 4096 { return err("xml.input_budget"); }
    let text = std::str::from_utf8(raw).map_err(|_| "xml.non_utf8")?;
    if !text.chars().all(xml_char) { return err("xml.invalid_character"); }
    let mut program = XmlProgram { rule_revision:XML_RULE_REVISION, source_profile:"pending", path: path.into(), raw_sha256: hash(raw), raw_bytes: raw.len(),
        snapshot_identity: String::new(), occurrences: Vec::new(), scripts: Vec::new(), diagnostics:Vec::new(), coverage: Value::Null };
    let mut reader = Reader::from_reader(raw);
    reader.config_mut().check_comments = true;
    let prefix_bytes=if raw.starts_with(b"\xef\xbb\xbf") {3}else{0};
    let mut stack = Vec::<Element>::new();
    let mut body: Option<Body> = None;
    let mut roots = 0;
    let mut declaration = false;
    let started = Instant::now();
    loop {
        if started.elapsed().as_secs() >= 30 || deadline.is_some_and(|end| Instant::now() >= end) {
            return err("xml.deadline");
        }
        let before=raw_cursor(raw.len(),prefix_bytes,reader.buffer_position())?;
        let event = reader.read_event().map_err(|e| format!("xml.malformed: {e}"))?;
        let empty=matches!(&event,Event::Empty(_));
        let after=raw_cursor(raw.len(),prefix_bytes,reader.buffer_position())?;
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let content = borrowed_span(raw,e.as_ref())?;
                let tag_start = content.start.checked_sub(1).ok_or("xml.tag_position")?;
                let range = Span::new(tag_start,after);
                if !xml_name(std::str::from_utf8(e.name().as_ref()).map_err(|_|"xml.tag_encoding")?) {
                    return err("xml.invalid_element_name");
                }
                let tag = std::str::from_utf8(e.local_name().as_ref()).map_err(|_| "xml.tag_encoding")?.to_owned();
                if e.name().as_ref() != tag.as_bytes() { return err("xml.prefixed_element_not_supported"); }
                if body.is_some() { return err("xml.nested_script_markup"); }
                if stack.len() >= MAX_DEPTH { return err("xml.depth_budget"); }
                if stack.is_empty() {
                    roots += 1;
                    if roots != 1 { return err("xml.expected_single_ui_root"); }
                    program.source_profile=match tag.as_str() {
                        "Ui"=>"Ui", "Bindings"=>"Bindings", _=>return Err(format!("xml.unsupported_root:{tag}")),
                    };
                    program.snapshot_identity=hash(format!("{COMMIT}\0{}\0{path}\0{}\0{XML_RULE_REVISION}\0{ENGINE}\0{}",
                        catalog.identity,program.raw_sha256,program.source_profile).as_bytes());
                }
                let attrs = attributes(raw,&e)?;
                let parent = stack.iter().rev().find_map(|entry| entry.occurrence);
                let under_scripts = stack.last().is_some_and(|entry| entry.name == "Scripts");
                let handler = under_scripts && (preserve_unknown_handlers || catalog.handlers.contains(&tag));
                if under_scripts && !handler { return err("xml.unsupported_handler"); }
                let occurrence;
                let mut body_kind = None;
                if program.source_profile=="Bindings" {
                    let kind=if stack.is_empty() {"bindings_root"}
                        else if stack.len()==1 && tag=="Binding" {"binding_declaration"}
                        else if stack.len()==1 && tag=="ModifiedClick" {"modified_click"}
                        else {return err("xml.unsupported_bindings_structure");};
                    occurrence=Some(push_occurrence(&mut program,kind,&tag,range,parent,attrs.clone())?);
                    if tag=="Binding" && !empty { body_kind=Some("binding"); }
                } else if tag == "Include" {
                    if !attrs.contains_key("file") { return err("xml.include_missing_file"); }
                    occurrence = Some(push_occurrence(&mut program,"include",&tag,range,parent,attrs.clone())?);
                } else if tag == "Script" {
                    let kind = if attrs.contains_key("file") { "script_file" } else { "inline_script" };
                    occurrence = Some(push_occurrence(&mut program,kind,&tag,range,parent,attrs.clone())?);
                    if !empty { body_kind = Some("top_level"); }
                } else if handler {
                    occurrence = Some(push_occurrence(&mut program,"handler_registration",&tag,range,parent,attrs.clone())?);
                    let owner=stack.iter().rev().nth(1).map_or("",|entry|entry.name.as_str());
                    program.occurrences[occurrence.expect("handler occurrence")].handler_catalog=Some(catalog.handler_provenance(owner,&tag));
                    for key in ["function","method"] {
                        if let Some(value) = attrs.get(key) {
                            push_occurrence(&mut program,&format!("handler_{key}_reference"),&tag,value.raw,occurrence,
                                BTreeMap::from([(key.into(),value.clone())]))?;
                        }
                    }
                    if !empty { body_kind = Some("handler"); }
                } else if tag == "Mixin" && stack.last().is_some_and(|entry| entry.name == "Mixins") {
                    occurrence = Some(push_occurrence(&mut program,"mixin_reference",&tag,range,parent,attrs.clone())?);
                } else if ["name","mixin","inherits","virtual"].iter().any(|name| attrs.contains_key(*name)) {
                    occurrence = Some(push_occurrence(&mut program,"object_declaration",&tag,range,parent,attrs.clone())?);
                } else {
                    // Preserve structural ancestry even for anonymous frames and unknown
                    // elements; this deliberately makes no object-type inference.
                    occurrence = Some(push_occurrence(&mut program,"xml_element",&tag,range,parent,attrs.clone())?);
                }
                if let Some(kind) = body_kind {
                    body = Some(Body { kind:kind.into(),tag:tag.clone(),start:tag_start,content_start:after,
                        occurrence:occurrence.expect("script occurrence"),decoded:MappedText::default() });
                }
                if !empty { stack.push(Element { name:tag, occurrence }); }
            }
            Event::End(e) => {
                if !xml_name(std::str::from_utf8(e.name().as_ref()).map_err(|_|"xml.tag_encoding")?) {
                    return err("xml.invalid_element_name");
                }
                let tag = std::str::from_utf8(e.local_name().as_ref()).map_err(|_| "xml.tag_encoding")?.to_owned();
                let entry=stack.pop().ok_or("xml.unbalanced")?;
                if entry.name!=tag { return err("xml.unbalanced"); }
                if program.source_profile=="Bindings" && let Some(index)=entry.occurrence {
                    program.occurrences[index].raw.end=after;
                }
                if body.as_ref().is_some_and(|source| source.tag == tag) {
                    finish_body(&mut program,body.take().expect("checked body"),after,Span::new(before,after),limits)?;
                }
            }
            Event::Text(e) => {
                let span = borrowed_span(raw,e.as_ref())?;
                let text_bytes: &[u8] = e.as_ref();
                if text_bytes.windows(3).any(|bytes|bytes==b"]]>") { return err("xml.cdata_close_in_text"); }
                if let Some(body) = &mut body { body.decoded.append_physical(raw,span)?; }
                else if stack.is_empty() && !std::str::from_utf8(e.as_ref()).map_err(|_| "xml.non_utf8")?.chars().all(xml_space) {
                    return err("xml.text_outside_root");
                } else if program.source_profile=="Bindings" && !std::str::from_utf8(e.as_ref()).map_err(|_|"xml.non_utf8")?.chars().all(xml_space) {
                    if stack.len()!=1 { return err("xml.text_outside_binding_body"); }
                    // XML character data is well formed here. Retain its original bytes;
                    // the native Bindings validator/runtime response is not established.
                    let value=std::str::from_utf8(e.as_ref()).map_err(|_|"xml.non_utf8")?;
                    let diagnostic_raw=Span::new(span.start+value.len()-value.trim_start_matches(xml_space).len(),
                        span.start+value.trim_end_matches(xml_space).len());
                    let parent=stack.last().and_then(|entry|entry.occurrence);
                    let occurrence=push_occurrence(&mut program,"unexpected_profile_text","#text",span,parent,BTreeMap::new())?;
                    program.occurrences[occurrence].source_text=Some(value.to_owned());
                    program.diagnostics.push(ProfileDiagnostic {code:"binding_root_text",severity:"warning",occurrence,
                        raw:diagnostic_raw,validation_state:"unresolved",native_runtime_behavior:"unknown"});
                }
            }
            Event::CData(e) => {
                let span = borrowed_span(raw,e.as_ref())?;
                if let Some(body) = &mut body { body.decoded.append_physical(raw,span)?; }
                else if stack.is_empty() { return err("xml.cdata_outside_root"); }
                else if program.source_profile=="Bindings" { return err("xml.cdata_outside_binding_body"); }
            }
            Event::GeneralRef(e) => {
                let inner = borrowed_span(raw,e.as_ref())?;
                let span = Span::new(inner.start.checked_sub(1).ok_or("xml.entity_position")?,inner.end + 1);
                if let Some(body) = &mut body { body.decoded.append_entity(raw,span)?; }
                else {
                    let mut validate = MappedText::default();
                    validate.append_entity(raw,span)?;
                    if stack.is_empty() { return err("xml.entity_outside_root"); }
                    if program.source_profile=="Bindings" { return err("xml.entity_outside_binding_body"); }
                }
            }
            Event::DocType(_) => return err("xml.dtd_not_allowed"),
            Event::PI(_) => return err("xml.processing_instruction_not_allowed"),
            Event::Decl(_) => {
                if declaration || roots != 0 || !stack.is_empty() || before != prefix_bytes { return err("xml.misplaced_declaration"); }
                declaration = true;
                declaration_fields(raw,Span::new(before,after))?;
            }
            Event::Comment(_) => {}
            Event::Eof => break,
        }
        if program.occurrences.len()>limits.occurrences { return err("xml.remaining_program_budget"); }
    }
    if roots != 1 || !stack.is_empty() || body.is_some() { return err("xml.incomplete_document"); }
    // Attribute references may be lexically before another emitted sibling reference.
    // Stable raw-source ordering, with parent IDs remapped to the final order.
    program.occurrences.sort_by_key(|entry| (entry.raw.start,entry.ordinal));
    let indices: BTreeMap<_,_> = program.occurrences.iter().enumerate().map(|(new,old)|(old.ordinal,new)).collect();
    for (index,entry) in program.occurrences.iter_mut().enumerate() {
        entry.ordinal = index;
        entry.parent = entry.parent.map(|old| indices[&old]);
    }
    for diagnostic in &mut program.diagnostics { diagnostic.occurrence=indices[&diagnostic.occurrence]; }
    let parsed = program.scripts.iter().all(|source| source.syntax["state"] == "parsed");
    program.coverage = json!({"xmlExtraction":"complete","emmySyntaxParsed":parsed,
        "inputCoverage":"complete_original_document","extractionComplete":true,
        "profileValidationPassed":if program.source_profile=="Bindings" {json!(program.diagnostics.is_empty())}else{Value::Null},
        "profileValidationState":if program.source_profile!="Bindings" {"not_performed"}
            else if program.diagnostics.is_empty() {"passed_observed_extraction_profile"}else{"unresolved"},
        "profileValidationScope":"observed extraction profile only; native validation not performed",
        "nativeRuntimeBehavior":"unknown",
        "rawCursorPrefixBytes":prefix_bytes,
        "sourceProfile":program.source_profile,
        "schemaValidation":if program.source_profile=="Bindings" {"not_performed; UI.xsd is not a Bindings schema"}
            else {"not_performed; schema supplies handler names only"},
        "bindings":if program.source_profile=="Bindings" {json!({"declarations":"ordered; attributes preserved; duplicates retained",
            "names":"binding identifiers, not Lua globals","modifiedClick":"metadata only","execution":"not_claimed",
            "signatureEvidence":"InputCommandCallback keystate is not a proven native XML Binding signature"})}else{Value::Null},
        "lua51Compilation":"not_run","semanticComplete":false,"implicitParameters":"not_injected",
        "loads":"ordered declarations only; targets not opened, execution not claimed",
        "schemaCompanionSHA256":hash(XSD),"handlerKinds":catalog.handlers,
        "handlerCatalogDeclarations":catalog.handler_declarations,
        "limits":{"fileBytes":MAX_FILE,"decodedBytesAggregate":MAX_DECODED,"segmentsAggregate":MAX_SEGMENTS,
            "syntaxFactsPerScript":MAX_SYNTAX_FACTS,"outputBytes":MAX_OUTPUT,
            "occurrences":MAX_OCCURRENCES,"scripts":MAX_SCRIPTS,"depth":MAX_DEPTH,"cooperativeParseWallSeconds":30}});
    Ok(program)
}

