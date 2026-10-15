//! Concrete, deliberately incomplete owned query backend.
//!
//! Completed paths use the same compiler/executor and per-model caches. Missing
//! capabilities leave through a typed continuation boundary, never a successful
//! Unknown or a raw contextual-inference fallback. This is not the production
//! semantic-model replacement yet. Selected objects retain owned identity and
//! nested carriers; unimplemented structural/member consumers remain explicit.
mod call;
mod member;
pub use member::OwnedMemberInfo;
mod signature;

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
    expressions: Memo<LuaSyntaxId, OwnedValue>,
    declarations: Memo<(FileId, SemanticId), Option<OwnedValue>>,
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
            expressions: Rc::new(RefCell::new(HashMap::new())),
            declarations: Rc::new(RefCell::new(HashMap::new())),
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
        let value = projector.evaluate_expression(self.site(syntax), self)?;
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

    fn declaration_body(&mut self, declaration: &SemanticId) -> QueryResult<Option<OwnedValue>, E> {
        if let Some(facts) = self.model.file_facts()
            && let Some(info) = facts.decl_by_id(declaration)
        {
            if let Some(syntax) = info.value_expr_syntax
                && self.manages_initializer(syntax)?
            {
                // Selected initializer takes priority even over a doc annotation.
                let ty = self.type_of_expr(syntax)?;
                return Ok(Some(Self::initializer_slot(ty, info.multi_return_index)));
            }
            let site = info.owner_syntax.map(|syntax| self.site(syntax));
            if !matches!(info.kind, DeclKind::Local { is_iter: false, .. })
                || info.doc_type_syntax.is_some()
                || info.module_path.is_some()
                || info.delayed_definition
                || info
                    .owner_syntax
                    .is_some_and(|owner| facts.type_def_by_owner_syntax(owner).is_some())
            {
                return self.continuation(
                    OwnedQueryCapability::DeclarationMetadata,
                    site,
                    None,
                    Some(declaration.clone()),
                );
            }
            if let Some(syntax) = info.value_expr_syntax {
                let node = self
                    .model
                    .syntax_tree()
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()));
                if node.clone().and_then(LuaClosureExpr::cast).is_some() {
                    return self.continuation(
                        OwnedQueryCapability::Function,
                        Some(self.site(syntax)),
                        None,
                        Some(declaration.clone()),
                    );
                }
                if node.and_then(LuaIndexExpr::cast).is_some() {
                    return self.continuation(
                        OwnedQueryCapability::ExpressionFlow,
                        Some(self.site(syntax)),
                        None,
                        Some(declaration.clone()),
                    );
                }
                let ty = self.type_of_expr(syntax)?;
                return Ok(Some(Self::initializer_slot(ty, info.multi_return_index)));
            }
            // Only this already-validated uninitialized local can reuse the
            // unmodified public decl query: there is no initializer, annotation,
            // module, class, iteration, parameter or delayed-definition branch
            // left which could observe selected context. The raw method reaches
            // exactly the old final shell projection without contextual work.
            self.checkpoint()?;
            let value = self.model.type_of_decl(declaration).map(OwnedValue::plain);
            self.checkpoint()?;
            return Ok(value);
        }
        let file = match declaration {
            SemanticId::Decl(key) => key.file_id,
            _ => self.model.file_id(),
        };
        if file != self.model.file_id() {
            let mut foreign = OwnedQuery::new(self.analysis, self.session, file, &mut *self.check);
            if let Some(value) = foreign.type_of_decl(declaration)? {
                return Ok(Some(value));
            }
        }
        self.continuation(
            OwnedQueryCapability::DeclarationShell,
            None,
            None,
            Some(declaration.clone()),
        )
    }

    fn declaration_at(
        &mut self,
        declaration: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let facts = self.model.file_facts();
        let info = facts.and_then(|facts| facts.decl_by_id(declaration));
        let start = match declaration {
            SemanticId::Decl(key) if key.file_id == self.model.file_id() => info
                .and_then(|info| info.owner_syntax)
                .map(|syntax| syntax.get_range().start()),
            _ => None,
        };
        let direct = match self.model.flow_tree() {
            None => true,
            Some(tree) if tree.get_flow_id_at(offset.into()).is_none() => true,
            Some(tree) => start.is_some_and(|start| {
                tree.latest_decl_assignment(declaration, offset.into())
                    .is_none()
                    && !tree.has_flow_event_between(start, offset.into(), true)
            }),
        };
        self.checkpoint()?;
        if !direct {
            return self.continuation(
                OwnedQueryCapability::DeclarationFlow,
                None,
                Some(offset),
                Some(declaration.clone()),
            );
        }
        let ty = self
            .type_of_decl(declaration)?
            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
        if info.is_some_and(|info| info.kind == DeclKind::Global) {
            return self.continuation(
                OwnedQueryCapability::GlobalGenericSanitization,
                None,
                Some(offset),
                Some(declaration.clone()),
            );
        }
        Ok(ty)
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
        foreign.type_of_expr(origin.syntax).map(Ok)
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        parameter: &SemanticId,
    ) -> ProjectionResult<OwnedValue, Self::Error> {
        self.continuation(
            OwnedQueryCapability::Parameter,
            Some(site),
            None,
            Some(parameter.clone()),
        )
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
        foreign
            .project_function_signature(site, function.closure)
            .map(|result| result.map(OwnedValue::plain))
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
        _: &str,
        offset: u32,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        let declaration = self.model.resolve_name(offset.into());
        self.checkpoint()?;
        let Some(declaration) = declaration else {
            return self.continuation(
                OwnedQueryCapability::UnresolvedName,
                None,
                Some(offset),
                None,
            );
        };
        let facts = self.model.file_facts();
        let info = facts.and_then(|facts| facts.decl_by_id(&declaration));
        if info.is_some_and(|info| info.kind == DeclKind::Param) {
            return self.continuation(
                OwnedQueryCapability::ClosureEnvironment,
                None,
                Some(offset),
                Some(declaration),
            );
        }
        let mut ty = self.declaration_at(&declaration, offset)?;
        if matches!(ty.structural_type(), LuaType::Unknown)
            && info.is_some_and(|info| is_uninitialized_local(info, facts.unwrap()))
        {
            ty = OwnedValue::plain(LuaType::Nil);
        }
        Ok(ExpressionValue {
            value: ty,
            owner: Some(declaration),
            closure_syntax: None,
            receiver: None,
        })
    }
    fn index_member(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<OwnedValue>,
        key: &LuaMemberKey,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.index_selected_member(owner, key)
    }
    fn index_dynamic(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        owner: ExpressionValue<OwnedValue>,
        key: ExpressionValue<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.index_selected_dynamic(owner, key)
    }
    fn expression_at(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        _: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.continuation(OwnedQueryCapability::ExpressionFlow, Some(site), None, None)
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
        let member = self
            .model
            .syntax_tree()
            .and_then(|tree| site.syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaIndexExpr::cast)
            .is_some();
        self.checkpoint()?;
        if member {
            self.continuation(
                OwnedQueryCapability::LogicalNullableMember,
                Some(site),
                None,
                None,
            )
        } else {
            Ok(None)
        }
    }
    fn merge_value(&mut self, left: OwnedValue, extra: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let (Ok(left), Ok(extra)) = (
            left.into_context_free_type(),
            extra.into_context_free_type(),
        ) else {
            return self.continuation(OwnedQueryCapability::SelectedTypeMerge, None, None, None);
        };
        Ok(OwnedValue::plain(LuaType::from_vec(vec![left, extra])))
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
                if (op == BinaryOperator::OpAnd && left.structural_type().is_always_truthy())
                    || (op == BinaryOperator::OpOr && left.structural_type().is_always_falsy())
                {
                    Ok(right)
                } else {
                    self.continuation(OwnedQueryCapability::LogicalUnion, None, None, None)
                }
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
