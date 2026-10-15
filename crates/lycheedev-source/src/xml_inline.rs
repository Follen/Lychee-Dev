//! Decoded XML Lua inputs retain segmented origins in the original UTF8 bytes.
//! A generated closure is syntax scaffolding, not a handler parameter contract.
pub mod source_map;
use source_map::{MappedText,Origin,Span,utf16_range,lua_line_span};
use lycheedev_core::{Result,ToolkitError,storage};
use serde_json::{Value,json};
use std::{collections::BTreeMap,sync::Arc};

pub const SOURCE_MAP_RULES:&str="xml-segmented-original-utf8-v1";
const WRAPPER:&str="return function(self, ...)\n";
const MAX_XML_BYTES:usize=32<<20;

pub struct Script {
    pub key:String,
    pub path:String,
    pub handler:String,
    pub kind:String,
    pub xml_sha256:String,
    pub compiler:MappedText,
    pub element:Span,
    body:Span,
    raw:Arc<str>,
}
fn invalid(message:impl ToString)->ToolkitError{ToolkitError::new("codebase.xml_invalid",message.to_string(),4)}
fn span(value:&Value)->Span{
    Span::new(value["start"].as_u64().expect("serialized span start")as usize,value["end"].as_u64().expect("serialized span end")as usize)
}
impl Script {
    /// Generated text has one owner; compiler and semantic consumers borrow the same bytes.
    pub fn source_bytes(&self)->&[u8]{self.compiler.text.as_bytes()}
    pub(crate) fn original_shared(&self)->Arc<str>{Arc::clone(&self.raw)}
    /// The only generated root closure spans the authenticated decoded body.
    /// Nested user closures cannot cross either synthetic wrapper boundary.
    pub(crate) fn is_syntax_wrapper(&self,start:usize,end:usize)->bool{
        matches!(self.kind.as_str(),"handler"|"binding")&&start<self.body.start&&end>self.body.end
    }
    pub fn body_text(&self)->&str{&self.compiler.text[self.body.start..self.body.end]}
    /// Tree-sitter fact rows count LF bytes. Project that decoded body line
    /// through the same segments as native generated sites, including entities.
    pub fn original_body_line(&self,line:usize)->Result<usize>{
        let body=self.body_text();
        let mut start=0;
        for _ in 1..line {start+=body[start..].find('\n').ok_or_else(||invalid("inline fact line outside decoded body"))?+1;}
        let end=start+body[start..].find('\n').unwrap_or(body.len()-start);
        let mapping=self.project(Span::new(self.body.start+start,self.body.start+end))?;
        let anchor=mapping["segments"].as_array().expect("segmented projection").iter().find_map(|segment|(!segment["raw"].is_null()).then(||span(&segment["raw"])).or_else(||(!segment["anchor"].is_null()).then(||span(&segment["anchor"])))).unwrap_or(self.element);
        Ok(source_map::utf16_position(&self.raw,anchor.start).map_err(invalid)?["line"].as_u64().expect("original line")as usize+1)
    }
    pub fn mapping_manifest(&self)->Value{
        json!({"rules":SOURCE_MAP_RULES,"path":self.key,"xmlPath":self.path,"xmlSHA256":self.xml_sha256,"xmlBytes":self.raw.len(),
            "compilerSHA256":storage::sha256(self.source_bytes()),"compilerBytes":self.source_bytes().len(),"segments":self.compiler.segments,"element":self.element,
            "wrapper":"syntax_only_closure","implicitHandlerParameters":if self.kind=="handler"{"self:xml_object; remaining:unknown"}else{"unknown"},"byteOffsets":"original_utf8_including_bom","positionEncoding":"utf-16","bomColumnPolicy":"encoding_marker_not_displayed"})
    }
    pub fn project(&self,range:Span)->Result<Value>{
        let mut mapping=if range.is_empty(){self.compiler.project_point(range.start)}else{self.compiler.project(range)}.map_err(invalid)?;
        for segment in mapping["segments"].as_array_mut().expect("segmented projection"){
            if !segment["raw"].is_null(){segment["rawRange"]=utf16_range(&self.raw,span(&segment["raw"])).map_err(invalid)?;}
            if !segment["anchor"].is_null(){segment["anchorRange"]=utf16_range(&self.raw,span(&segment["anchor"])).map_err(invalid)?;}
        }
        mapping["rules"]=json!(SOURCE_MAP_RULES);
        Ok(mapping)
    }
    pub fn position(&self,range:Span)->Result<Value>{
        let mapping=self.project(range)?;
        let covering=if mapping["covering"].is_null(){Value::Null}else{utf16_range(&self.raw,span(&mapping["covering"])).map_err(invalid)?};
        let synthetic=mapping["accuracy"]=="synthetic_or_mixed";
        Ok(json!({"path":self.path,"range":if synthetic{Value::Null}else{covering.clone()},"coveringRange":covering,
            "positionAccuracy":if synthetic{"synthetic-or-mixed"}else if mapping["accuracy"]=="exact"{"utf16-character"}else{"covering"},
            "generatedRange":utf16_range(&self.compiler.text,range).map_err(invalid)?,"generatedBytes":range,
            "sourceMap":mapping,"inlineHandler":self.handler,"xmlContentHash":self.xml_sha256}))
    }
    pub fn position_from_utf16(&self,range:&Value)->Result<Value>{
        let offset=|name:&str|->Result<usize>{
            let line=range[name]["line"].as_u64().ok_or_else(||invalid("diagnostic line missing"))?as usize;
            let column=range[name]["character"].as_u64().ok_or_else(||invalid("diagnostic character missing"))?as usize;
            source_map::utf16_offset(&self.compiler.text,line,column).map_err(invalid)
        };
        self.position(Span::new(offset("start")?,offset("end")?))
    }
    pub fn compiler_line(&self,line:usize)->Result<Value>{
        let range=lua_line_span(&self.compiler.text,line).map_err(invalid)?;
        let mapping=self.project(range)?;
        let anchor=mapping["segments"].as_array().expect("segmented projection").iter().find_map(|segment|
            (!segment["raw"].is_null()).then(||span(&segment["raw"]))
                .or_else(||(!segment["anchor"].is_null()).then(||span(&segment["anchor"]))))
            .unwrap_or(self.element);
        let location=source_map::utf16_position(&self.raw,anchor.start).map_err(invalid)?;
        Ok(json!({"line":location["line"].as_u64().expect("line")+1,"columnKnown":false,"compiledLine":line,"sourceMap":mapping}))
    }
}

