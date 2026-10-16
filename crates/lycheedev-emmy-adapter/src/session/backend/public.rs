//! Public owned query surfaces used by Source consumers and their regressions.
//! Algorithms adapted from the fixed Emmy semantic_model signature/infer APIs;
//! Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::{OwnedQuery, OwnedResolvedMember, QueryResult};
use crate::session::{
    BindingDecision, BoundValue, OwnedValue, SelectedProjectionBackend, SelectedProjector,
};
use emmylua_code_analysis::{FileId, LuaType, VariadicType};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaIndexExpr, LuaSyntaxId};

impl<E> OwnedQuery<'_, '_, E> {
    /// Frozen decision inspection has no type projection or ledger side effect.
    pub fn binding_decision(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<BindingDecision<BoundValue>, E> {
        self.checkpoint()?;
        let result = if self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .is_none()
        {
            BindingDecision::Unavailable {
                proof: String::new(),
                reason: "binding_syntax_missing".into(),
            }
        } else {
            self.session.context().resolve(self.site(syntax))
        };
        self.checkpoint()?;
        Ok(result)
    }

    pub fn type_of_signature(
        &mut self,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.signature_local(closure)
    }

    pub fn type_of_signature_in_file(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.signature_in(file, closure).map(Result::ok)
    }

    pub fn return_type(&mut self, closure: LuaSyntaxId) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(signature) = self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure))
        else {
            return Ok(None);
        };
        // Keep contextual body inference and Some(Unknown), but retain the
        // ordinary public return query's normalization before its consumers
        // compare values: `self` denotes this member's owner, then aliases
        // expand. The historical patched session early return omitted these
        // steps; using its raw SelfInfer here accepts unrelated named classes.
        // Both transformations keep selected carriers and the original control.
        let mut value = self.contextual_signature_return(self.model.file_id(), signature)?;
        if let Some(docs) = &signature.docs
            && (!docs.returns.is_empty() || !docs.return_overload_rows.is_empty())
            && !docs.generic_params.is_empty()
        {
            value = self.bind_owned_return_generics(value, &docs.generic_params)?;
        }
        if let Some(owner) = self.method_owner_type(closure)? {
            value = self.replace_owned_self(value, &owner)?;
        }
        self.expand_aliases(value).map(Some)
    }

    pub fn resolve_member(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<OwnedResolvedMember>, E> {
        self.resolve_owned_member(index)
    }

    pub fn resolved_member_type(
        &mut self,
        member: &OwnedResolvedMember,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if member
            .binding_token
            .as_ref()
            .is_some_and(|token| *token != self.session.token)
        {
            return Ok(None);
        }
        if let Some(value) = &member.member_type {
            return Ok(Some(value.clone()));
        }
        let Some(id) = member.member_id.as_ref() else {
            return Ok(None);
        };
        self.type_of_member(id)
    }

    /// Fixed multi-expression producer, including its early variadic stop and
    /// original expression-index based truncation. Inference is intentionally
    /// uncached at this outer entry, just like upstream infer_expr_list_types;
    /// child inference still uses this model's existing caches and sole ledger.
    pub fn infer_expr_list_types(
        &mut self,
        expressions: &[LuaExpr],
        variable_count: Option<usize>,
    ) -> QueryResult<Vec<(OwnedValue, rowan::TextRange)>, E> {
        let mut values = Vec::new();
        for (index, expression) in expressions.iter().enumerate() {
            self.checkpoint()?;
            if variable_count.is_some_and(|count| values.len() >= count) {
                break;
            }
            let projector = SelectedProjector::new(self.analysis, self.session);
            let value = {
                let _scope = self.enter_execution(std::collections::HashMap::new());
                projector.evaluate_expression(self.site(expression.get_syntax_id()), self)?
            };
            if let Some(count) = variable_count
                && value.structural_type().contain_multi_return()
            {
                if index < count {
                    for slot in index..count {
                        self.checkpoint()?;
                        let Some(typ) = value.result_slot(slot - index) else {
                            break;
                        };
                        values.push((typ, expression.get_range()));
                    }
                }
                break;
            }
            match value.structural_type() {
                LuaType::Variadic(variadic) => {
                    match variadic.as_ref() {
                        VariadicType::Base(base) => {
                            values
                                .push((self.owned_child(&value, 0, base), expression.get_range()));
                        }
                        VariadicType::Multi(types) => {
                            for (slot, typ) in types.iter().enumerate() {
                                self.checkpoint()?;
                                values.push((
                                    self.owned_child(&value, slot, typ),
                                    expression.get_range(),
                                ));
                            }
                        }
                    }
                    break;
                }
                _ => values.push((value, expression.get_range())),
            }
        }
        self.checkpoint()?;
        Ok(values)
    }
}
