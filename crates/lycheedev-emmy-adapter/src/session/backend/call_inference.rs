//! Generic-constraint/flow call inference has a distinct schedule from VM calls.
//! Adapted from fixed infer/mod.rs: callee first, arguments in source order,
//! simple parameter zip, no overload/default/colon alignment pass.
use super::{OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{BindingDecision, OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, SemanticId};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaExpr, LuaSyntaxId};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn infer_owned_call_bindings(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<Option<(OwnedValue, Bindings)>, E> {
        self.checkpoint()?;
        let Some(call) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaCallExpr::cast)
        else {
            return Ok(None);
        };
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(None);
        };
        let mut function = self.type_of_expr(callee.get_syntax_id())?;
        if !matches!(function.structural_type(), LuaType::DocFunction(_)) {
            self.checkpoint()?;
            let managed = !matches!(
                self.session
                    .context()
                    .resolve(self.site(callee.get_syntax_id())),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            if managed {
                return Ok(None);
            }
            let LuaExpr::IndexExpr(index) = callee else {
                return Ok(None);
            };
            let Some(member) = self.resolve_owned_member(&index)? else {
                return Ok(None);
            };
            let Some(id @ SemanticId::Member(_)) = member.member_id else {
                return Ok(None);
            };
            let SemanticId::Member(key) = &id else {
                unreachable!();
            };
            let file = key.file_id;
            let Some(syntax) = self
                .model
                .file_facts_of(file)
                .and_then(|facts| facts.member_by_id(&id))
                .and_then(|member| member.value_syntax)
            else {
                return Ok(None);
            };
            let Ok(value) = self.signature_in(file, syntax)? else {
                return Ok(None);
            };
            function = value;
        }
        let mut arguments = Vec::new();
        if let Some(list) = call.get_args_list() {
            for arg in list.get_args() {
                arguments.push(self.type_of_expr(arg.get_syntax_id())?);
            }
        }
        let mut bindings = Bindings::new();
        for (parameter, argument) in function
            .function_parameters()
            .expect("function parameters")
            .iter()
            .zip(&arguments)
        {
            self.checkpoint()?;
            if let Some(parameter) = parameter {
                let _ = self.unify_call_bindings(parameter, argument, &mut bindings)?;
            }
        }
        let ret = self.substitute_owned(
            function.function_return().expect("function return"),
            &bindings,
        )?;
        let ret = self.expand_owned_aliases(ret)?;
        let ret = self.eval_owned_conditionals(ret)?;
        Ok(Some((ret, bindings)))
    }
}
