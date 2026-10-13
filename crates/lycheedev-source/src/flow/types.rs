use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct RuleIdentity {pub client:String,pub api_commit:String,pub metadata_digest:String,pub version:String}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct SlotRule {pub state:String,#[serde(default,skip_serializing_if="String::is_empty")]pub condition:String}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct ArgumentRule {pub secret:String,#[serde(default,skip_serializing_if="String::is_empty")]pub condition:String}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct CallRule {pub name:String,pub path:String,pub line:usize,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub returns:BTreeMap<usize,SlotRule>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub arguments:BTreeMap<usize,ArgumentRule>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub raw:BTreeMap<String,Value>}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct RuleSet {pub identity:RuleIdentity,pub calls:BTreeMap<String,CallRule>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub events:BTreeMap<String,CallRule>,#[serde(default,skip_serializing_if="Vec::is_empty")]pub issues:Vec<Boundary>}
#[derive(Clone,Debug,Default,Serialize,Deserialize)]
pub struct Target {#[serde(default,skip_serializing_if="String::is_empty")]pub path:String,#[serde(default,skip_serializing_if="is_zero")]pub line:usize,#[serde(default,skip_serializing_if="String::is_empty")]pub symbol:String}
fn is_zero(value:&usize)->bool{*value==0}
#[derive(Clone,Debug)]
pub struct Budget {pub max_files:usize,pub max_functions:usize,pub max_depth:usize,pub max_paths:usize,pub max_millis:u64,pub max_output_bytes:usize}
impl Default for Budget {fn default()->Self{Self{max_files:32,max_functions:512,max_depth:4,max_paths:128,max_millis:2000,max_output_bytes:1<<20}}}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct Step {pub path:String,pub line:usize,pub kind:String,#[serde(default,skip_serializing_if="String::is_empty")]pub name:String,#[serde(default,skip_serializing_if="String::is_empty")]pub detail:String,#[serde(default,skip_serializing_if="String::is_empty")]pub condition:String,#[serde(default,skip_serializing_if="String::is_empty")]pub state:String}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct Finding {pub state:String,pub source:String,#[serde(rename="use")]pub use_name:String,pub argument:usize,#[serde(default,skip_serializing_if="String::is_empty")]pub rule_path:String,#[serde(default,skip_serializing_if="is_zero")]pub rule_line:usize,#[serde(default,skip_serializing_if="String::is_empty")]pub condition:String,pub steps:Vec<Step>}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct Boundary {#[serde(default,skip_serializing_if="String::is_empty")]pub path:String,#[serde(default,skip_serializing_if="is_zero")]pub line:usize,pub code:String,#[serde(default,skip_serializing_if="String::is_empty")]pub detail:String}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct Coverage {pub state:String,pub files:usize,pub functions:usize,pub paths:usize,pub complete:bool}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct Analysis {pub rule_identity:RuleIdentity,pub findings:Vec<Finding>,pub boundaries:Vec<Boundary>,pub coverage:Coverage,pub truncated:bool}
