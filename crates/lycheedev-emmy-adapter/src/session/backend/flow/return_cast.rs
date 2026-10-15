//! Return-cast lookup and branch projection, including alias/re-export origin
//! identities and the fixed 16-hop lookup bound.
use super::*;
use crate::session::BindingDecision;
use emmylua_parser::{BinaryOperator, LuaCallExpr, LuaIndexKey, LuaLiteralToken};
#[derive(Clone)]
pub(in crate::session::backend) struct ReturnCast {
    pub(super) argument: LuaExpr,
    pub(super) cast: OwnedValue,
    pub(super) fallback: Option<OwnedValue>,
    pub(super) is_self: bool,
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn return_cast_for_call(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<ReturnCast>, E> {
        self.checkpoint()?;
        let key = (self.model.file_id(), call.get_syntax_id());
        if let Some(value) = self.return_casts.get(&key).cloned() {
            return Ok(value);
        }
        let value = self.return_cast_uncached(call)?;
        self.checkpoint()?;
        self.return_casts.insert(key, value.clone());
        Ok(value)
    }
    fn return_cast_uncached(&mut self, call: &LuaCallExpr) -> QueryResult<Option<ReturnCast>, E> {
        let site = self.site(call.get_syntax_id());
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().call(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        let managed = if managed {
            true
        } else {
            let managed = !matches!(
                self.session.context().resolve(site),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            managed
        };
        if managed {
            return Ok(None);
        }
        let Some(prefix) = call.get_prefix_expr() else {
            return Ok(None);
        };
        self.checkpoint()?;
        let managed = !matches!(
            self.session
                .context()
                .resolve(self.site(prefix.get_syntax_id())),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        if managed {
            return Ok(None);
        }
        let declaration = match &prefix {
            LuaExpr::NameExpr(name) => self.model.resolve_name(name.get_position()),
            LuaExpr::IndexExpr(index) => {
                let name = match index.get_index_key() {
                    Some(LuaIndexKey::Name(name)) => name.get_name_text().to_string(),
                    Some(LuaIndexKey::String(value)) => value.get_value(),
                    _ => return Ok(None),
                };
                match self
                    .resolve_owned_member(index)?
                    .and_then(|resolved| resolved.member_id)
                {
                    Some(id) => Some(id),
                    None => self.self_return_cast_member(&name)?,
                }
            }
            _ => return Ok(None),
        };
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        let Some(declaration) = self.return_cast_origin(declaration)? else {
            return Ok(None);
        };
        let (file, syntax) = match &declaration {
            SemanticId::Decl(key) => (
                key.file_id,
                self.model
                    .file_facts_of(key.file_id)
                    .and_then(|facts| facts.decl_by_id(&declaration))
                    .and_then(|decl| decl.value_expr_syntax),
            ),
            SemanticId::Member(key) => (
                key.file_id,
                self.model
                    .file_facts_of(key.file_id)
                    .and_then(|facts| facts.member_by_id(&declaration))
                    .and_then(|member| member.value_syntax),
            ),
            _ => return Ok(None),
        };
        let Some(syntax) = syntax else {
            return Ok(None);
        };
        let Some(signature) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.signature_by_closure(syntax))
        else {
            return Ok(None);
        };
        let Some(docs) = &signature.docs else {
            return Ok(None);
        };
        let Some(cast) = &docs.return_cast else {
            return Ok(None);
        };
        let is_self = cast.name == "self" && call.is_colon_call();
        let index = if is_self {
            0
        } else {
            let Some(index) = signature
                .param_names
                .iter()
                .position(|name| name == &cast.name)
            else {
                return Ok(None);
            };
            index
        };
        let Some(list) = call.get_args_list() else {
            return Ok(None);
        };
        let mut args = Vec::new();
        for arg in list.get_args() {
            self.checkpoint()?;
            args.push(arg);
        }
        let argument = if is_self {
            match prefix {
                LuaExpr::IndexExpr(index) => {
                    let Some(prefix) = index.get_prefix_expr() else {
                        return Ok(None);
                    };
                    prefix
                }
                other => other,
            }
        } else {
            let Some(arg) = args.get(index) else {
                return Ok(None);
            };
            arg.clone()
        };
        let value = OwnedValue::plain(self.model.doc_type_lua_in(
            file,
            cast.cast,
            &docs.generic_params,
        ));
        let fallback = cast.fallback.map(|syntax| {
            OwnedValue::plain(
                self.model
                    .doc_type_lua_in(file, syntax, &docs.generic_params),
            )
        });
        self.checkpoint()?;
        Ok(Some(ReturnCast {
            argument,
            cast: value,
            fallback,
            is_self,
        }))
    }
    fn return_cast_origin(
        &mut self,
        mut current: SemanticId,
    ) -> QueryResult<Option<SemanticId>, E> {
        let mut seen = HashSet::new();
        for _ in 0..16 {
            self.checkpoint()?;
            if !seen.insert(current.clone()) {
                return Ok(None);
            }
            let (file, syntax) = match &current {
                SemanticId::Decl(key) => (
                    key.file_id,
                    self.model
                        .file_facts_of(key.file_id)
                        .and_then(|facts| facts.decl_by_id(&current))
                        .and_then(|decl| decl.value_expr_syntax),
                ),
                SemanticId::Member(key) => (
                    key.file_id,
                    self.model
                        .file_facts_of(key.file_id)
                        .and_then(|facts| facts.member_by_id(&current))
                        .and_then(|member| member.value_syntax),
                ),
                _ => return Ok(Some(current)),
            };
            let Some(syntax) = syntax else {
                return Ok(None);
            };
            let Some(expr) = self
                .model
                .syntax_tree_of(file)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                .and_then(LuaExpr::cast)
            else {
                return Ok(None);
            };
            match expr {
                LuaExpr::ClosureExpr(_) => return Ok(Some(current)),
                LuaExpr::NameExpr(name) => {
                    let foreign = self.analysis.semantic_model(file);
                    let Some(id) = foreign.resolve_name(name.get_position()) else {
                        return Ok(None);
                    };
                    current = id;
                }
                LuaExpr::IndexExpr(index) => {
                    let mut foreign =
                        OwnedQuery::new(self.analysis, self.session, file, &mut *self.check);
                    foreign.contribution_order = self.contribution_order.clone();
                    foreign.workspace_facts = Arc::clone(&self.workspace_facts);
                    foreign.flow_trees = std::rc::Rc::clone(&self.flow_trees);
                    let Some(id) = foreign
                        .resolve_owned_member(&index)?
                        .and_then(|resolved| resolved.member_id)
                    else {
                        return Ok(None);
                    };
                    current = id;
                }
                _ => return Ok(Some(current)),
            }
        }
        Ok(None)
    }
    fn self_return_cast_member(&mut self, name: &str) -> QueryResult<Option<SemanticId>, E> {
        for file in self.model.file_ids() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(file) else {
                continue;
            };
            for member in facts.members_named(name) {
                self.checkpoint()?;
                if let Some(signature) = member
                    .value_syntax
                    .and_then(|syntax| facts.signature_by_closure(syntax))
                    && signature
                        .docs
                        .as_ref()
                        .and_then(|docs| docs.return_cast.as_ref())
                        .is_some_and(|cast| cast.name == "self")
                {
                    return Ok(Some(member.id.clone()));
                }
            }
        }
        Ok(None)
    }
    pub(super) fn conditional_receiver(&mut self, expr: &LuaExpr) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let LuaExpr::NameExpr(name) = expr else {
            return Ok(false);
        };
        let Some(decl @ SemanticId::Decl(_)) = self.model.resolve_name(name.get_position()) else {
            return Ok(false);
        };
        let SemanticId::Decl(key) = &decl else {
            unreachable!()
        };
        let syntax = self
            .model
            .file_facts_of(key.file_id)
            .and_then(|facts| facts.decl_by_id(&decl))
            .and_then(|decl| decl.value_expr_syntax);
        let expr = syntax
            .and_then(|syntax| {
                self.model
                    .syntax_tree_of(key.file_id)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            })
            .and_then(LuaExpr::cast);
        Ok(
            matches!(expr, Some(LuaExpr::BinaryExpr(binary)) if binary.get_op_token().is_some_and(|token| matches!(token.get_op(), BinaryOperator::OpAnd | BinaryOperator::OpOr))),
        )
    }
    pub(super) fn call_may_narrow(
        &mut self,
        call: &LuaCallExpr,
        declaration: &SemanticId,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if call.is_colon_call()
            && let Some(prefix) = call.get_prefix_expr()
        {
            let target = match prefix {
                LuaExpr::NameExpr(name) => self.model.resolve_name(name.get_position()),
                LuaExpr::IndexExpr(index) => index.get_prefix_expr().and_then(|expr| match expr {
                    LuaExpr::NameExpr(name) => self.model.resolve_name(name.get_position()),
                    _ => None,
                }),
                _ => None,
            };
            if target.as_ref() == Some(declaration) {
                return Ok(true);
            }
        }
        if let Some(list) = call.get_args_list() {
            for arg in list.get_args() {
                self.checkpoint()?;
                if self.is_decl_name(&arg, declaration) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    pub(super) fn call_cast_narrowing(
        &mut self,
        call: &LuaCallExpr,
        declaration: &SemanticId,
        truthy: bool,
        prior: bool,
        remove_missing_fallback: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        let Some(cast) = self.return_cast_for_call(call)? else {
            return Ok(None);
        };
        if cast.is_self && !prior && self.conditional_receiver(&cast.argument)? {
            return Ok(None);
        }
        if !self.is_decl_name(&cast.argument, declaration) {
            return Ok(None);
        }
        if truthy {
            Ok(Some(Narrowing::Replace(cast.cast)))
        } else if let Some(fallback) = cast.fallback {
            Ok(Some(Narrowing::Replace(fallback)))
        } else if remove_missing_fallback && !cast.cast.structural_type().is_unknown() {
            Ok(Some(Narrowing::Remove(cast.cast)))
        } else {
            Ok(None)
        }
    }
    pub(super) fn binary_guard_narrowing(
        &mut self,
        left: &LuaExpr,
        right: &LuaExpr,
        op: BinaryOperator,
        declaration: &SemanticId,
        truthy: bool,
        prior: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        let (call, literal) = match (left, right) {
            (LuaExpr::CallExpr(call), LuaExpr::LiteralExpr(literal))
            | (LuaExpr::LiteralExpr(literal), LuaExpr::CallExpr(call)) => (call, literal),
            _ => return Ok(None),
        };
        let Some(LuaLiteralToken::Bool(value)) = literal.get_literal() else {
            return Ok(None);
        };
        let call_true = match op {
            BinaryOperator::OpEq => value.is_true(),
            BinaryOperator::OpNe => !value.is_true(),
            _ => return Ok(None),
        };
        // Both original direct true and false comparison producers pass false
        // to return_cast_bool_binary. Retain that legacy branch asymmetry.
        if let Some(value) =
            self.call_cast_narrowing(call, declaration, !call_true, prior, false)?
        {
            return Ok(Some(value));
        }
        if call_true == truthy
            && let Some((arg, value)) = self.type_guard_target(call)?
            && self.is_decl_name(&arg, declaration)
        {
            return Ok(Some(Narrowing::Replace(value)));
        }
        Ok(None)
    }
}
