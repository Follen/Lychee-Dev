//! Concrete, deliberately incomplete owned query backend.
//!
//! Completed paths use the same compiler/executor and per-model caches. Missing
//! capabilities leave through a typed continuation boundary, never a successful
//! Unknown or a raw contextual-inference fallback. This is not the production
//! semantic-model replacement yet. Selected objects retain owned identity and
//! nested carriers; unimplemented structural/member consumers remain explicit.
mod aliases;
mod call;
mod call_bindings;
mod call_builtins;
mod call_candidates;
mod call_inference;
mod call_match;
mod call_return;
mod call_unify;
mod callable;
mod cfg;
mod declaration;
mod environment;
mod flow;
mod index_general;
mod iteration;
mod logical;
mod member;
pub use member::OwnedMemberInfo;
mod member_general;
mod resolver;
pub use resolver::OwnedResolvedMember;
mod relation;
mod relation_assign;
mod relation_complex;
mod relation_function;
mod relation_generic;
mod relation_ref;
mod relation_simple;
mod signature;
mod structure;
mod transform;

use super::{
    BindingDecision, BindingSite, DenseTupleBackend, DenseTupleProjection,
    ExpressionExecutionBackend, ExpressionValue, FunctionRef, ObjectRef, OwnedBindingSession,
    OwnedValue, PrimitiveBinaryProjection, ProjectionResult, SelectedExpressionBackend,
    SelectedProjectionBackend, SelectedProjector,
};
use emmylua_code_analysis::{
    Decl, DeclKind, EmmyLuaAnalysis, FileId, LuaMemberKey, LuaTupleStatus, LuaType, SemanticId,
    SemanticModel,
};
use emmylua_parser::{BinaryOperator, LuaAstNode, LuaClosureExpr, LuaIndexExpr, LuaSyntaxId};
use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc, sync::Arc};

/// Unfinished branches are explicit and must be implemented on this backend.
/// These are development continuation labels, not product diagnostics/coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedQueryCapability {
    IntakeOrder,
    Parameter,
    Function,
    FunctionMetadata,
    FunctionReturnType,
    FunctionRawReturnFallback,
    CallSignature,
    CallReturnType,
    Object,
    SelectedTypeMerge,
    DeclarationMetadata,
    ClosureEnvironment,
    DeclarationFlow,
    GlobalGenericSanitization,
    DeclarationShell,
    UnresolvedName,
    IndexMember,
    IndexDynamic,
    MemberCallable,
    ExpressionFlow,
    Call,
    LogicalNullableMember,
    LogicalUnion,
    TypeRelation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedQueryContinuation {
    pub capability: OwnedQueryCapability,
    pub file_id: FileId,
    pub syntax: Option<LuaSyntaxId>,
    pub offset: Option<u32>,
    pub declaration: Option<SemanticId>,
}

#[derive(Debug)]
pub enum OwnedQueryFailure<E> {
    /// The exact caller error, without string conversion or a new deadline.
    Control(E),
    /// No successful semantic result and no synthetic completion receipt.
    Continue(OwnedQueryContinuation),
}

type QueryResult<T, E> = Result<T, OwnedQueryFailure<E>>;

#[derive(Clone)]
enum CacheEntry<T> {
    InProgress,
    Ready(T),
}
type Memo<K, V> = Rc<RefCell<HashMap<K, CacheEntry<V>>>>;

/// A stop, unsupported branch or unwind removes only this unfinished entry.
/// Completed older entries remain valid; query hits still check caller control.
struct PendingMemo<K: Eq + Hash, V> {
    memo: Memo<K, V>,
    key: Option<K>,
}
impl<K: Eq + Hash, V> PendingMemo<K, V> {
    fn commit(mut self, value: V) {
        self.memo
            .borrow_mut()
            .insert(self.key.take().unwrap(), CacheEntry::Ready(value));
    }
}
impl<K: Eq + Hash, V> Drop for PendingMemo<K, V> {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            self.memo.borrow_mut().remove(key);
        }
    }
}

