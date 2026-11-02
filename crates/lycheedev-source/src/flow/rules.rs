use super::types::*;
use crate::environment::Environment;
use lycheedev_core::{Result,ToolkitError,control::OperationControl};
use std::collections::{BTreeMap,BTreeSet};
fn condition(raw:&BTreeMap<String,serde_json::Value>,keys:&[&str])->String{keys.iter().filter_map(|key|raw.get(*key).map(|value|format!("{key}={}",value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string())))).collect::<Vec<_>>().join("; ")}
pub fn from_facts(identity:RuleIdentity,environment:&Environment,control:&OperationControl)->Result<RuleSet>{
    control.check()?;
    if identity.client.is_empty()||identity.api_commit.is_empty()||identity.metadata_digest.is_empty()||identity.version.is_empty()||identity.metadata_digest!=environment.manifest.input_sha256||identity.client!=environment.manifest.identity.client||identity.api_commit!=environment.manifest.identity.commit{return Err(ToolkitError::new("flow.invalid_input","flow.invalid_input: rule identity or environment manifest",4));}
    let mut rules=RuleSet{identity,..Default::default()};
    for gap in &environment.coverage.gaps{if !["unknown_type","may_return_nothing"].contains(&gap.code.as_str()){rules.issues.push(Boundary{path:gap.path.clone(),line:gap.line,code:format!("metadata_{}",gap.code),detail:gap.detail.clone()});}}
    let(mut ambiguous_calls,mut ambiguous_events)=(BTreeSet::new(),BTreeSet::new());
    for record in &environment.facts{
        control.check()?;
        if !["function","event"].contains(&record.kind.as_str())||record.owner.is_some(){continue;}
        if record.name.is_empty(){rules.issues.push(Boundary{path:record.path.clone(),line:record.line,code:"metadata_unnamed_callable".into(),detail:String::new()});continue;}
        let mut rule=CallRule{name:record.name.clone(),path:record.path.clone(),line:record.line,..Default::default()};
        rule.raw=record.raw.iter().filter(|(key,_)|key.contains("Secret")||key.contains("Restricted")||*key=="IsProtectedFunction").map(|(key,value)|(key.clone(),value.clone())).collect();
        let (returns,arguments)=if record.kind=="event"{(&record.payload,&record.payload)}else{(&record.returns,&record.parameters)};
        let return_condition=condition(&rule.raw,&["SecretReturns","SecretReturnsForAspect","SecretPayloads"]);
        for slot in returns{
            if slot.index==0{continue;}
            let mut state="unknown";let mut slot_condition=return_condition.clone();
            if slot.raw.get("NeverSecret")==Some(&serde_json::Value::Bool(true)){state="never";}else if !return_condition.is_empty()||slot.raw.get("ConditionalSecret")==Some(&serde_json::Value::Bool(true)){state="possible";}
            if slot.raw.get("ConditionalSecret")==Some(&serde_json::Value::Bool(true)){slot_condition=format!("{slot_condition}; ConditionalSecret").trim().trim_start_matches("; ").to_owned();}
            if state!="unknown"{rule.returns.insert(slot.index,SlotRule{state:state.into(),condition:slot_condition});}
        }
        let argument_condition=condition(&rule.raw,&["SecretArguments","SecretArgumentsAddAspect","IsPreventingSecretValues"]);
        if !argument_condition.is_empty(){let secret=if matches!(rule.raw.get("SecretArguments").and_then(|value|value.as_str()),Some("AllowedWhenTainted"|"AllowedWhenUntainted")){"conditional"}else{"unknown"};for slot in arguments{if slot.index>0{rule.arguments.insert(slot.index,ArgumentRule{secret:secret.into(),condition:argument_condition.clone()});}}}
        if rule.returns.is_empty()&&rule.arguments.is_empty()&&rule.raw.is_empty(){continue;}
        let (destination,ambiguous)=if record.kind=="event"{(&mut rules.events,&mut ambiguous_events)}else{(&mut rules.calls,&mut ambiguous_calls)};
        if ambiguous.contains(&rule.name){continue;}
        if let Some(previous)=destination.remove(&rule.name){rules.issues.push(Boundary{path:record.path.clone(),line:record.line,code:"duplicate_rule".into(),detail:format!("also declared in {}",previous.path)});ambiguous.insert(rule.name);continue;}
        destination.insert(rule.name.clone(),rule);
    }
    if !rules.events.is_empty(){rules.issues.push(Boundary{code:"event_payload_mapping_unavailable".into(),detail:"event payload metadata is retained separately; handler registration and payload routing are outside this flow scope".into(),..Default::default()});}
    Ok(rules)
}
