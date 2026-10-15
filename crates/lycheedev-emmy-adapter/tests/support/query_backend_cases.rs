//! Real concrete query fixtures; no substitute name/flow/call backend.
#![allow(dead_code)]
use emmylua_code_analysis::{EmmyLuaAnalysis, FileId, SemanticId};
use emmylua_parser::{LuaAstNode, LuaExpr};
use lycheedev_emmy_adapter::session::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Resolve(BindingSite),
    Call(BindingSite),
    Observation(BindingSite),
    Shape(ObjectViewKey),
}
#[derive(Clone)]
pub struct Provider {
    pub names: HashMap<BindingSite, String>,
    pub values: HashMap<BindingSite, BindingDecision<BoundValue>>,
    pub calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
    pub observations: HashMap<BindingSite, BindingDecision<CallObservation>>,
    pub shapes: HashMap<ObjectViewKey, BindingDecision<ObjectShape>>,
    pub events: Arc<Mutex<Vec<Event>>>,
}
impl Provider {
    pub fn isolated(&self) -> Self {
        Self {
            events: Arc::new(Mutex::new(vec![])),
            ..self.clone()
        }
    }
    pub fn named(&self, text: &str) -> BindingSite {
        let found: Vec<_> = self
            .names
            .iter()
            .filter(|(_, name)| name.as_str() == text)
            .collect();
        assert_eq!(found.len(), 1, "ambiguous fixture expression: {text}");
        *found[0].0
    }
    pub fn select(&mut self, text: &str, value: BindingDecision<BoundValue>) {
        self.values.insert(self.named(text), value);
    }
}
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        "owned-query-backend-oracle"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.events.lock().unwrap().push(Event::Resolve(site));
        self.values
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        self.events.lock().unwrap().push(Event::Call(site));
        self.calls
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        self.events.lock().unwrap().push(Event::Observation(site));
        self.observations
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn object_shape(&self, key: &ObjectViewKey) -> BindingDecision<ObjectShape> {
        self.events.lock().unwrap().push(Event::Shape(key.clone()));
        self.shapes
            .get(key)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
}
#[derive(Clone, Debug)]
pub struct Stop {
    pub ordinal: usize,
    pub token: Arc<()>,
}

pub fn add_names(analysis: &EmmyLuaAnalysis, file_id: FileId, provider: &mut Provider) {
    let model = analysis.semantic_model(file_id);
    for expression in model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
    {
        provider.names.insert(
            BindingSite {
                file_id,
                syntax: expression.get_syntax_id(),
            },
            expression.syntax().text().to_string(),
        );
    }
}
pub fn load(source: &str, query: &str) -> (EmmyLuaAnalysis, BindingSite, Provider) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("owned-query-backend-oracle");
    analysis.add_main_workspace(root.clone());
    let file =
        analysis.update_files_by_path(vec![(root.join("input.lua"), Some(source.into()))])[0];
    let mut provider = Provider {
        names: HashMap::new(),
        values: HashMap::new(),
        calls: HashMap::new(),
        observations: HashMap::new(),
        shapes: HashMap::new(),
        events: Arc::new(Mutex::new(vec![])),
    };
    add_names(&analysis, file, &mut provider);
    let query = provider.named(query);
    (analysis, query, provider)
}
pub fn declaration(analysis: &EmmyLuaAnalysis, file: FileId, name: &str) -> SemanticId {
    analysis
        .semantic_model(file)
        .file_facts()
        .unwrap()
        .decls
        .iter()
        .find(|decl| decl.name.as_str() == name)
        .unwrap()
        .id
        .clone()
}
pub fn absent(proof: &str) -> BindingDecision<BoundValue> {
    BindingDecision::KnownAbsent {
        proof: proof.into(),
    }
}
pub fn expression(origin: BindingSite, identity: &str) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: identity.into(),
        proof: format!("proof:{identity}"),
        value: SelectedValue::Expression(origin),
    })
}
pub fn session(provider: &Provider) -> Arc<OwnedBindingSession> {
    OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap()
}
pub fn rows(session: &OwnedBindingSession) -> Vec<BindingCoverage> {
    session.coverage_checked(|| Ok::<_, Stop>(())).unwrap()
}
