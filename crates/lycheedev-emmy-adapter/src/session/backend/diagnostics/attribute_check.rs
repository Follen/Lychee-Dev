//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::relation::TypeCheckFailReason;
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::humanize_semantic_type as humanize_type;
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_code_analysis::{LuaTypeDeclId, TypeDef, TypeScope};
use emmylua_parser::{
    LuaAstNode, LuaDocAttributeUse, LuaDocTagAttributeUse, LuaExpr, LuaLiteralExpr,
};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_attribute_check(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for tag in tree
            .get_red_root()
            .descendants()
            .filter_map(LuaDocTagAttributeUse::cast)
        {
            for use_ in tag.get_attribute_uses() {
                self.attribute_use(context, &use_)?;
            }
        }
        Ok(())
    }
    fn attribute_use(
        &mut self,
        context: &mut CheckContext<'_>,
        use_: &LuaDocAttributeUse,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(name) = use_.get_type() else {
            return Ok(());
        };
        let ty = self.model.doc_type_lua(name.get_syntax_id());
        let (LuaType::Ref(id) | LuaType::Def(id)) = &ty else {
            return Ok(());
        };
        if !is_attribute_class(&self.model, id) {
            return Ok(());
        }
        let args = use_
            .get_arg_list()
            .map(|list| list.get_args().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut arg_types = Vec::new();
        for arg in &args {
            arg_types.push(self.type_of_expr(LuaExpr::LiteralExpr(arg.clone()).get_syntax_id())?);
        }
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(());
        };
        let Some(func) = self.attribute_select_overload(&def, &arg_types)? else {
            return Ok(());
        };
        let LuaType::DocFunction(raw) = func.structural_type() else {
            unreachable!("attribute constructor")
        };
        check_param_count(context, raw.get_params(), use_, &args);
        let parameters = func.function_parameters().expect("function parameters");
        for (idx, ((name, _), param)) in raw.get_params().iter().zip(parameters).enumerate() {
            self.checkpoint()?;
            if name == "..." {
                if arg_types.len() < idx {
                    break;
                }
                if let Some(variadic) = param {
                    for (offset, arg_ty) in arg_types[idx..].iter().enumerate() {
                        if let Some(arg) = args.get(idx + offset) {
                            self.attribute_type_diagnostic(
                                context,
                                arg.get_range(),
                                arg_ty,
                                &variadic,
                            )?;
                        }
                    }
                }
                break;
            }
            if let Some(param) = param {
                let any = OwnedValue::plain(LuaType::Any);
                let arg_ty = arg_types.get(idx).unwrap_or(&any);
                if let Some(arg) = args.get(idx) {
                    self.attribute_type_diagnostic(context, arg.get_range(), arg_ty, &param)?;
                }
            }
        }
        Ok(())
    }
    fn attribute_select_overload(
        &mut self,
        def: &TypeDef,
        args: &[OwnedValue],
    ) -> QueryResult<Option<OwnedValue>, E> {
        let mut overloads = Vec::new();
        for syntax in &def.call_overloads {
            self.checkpoint()?;
            let ty = self.model.doc_type_lua(*syntax);
            if matches!(ty, LuaType::DocFunction(_)) {
                overloads.push(OwnedValue::plain(ty));
            }
        }
        if overloads.is_empty() {
            return Ok(None);
        }
        let only = overloads.len() == 1;
        let mut fallback = None;
        let mut count_fallback = None;
        for func in overloads {
            fallback.get_or_insert_with(|| func.clone());
            let LuaType::DocFunction(raw) = func.structural_type() else {
                unreachable!()
            };
            if !attribute_params_accept_arg_count(raw.get_params(), args.len()) {
                continue;
            }
            count_fallback.get_or_insert_with(|| func.clone());
            if only || self.attribute_accepts_args(&func, args)? {
                return Ok(Some(func));
            }
        }
        Ok(count_fallback.or(fallback))
    }
    fn attribute_accepts_args(
        &mut self,
        func: &OwnedValue,
        args: &[OwnedValue],
    ) -> QueryResult<bool, E> {
        let LuaType::DocFunction(raw) = func.structural_type() else {
            unreachable!()
        };
        for (index, ((name, _), param)) in raw
            .get_params()
            .iter()
            .zip(func.function_parameters().expect("parameters"))
            .enumerate()
        {
            if name == "..." {
                if let Some(param) = param {
                    for arg in &args[index..] {
                        if !self.type_check(arg, &param)? {
                            return Ok(false);
                        }
                    }
                }
                return Ok(true);
            }
            let Some(param) = param else {
                continue;
            };
            let Some(arg) = args.get(index) else {
                return Ok(true);
            };
            if !self.type_check(arg, &param)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn attribute_type_diagnostic(
        &mut self,
        context: &mut CheckContext<'_>,
        range: rowan::TextRange,
        arg: &OwnedValue,
        param: &OwnedValue,
    ) -> QueryResult<(), E> {
        if let Err(reason) = self.check_type_detail(arg, param)? {
            let message = match reason {
                TypeCheckFailReason::TypeNotMatchWithReason(reason) => reason,
                TypeCheckFailReason::TypeNotMatch => String::new(),
                TypeCheckFailReason::TypeRecursion => "type recursion".to_string(),
            };
            let reason = if message.is_empty() {
                String::new()
            } else {
                format!(" {}", message)
            };
            context.add_diagnostic(
                DiagnosticCode::AttributeParamTypeMismatch,
                range,
                t!(
                    "expected `%{expected}` but found `%{found}`.%{reason}",
                    expected = humanize_type(&self.model, param.structural_type()),
                    found = humanize_type(&self.model, arg.structural_type()),
                    reason = reason
                ),
            );
        }
        Ok(())
    }
}
fn is_attribute_class(
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    decl_id: &LuaTypeDeclId,
) -> bool {
    let Some(def) = semantic_model.type_def_of(decl_id) else {
        return false;
    };
    let mut visited = vec![def.id.clone()];
    let mut stack = vec![def];
    while let Some(def) = stack.pop() {
        if def.full_name == "Attribute" {
            return true;
        }
        for super_name in &def.super_names {
            let super_def = semantic_model
                .type_defs_in_scope(TypeScope::Global, super_name.as_str())
                .iter()
                .next()
                .cloned();
            if let Some(super_def) = super_def
                && !visited.contains(&super_def.id)
            {
                visited.push(super_def.id.clone());
                stack.push(super_def.clone());
            }
        }
    }
    false
}

fn attribute_params_accept_arg_count(
    def_params: &[(String, Option<LuaType>)],
    arg_count: usize,
) -> bool {
    let required_count = def_params
        .iter()
        .take_while(|(name, typ)| name != "..." && !typ.as_ref().is_some_and(LuaType::is_variadic))
        .filter(|(_, typ)| !typ.as_ref().is_some_and(LuaType::is_optional))
        .count();
    let allows_more = def_params
        .last()
        .is_some_and(|(name, typ)| name == "..." || typ.as_ref().is_some_and(LuaType::is_variadic));
    arg_count >= required_count && (allows_more || arg_count <= def_params.len())
}

fn check_param_count(
    context: &mut CheckContext<'_>,
    def_params: &[(String, Option<LuaType>)],
    attribute_use: &LuaDocAttributeUse,
    args: &[LuaLiteralExpr],
) {
    let call_args_count = args.len();
    if call_args_count < def_params.len() {
        for def_param in def_params[call_args_count..].iter() {
            if def_param.0 == "..." {
                break;
            }
            if def_param.1.as_ref().is_some_and(LuaType::is_optional) {
                continue;
            }
            context.add_diagnostic(
                DiagnosticCode::AttributeMissingParameter,
                args.last()
                    .map(|arg| arg.get_range())
                    .unwrap_or_else(|| attribute_use.get_range()),
                t!(
                    "expected %{expected} parameters but found %{found}",
                    expected = def_params.len(),
                    found = call_args_count
                ),
            );
        }
    } else if call_args_count > def_params.len() {
        if def_params.last().is_some_and(|(name, typ)| {
            name == "..." || typ.as_ref().is_some_and(LuaType::is_variadic)
        }) {
            return;
        }
        for arg in args[def_params.len()..].iter() {
            context.add_diagnostic(
                DiagnosticCode::AttributeRedundantParameter,
                arg.get_range(),
                t!(
                    "expected %{expected} parameters but found %{found}",
                    expected = def_params.len(),
                    found = call_args_count
                ),
            );
        }
    }
}
