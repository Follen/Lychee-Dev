//! Query memoization and VM execution environments have different lifetimes.
//! Each ordinary expression starts a fresh environment; callback return probes
//! explicitly inherit only the parameter bindings, matching the original VM.
use super::*;
use emmylua_code_analysis::{LuaSignatureId, LuaTypeDeclId, VariadicType};
use emmylua_parser::{LuaCallExpr, LuaReturnStat};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Environment {
    pub(super) parameters: HashMap<(LuaSyntaxId, usize), OwnedValue>,
    pub(super) computing: HashSet<SemanticId>,
    pub(super) callback_depth: usize,
}
pub(super) type Environments = Rc<RefCell<Vec<Rc<RefCell<Environment>>>>>;
pub(super) struct ExecutionScope(Environments);
impl Drop for ExecutionScope {
    fn drop(&mut self) {
        self.0.borrow_mut().pop();
    }
}
struct DeclGuard {
    env: Rc<RefCell<Environment>>,
    id: SemanticId,
}
impl Drop for DeclGuard {
    fn drop(&mut self) {
        self.env.borrow_mut().computing.remove(&self.id);
    }
}
struct ReturnGuard {
    set: Rc<RefCell<HashSet<LuaSyntaxId>>>,
    syntax: LuaSyntaxId,
}
impl Drop for ReturnGuard {
    fn drop(&mut self) {
        self.set.borrow_mut().remove(&self.syntax);
    }
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn enter_execution(
        &mut self,
        parameters: HashMap<(LuaSyntaxId, usize), OwnedValue>,
    ) -> ExecutionScope {
        self.environments
            .borrow_mut()
            .push(Rc::new(RefCell::new(Environment {
                parameters,
                ..Environment::default()
            })));
        ExecutionScope(Rc::clone(&self.environments))
    }
    pub(super) fn environment(&self) -> Rc<RefCell<Environment>> {
        self.environments
            .borrow()
            .last()
            .cloned()
            .expect("expression execution scope")
    }
    pub(super) fn type_of_expr_inherit(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<OwnedValue, E> {
        self.owned_expr_inherit(syntax)
    }

    pub(super) fn closure_parameter(
        &mut self,
        closure: LuaSyntaxId,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if self.closure_returns.borrow().contains(&closure) {
            return Ok(None);
        }
        let Some(closure_node) = self
            .model
            .syntax_tree()
            .and_then(|tree| closure.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaClosureExpr::cast)
        else {
            return Ok(None);
        };
        let Some(call) = closure_node.ancestors::<LuaCallExpr>().next() else {
            return Ok(None);
        };
        let _scope = self.enter_execution(HashMap::new());
        let env = self.environment();
        let projector = SelectedProjector::new(self.analysis, self.session);
        let _ = projector.evaluate_expression(self.site(call.get_syntax_id()), self)?;
        self.checkpoint()?;
        let value = env
            .borrow()
            .parameters
            .get(&(closure, index))
            .cloned()
            .filter(|value| !value.structural_type().is_unknown());
        Ok(value)
    }
    pub(super) fn enclosing_parameter(
        &mut self,
        decl: &SemanticId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let Some((signature, index)) = self.model.file_facts().and_then(|facts| {
            let info = facts.decl_by_id(decl)?;
            (info.kind == DeclKind::Param).then_some(())?;
            facts.signature_and_param_index_of_decl(info)
        }) else {
            return Ok(None);
        };
        let key = (signature.closure_syntax, index);
        let env = self.environment();
        let cached = env.borrow().parameters.get(&key).cloned();
        if let Some(value) = cached {
            return Ok(Some(value));
        }
        let value = self.closure_parameter(key.0, key.1)?;
        if let Some(value) = &value {
            env.borrow_mut().parameters.insert(key, value.clone());
        }
        Ok(value)
    }
    pub(super) fn owned_load_name(
        &mut self,
        name: &str,
        offset: u32,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        let declaration = self.model.resolve_name(offset.into());
        self.checkpoint()?;
        if let Some(declaration) = declaration {
            if let Some(value) = self.enclosing_parameter(&declaration)? {
                return Ok(ExpressionValue::plain(value));
            }
            let env = self.environment();
            if !env.borrow_mut().computing.insert(declaration.clone()) {
                return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Unknown)));
            }
            let guard = DeclGuard {
                env,
                id: declaration.clone(),
            };
            let mut value = self.declaration_at(&declaration, offset)?;
            drop(guard);
            if value.structural_type().is_unknown()
                && let Some(facts) = self.model.file_facts()
                && facts
                    .decl_by_id(&declaration)
                    .is_some_and(|info| is_uninitialized_local(info, facts))
            {
                value = OwnedValue::plain(LuaType::Nil);
            }
            let owner = if self.owner_initializer_managed(&declaration)? {
                None
            } else {
                Some(declaration)
            };
            return Ok(ExpressionValue {
                value,
                owner,
                closure_syntax: None,
                receiver: None,
            });
        }
        if name == "self"
            && let Some(value) = self.method_self_value(offset)?
        {
            return Ok(value);
        }
        if name == "..."
            && let Some(closure) = self.enclosing_closure(offset)?
            && let Some(value) = self.signature_local(closure.get_syntax_id())?
            && let LuaType::DocFunction(function) = value.structural_type()
            && let Some(index) = function
                .get_params()
                .iter()
                .position(|(name, _)| name == "...")
        {
            let value = value
                .function_parameters()
                .and_then(|params| params.get(index).cloned().flatten())
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            let value = if matches!(value.structural_type(), LuaType::Variadic(_)) {
                value
            } else {
                OwnedValue::with_structural_children(
                    LuaType::Variadic(Arc::new(VariadicType::Base(
                        value.structural_type().clone(),
                    ))),
                    vec![value],
                )
            };
            return Ok(ExpressionValue::plain(value));
        }
        let owner = SemanticId::name(name.into());
        let value = match self.model.resolve_owner(&owner) {
            Some(decl @ SemanticId::Decl(_)) => self
                .type_of_decl(&decl)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)),
            Some(SemanticId::TypeDef(def)) => {
                OwnedValue::plain(LuaType::Ref(LuaTypeDeclId::global(def.full_name.as_str())))
            }
            _ => OwnedValue::plain(LuaType::Unknown),
        };
        let owner = if self.owner_initializer_managed(&owner)? {
            None
        } else {
            Some(owner)
        };
        Ok(ExpressionValue {
            value,
            owner,
            closure_syntax: None,
            receiver: None,
        })
    }
    fn enclosing_closure(&mut self, offset: u32) -> QueryResult<Option<LuaClosureExpr>, E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(None);
        };
        let root = tree.get_red_root();
        let token = match root.token_at_offset(offset.into()) {
            rowan::TokenAtOffset::Single(token) | rowan::TokenAtOffset::Between(_, token) => token,
            rowan::TokenAtOffset::None => return Ok(None),
        };
        for ancestor in token.parent_ancestors() {
            self.checkpoint()?;
            if let Some(closure) = LuaClosureExpr::cast(ancestor) {
                return Ok(Some(closure));
            }
        }
        Ok(None)
    }
    fn method_self_value(
        &mut self,
        offset: u32,
    ) -> QueryResult<Option<ExpressionValue<OwnedValue>>, E> {
        let Some(closure) = self.enclosing_closure(offset)? else {
            return Ok(None);
        };
        let Some(member) = self
            .model
            .file_facts()
            .and_then(|facts| facts.member_by_value_syntax(closure.get_syntax_id()))
        else {
            return Ok(None);
        };
        let mut owner = member.owner.clone();
        if matches!(owner, SemanticId::Name(_)) {
            let Some(resolved) = self.model.resolve_owner(&owner) else {
                return Ok(None);
            };
            owner = resolved;
        }
        let Some(value) = self.method_owner_type(closure.get_syntax_id())? else {
            return Ok(None);
        };
        if let SemanticId::Decl(id) = &owner
            && let Some(facts) = self.model.file_facts_of(id.file_id)
            && let Some(decl) = facts.decl_by_id(&owner)
            && let Some(def) = facts
                .type_defs
                .iter()
                .find(|def| def.owner_syntax.is_some() && def.owner_syntax == decl.owner_syntax)
        {
            owner = def.id.clone();
        }
        Ok(Some(ExpressionValue {
            value,
            owner: Some(owner),
            closure_syntax: None,
            receiver: None,
        }))
    }
    pub(super) fn closure_return_with_env(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if !self.closure_returns.borrow_mut().insert(syntax) {
            return Ok(OwnedValue::plain(LuaType::Unknown));
        }
        let _guard = ReturnGuard {
            set: Rc::clone(&self.closure_returns),
            syntax,
        };
        let Some(closure) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaClosureExpr::cast)
        else {
            return Ok(OwnedValue::plain(LuaType::Unknown));
        };
        let parameters = self
            .environments
            .borrow()
            .last()
            .map(|env| env.borrow().parameters.clone())
            .unwrap_or_default();
        let mut expressions = Vec::new();
        for row in closure.descendants::<LuaReturnStat>() {
            self.checkpoint()?;
            for expr in row.get_expr_list() {
                self.checkpoint()?;
                expressions.push(expr);
            }
        }
        let mut types = Vec::new();
        for expr in expressions {
            self.checkpoint()?;
            let syntax = expr.get_syntax_id();
            let mut value = {
                let _scope = self.enter_execution(parameters.clone());
                let projector = SelectedProjector::new(self.analysis, self.session);
                projector.evaluate_expression(self.site(syntax), self)?
            };
            let raw = self.type_of_expr(syntax)?;
            let flow = self.apply_expr_casts(syntax, raw)?;
            if !matches!(flow.structural_type(), LuaType::Unknown | LuaType::Any) && flow != value {
                value = flow;
            }
            if !matches!(value.structural_type(), LuaType::Unknown | LuaType::Any)
                && !types.contains(&value)
            {
                types.push(value);
            }
        }
        // The closure-return producer filters Unknown/Any candidates above.
        // Its empty result means unavailable inference, not Lua's Nil value:
        // the fixed VM explicitly returns Unknown here before LuaType::from_vec.
        if types.is_empty() {
            self.checkpoint()?;
            return Ok(OwnedValue::plain(LuaType::Unknown));
        }
        OwnedValue::union_checked(types, || self.checkpoint())
    }
}
