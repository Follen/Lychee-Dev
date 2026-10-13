//! Segmented byte origins shared by production XML inputs and the verified XML prototype.
use serde::Serialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;
const MAX_DECODED: usize = 16 << 20;
const MAX_SEGMENTS: usize = 200_000;
fn err<T>(message: &str) -> Result<T> { Err(message.to_owned()) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Span { pub start: usize, pub end: usize }
impl Span {
    pub fn new(start: usize, end: usize) -> Self { Self { start, end } }
    pub fn len(self) -> usize { self.end - self.start }
    pub fn is_empty(self) -> bool { self.start == self.end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Origin { Exact, Entity, NewlineNormalization, Synthetic }

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub output: Span,
    pub raw: Option<Span>,
    pub origin: Origin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Span>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MappedText {
    pub text: String, pub segments: Vec<Segment>,
    #[serde(skip)] max_bytes:usize,
    #[serde(skip)] max_segments:usize,
}
impl Default for MappedText {
    fn default()->Self{Self::with_limits(MAX_DECODED,MAX_SEGMENTS)}
}

impl MappedText {
    pub fn with_limits(max_bytes:usize,max_segments:usize)->Self{
        Self{text:String::new(),segments:Vec::new(),max_bytes,max_segments}
    }
    pub fn append(&mut self, text: &str, raw: Option<Span>, origin: Origin, anchor: Option<Span>) -> Result<()> {
        if text.is_empty() { return Ok(()); }
        if self.text.len().saturating_add(text.len()) > self.max_bytes {
            return err("xml.decoded_budget");
        }
        let output = Span::new(self.text.len(), self.text.len() + text.len());
        if let Some(last) = self.segments.last_mut()
            && origin == Origin::Exact && last.origin == Origin::Exact
                && last.output.end == output.start
                && last.raw.zip(raw).is_some_and(|(a, b)| a.end == b.start)
        {
            last.output.end = output.end;
            last.raw.as_mut().expect("checked raw origin").end = raw.expect("checked raw").end;
            self.text.push_str(text);
            return Ok(());
        }
        if self.segments.len() >= self.max_segments { return err("xml.segment_budget"); }
        self.segments.push(Segment { output, raw, origin, anchor });
        self.text.push_str(text);
        Ok(())
    }

    pub fn append_physical(&mut self, raw: &[u8], span: Span) -> Result<()> {
        let text = std::str::from_utf8(&raw[span.start..span.end]).map_err(|_| "xml.non_utf8")?;
        let mut at = 0;
        while let Some(found) = text[at..].find('\r') {
            let cr = at + found;
            self.append(&text[at..cr], Some(Span::new(span.start + at, span.start + cr)), Origin::Exact, None)?;
            let end = cr + if text.as_bytes().get(cr + 1) == Some(&b'\n') { 2 } else { 1 };
            self.append("\n", Some(Span::new(span.start + cr, span.start + end)), Origin::NewlineNormalization, None)?;
            at = end;
        }
        self.append(&text[at..], Some(Span::new(span.start + at, span.end)), Origin::Exact, None)
    }

    pub fn append_entity(&mut self, raw: &[u8], span: Span) -> Result<()> {
        let escaped = std::str::from_utf8(&raw[span.start..span.end]).map_err(|_| "xml.non_utf8")?;
        let decoded = quick_xml::escape::unescape(escaped).map_err(|e| format!("xml.entity: {e}"))?;
        if decoded == escaped || decoded.chars().count() != 1 || !decoded.chars().all(xml_char) {
            return err("xml.invalid_entity");
        }
        self.append(&decoded, Some(span), Origin::Entity, None)
    }

    /// Original XML range projection, also suitable for Emmy byte ranges.
    /// Multiple segments are retained: a covering range is not an exact token range.
    pub fn project(&self, requested: Span) -> Result<Value> {
        if requested.start >= requested.end || requested.end > self.text.len()
            || !self.text.is_char_boundary(requested.start) || !self.text.is_char_boundary(requested.end)
        { return err("xml.invalid_projection_range"); }
        let mut spans = Vec::new();
        let mut min = usize::MAX;
        let mut max = 0;
        let mut synthetic = false;
        let mut all_exact = true;
        let mut covered = 0;
        let first = self.segments.partition_point(|segment| segment.output.end <= requested.start);
        for segment in self.segments[first..].iter().take_while(|segment| segment.output.start < requested.end) {
            let start = requested.start.max(segment.output.start);
            let end = requested.end.min(segment.output.end);
            if start >= end { continue; }
            covered += end - start;
            let projected = segment.raw.map(|raw| {
                if segment.origin == Origin::Exact {
                    Span::new(raw.start + start - segment.output.start, raw.start + end - segment.output.start)
                } else { raw }
            });
            if let Some(raw) = projected { min = min.min(raw.start); max = max.max(raw.end); }
            synthetic |= segment.origin == Origin::Synthetic;
            all_exact &= segment.origin == Origin::Exact;
            spans.push(json!({"output":Span::new(start,end),"raw":projected,
                "origin":segment.origin,"anchor":segment.anchor}));
        }
        if covered != requested.len() { return err("xml.mapping_hole"); }
        let covering = (min != usize::MAX).then_some(Span::new(min, max));
        let accuracy = if synthetic { "synthetic_or_mixed" }
            else if all_exact && spans.len() == 1 { "exact" } else { "covering" };
        Ok(json!({"segments":spans,"covering":covering,"accuracy":accuracy}))
    }

    /// Zero-length parser locations retain point or synthetic-anchor provenance.
    pub fn project_point(&self, offset:usize)->Result<Value>{
        if offset>self.text.len() || !self.text.is_char_boundary(offset){return err("xml.invalid_projection_range");}
        let segment=self.segments.iter().find(|segment|segment.output.start<=offset&&offset<segment.output.end)
            .or_else(||self.segments.last().filter(|segment|segment.output.end==offset))
            .ok_or("xml.mapping_hole")?;
        let raw=segment.raw.map(|raw|if segment.origin==Origin::Exact{
            let position=raw.start+offset-segment.output.start;Span::new(position,position)
        }else{raw});
        Ok(json!({"segments":[{"output":Span::new(offset,offset),"raw":raw,"origin":segment.origin,"anchor":segment.anchor}],
            "covering":raw,"accuracy":match segment.origin{Origin::Exact=>"exact",Origin::Synthetic=>"synthetic_or_mixed",_=>"covering"}}))
    }

    /// Reverse relation; entity and normalized-newline positions map to the whole output segment.
    pub fn reverse(&self, raw: Span) -> Result<Vec<Span>> {
        if raw.start >= raw.end { return err("xml.invalid_raw_range"); }
        let mut output = Vec::new();
        for segment in &self.segments {
            let Some(origin) = segment.raw else { continue; };
            let start = raw.start.max(origin.start);
            let end = raw.end.min(origin.end);
            if start < end {
                output.push(if segment.origin == Origin::Exact {
                    Span::new(segment.output.start + start - origin.start, segment.output.start + end - origin.start)
                } else { segment.output });
            }
        }
        Ok(output)
    }

    pub fn extend(&mut self, other: &MappedText) -> Result<()> {
        for segment in &other.segments {
            self.append(&other.text[segment.output.start..segment.output.end], segment.raw, segment.origin, segment.anchor)?;
        }
        Ok(())
    }
}

pub fn xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r') || ch >= '\u{20}' && ch != '\u{fffe}' && ch != '\u{ffff}'
}

// read_event on an immutable slice borrows original event/attribute bytes.
// Verify containment and byte equality rather than guessing offsets by searching names.
pub fn borrowed_span(raw: &[u8], bytes: &[u8]) -> Result<Span> {
    let start = (bytes.as_ptr() as usize).checked_sub(raw.as_ptr() as usize).ok_or("xml.nonoriginal_event_bytes")?;
    let end = start.checked_add(bytes.len()).ok_or("xml.span_overflow")?;
    if raw.get(start..end) != Some(bytes) { return err("xml.nonoriginal_event_bytes"); }
    Ok(Span::new(start,end))
}

// quick-xml's slice reader removes the leading UTF-8 BOM without counting it
// in buffer_position. Borrowed event slices still address the original bytes.
// Route every numeric cursor through this one conversion; never adjust borrowed
// spans again and never guess Event::Empty from bytes at an unconverted cursor.
pub fn raw_cursor(raw_length:usize, prefix_bytes:usize, reader_position:u64)->Result<usize> {
    let position=usize::try_from(reader_position).map_err(|_|"xml.cursor_overflow")?
        .checked_add(prefix_bytes).ok_or("xml.cursor_overflow")?;
    if position>raw_length { return err("xml.cursor_outside_raw_input"); }
    Ok(position)
}

/// Editor positions use UTF16 code units; byte offsets always include the BOM.
/// A leading UTF8 BOM is an encoding marker and occupies no displayed column.
pub fn utf16_position(text:&str,offset:usize)->Result<Value>{
    let prefix=text.get(..offset).ok_or("xml.position_not_character_boundary")?;
    let mut line=0;let mut character=0;let mut previous_cr=false;
    for ch in prefix.strip_prefix('\u{feff}').unwrap_or(prefix).chars(){
        match ch{
            '\r'=>{line+=1;character=0;previous_cr=true;}
            '\n'=>{if !previous_cr{line+=1;}character=0;previous_cr=false;}
            _=>{character+=ch.len_utf16();previous_cr=false;}
        }
    }
    Ok(json!({"line":line,"character":character}))
}

pub fn utf16_range(text:&str,range:Span)->Result<Value>{
    Ok(json!({"start":utf16_position(text,range.start)?,"end":utf16_position(text,range.end)?}))
}

pub fn utf16_offset(text:&str,line:usize,character:usize)->Result<usize>{
    let prefix=if text.starts_with('\u{feff}'){3}else{0};
    let mut current_line=0;let mut column=0;let mut previous_cr=false;
    for (relative,ch) in text[prefix..].char_indices(){
        let offset=prefix+relative;
        if current_line==line&&column==character&&!(previous_cr&&ch=='\n'){return Ok(offset);}
        match ch{
            '\r'=>{current_line+=1;column=0;previous_cr=true;}
            '\n'=>{if !previous_cr{current_line+=1;}column=0;previous_cr=false;}
            _=>{column+=ch.len_utf16();previous_cr=false;}
        }
        if current_line>line || current_line==line&&column>character{return err("xml.utf16_position_outside_character_boundary");}
    }
    if current_line==line&&column==character{Ok(text.len())}else{err("xml.utf16_position_outside_input")}
}

/// Lua 5.1 consumes CRLF and LFCR as one newline, including entity-produced CR.
pub fn lua_line_span(text:&str,line:usize)->Result<Span>{
    lua_line_span_bytes(text.as_bytes(),line)
}
pub fn lua_line_span_bytes(bytes:&[u8],line:usize)->Result<Span>{
    if line==0{return err("xml.invalid_compiler_line");}
    let mut start=0;let mut current=1;
    let next=|start:usize|->Option<usize>{
        let newline=start+bytes[start..].iter().position(|byte|matches!(*byte,b'\r'|b'\n'))?;
        let mut end=newline+1;
        if bytes.get(end).is_some_and(|byte|matches!(*byte,b'\r'|b'\n')&&*byte!=bytes[newline]){end+=1;}
        Some(end)
    };
    while current<line{start=next(start).ok_or("xml.compiler_line_outside_input")?;current+=1;}
    Ok(Span::new(start,next(start).unwrap_or(bytes.len())))
}

