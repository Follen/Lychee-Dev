//! The IPC owner routes an authenticated response; it does not consume its
//! research tree. Keep the one bounded frame through join, cancellation and
//! retirement, so none of those paths creates or drops a recursive Value DOM.
use super::*;
use serde_json::value::RawValue;
use std::ops::Range;

#[derive(Deserialize)]
struct ResponseHead<'a> {
    #[serde(borrow)]
    value: Option<&'a RawValue>,
    #[serde(borrow)]
    error: Option<ErrorHead<'a>>,
    #[serde(default)]
    terminal: Option<analysis_process::AnalysisTerminal>,
}
#[derive(Deserialize)]
struct ErrorHead<'a> {
    code: String,
    message: String,
    exit_code: i32,
    #[serde(borrow)]
    details: &'a RawValue,
}
struct ErrorFields {
    code: String,
    message: String,
    exit_code: i32,
    details: Range<usize>,
}
pub(super) struct RawResponse {
    bytes: Vec<u8>,
    error: Option<ErrorFields>,
}
impl RawResponse {
    pub(super) fn decode(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len()>RESPONSE_BYTES {return Err(frame_limit());}
        let head:ResponseHead<'_>=serde_json::from_slice(&bytes)?;
        // Full grammar/tail and typed envelope decoding stays with pinned serde.
        // Ordinary business data stays borrowed. Only RawValue's private escape
        // hatch needs one local canonical pass to preserve the former owner
        // decode -> encode -> final CLI decode, including its error outcomes.
        let mut replacements=Vec::new();
        let mut validate=|raw:&RawValue,depth|->Result<()> {
            let start=raw.get().as_ptr() as usize-bytes.as_ptr() as usize;
            for range in response_validation::validate_value(raw.get().as_bytes(),depth)? {
                replacements.push(start+range.start..start+range.end);
            }
            Ok(())
        };
        if let Some(value)=head.value {validate(value,1)?;}
        let mut error=if let Some(error)=head.error {
            validate(error.details,2)?;
            let start=error.details.get().as_ptr() as usize-bytes.as_ptr() as usize;
            Some(ErrorFields{code:error.code,message:error.message,exit_code:error.exit_code,
                details:start..start+error.details.get().len()})
        } else {None};
        drop(head.terminal);
        let bytes=if replacements.is_empty(){bytes}else{
            // The ordinary path above does not allocate a second frame. Private
            // marker replacement alone has a second, independently bounded one.
            replacements.sort_by_key(|range|range.start);
            let mut output=Vec::new();let mut cursor=0;let mut shifts=Vec::new();
            for range in &replacements {
                append_bounded(&mut output,&bytes[cursor..range.start])?;
                let marker:BTreeMap<String,&RawValue>=serde_json::from_slice(&bytes[range.clone()])?;
                let text:String=serde_json::from_str(marker["$serde_json::private::RawValue"].get())?;
                let canonical=private_raw_value::canonical_value(text)?;
                append_bounded(&mut output,&canonical)?;
                shifts.push((range.end,canonical.len() as isize-range.len() as isize));
                cursor=range.end;
            }
            append_bounded(&mut output,&bytes[cursor..])?;
            if let Some(error)=&mut error {
                let translate=|offset:usize|->usize{(offset as isize+shifts.iter().filter(|(end,_)|*end<=offset).map(|(_,shift)|shift).sum::<isize>()) as usize};
                error.details=translate(error.details.start)..translate(error.details.end);
            }
            output
        };
        Ok(Self{bytes,error})
    }
    pub(super) fn failed(&self)->bool {self.error.is_some()}
    fn error_reply(&self,retirement:Option<&ToolkitError>,terminal:Option<&analysis_process::AnalysisTerminal>,writer:&mut dyn Write)->Result<()> {
        let error=self.error.as_ref().expect("child error routing");
        let raw_details:&RawValue=serde_json::from_slice(&self.bytes[error.details.clone()])?;
        let normalized=normalized_root(raw_details)?;
        let details:&RawValue=serde_json::from_str(&normalized)?;
        #[derive(Serialize)]
        struct ErrorReply<'a> {value:Option<()>,error:RoutedError<'a>,#[serde(skip_serializing_if="Option::is_none")]terminal:Option<&'a analysis_process::AnalysisTerminal>}
        #[derive(Serialize)]
        struct RoutedError<'a> {code:&'a str,message:&'a str,exit_code:i32,details:RetirementDetails<'a>}
        struct RetirementDetails<'a>{original:&'a RawValue,retirement:Option<&'a ToolkitError>}
        impl Serialize for RetirementDetails<'_> {
            fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{
                use serde::ser::SerializeMap;
                let Some(retirement)=self.retirement else{return self.original.serialize(serializer);};
                let mut fields=if self.original.get().starts_with('{') {
                    serde_json::from_str::<BTreeMap<String,&RawValue>>(self.original.get()).map_err(serde::ser::Error::custom)?
                } else {BTreeMap::from([("originalDetails".into(),self.original)])};
                // Preserve the original Value map's last-field replacement and
                // canonical order, without decoding any nested details value.
                fields.remove("analysisRetirementError");
                let mut map=serializer.serialize_map(Some(fields.len()+1))?;
                let mut pending=true;
                for (key,value) in fields {
                    if pending&&key.as_str()>"analysisRetirementError"{map.serialize_entry("analysisRetirementError",retirement)?;pending=false;}
                    map.serialize_entry(&key,value)?;
                }
                if pending{map.serialize_entry("analysisRetirementError",retirement)?;}
                map.end()
            }
        }
        write_frame(writer,&ErrorReply{value:None,error:RoutedError{code:&error.code,message:&error.message,exit_code:error.exit_code,
            details:RetirementDetails{original:details,retirement}},terminal},RESPONSE_BYTES)
    }
}
fn append_bounded(output:&mut Vec<u8>,bytes:&[u8])->Result<()> {
    if output.len().saturating_add(bytes.len())>RESPONSE_BYTES{return Err(frame_limit());}
    output.extend_from_slice(bytes);Ok(())
}
fn frame_limit()->ToolkitError {ToolkitError::new("source.native_frame_limit","Source frame exceeds budget",3)}

