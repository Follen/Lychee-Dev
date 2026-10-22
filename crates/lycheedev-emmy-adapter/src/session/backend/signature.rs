//! Cross-file selected Function signatures using public doc/fact projection and
//! owned contextual return values. No signature may erase a selected return.
mod expected;
mod local;

use super::return_pack::{return_arity, return_expands};
use super::{OwnedQuery, OwnedQueryCapability, ProjectionResult, QueryResult};
use crate::session::{BindingCoverage, BindingSite, OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    AsyncState, FileId, LuaFunctionType, LuaSignatureId, LuaType, LuaUnionType, Signature,
    VariadicType,
};
use emmylua_parser::{
    LuaAstNode, LuaClosureExpr, LuaCommentOwner, LuaDocTag, LuaExpr, LuaReturnStat, LuaStat,
    LuaSyntaxId, LuaTableField,
};
use std::sync::Arc;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn project_function_signature(
        &mut self,
        _projection_site: BindingSite,
        closure: LuaSyntaxId,
    ) -> ProjectionResult<OwnedValue, super::OwnedQueryFailure<E>> {
        self.signature_in(self.model.file_id(), closure)
    }

    pub(super) fn signature_by_id(
        &mut self,
        id: &LuaSignatureId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        // FileFacts exposes signatures but its positional index is private.
        // Reproduce the index's last-insertion-wins rule, including collisions,
        // without another upstream visibility patch or contextual inference.
        let mut closure = None;
        if let Some(facts) = self.model.file_facts_of(id.get_file_id()) {
            for signature in facts.signatures.iter().rev() {
                self.checkpoint()?;
                if signature.closure_syntax.get_range().start() == id.get_position() {
                    closure = Some(signature.closure_syntax);
                    break;
                }
            }
        }
        let Some(closure) = closure else {
            return Ok(None);
        };
        self.signature_in(id.get_file_id(), closure)
            .map(|value| value.ok())
    }

    pub(super) fn signature_in(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
    ) -> ProjectionResult<OwnedValue, super::OwnedQueryFailure<E>> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(Err("selected_function_signature_unavailable"));
        };
        let Some(signature) = facts.signature_by_closure(closure) else {
            return Ok(Err("selected_function_signature_unavailable"));
        };
        let generics = signature
            .docs
            .as_ref()
            .map(|docs| docs.generic_params.as_slice())
            .unwrap_or(&[]);
        self.checkpoint()?;
        let generic_params = self.model.generic_tpls_with_metadata(file, generics);
        self.checkpoint()?;
        let mut params = Vec::new();
        for name in &signature.param_names {
            self.checkpoint()?;
            // The public doc_type_lua_in is exactly lower_doc_type followed by
            // type_shell_lua_in with these signature generic names. Unlike
            // param_type(), it does not trigger a member-parameter fallback that
            // the original cross-file signature producer never ran.
            let syntax = signature.docs.as_ref().and_then(|docs| {
                docs.param_types
                    .iter()
                    .find(|(param, _)| param == name)
                    .map(|(_, syntax)| *syntax)
            });
            let mut ty = syntax
                .map(|syntax| self.model.doc_type_lua_in(file, syntax, generics))
                .filter(|ty| !matches!(ty, LuaType::Unknown))
                .or_else(|| syntax.map(|syntax| self.model.doc_type_lua_rich_in(file, syntax)))
                .unwrap_or(LuaType::Any);
            self.checkpoint()?;
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
        let mut ret = self.contextual_signature_return(file, signature)?;
        ret = self.expand_aliases(ret)?;
        self.checkpoint()?;
        // The fixed in-file function wrapper invokes closure_return_lua again
        // for Unknown without docs. That helper has a distinct inherited VM /
        // flow-cast schedule and may recover a value; silently returning Unknown
        // here would weaken the original contract.
        if matches!(ret.structural_type(), LuaType::Unknown)
            && signature.docs.is_none()
            && file == self.model.file_id()
        {
            let inferred = self.closure_return_with_env(closure)?;
            if !matches!(inferred.structural_type(), LuaType::Unknown | LuaType::Any) {
                ret = inferred;
            }
        }
        if signature.docs.is_none() {
            (params, ret) = self.table_field_signature_override(file, closure, params, ret)?;
        }
        let async_state = if signature.docs.as_ref().is_some_and(|docs| docs.is_async) {
            AsyncState::Async
        } else {
            AsyncState::None
        };
        self.checkpoint()?;
        let metadata = LuaFunctionType::new(
            async_state,
            signature.is_method,
            signature.is_variadic,
            params,
            ret.structural_type().clone(),
            Some(generic_params),
        );
        let params = metadata
            .get_params()
            .iter()
            .map(|(_, ty)| ty.clone().map(OwnedValue::plain))
            .collect();
        Ok(Ok(OwnedValue::function(&metadata, params, ret)))
    }

    pub(super) fn contextual_signature_return(
        &mut self,
        file: FileId,
        signature: &Signature,
    ) -> QueryResult<OwnedValue, E> {
        if let Some(docs) = &signature.docs {
            if !docs.returns.is_empty() {
                let mut returns = Vec::new();
                for syntax in &docs.returns {
                    self.checkpoint()?;
                    returns.push(self.model.doc_type_lua_rich_in(file, *syntax));
                    self.checkpoint()?;
                }
                return Ok(OwnedValue::plain(if returns.len() == 1 {
                    returns.remove(0)
                } else {
                    LuaType::Variadic(Arc::new(VariadicType::Multi(returns)))
                }));
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
                        return Ok(OwnedValue::plain(LuaType::Unknown));
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
                return Ok(OwnedValue::plain(if index == docs.return_overloads.len() {
                    LuaType::from_vec(rows)
                } else {
                    LuaType::Unknown
                }));
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
            return Ok(OwnedValue::plain(LuaType::Unknown));
        };
        let mut body = OwnedQuery::new(self.analysis, session, file, &mut *self.check);
        body.contribution_order = self.contribution_order.clone();
        body.workspace_facts = Arc::clone(&self.workspace_facts);
        body.flow_trees = std::rc::Rc::clone(&self.flow_trees);
        body.initializer_selection = std::rc::Rc::clone(&self.initializer_selection);
        body.contextual_closure_return(signature.closure_syntax)
    }

    fn contextual_closure_return(&mut self, syntax: LuaSyntaxId) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let Some(closure) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaClosureExpr::cast)
        else {
            return Ok(OwnedValue::plain(LuaType::Unknown));
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
                return Ok(OwnedValue::plain(LuaType::Unknown));
            }
            let mut exprs = Vec::new();
            for expr in ret.get_expr_list() {
                self.checkpoint()?;
                exprs.push(expr);
                if exprs.len() > 32 {
                    return Ok(OwnedValue::plain(LuaType::Unknown));
                }
            }
            let mut slots = Vec::new();
            for (index, expr) in exprs.iter().enumerate() {
                self.checkpoint()?;
                let ty = self.type_of_expr(expr.get_syntax_id())?;
                if matches!(ty.structural_type(), LuaType::Never) {
                    slots.clear();
                    slots.push(ty);
                    break;
                }
                if return_expands(expr, index + 1 == exprs.len()) {
                    if matches!(ty.structural_type(), LuaType::Unknown | LuaType::Any) {
                        slots.push(OwnedValue::plain(LuaType::Variadic(Arc::new(
                            VariadicType::Base(LuaType::Unknown),
                        ))));
                    } else if let Some(arity) = return_arity(ty.structural_type()) {
                        if matches!(ty.structural_type(), LuaType::Union(_)) {
                            slots.push(ty);
                        } else if let Some(count) = arity.max.filter(|max| *max == arity.min) {
                            for slot in 0..count {
                                if let Some(value) = self.owned_return_slot(&ty, slot)? {
                                    slots.push(value);
                                }
                            }
                        } else {
                            // Preserve the original branch/tail value instead
                            // of flattening an unknown length into fixed slots.
                            slots.push(ty);
                        }
                    } else {
                        slots.push(ty);
                    }
                } else {
                    slots.push(self.owned_return_scalar(&ty)?);
                }
                if slots.len() > 32 {
                    return Ok(OwnedValue::plain(LuaType::Unknown));
                }
            }
            rows.push(if slots.len() == 1 {
                slots.remove(0)
            } else {
                OwnedValue::pack(slots)
            });
        }
        if rows.is_empty() {
            return Ok(OwnedValue::pack(Vec::new()));
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
            return Ok(OwnedValue::plain(LuaType::Unknown));
        }
        OwnedValue::union_checked(rows, || self.checkpoint())
    }

    fn table_field_signature_override(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
        mut params: Vec<(String, Option<LuaType>)>,
        mut ret: OwnedValue,
    ) -> QueryResult<(Vec<(String, Option<LuaType>)>, OwnedValue), E> {
        self.checkpoint()?;
        if self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.member_by_value_syntax(closure))
            .is_none()
        {
            return Ok((params, ret));
        }
        let field = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| closure.to_node_from_root(&tree.get_red_root()))
            .and_then(|node| node.ancestors().find_map(LuaTableField::cast));
        let Some(field) = field else {
            return Ok((params, ret));
        };
        let mut returns = Vec::new();
        let mut has_return = false;
        for comment in field.get_comments() {
            self.checkpoint()?;
            for tag in comment.get_doc_tags() {
                self.checkpoint()?;
                match tag {
                    LuaDocTag::Param(param) => {
                        if let Some(name) = param.get_name_token()
                            && let Some(doc_ty) = param.get_type()
                        {
                            let mut ty = self
                                .model
                                .doc_type_lua_rich_in(file, doc_ty.get_syntax_id());
                            self.checkpoint()?;
                            if param.is_nullable() {
                                ty = LuaType::from_vec(vec![ty, LuaType::Nil]);
                            }
                            if let Some(slot) =
                                params.iter_mut().find(|(n, _)| n == name.get_name_text())
                            {
                                slot.1 = Some(ty);
                            }
                        }
                    }
                    LuaDocTag::Return(tag) => {
                        has_return = true;
                        for doc_ty in tag.get_types() {
                            self.checkpoint()?;
                            let ty = self
                                .model
                                .doc_type_lua_rich_in(file, doc_ty.get_syntax_id());
                            if !matches!(ty, LuaType::Unknown) {
                                returns.push(OwnedValue::plain(ty));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if has_return && !returns.is_empty() {
            ret = if returns.len() == 1 {
                returns.pop().unwrap()
            } else {
                OwnedValue::pack(returns)
            };
        }
        self.checkpoint()?;
        Ok((params, ret))
    }
}
