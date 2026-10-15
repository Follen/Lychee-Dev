//! A deliberately partial test backend. It proves executor scheduling/carrier
//! transport, never general name/member/flow/call/binary inference equivalence.
#![allow(dead_code)]
use emmylua_code_analysis::{
    EmmyLuaAnalysis, FileId, LuaMemberKey, LuaType, SemanticId, VariadicType,
};
use emmylua_parser::{BinaryOperator, LuaAstNode, LuaExpr, LuaSyntaxId};
use lycheedev_emmy_adapter::session::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub struct Marker {
    pub identity: String,
    pub epoch: String,
    pub token: Arc<()>,
}

// Intentionally not Clone: the executor must move the actual carrier.
#[derive(Debug)]
pub struct Value {
    pub ty: LuaType,
    pub marker: Option<Arc<Marker>>,
    pub slots: Vec<Value>,
}
impl Value {
    fn plain(ty: LuaType) -> Self {
        Self {
            ty,
            marker: None,
            slots: vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Stop {
    pub ordinal: usize,
    pub token: Arc<()>,
}

#[derive(Clone)]
pub struct Provider {
    pub names: HashMap<BindingSite, String>,
    pub values: HashMap<BindingSite, BindingDecision<BoundValue>>,
    pub events: Arc<Mutex<Vec<String>>>,
}
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        "expression-execution-oracle"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.events.lock().unwrap().push(format!(
            "resolve:{}",
            self.names
                .get(&site)
                .map(String::as_str)
                .unwrap_or("missing")
        ));
        self.values
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
}

pub struct Backend {
    pub events: Arc<Mutex<Vec<String>>>,
    pub checkpoints: usize,
    pub stop_at: usize,
    pub token: Arc<()>,
    pub name_types: HashMap<String, LuaType>,
    pub name_markers: HashMap<String, Arc<Marker>>,
    pub flow_types: HashMap<String, LuaType>,
    pub flow_markers: HashMap<String, Arc<Marker>>,
    pub nullable: bool,
    pub recursive_initializers: bool,
    pub last_arguments: Vec<ExpressionValue<Value>>,
    pub last_callee: Option<ExpressionValue<Value>>,
}
impl Backend {
    pub fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            checkpoints: 0,
            stop_at: usize::MAX,
            token: Arc::new(()),
            name_types: HashMap::new(),
            name_markers: HashMap::new(),
            flow_types: HashMap::new(),
            flow_markers: HashMap::new(),
            nullable: false,
            recursive_initializers: false,
            last_arguments: vec![],
            last_callee: None,
        }
    }
    fn event(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
    fn text(projector: &SelectedProjector<'_>, site: BindingSite) -> String {
        let model = projector.analysis().semantic_model(site.file_id);
        let tree = model.syntax_tree().unwrap();
        site.syntax
            .to_node_from_root(&tree.get_red_root())
            .unwrap()
            .text()
            .to_string()
    }
}
impl SelectedProjectionBackend for Backend {
    type Value = Value;
    type Error = Stop;
    fn checkpoint(&mut self) -> Result<(), Stop> {
        self.checkpoints += 1;
        if self.checkpoints == self.stop_at {
            Err(Stop {
                ordinal: self.checkpoints,
                token: self.token.clone(),
            })
        } else {
            Ok(())
        }
    }
    fn plain(&mut self, ty: LuaType) -> Value {
        Value::plain(ty)
    }
    fn expression(
        &mut self,
        projector: &SelectedProjector<'_>,
        _: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<Value, Stop> {
        Ok(Ok(projector.evaluate_expression(origin, self)?))
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<Value, Stop> {
        panic!("parameter backend is not implemented by this test fixture")
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<Value, Stop> {
        panic!("function backend is not implemented by this test fixture")
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        object: &ObjectRef,
    ) -> ProjectionResult<Value, Stop> {
        // Carrier transport only: this deliberately does not claim shape validation.
        Ok(Ok(Value {
            ty: LuaType::Table,
            marker: Some(Arc::new(Marker {
                identity: object.identity.clone(),
                epoch: object.epoch.clone(),
                token: self.token.clone(),
            })),
            slots: vec![],
        }))
    }
}
impl SelectedExpressionBackend for Backend {
    fn result_pack(&mut self, slots: Vec<Value>) -> Value {
        Value {
            ty: LuaType::Variadic(Arc::new(VariadicType::Multi(
                slots.iter().map(|value| value.ty.clone()).collect(),
            ))),
            marker: None,
            slots,
        }
    }
    fn scalar_result(&mut self, mut value: Value) -> Value {
        if matches!(value.ty, LuaType::Variadic(_)) {
            if value.slots.is_empty() {
                Value::plain(value.ty.get_result_slot_type(0).unwrap_or(LuaType::Nil))
            } else {
                value.slots.remove(0)
            }
        } else {
            value
        }
    }
}
impl ExpressionExecutionBackend for Backend {
    fn value_type<'a>(&self, value: &'a Value) -> &'a LuaType {
        &value.ty
    }
    fn same_value_type(&self, left: &Value, right: &Value) -> bool {
        left.ty == right.ty
            && match (&left.marker, &right.marker) {
                (None, None) => true,
                (Some(left), Some(right)) => {
                    Arc::ptr_eq(&left.token, &right.token)
                        && left.identity == right.identity
                        && left.epoch == right.epoch
                }
                _ => false,
            }
    }
    fn load_name(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        name: &str,
        offset: u32,
    ) -> Result<ExpressionValue<Value>, Stop> {
        self.event(format!("load:{name}"));
        let model = projector.analysis().semantic_model(file_id);
        let owner = model
            .resolve_name(offset.into())
            .unwrap_or_else(|| SemanticId::name(name.into()));
        let mut value = if self.recursive_initializers {
            let syntax = model
                .file_facts()
                .and_then(|facts| facts.decl_by_id(&owner))
                .and_then(|info| info.value_expr_syntax)
                .expect("fixture must have a single initializer");
            projector.evaluate_expression(BindingSite { file_id, syntax }, self)?
        } else {
            Value::plain(
                self.name_types
                    .get(name)
                    .cloned()
                    .unwrap_or(LuaType::Unknown),
            )
        };
        value.marker = self.name_markers.get(name).cloned().or(value.marker);
        Ok(ExpressionValue {
            value,
            owner: Some(owner),
            closure_syntax: None,
            receiver: Some(Value::plain(LuaType::IntegerConst(11))),
        })
    }
    fn index_member(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<Value>,
        key: &LuaMemberKey,
    ) -> Result<ExpressionValue<Value>, Stop> {
        self.event(format!("index:{key:?}"));
        Ok(owner)
    }
    fn index_dynamic(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<Value>,
        _: ExpressionValue<Value>,
    ) -> Result<ExpressionValue<Value>, Stop> {
        self.event("index:dynamic".into());
        Ok(owner)
    }
    fn expression_at(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        offset: u32,
    ) -> Result<Value, Stop> {
        assert_eq!(offset, u32::from(site.syntax.get_range().start()));
        let text = Self::text(projector, site);
        self.event(format!("flow:{text}"));
        Ok(Value {
            ty: self
                .flow_types
                .get(&text)
                .cloned()
                .unwrap_or(LuaType::Unknown),
            marker: self.flow_markers.get(&text).cloned(),
            slots: vec![],
        })
    }
    fn call(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        callee: ExpressionValue<Value>,
        arguments: Vec<ExpressionValue<Value>>,
        colon_call: bool,
        generics: &[LuaSyntaxId],
    ) -> Result<ExpressionValue<Value>, Stop> {
        self.event(format!(
            "call:{}:{colon_call}:{}",
            arguments.len(),
            generics.len()
        ));
        self.last_callee = Some(callee);
        self.last_arguments = arguments;
        Ok(ExpressionValue::plain(Value::plain(LuaType::IntegerConst(
            999,
        ))))
    }
    fn logical_nullable_extra(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> Result<Option<Value>, Stop> {
        self.event(format!("nullable:{}", Self::text(projector, site)));
        Ok(self.nullable.then(|| Value::plain(LuaType::Nil)))
    }
    fn merge_value(&mut self, left: Value, extra: Value) -> Result<Value, Stop> {
        self.event("merge".into());
        Ok(Value::plain(LuaType::from_vec(vec![left.ty, extra.ty])))
    }
    fn binary(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        op: BinaryOperator,
        left: Value,
        right: Value,
    ) -> Result<Value, Stop> {
        self.event(format!("binary:{op:?}"));
        // This oracle fixture covers only Nil+Nil -> Number, not a production
        // binary backend. Any other operands are intentionally not interpreted.
        assert!(
            matches!((left.ty, right.ty), (LuaType::Nil, LuaType::Nil)) || self.nullable,
            "fixture binary inputs outside the declared subset"
        );
        Ok(Value::plain(LuaType::Number))
    }
}

pub fn load(source: &str, query_text: &str) -> (EmmyLuaAnalysis, BindingSite, Provider) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("expression-execution-logical");
    analysis.add_main_workspace(root.clone());
    let file =
        analysis.update_files_by_path(vec![(root.join("input.lua"), Some(source.into()))])[0];
    let model = analysis.semantic_model(file);
    let tree = model.syntax_tree().unwrap();
    let names = tree
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .map(|expr| {
            (
                BindingSite {
                    file_id: file,
                    syntax: expr.get_syntax_id(),
                },
                expr.syntax().text().to_string(),
            )
        })
        .collect::<HashMap<_, _>>();
    let query = *names
        .iter()
        .find(|(_, text)| text.as_str() == query_text)
        .unwrap()
        .0;
    (
        analysis,
        query,
        Provider {
            names,
            values: HashMap::new(),
            events: Arc::new(Mutex::new(vec![])),
        },
    )
}

pub fn select(provider: &mut Provider, text: &str, decision: BindingDecision<BoundValue>) {
    let site = *provider
        .names
        .iter()
        .find(|(_, name)| name.as_str() == text)
        .unwrap()
        .0;
    provider.values.insert(site, decision);
}
pub fn absent(proof: &str) -> BindingDecision<BoundValue> {
    BindingDecision::KnownAbsent {
        proof: proof.into(),
    }
}
