//! Adapted from fixed Emmy call_non_callable.rs; Copyright (c) CppCXY, MIT.
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{DiagnosticCode, LuaType, TypeDefKind, humanize_semantic_type};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaExpr, LuaLocalStat};

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_call_non_callable(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for call in root.descendants().filter_map(LuaCallExpr::cast) {
            self.checkpoint()?;
            self.check_non_callable_call(context, &call)?;
            self.binding_finish_call_check(&call, "callable_check")?;
        }
        Ok(())
    }

    fn check_non_callable_call(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        if !self.binding_call_allowed(call, "callable_check")? {
            return Ok(());
        }
        let Some(prefix) = call.get_prefix_expr() else {
            return Ok(());
        };
        let value = if let Some(value) = self.binding_type_of_expr(prefix.get_syntax_id())? {
            value
        } else {
            match &prefix {
                LuaExpr::NameExpr(name) => {
                    if let Some(decl) = self.model.resolve_name(name.get_position()) {
                        if call
                            .syntax()
                            .ancestors()
                            .any(|node| LuaLocalStat::cast(node).is_some())
                        {
                            self.type_of_decl_assign_target_at(&decl, name.get_position().into())?
                        } else {
                            self.type_of_decl_at(&decl, name.get_position().into())?
                        }
                    } else {
                        self.type_of_expr(prefix.get_syntax_id())?
                    }
                }
                _ => self.type_of_expr(prefix.get_syntax_id())?,
            }
        };
        let selected_nil = self.binding_manages(prefix.get_syntax_id())?
            && matches!(value.structural_type(), LuaType::Nil);
        if !selected_nil && self.diagnostic_is_callable(&value)? {
            return Ok(());
        }
        context.add_diagnostic(
            DiagnosticCode::CallNonCallable,
            prefix.get_range(),
            t!(
                "Cannot call expression of type `%{name}`.",
                name = humanize_semantic_type(&self.model, value.structural_type())
            ),
        );
        Ok(())
    }

    fn diagnostic_is_callable(&mut self, value: &OwnedValue) -> QueryResult<bool, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Function
            | LuaType::DocFunction(_)
            | LuaType::Signature(_)
            | LuaType::Any
            | LuaType::Unknown
            | LuaType::Nil
            | LuaType::SelfInfer
            | LuaType::Global => Ok(true),
            LuaType::TplRef(template) => match template.get_constraint() {
                Some(constraint) => {
                    self.diagnostic_is_callable(&self.owned_child(value, 0, constraint))
                }
                None => Ok(false),
            },
            LuaType::Union(union) => {
                for (index, ty) in union.into_vec().iter().enumerate() {
                    if !self.diagnostic_is_callable(&self.owned_child(value, index, ty))? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LuaType::Intersection(intersection) => {
                for (index, ty) in intersection.get_types().iter().enumerate() {
                    if self.diagnostic_is_callable(&self.owned_child(value, index, ty))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(true);
                };
                if def.kind == TypeDefKind::Alias
                    && let Some(target) = self.model.alias_target(&def).map(OwnedValue::plain)
                {
                    return self.diagnostic_is_callable(&target);
                }
                Ok(self
                    .model
                    .file_facts_of(def.file_id)
                    .is_some_and(|facts| facts.operator_of(&def.id, "call").is_some()))
            }
            _ => Ok(false),
        }
    }
}