/// The compiler and semantic paths consume the same ordered v8 extraction.
pub fn extract(path:&str,raw:&[u8])->Result<Vec<Script>>{
    let program=crate::xml_program::extract_production(path,raw).map_err(invalid)?;
    scripts(&program,raw)
}
pub(crate) fn scripts(program:&crate::xml_program::XmlProgram,raw:&[u8])->Result<Vec<Script>>{
    let raw_text:Arc<str>=std::str::from_utf8(raw).map_err(invalid)?.into();
    let mut scripts=Vec::new();
    for (index,source) in program.scripts.iter().enumerate(){
        let mut compiler=MappedText::with_limits(MAX_XML_BYTES,raw.len()+2);
        match source.kind.as_str(){
            "handler"=>compiler.append(WRAPPER,None,Origin::Synthetic,source.compiler.segments[0].anchor).map_err(invalid)?,
            "binding"=>compiler.append("return function(...)\n",None,Origin::Synthetic,source.compiler.segments[0].anchor).map_err(invalid)?,
            _=>{},
        }
        let body=Span::new(compiler.text.len(),compiler.text.len()+source.decoded.text.len());
        compiler.extend(&source.decoded).map_err(invalid)?;
        if matches!(source.kind.as_str(),"handler"|"binding"){
            compiler.append("\nend\n",None,Origin::Synthetic,source.compiler.segments.last().expect("wrapped source footer").anchor).map_err(invalid)?;
        }
        scripts.push(Script{key:format!("{}/__inline_{index}.lua",program.path),path:program.path.clone(),handler:source.tag.clone(),kind:source.kind.clone(),
            xml_sha256:program.raw_sha256.clone(),compiler,element:source.element,body,raw:raw_text.clone()});
    }
    Ok(scripts)
}
pub type CompilerInputs=(BTreeMap<String,Vec<u8>>,BTreeMap<String,Script>);
pub fn inputs(documents:&[crate::closure::Document])->Result<CompilerInputs>{
    let mut files=BTreeMap::new();let mut scripts=BTreeMap::new();
    for document in documents{
        if document.path.to_ascii_lowercase().ends_with(".lua"){files.insert(document.path.clone(),document.bytes.clone());}
        else if document.path.to_ascii_lowercase().ends_with(".xml"){
            for script in extract(&document.path,&document.bytes)?{files.insert(script.key.clone(),script.source_bytes().to_vec());scripts.insert(script.key.clone(),script);}
        }
    }
    Ok((files,scripts))
}
pub fn remap(result:&mut Value,scripts:&BTreeMap<String,Script>)->Result<()>{
    if let Some(rows)=result["diagnostics"].as_array_mut(){for row in rows{
        if let Some(script)=row["file"].as_str().and_then(|path|scripts.get(path)){
            let compiled=row["line"].as_u64().unwrap_or(1)as usize;
            let mapped=script.compiler_line(compiled)?;
            row["file"]=json!(script.path);row["line"]=mapped["line"].clone();row["sourceMap"]=mapped["sourceMap"].clone();
            row["inlineHandler"]=json!(script.handler);row["compiledLine"]=json!(compiled);row["xmlContentHash"]=json!(script.xml_sha256);
            row["generatedWrapper"]=json!("syntax_only_function(self, ...)");
        }
    }}
    result["checkedInlineScripts"]=json!(scripts.len());Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn xml_entities_cdata_and_logical_lines_keep_original_line_map(){
        let raw=b"<Ui>\n<Frame><Scripts><OnLoad>\nlocal x = 1 &lt; 2\n<![CDATA[local y = ...]]>&#10;local z = &quot;text&quot;\n</OnLoad></Scripts></Frame></Ui>";
        let scripts=extract("UI CJK.xml",raw).unwrap();
        assert_eq!(scripts.len(),1);
        assert_eq!(scripts[0].source_bytes().as_ptr(),scripts[0].compiler.text.as_ptr());
        assert_eq!(scripts[0].mapping_manifest()["compilerSHA256"],storage::sha256(scripts[0].source_bytes()));
        assert_eq!(String::from_utf8_lossy(scripts[0].source_bytes()),"return function(self, ...)\n\nlocal x = 1 < 2\nlocal y = ...\nlocal z = \"text\"\n\nend\n");
        assert_eq!(scripts[0].compiler_line(4).unwrap()["line"],4);assert_eq!(scripts[0].compiler_line(5).unwrap()["line"],4);assert_eq!(scripts[0].compiler_line(6).unwrap()["line"],5);
        let footer=scripts[0].compiler.text.rfind("end\n").unwrap();
        let projected=scripts[0].project(Span::new(footer,footer+3)).unwrap();
        let closing=span(&projected["segments"][0]["anchor"]);
        assert_eq!(&raw[closing.start..closing.end],b"</OnLoad>");
        assert!(projected["segments"][0]["raw"].is_null());
        assert!(extract("bad.xml",b"<Ui><OnLoad>&unknown;</OnLoad></Ui>").is_err());
    }
    #[test]
    fn xml_split_fixture_preserves_entity_and_cdata_token_segments(){
        let raw=include_bytes!("../../../tests/rust-source-engine/xml-split.xml");
        let script=extract("split.xml",raw).unwrap().remove(0);
        let runs=script.compiler.text.match_indices("Run").take(2).map(|(at,_)|script.project(Span::new(at,at+3)).unwrap()).collect::<Vec<_>>();
        assert_eq!(runs.len(),2);
        assert_eq!(runs[0]["accuracy"],"covering");
        assert_eq!(runs[0]["segments"][1]["origin"],"Entity");
        let entity=span(&runs[0]["segments"][1]["raw"]);
        assert_eq!(&raw[entity.start..entity.end],b"&#110;");
        let segments=runs[1]["segments"].as_array().unwrap();
        assert_eq!(segments.len(),2);
        assert_eq!(segments.iter().map(|part|{let range=span(&part["raw"]);std::str::from_utf8(&raw[range.start..range.end]).unwrap()}).collect::<Vec<_>>(),vec!["R","un"]);
        assert_eq!(runs[1]["accuracy"],"covering");
    }
    #[test]
    fn xml_golden_fixture_keeps_bom_crlf_utf16_and_synthetic_origins_distinct(){
        let raw=include_bytes!("../../../tests/rust-source-engine/xml-golden-204.xml");
        assert!(raw.starts_with(b"\xef\xbb\xbf<Ui>\r\n"));
        let mut doubled_bom=b"\xef\xbb\xbf".to_vec();doubled_bom.extend_from_slice(raw);
        assert_eq!(extract("double-bom.xml",&doubled_bom).err().expect("second BOM is root text").message,"xml.text_outside_root");
        let scripts=extract("golden.xml",raw).unwrap();
        assert_eq!(scripts.len(),2);
        let script=&scripts[0];
        let emoji=script.compiler.text.find('🙂').unwrap();
        let mapped=script.position(Span::new(emoji,emoji+'🙂'.len_utf8())).unwrap();
        assert_eq!(mapped["sourceMap"]["accuracy"],"exact");
        let bytes=span(&mapped["sourceMap"]["covering"]);
        assert_eq!(&raw[bytes.start..bytes.end],"🙂".as_bytes());
        assert_eq!(mapped["range"]["end"]["character"].as_u64().unwrap()-mapped["range"]["start"]["character"].as_u64().unwrap(),2);
        assert!(script.compiler.segments.iter().any(|segment|segment.origin==Origin::NewlineNormalization&&segment.raw.is_some_and(|raw|raw.len()==2)));
        let synthetic=script.position(Span::new(0,WRAPPER.len()-1)).unwrap();
        assert_eq!(synthetic["positionAccuracy"],"synthetic-or-mixed");assert!(synthetic["range"].is_null());
        assert!(synthetic["sourceMap"]["segments"][0]["raw"].is_null());assert_eq!(synthetic["sourceMap"]["segments"][0]["origin"],"Synthetic");
        assert!(script.source_bytes().starts_with(b"return function(self, ...)"));
        assert_eq!(script.mapping_manifest()["implicitHandlerParameters"],"self:xml_object; remaining:unknown");
        assert_eq!(source_map::utf16_position("\u{feff}🙂\r\n界",3).unwrap(),json!({"line":0,"character":0}));
        assert_eq!(source_map::utf16_position("\u{feff}🙂\r\n界",7).unwrap(),json!({"line":0,"character":2}));
        assert_eq!(source_map::utf16_position("\u{feff}🙂\r\n界",9).unwrap(),json!({"line":1,"character":0}));
        assert_eq!(source_map::utf16_offset("\u{feff}🙂\r\n界",1,0).unwrap(),9);
        assert!(source_map::utf16_offset("\u{feff}🙂\r\n界",0,1).is_err());
    }
    #[test]
    fn xml_entity_cr_keeps_original_bytes_and_lua_lexer_line_order(){
        let raw=b"<Ui><Frame><Scripts><OnLoad>local a=1&#13;local b=2\r\nlocal c=3&#10;&#13;local d=4</OnLoad></Scripts></Frame></Ui>";
        let script=extract("cr.xml",raw).unwrap().remove(0);
        assert!(script.compiler.text.contains("local a=1\rlocal b=2\nlocal c=3\n\rlocal d=4"));
        let line=lua_line_span(&script.compiler.text,5).unwrap();
        assert!(script.compiler.text[line.start..line.end].starts_with("local d=4"));
        let projected=script.compiler_line(5).unwrap();
        assert_eq!(projected["line"],2);
        let newline=script.compiler.segments.iter().find(|segment|segment.origin==Origin::Entity&&&script.compiler.text[segment.output.start..segment.output.end]=="\r").unwrap();
        let raw_range=newline.raw.unwrap();assert_eq!(&raw[raw_range.start..raw_range.end],b"&#13;");
    }
}
