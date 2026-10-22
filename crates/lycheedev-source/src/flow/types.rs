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
fn is_zero(value:&usize)->bool{*value==0}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq)]
pub struct Boundary {#[serde(default,skip_serializing_if="String::is_empty")]pub path:String,#[serde(default,skip_serializing_if="is_zero")]pub line:usize,pub code:String,#[serde(default,skip_serializing_if="String::is_empty")]pub detail:String}