/// One model scope, not a file-global cache. The provider/session and database
/// are borrowed once; selected Expression origins construct a fresh scope even
/// when their file equals this model's file, exactly like old model_for.
///
/// Selected Function signatures and restricted calls are implemented. Parameter
/// projection and general Function/flow/member/call inference remain incomplete.
/// Object identity and nested carriers
/// remain owned through the accepted expression, declaration and tuple paths.
/// This entry is not installed in Source or the diagnostic registry yet.
pub struct OwnedQuery<'a, 'control, E> {
    analysis: &'a EmmyLuaAnalysis,
    session: &'a OwnedBindingSession,
    model: SemanticModel<'a>,
    check: &'control mut dyn FnMut() -> Result<(), E>,
    contribution_order: Option<Arc<[FileId]>>,
    expressions: Memo<LuaSyntaxId, OwnedValue>,
    declarations: Memo<(FileId, SemanticId), Option<OwnedValue>>,
    members: Memo<(FileId, SemanticId), Option<OwnedValue>>,
    member_lists: HashMap<OwnedValue, Vec<OwnedMemberInfo>>,
    resolved_members: HashMap<(FileId, LuaSyntaxId), Option<OwnedResolvedMember>>,
    // This cache's owner is the immutable analysis/session borrow, not FileId
    // alone. Fresh semantic scopes share CFGs but never their inference caches.
    flow_trees: Rc<RefCell<HashMap<FileId, Arc<cfg::FlowTree>>>>,
    flow_declarations: HashMap<(FileId, SemanticId, cfg::FlowId), OwnedValue>,
    flow_expressions: HashMap<(LuaSyntaxId, u32), OwnedValue>,
    type_relations: HashMap<(OwnedValue, OwnedValue), bool>,
    environments: environment::Environments,
    closure_returns: Rc<RefCell<std::collections::HashSet<LuaSyntaxId>>>,
    return_casts: HashMap<(FileId, LuaSyntaxId), Option<flow::ReturnCast>>,
}

impl<'a, 'control, E> OwnedQuery<'a, 'control, E> {
    pub fn new(
        analysis: &'a EmmyLuaAnalysis,
        session: &'a OwnedBindingSession,
        file_id: FileId,
        check: &'control mut dyn FnMut() -> Result<(), E>,
    ) -> Self {
        Self {
            analysis,
            session,
            model: analysis.semantic_model(file_id),
            check,
            contribution_order: None,
            expressions: Rc::new(RefCell::new(HashMap::new())),
            declarations: Rc::new(RefCell::new(HashMap::new())),
            members: Rc::new(RefCell::new(HashMap::new())),
            member_lists: HashMap::new(),
            resolved_members: HashMap::new(),
            flow_trees: Rc::new(RefCell::new(HashMap::new())),
            flow_declarations: HashMap::new(),
            flow_expressions: HashMap::new(),
            type_relations: HashMap::new(),
            environments: Rc::new(RefCell::new(Vec::new())),
            closure_returns: Rc::new(RefCell::new(std::collections::HashSet::new())),
            return_casts: HashMap::new(),
        }
    }

    /// Full-batch owners supply the contribution order captured immediately
    /// after their public BatchChange rebuild. The analysis must remain that
    /// immutable snapshot for the entire query/session borrow. Incremental
    /// editor models must not reuse a stale full-batch order.
    pub fn new_with_contribution_order(
        analysis: &'a EmmyLuaAnalysis,
        session: &'a OwnedBindingSession,
        file_id: FileId,
        check: &'control mut dyn FnMut() -> Result<(), E>,
        contribution_order: Arc<[FileId]>,
    ) -> Self {
        let mut query = Self::new(analysis, session, file_id, check);
        query.contribution_order = Some(contribution_order);
        query
    }

    fn owner_contribution_order(&mut self) -> QueryResult<Arc<[FileId]>, E> {
        self.checkpoint()?;
        match &self.contribution_order {
            Some(order) => Ok(Arc::clone(order)),
            None => self.continuation(OwnedQueryCapability::IntakeOrder, None, None, None),
        }
    }

