//! Request-local ownership of repeated analysis evidence. Full public JSON is
//! expanded only after scope graphs have been released, never in References.
use super::*;
use serde::ser::SerializeMap;

// Direction-specific metadata is small. The potentially large immutable
// control tree is shared even when rows exceed projection retention, when both
// directions need different metadata, or when budget replay takes the old path.
#[derive(Clone)]
pub(crate) struct ResearchAnalysis {
    metadata:Value,
    graph_control:Arc<Value>,
}
impl ResearchAnalysis {
    pub(super) fn new(metadata:Value,graph_control:Arc<Value>)->Self {Self{metadata,graph_control}}
    fn into_value(self)->Value {
        let mut metadata=self.metadata;
        metadata["graphControl"]=Arc::try_unwrap(self.graph_control).unwrap_or_else(|shared|shared.as_ref().clone());
        metadata
    }
    #[cfg(test)]
    pub(crate) fn control_identity(&self)->&Arc<Value> {&self.graph_control}
}
impl std::ops::Index<&str> for ResearchAnalysis {
    type Output=Value;
    fn index(&self,key:&str)->&Value {
        if key=="graphControl" {&self.graph_control} else {&self.metadata[key]}
    }
}
impl Serialize for ResearchAnalysis {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
        // Reproduce Value's canonical key order without materializing or
        // copying the large control subtree. Retention still counts every byte.
        let fields=self.metadata.as_object().expect("research analysis object");
        let mut map=serializer.serialize_map(Some(fields.len()+1))?;
        let mut control_pending=true;
        for (key,value) in fields {
            if control_pending&&key.as_str()>"graphControl" {
                map.serialize_entry("graphControl",self.graph_control.as_ref())?;
                control_pending=false;
            }
            map.serialize_entry(key,value)?;
        }
        if control_pending {map.serialize_entry("graphControl",self.graph_control.as_ref())?;}
        map.end()
    }
}
fn shared_analysis<S: serde::Serializer>(value:&Arc<ResearchAnalysis>,serializer:S)->std::result::Result<S::Ok,S::Error> {
    value.as_ref().serialize(serializer)
}
pub(crate) fn owned_analysis(value:Arc<ResearchAnalysis>)->Value {
    Arc::try_unwrap(value).unwrap_or_else(|shared|shared.as_ref().clone()).into_value()
}

// Field order matches the former canonical Value map. The existing retention
// counter sees exactly the same complete analysis and rows, including controls.
#[derive(Serialize)]
pub(crate) struct ResearchProjectionResult {
    #[serde(serialize_with="shared_analysis")]
    pub(crate) analysis:Arc<ResearchAnalysis>,
    pub(crate) relations:Vec<Value>,
}
impl ResearchProjectionResult {
    pub(super) fn into_context_rows(self)->Value {
        // ContextFiles never retains analysis; the operation answer owns its
        // shared reference. Do not expand a JSON copy just to discard it.
        super::research_use_index::assemble_projection(self.relations,Value::Null)
    }
}

#[derive(Clone)]
pub(crate) struct ResearchOperationAnswer {
    value:Value,
    analysis:Option<Arc<ResearchAnalysis>>,
}
impl ResearchOperationAnswer {
    pub(super) fn new(value:Value,analysis:Option<Arc<ResearchAnalysis>>)->Self {Self{value,analysis}}
    #[cfg(test)]
    pub(crate) fn analysis_control(&self)->Option<&Arc<Value>> {self.analysis.as_ref().map(|analysis|analysis.control_identity())}
}
impl From<Value> for ResearchOperationAnswer {
    fn from(value:Value)->Self {Self::new(value,None)}
}
impl std::ops::Deref for ResearchOperationAnswer {
    type Target=Value;
    fn deref(&self)->&Value {&self.value}
}
impl std::ops::DerefMut for ResearchOperationAnswer {
    fn deref_mut(&mut self)->&mut Value {&mut self.value}
}
impl Serialize for ResearchOperationAnswer {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
        let Some(analysis)=&self.analysis else{return self.value.serialize(serializer);};
        struct ReferenceResult<'a>{fields:&'a serde_json::Map<String,Value>,analysis:&'a ResearchAnalysis}
        impl Serialize for ReferenceResult<'_> {
            fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
                // The three established result fields are canonical by name.
                let mut map=serializer.serialize_map(Some(self.fields.len()+1))?;
                map.serialize_entry("analysis",self.analysis)?;
                for (key,value) in self.fields {map.serialize_entry(key,value)?;}
                map.end()
            }
        }
        let fields=self.value.as_object().expect("operation answer object");
        let mut map=serializer.serialize_map(Some(fields.len()))?;
        for (key,value) in fields {
            if key=="result" {
                map.serialize_entry(key,&ReferenceResult{fields:value.as_object().expect("references result object"),analysis})?;
            } else {map.serialize_entry(key,value)?;}
        }
        map.end()
    }
}

