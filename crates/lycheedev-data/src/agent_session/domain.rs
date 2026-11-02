//! Native Arrow-to-domain values: no JSON roundtrip or public-ID reconstruction.
use crate::{fault,resource::{Budget,Cost},table::Value};
use arrow_array::*;
use lycheedev_core::Result;
pub(super) fn convert_array(array:&dyn Array,index:usize,budget:&Budget,depth:usize)->Result<Value> {
    budget.check()?;
    if depth>32 || index>=array.len() { return Err(fault("data.domain.value",3,"Invalid domain value depth or index")); }
    if array.is_null(index) { return Ok(Value::Null); }
    budget.charge(Cost::retained(64))?;
    macro_rules! number { ($t:ty,$variant:ident,$cast:ty)=>{if let Some(a)=array.as_any().downcast_ref::<$t>() { return Ok(Value::$variant(a.value(index) as $cast)); }}; }
    number!(Int8Array,Int,i64);number!(Int16Array,Int,i64);number!(Int32Array,Int,i64);number!(Int64Array,Int,i64);
    number!(UInt8Array,UInt,u64);number!(UInt16Array,UInt,u64);number!(UInt32Array,UInt,u64);number!(UInt64Array,UInt,u64);
    number!(Float32Array,Float32,f32);number!(Float64Array,Float,f64);
    macro_rules! text { ($t:ty)=>{if let Some(a)=array.as_any().downcast_ref::<$t>() {let s=a.value(index);budget.charge(Cost::retained(s.len()))?;return Ok(Value::String(s.into()));}}; }
    text!(StringArray);text!(LargeStringArray);text!(StringViewArray);
    let child:Option<ArrayRef>=if let Some(a)=array.as_any().downcast_ref::<ListArray>() {Some(a.value(index))}else if let Some(a)=array.as_any().downcast_ref::<LargeListArray>() {Some(a.value(index))}else if let Some(a)=array.as_any().downcast_ref::<FixedSizeListArray>() {Some(a.value(index))}else{None};
    if let Some(child)=child {
        budget.charge(Cost::retained(child.len().checked_mul(std::mem::size_of::<Value>()).ok_or_else(||fault("records.query_resource_budget",3,"Domain array admission overflow"))?))?;
        let mut values=Vec::with_capacity(child.len());for i in 0..child.len(){values.push(convert_array(child.as_ref(),i,budget,depth+1)?);}return Ok(Value::Array(values));
    }
    Err(fault("data.domain.value",3,"Unsupported native domain column type"))
}
pub(super) fn validate_captures(mode:&crate::engine::SourceMode,captures:&[String])->Result<()> {
    use crate::engine::SourceMode;
    if captures.len()>16 || captures.iter().any(|s|s.is_empty()||s.len()>256||s.chars().any(char::is_control)) || captures.iter().enumerate().any(|(i,s)|captures[..i].contains(s)) || matches!(mode,SourceMode::Static)&&!captures.is_empty() || matches!(mode,SourceMode::Effective)&&captures.is_empty() {return Err(fault("data.domain.source_mode",2,"Static requires no captures; effective requires 1..16 distinct explicit captures"));}Ok(())
}