    fn site(&self, syntax: LuaSyntaxId) -> BindingSite {
        BindingSite {
            file_id: self.model.file_id(),
            syntax,
        }
    }

    fn continuation<T>(
        &self,
        capability: OwnedQueryCapability,
        site: Option<BindingSite>,
        offset: Option<u32>,
        declaration: Option<SemanticId>,
    ) -> QueryResult<T, E> {
        Err(OwnedQueryFailure::Continue(OwnedQueryContinuation {
            capability,
            file_id: site.map_or(self.model.file_id(), |site| site.file_id),
            syntax: site.map(|site| site.syntax),
            offset,
            declaration,
        }))
    }

    /// Replaces the old type_of_expr_impl cache/body on the accepted subset.
    /// Ready and InProgress hits do not call the provider or append coverage.
    /// The original caller control is checked on both sides of every cache hit.
    pub fn type_of_expr(&mut self, syntax: LuaSyntaxId) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let cached = self.expressions.borrow().get(&syntax).cloned();
        if let Some(entry) = cached {
            let value = match entry {
                CacheEntry::Ready(value) => value,
                CacheEntry::InProgress => OwnedValue::plain(LuaType::Unknown),
            };
            self.checkpoint()?;
            return Ok(value);
        }
        self.expressions
            .borrow_mut()
            .insert(syntax, CacheEntry::InProgress);
        let pending = PendingMemo {
            memo: Rc::clone(&self.expressions),
            key: Some(syntax),
        };
        let projector = SelectedProjector::new(self.analysis, self.session);
        let value = {
            let _scope = self.enter_execution(HashMap::new());
            projector.evaluate_expression(self.site(syntax), self)?
        };
        self.checkpoint()?;
        pending.commit(value.clone());
        Ok(value)
    }

    /// Same current-model declaration lookup and foreign-key cache identity as
    /// the fixed engine. Unsupported branches do not leave a Ready(None) entry.
    pub fn type_of_decl(&mut self, declaration: &SemanticId) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let file = match declaration {
            SemanticId::Decl(key) => key.file_id,
            SemanticId::Member(key) => key.file_id,
            _ => self.model.file_id(),
        };
        let key = (file, declaration.clone());
        let cached = self.declarations.borrow().get(&key).cloned();
        if let Some(entry) = cached {
            let value = match entry {
                CacheEntry::Ready(value) => value,
                CacheEntry::InProgress => None,
            };
            self.checkpoint()?;
            return Ok(value);
        }
        self.declarations
            .borrow_mut()
            .insert(key.clone(), CacheEntry::InProgress);
        let pending = PendingMemo {
            memo: Rc::clone(&self.declarations),
            key: Some(key),
        };
        let value = self.declaration_body(declaration)?;
        self.checkpoint()?;
        pending.commit(value.clone());
        Ok(value)
    }

    fn manages_initializer(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        let site = self.site(syntax);
        // Preserve resolve || (call || resolve), including the second resolve
        // when both preceding decisions are NotManaged. These probes do not
        // themselves project or append a receipt.
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().resolve(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        if managed {
            return Ok(true);
        }
        let managed = !matches!(
            self.session.context().call(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        if managed {
            return Ok(true);
        }
        let managed = !matches!(
            self.session.context().resolve(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        Ok(managed)
    }

    fn initializer_slot(ty: OwnedValue, index: Option<usize>) -> OwnedValue {
        match index {
            None => ty,
            Some(_) if matches!(ty.structural_type(), LuaType::Unknown | LuaType::Any) => {
                OwnedValue::plain(LuaType::Unknown)
            }
            Some(index) => ty
                .result_slot(index)
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil)),
        }
    }

    fn declaration_at(
        &mut self,
        declaration: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.type_of_decl_at(declaration, offset)
    }
}

impl<E> SelectedProjectionBackend for OwnedQuery<'_, '_, E> {
    type Value = OwnedValue;
    type Error = OwnedQueryFailure<E>;
    fn checkpoint(&mut self) -> QueryResult<(), E> {
        (self.check)().map_err(OwnedQueryFailure::Control)
    }
    fn plain(&mut self, ty: LuaType) -> OwnedValue {
        OwnedValue::plain(ty)
    }
    fn expression(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<OwnedValue, Self::Error> {
        let mut foreign = OwnedQuery::new(
            self.analysis,
            self.session,
            origin.file_id,
            &mut *self.check,
        );
        foreign.contribution_order = self.contribution_order.clone();
        foreign.flow_trees = Rc::clone(&self.flow_trees);
        foreign.type_of_expr(origin.syntax).map(Ok)
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        parameter: &SemanticId,
    ) -> ProjectionResult<OwnedValue, Self::Error> {
        self.checkpoint()?;
        let SemanticId::Decl(key) = parameter else {
            return Ok(Err("selected_parameter_not_declaration"));
        };
        let mut foreign =
            OwnedQuery::new(self.analysis, self.session, key.file_id, &mut *self.check);
        foreign.contribution_order = self.contribution_order.clone();
        foreign.flow_trees = Rc::clone(&self.flow_trees);
        let Some(declaration) = foreign
            .model
            .file_facts()
            .and_then(|facts| facts.decl_by_id(parameter))
        else {
            return Ok(Err("selected_parameter_missing"));
        };
        if declaration.kind != DeclKind::Param {
            return Ok(Err("selected_parameter_not_parameter"));
        }
        foreign
            .type_of_decl(parameter)
            .map(|value| value.ok_or("selected_parameter_type_unavailable"))
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        function: &FunctionRef,
    ) -> ProjectionResult<OwnedValue, Self::Error> {
        let mut foreign = OwnedQuery::new(
            self.analysis,
            self.session,
            function.file_id,
            &mut *self.check,
        );
        foreign.contribution_order = self.contribution_order.clone();
        foreign.flow_trees = Rc::clone(&self.flow_trees);
        foreign.project_function_signature(site, function.closure)
    }
    fn object(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        object: &ObjectRef,
    ) -> ProjectionResult<OwnedValue, Self::Error> {
        projector.project_owned_object(site, object, self)
    }
}
impl<E> SelectedExpressionBackend for OwnedQuery<'_, '_, E> {
    fn result_pack(&mut self, slots: Vec<OwnedValue>) -> OwnedValue {
        OwnedValue::pack(slots)
    }
    fn scalar_result(&mut self, value: OwnedValue) -> OwnedValue {
        if matches!(value.structural_type(), LuaType::Variadic(_)) {
            value
                .result_slot(0)
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil))
        } else {
            value
        }
    }
}

impl<E> ExpressionExecutionBackend for OwnedQuery<'_, '_, E> {
    fn value_type<'v>(&self, value: &'v OwnedValue) -> &'v LuaType {
        value.structural_type()
    }
    fn same_value_type(&self, left: &OwnedValue, right: &OwnedValue) -> bool {
        left == right
    }
    fn load_name(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        name: &str,
        offset: u32,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.owned_load_name(name, offset)
    }
    fn index_member(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<OwnedValue>,
        key: &LuaMemberKey,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.index_member_general(owner, key)
    }
    fn index_dynamic(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<OwnedValue>,
        key: ExpressionValue<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.index_dynamic_general(owner, key)
    }
    fn expression_at(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.type_of_expr_at(site.syntax, offset)
    }
    fn call(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        callee: ExpressionValue<OwnedValue>,
        arguments: Vec<ExpressionValue<OwnedValue>>,
        colon_call: bool,
        generic_type_syntaxes: &[LuaSyntaxId],
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.call_plain_signature(callee, arguments, colon_call, generic_type_syntaxes)
    }
    fn logical_nullable_extra(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let index = self
            .model
            .syntax_tree()
            .and_then(|tree| site.syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaIndexExpr::cast);
        let Some(index) = index else {
            return Ok(None);
        };
        let Some(resolved) = self.resolve_owned_member(&index)? else {
            return Ok(None);
        };
        if resolved.member_type.is_some() {
            return Ok(None);
        }
        let Some(member) = resolved.member_id else {
            return Ok(None);
        };
        let file = match &member {
            SemanticId::Member(key) => key.file_id,
            _ => self.model.file_id(),
        };
        let nullable = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.member_by_id(&member))
            .is_some_and(|member| member.is_nullable);
        self.checkpoint()?;
        Ok(nullable.then(|| OwnedValue::plain(LuaType::Nil)))
    }

    fn merge_value(&mut self, left: OwnedValue, extra: OwnedValue) -> QueryResult<OwnedValue, E> {
        OwnedValue::union_checked(vec![left, extra], || self.checkpoint())
    }
    fn binary(
        &mut self,
        projector: &SelectedProjector<'_>,
        file: FileId,
        op: BinaryOperator,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        match projector.project_primitive_binary(file, op, left, right, self)? {
            PrimitiveBinaryProjection::Complete(value) => Ok(value),
            PrimitiveBinaryProjection::DeferredLogical { op, left, right } => {
                let right = match projector.project_dense_table_tuple(file, right, self)? {
                    DenseTupleProjection::Projected(value)
                    | DenseTupleProjection::Unchanged(value) => value,
                };
                self.finish_logical(op, left, right)
            }
        }
    }
}
impl<E> DenseTupleBackend for OwnedQuery<'_, '_, E> {
    fn expression_value(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> QueryResult<OwnedValue, E> {
        debug_assert_eq!(site.file_id, self.model.file_id());
        self.type_of_expr(site.syntax)
    }
    fn tuple_value(
        &mut self,
        slots: Vec<OwnedValue>,
        status: LuaTupleStatus,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        OwnedValue::tuple_checked(slots, status, || self.checkpoint())
    }
}

fn is_uninitialized_local(info: &Decl, facts: &emmylua_code_analysis::FileFacts) -> bool {
    matches!(info.kind, DeclKind::Local { is_iter: false, .. })
        && info.value_expr_syntax.is_none()
        && info.doc_type_syntax.is_none()
        && info.module_path.is_none()
        && !info.owner_syntax.is_some_and(|owner| {
            facts
                .type_defs
                .iter()
                .any(|def| def.owner_syntax == Some(owner))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Empty;
    impl super::super::BindingContext for Empty {
        fn identity(&self) -> &str {
            "memo-unit"
        }
        fn resolve(&self, _: BindingSite) -> BindingDecision<super::super::BoundValue> {
            panic!("cache hits must not resolve")
        }
    }
    #[test]
    fn in_progress_expression_and_declaration_use_distinct_original_sentinels() {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("owned-query-memo-unit");
        analysis.add_main_workspace(root.clone());
        let file = analysis.update_files_by_path(vec![(
            root.join("input.lua"),
            Some("local x=1\nreturn x".into()),
        )])[0];
        let model = analysis.semantic_model(file);
        let decl = model.file_facts().unwrap().decls[0].id.clone();
        let syntax = model.file_facts().unwrap().decls[0]
            .value_expr_syntax
            .unwrap();
        let session =
            OwnedBindingSession::new_checked(Arc::new(Empty), || Ok::<_, ()>(())).unwrap();
        let count = std::cell::Cell::new(0);
        let mut check = || {
            count.set(count.get() + 1);
            Ok::<_, ()>(())
        };
        let mut query = OwnedQuery::new(&analysis, &session, file, &mut check);
        query
            .expressions
            .borrow_mut()
            .insert(syntax, CacheEntry::InProgress);
        query
            .declarations
            .borrow_mut()
            .insert((file, decl.clone()), CacheEntry::InProgress);
        assert_eq!(
            query.type_of_expr(syntax).unwrap().structural_type(),
            &LuaType::Unknown
        );
        assert_eq!(query.type_of_decl(&decl).unwrap(), None);
        assert_eq!(count.get(), 4);
        assert!(
            session
                .coverage_checked(|| Ok::<_, ()>(()))
                .unwrap()
                .is_empty()
        );
    }
}
