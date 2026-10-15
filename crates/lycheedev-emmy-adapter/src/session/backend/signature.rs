//! Selected non-member, non-generic Function signatures on the actual owned
//! backend. Body returns use fresh owned model scopes, never a context-free
//! signature/return query. Unsupported type/metadata work remains a continuation.
use super::{OwnedQuery, OwnedQueryCapability, ProjectionResult, QueryResult};
use crate::session::{BindingCoverage, BindingSite, SelectedProjectionBackend};
use emmylua_code_analysis::{
    AsyncState, LuaFunctionType, LuaType, LuaUnionType, Signature, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaClosureExpr, LuaExpr, LuaReturnStat, LuaStat, LuaSyntaxId};
use std::sync::Arc;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn project_function_signature(
        &mut self,
        projection_site: BindingSite,
        closure: LuaSyntaxId,
    ) -> ProjectionResult<LuaType, super::OwnedQueryFailure<E>> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts() else {
            return Ok(Err("selected_function_signature_unavailable"));
        };
        let Some(signature) = facts.signature_by_closure(closure) else {
            return Ok(Err("selected_function_signature_unavailable"));
        };
        if signature.is_method
            || facts.member_by_value_syntax(closure).is_some()
            || signature
                .docs
                .as_ref()
                .is_some_and(|docs| !docs.generic_params.is_empty())
        {
            return self.continuation(
                OwnedQueryCapability::FunctionMetadata,
                Some(projection_site),
                None,
                None,
            );
        }
        let mut params = Vec::new();
        for (index, name) in signature.param_names.iter().enumerate() {
            self.checkpoint()?;
            // param_type's only addition to the old direct shell projection is
            // expected_member_param_for_closure. The member exclusion above
            // makes that branch unavailable; generic names are empty on both.
            let mut ty = self
                .model
                .param_type(closure, index)
                .or_else(|| {
                    signature.docs.as_ref().and_then(|docs| {
                        docs.param_types
                            .iter()
                            .find(|(param, _)| param == name)
                            .map(|(_, syntax)| {
                                self.model
                                    .doc_type_lua_rich_in(self.model.file_id(), *syntax)
                            })
                    })
                })
                .unwrap_or(LuaType::Any);
            self.checkpoint()?;
            self.require_plain_signature_type(&ty, projection_site)?;
            ty = self.model.expand_alias_for_hover(&ty);
            self.checkpoint()?;
            if signature
                .docs
                .as_ref()
                .is_some_and(|docs| docs.nullable_params.iter().any(|param| param == name))
                && !ty.is_nullable()
            {
                ty = LuaType::Union(Arc::new(LuaUnionType::from_vec(vec![ty, LuaType::Nil])));
            }
            params.push((name.to_string(), Some(ty)));
        }
        let mut ret = self.contextual_signature_return(signature)?;
        self.require_plain_signature_type(&ret, projection_site)?;
        ret = self.model.expand_alias_for_hover(&ret);
        self.checkpoint()?;
        // The fixed in-file function wrapper invokes closure_return_lua again
        // for Unknown without docs. That helper has a distinct inherited VM /
        // flow-cast schedule and may recover a value; silently returning Unknown
        // here would weaken the original contract.
        if matches!(ret, LuaType::Unknown) && signature.docs.is_none() {
            return self.continuation(
                OwnedQueryCapability::FunctionRawReturnFallback,
                Some(BindingSite {
                    file_id: self.model.file_id(),
                    syntax: closure,
                }),
                None,
                None,
            );
        }
        let async_state = if signature.docs.as_ref().is_some_and(|docs| docs.is_async) {
            AsyncState::Async
        } else {
            AsyncState::None
        };
        self.checkpoint()?;
        Ok(Ok(LuaType::DocFunction(Arc::new(LuaFunctionType::new(
            async_state,
            signature.is_method,
            signature.is_variadic,
            params,
            ret,
            Some(Vec::new()),
        )))))
    }

    fn contextual_signature_return(&mut self, signature: &Signature) -> QueryResult<LuaType, E> {
        let file = self.model.file_id();
        if let Some(docs) = &signature.docs {
            if !docs.returns.is_empty() {
                let mut returns = Vec::new();
                for syntax in &docs.returns {
                    self.checkpoint()?;
                    returns.push(self.model.doc_type_lua_rich_in(file, *syntax));
                    self.checkpoint()?;
                }
                return Ok(if returns.len() == 1 {
                    returns.remove(0)
                } else {
                    LuaType::Variadic(Arc::new(VariadicType::Multi(returns)))
                });
            }
            if !docs.return_overload_rows.is_empty() {
                let mut rows = Vec::new();
                let mut index = 0usize;
                for &len in &docs.return_overload_rows {
                    self.checkpoint()?;
                    let Some(end) = index
                        .checked_add(len)
                        .filter(|end| *end <= docs.return_overloads.len())
                    else {
                        return Ok(LuaType::Unknown);
                    };
                    let mut values = Vec::new();
                    for (_, syntax) in &docs.return_overloads[index..end] {
                        self.checkpoint()?;
                        values.push(self.model.doc_type_lua_rich_in(file, *syntax));
                        self.checkpoint()?;
                    }
                    rows.push(LuaType::Variadic(Arc::new(VariadicType::Multi(values))));
                    index = end;
                }
                return Ok(if index == docs.return_overloads.len() {
                    LuaType::from_vec(rows)
                } else {
                    LuaType::Unknown
                });
            }
        }
        let site = BindingSite {
            file_id: file,
            syntax: signature.closure_syntax,
        };
        let session = self.session;
        let Some(_guard) = session.enter(site, || self.checkpoint())? else {
            // This guard failure has no signature_return receipt in the fixed
            // producer, unlike project_selected's projection-cycle receipt.
            return Ok(LuaType::Unknown);
        };
        let mut body = OwnedQuery::new(self.analysis, session, file, &mut *self.check);
        body.contextual_closure_return(signature.closure_syntax)
    }

    fn contextual_closure_return(&mut self, syntax: LuaSyntaxId) -> QueryResult<LuaType, E> {
        self.checkpoint()?;
        let Some(closure) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaClosureExpr::cast)
        else {
            return Ok(LuaType::Unknown);
        };
        let mut rows = Vec::new();
        for node in closure.syntax().descendants() {
            self.checkpoint()?;
            let Some(ret) = LuaReturnStat::cast(node) else {
                continue;
            };
            let mut owner = None;
            for ancestor in ret.syntax().ancestors() {
                self.checkpoint()?;
                if let Some(closure) = LuaClosureExpr::cast(ancestor) {
                    owner = Some(closure.get_syntax_id());
                    break;
                }
            }
            if owner != Some(syntax) {
                continue;
            }
            if rows.len() >= 256 {
                return Ok(LuaType::Unknown);
            }
            let mut exprs = Vec::new();
            for expr in ret.get_expr_list() {
                self.checkpoint()?;
                exprs.push(expr);
                if exprs.len() > 32 {
                    return Ok(LuaType::Unknown);
                }
            }
            let mut slots = Vec::new();
            for (index, expr) in exprs.iter().enumerate() {
                self.checkpoint()?;
                let ty = self.type_of_expr(expr.get_syntax_id())?;
                if index + 1 == exprs.len()
                    && matches!(expr, LuaExpr::CallExpr(_))
                    && matches!(ty, LuaType::Unknown | LuaType::Any)
                {
                    slots.push(LuaType::Variadic(Arc::new(VariadicType::Base(
                        LuaType::Unknown,
                    ))));
                } else if index + 1 == exprs.len()
                    && let LuaType::Variadic(values) = &ty
                {
                    match values.as_ref() {
                        VariadicType::Multi(values) => {
                            for value in values {
                                self.checkpoint()?;
                                slots.push(value.clone());
                            }
                        }
                        VariadicType::Base(_) => slots.push(ty),
                    }
                } else {
                    slots.push(ty.get_result_slot_type(0).unwrap_or(LuaType::Nil));
                }
                if slots.len() > 32 {
                    return Ok(LuaType::Unknown);
                }
            }
            rows.push(if slots.len() == 1 {
                slots.remove(0)
            } else {
                LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
            });
        }
        if rows.is_empty() {
            return Ok(LuaType::Variadic(Arc::new(VariadicType::Multi(Vec::new()))));
        }
        let mut terminal = false;
        if let Some(block) = closure.get_block() {
            for stat in block.get_stats() {
                self.checkpoint()?;
                terminal = matches!(stat, LuaStat::ReturnStat(_));
            }
        }
        self.checkpoint()?;
        if !terminal {
            let session = self.session;
            session.record(
                BindingCoverage {
                    site: BindingSite {
                        file_id: self.model.file_id(),
                        syntax,
                    },
                    operation: "signature_return".into(),
                    state: "unavailable".into(),
                    proof: String::new(),
                    reason: Some("contextual_return_flow_unavailable".into()),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                },
                || self.checkpoint(),
            )?;
            return Ok(LuaType::Unknown);
        }
        Ok(LuaType::from_vec(rows))
    }

    fn require_plain_signature_type(
        &mut self,
        ty: &LuaType,
        site: BindingSite,
    ) -> QueryResult<(), E> {
        // Arc-backed LuaType clones keep an explicit cancellable traversal;
        // nesting depth never consumes the Rust call stack. This is a type
        // frontier, not a copied analysis graph or completed ledger.
        let mut pending = vec![ty.clone()];
        while let Some(ty) = pending.pop() {
            self.checkpoint()?;
            match &ty {
                LuaType::Variadic(values) => match values.as_ref() {
                    VariadicType::Base(value) => pending.push(value.clone()),
                    VariadicType::Multi(values) => {
                        for value in values.iter().rev() {
                            self.checkpoint()?;
                            pending.push(value.clone());
                        }
                    }
                },
                LuaType::Union(union) => {
                    for value in union.into_vec().into_iter().rev() {
                        self.checkpoint()?;
                        pending.push(value);
                    }
                }
                value if super::call::plain_atom(value) => {}
                _ => {
                    return self.continuation(
                        OwnedQueryCapability::FunctionReturnType,
                        Some(site),
                        None,
                        None,
                    );
                }
            }
        }
        Ok(())
    }
}