pub(super) enum OwnerReply {
    Forward(RawResponse),
    ChildError {response:RawResponse,retirement:Option<ToolkitError>,terminal:Option<analysis_process::AnalysisTerminal>},
    Local(Response),
}
impl OwnerReply {
    pub(super) fn write(&self,writer:&mut dyn Write)->Result<()> {
        match self {
            Self::Forward(response)=>write_raw_frame(writer,&response.bytes),
            Self::ChildError{response,retirement,terminal}=>response.error_reply(retirement.as_ref(),terminal.as_ref(),writer),
            Self::Local(response)=>write_frame(writer,response,RESPONSE_BYTES),
        }
    }
}

// RawValue wrappers were normalized at admission. A remaining original root
// private Number is a scalar, not a map into which retirement fields may merge.
// Multi-member objects produced by the one-pass normalization remain objects;
// interpreting their newly sorted first key again would change the old result.
fn normalized_root(raw:&RawValue)->Result<std::borrow::Cow<'_,str>> {
    struct FirstField<'a>(Option<(String,&'a RawValue)>);
    impl<'de> Deserialize<'de> for FirstField<'de> {
        fn deserialize<D:serde::Deserializer<'de>>(de:D)->std::result::Result<Self,D::Error>{
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value=FirstField<'de>;
                fn expecting(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("object")}
                fn visit_map<M:serde::de::MapAccess<'de>>(self,mut map:M)->std::result::Result<Self::Value,M::Error>{
                    let first=map.next_entry::<String,&'de RawValue>()?;
                    let mut additional=false;
                    while map.next_entry::<serde::de::IgnoredAny,serde::de::IgnoredAny>()?.is_some(){additional=true;}
                    Ok(FirstField(if additional{None}else{first}))
                }
            }
            de.deserialize_map(Visitor)
        }
    }
    if !raw.get().starts_with('{'){return Ok(std::borrow::Cow::Borrowed(raw.get()));}
    let first:FirstField<'_>=serde_json::from_str(raw.get())?;
    if let Some((key,value))=first.0 && key=="$serde_json::private::Number" {
        let token:String=serde_json::from_str(value.get())?;
        let number:serde_json::Number=token.parse()?;
        Ok(std::borrow::Cow::Owned(number.to_string()))
    } else {Ok(std::borrow::Cow::Borrowed(raw.get()))}
}

#[cfg(test)]
#[path="raw_response_tests.rs"]
mod tests;