// Each completed scope shares its analysis with its References answer. The
// latest top-level analysis is another Arc, not a third full graphControl tree.
pub(crate) struct ResearchScopeEvidence {
    pub(crate) analysis:Arc<ResearchAnalysis>,
    pub(crate) scope:Value,
    pub(crate) xml_facts:Value,
}
impl ResearchScopeEvidence {
    pub(crate) fn into_value(self)->Value {
        let mut result=serde_json::Map::new();
        result.insert("analysis".into(),owned_analysis(self.analysis));
        result.insert("scope".into(),self.scope);
        result.insert("xmlFacts".into(),self.xml_facts);
        Value::Object(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_analysis(mut value:Value)->Arc<ResearchAnalysis> {
        let control=value.as_object_mut().unwrap().remove("graphControl").unwrap();
        Arc::new(ResearchAnalysis::new(value,Arc::new(control)))
    }

    #[test]
    fn references_share_analysis_and_serialize_every_original_field() {
        let analysis=fixture_analysis(json!({"state":"partial","graphControl":{"complete":false,"contexts":[null,{"path":"界😀\\\"\r\n","number":-0.0}],"unresolved":2}}));
        let rows=vec![json!({"target":"T","source":"A","site":{"start":3,"end":7}}),Value::Null];
        let projection=ResearchProjectionResult{analysis:Arc::clone(&analysis),relations:rows};
        let answer=ResearchOperationAnswer::new(json!({"kind":"references","state":"partial","result":{"relationsSHA256":digest(&projection.relations),"relationCount":projection.relations.len()},"evidence":"full","executed":true,"operationIdentity":"identity","batch":0,"operation":0}),Some(Arc::clone(&analysis)));
        assert!(Arc::ptr_eq(&projection.analysis,answer.analysis.as_ref().unwrap()));
        assert_eq!(Arc::strong_count(&analysis),3);
        let expected=json!({"kind":"references","state":"partial","result":{"relationsSHA256":hash(&serde_json::to_vec(&projection.relations).unwrap()),"relationCount":2,"analysis":analysis.as_ref()},"evidence":"full","executed":true,"operationIdentity":"identity","batch":0,"operation":0});
        assert_eq!(serde_json::to_vec(&answer).unwrap(),serde_json::to_vec(&expected).unwrap());
        let expected_projection=json!({"analysis":analysis.as_ref(),"relations":projection.relations});
        assert_eq!(serde_json::to_vec(&projection).unwrap(),serde_json::to_vec(&expected_projection).unwrap());
    }

    #[test]
    fn two_scopes_keep_distinct_shared_analysis_and_last_scope_top_level() {
        let mut scopes=Vec::new();let mut answers=Vec::new();let mut latest=None;
        for id in 0..2 {
            let analysis=fixture_analysis(json!({"state":if id==0{"complete"}else{"partial"},"graphControl":{"scope":id,"coverage":[id,id+1]}}));
            let answer=ResearchOperationAnswer::new(json!({"kind":"references","result":{"relationsSHA256":"digest","relationCount":id}}),Some(Arc::clone(&analysis)));
            let scope=ResearchScopeEvidence{analysis:Arc::clone(&analysis),scope:json!({"manifest":id}),xml_facts:json!({"original":id})};
            assert!(Arc::ptr_eq(&scope.analysis,answer.analysis.as_ref().unwrap()));
            latest=Some(analysis);scopes.push(scope);answers.push(answer);
        }
        assert_eq!(Arc::strong_count(&scopes[0].analysis),2);
        assert_eq!(Arc::strong_count(&scopes[1].analysis),3);
        let top=owned_analysis(latest.unwrap());
        let expanded=scopes.into_iter().map(ResearchScopeEvidence::into_value).collect::<Vec<_>>();
        let operations=serde_json::to_value(answers).unwrap();
        for id in 0..2 {
            assert_eq!(expanded[id]["analysis"]["graphControl"]["scope"],id);
            assert_eq!(operations[id]["result"]["analysis"],expanded[id]["analysis"]);
        }
        assert_eq!(top,expanded[1]["analysis"]);
        assert_ne!(top,expanded[0]["analysis"]);
    }
}
