//! Concrete, deliberately incomplete owned query backend.
//!
//! Completed paths use the same compiler/executor and per-model caches. Missing
//! capabilities leave through a typed continuation boundary, never a successful
//! Unknown or a raw contextual-inference fallback. This is not the production
//! semantic-model replacement yet: notably selected objects cannot enter this
//! public-LuaType-only carrier until the owned provenance carrier is implemented.
use super::{
    BindingDecision, BindingSite, DenseTupleBackend, DenseTupleProjection,
    ExpressionExecutionBackend, ExpressionValue, FunctionRef, ObjectRef, OwnedBindingSession,
    PrimitiveBinaryProjection, ProjectionResult, SelectedExpressionBackend,
    SelectedProjectionBackend, SelectedProjector,
};
use emmylua_code_analysis::{
    Decl, DeclKind, EmmyLuaAnalysis, FileId, LuaMemberKey, LuaTupleStatus, LuaTupleType, LuaType,
    SemanticId, SemanticModel, VariadicType,
};
use emmylua_parser::{BinaryOperator, LuaAstNode, LuaClosureExpr, LuaIndexExpr, LuaSyntaxId};
use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc, sync::Arc};

/// Unfinished branches are explicit and must be implemented on this backend.
/// These are development continuation labels, not product diagnostics/coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedQueryCapability {
    Parameter,
    Function,
    Object,
    DeclarationMetadata,
    ClosureEnvironment,
    DeclarationFlow,
    GlobalGenericSanitization,
    DeclarationShell,
    UnresolvedName,
    IndexMember,
    IndexDynamic,
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
/// A successful result currently excludes selected Object/Function/Parameter
/// projections and general flow/member/call inference. Such a branch returns
/// Continue before any plain LuaType can erase selected identity or provenance.
/// This entry is not installed in Source or the diagnostic registry yet.
pub struct OwnedQuery<'a, 'control, E> {
    analysis: &'a EmmyLuaAnalysis,
    session: &'a OwnedBindingSession,
    model: SemanticModel<'a>,
    check: &'control mut dyn FnMut() -> Result<(), E>,
    expressions: Memo<LuaSyntaxId, LuaType>,
    declarations: Memo<(FileId, SemanticId), Option<LuaType>>,
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
    pub fn type_of_expr(&mut self, syntax: LuaSyntaxId) -> QueryResult<LuaType, E> {
        self.checkpoint()?;
        let cached = self.expressions.borrow().get(&syntax).cloned();
        if let Some(entry) = cached {
            let value = match entry {
                CacheEntry::Ready(value) => value,
                CacheEntry::InProgress => LuaType::Unknown,
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
    pub fn type_of_decl(&mut self, declaration: &SemanticId) -> QueryResult<Option<LuaType>, E> {
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

    fn initializer_slot(ty: LuaType, index: Option<usize>) -> LuaType {
        match index {
            None => ty,
            Some(_) if matches!(ty, LuaType::Unknown | LuaType::Any) => LuaType::Unknown,
            Some(index) => ty.get_result_slot_type(index).unwrap_or(LuaType::Nil),
        }
    }

    fn declaration_body(&mut self, declaration: &SemanticId) -> QueryResult<Option<LuaType>, E> {
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
            let value = self.model.type_of_decl(declaration);
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

    fn declaration_at(&mut self, declaration: &SemanticId, offset: u32) -> QueryResult<LuaType, E> {
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
        let ty = self.type_of_decl(declaration)?.unwrap_or(LuaType::Unknown);
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
    type Value = LuaType;
    type Error = OwnedQueryFailure<E>;
    fn checkpoint(&mut self) -> QueryResult<(), E> {
        (self.check)().map_err(OwnedQueryFailure::Control)
    }
    fn plain(&mut self, ty: LuaType) -> LuaType {
        ty
    }
    fn expression(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<LuaType, Self::Error> {
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
    ) -> ProjectionResult<LuaType, Self::Error> {
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
        _: &FunctionRef,
    ) -> ProjectionResult<LuaType, Self::Error> {
        self.continuation(OwnedQueryCapability::Function, Some(site), None, None)
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        _: &ObjectRef,
    ) -> ProjectionResult<LuaType, Self::Error> {
        self.continuation(OwnedQueryCapability::Object, Some(site), None, None)
    }
}
impl<E> SelectedExpressionBackend for OwnedQuery<'_, '_, E> {
    fn result_pack(&mut self, slots: Vec<LuaType>) -> LuaType {
        LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
    }
    fn scalar_result(&mut self, value: LuaType) -> LuaType {
        if matches!(value, LuaType::Variadic(_)) {
            value.get_result_slot_type(0).unwrap_or(LuaType::Nil)
        } else {
            value
        }
    }
}
impl<E> ExpressionExecutionBackend for OwnedQuery<'_, '_, E> {
    fn value_type<'v>(&self, value: &'v LuaType) -> &'v LuaType {
        value
    }
    fn same_value_type(&self, left: &LuaType, right: &LuaType) -> bool {
        left == right
    }
    fn load_name(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        _: &str,
        offset: u32,
    ) -> QueryResult<ExpressionValue<LuaType>, E> {
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
        if matches!(ty, LuaType::Unknown)
            && info.is_some_and(|info| is_uninitialized_local(info, facts.unwrap()))
        {
            ty = LuaType::Nil;
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
        _: ExpressionValue<LuaType>,
        _: &LuaMemberKey,
    ) -> QueryResult<ExpressionValue<LuaType>, E> {
        self.continuation(OwnedQueryCapability::IndexMember, None, None, None)
    }
    fn index_dynamic(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        _: ExpressionValue<LuaType>,
        _: ExpressionValue<LuaType>,
    ) -> QueryResult<ExpressionValue<LuaType>, E> {
        self.continuation(OwnedQueryCapability::IndexDynamic, None, None, None)
    }
    fn expression_at(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
        _: u32,
    ) -> QueryResult<LuaType, E> {
        self.continuation(OwnedQueryCapability::ExpressionFlow, Some(site), None, None)
    }
    fn call(
        &mut self,
        _: &SelectedProjector<'_>,
        _: FileId,
        _: ExpressionValue<LuaType>,
        _: Vec<ExpressionValue<LuaType>>,
        _: bool,
        _: &[LuaSyntaxId],
    ) -> QueryResult<ExpressionValue<LuaType>, E> {
        self.continuation(OwnedQueryCapability::Call, None, None, None)
    }
    fn logical_nullable_extra(
        &mut self,
        _: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> QueryResult<Option<LuaType>, E> {
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
    fn merge_value(&mut self, left: LuaType, extra: LuaType) -> QueryResult<LuaType, E> {
        self.checkpoint()?;
        Ok(LuaType::from_vec(vec![left, extra]))
    }
    fn binary(
        &mut self,
        projector: &SelectedProjector<'_>,
        file: FileId,
        op: BinaryOperator,
        left: LuaType,
        right: LuaType,
    ) -> QueryResult<LuaType, E> {
        match projector.project_primitive_binary(file, op, left, right, self)? {
            PrimitiveBinaryProjection::Complete(value) => Ok(value),
            PrimitiveBinaryProjection::DeferredLogical { op, left, right } => {
                let right = match projector.project_dense_table_tuple(file, right, self)? {
                    DenseTupleProjection::Projected(value)
                    | DenseTupleProjection::Unchanged(value) => value,
                };
                if (op == BinaryOperator::OpAnd && left.is_always_truthy())
                    || (op == BinaryOperator::OpOr && left.is_always_falsy())
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
    ) -> QueryResult<LuaType, E> {
        debug_assert_eq!(site.file_id, self.model.file_id());
        self.type_of_expr(site.syntax)
    }
    fn tuple_value(
        &mut self,
        slots: Vec<LuaType>,
        status: LuaTupleStatus,
    ) -> QueryResult<LuaType, E> {
        self.checkpoint()?;
        Ok(LuaType::Tuple(Arc::new(LuaTupleType::new(slots, status))))
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
        assert_eq!(query.type_of_expr(syntax).unwrap(), LuaType::Unknown);
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
