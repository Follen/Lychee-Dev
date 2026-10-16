//! Owned call-argument type checker. Adapted from EmmyLuaAnalyzer (MIT),
//! 08b0873962e83bb8307e9a81052e3f41578fbc78, check/checker/param_type_check.rs.
//! Every inferred value, candidate, binding and child retains its owned carrier.
mod arguments;
mod function_solver;
mod shapes;
mod types;

use super::CheckContext;
use crate::session::backend::{
    OwnedQuery, QueryResult, call_access::first_param_is_self, call_unify::Bindings,
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, LuaMemberKey, LuaTupleStatus, LuaType, LuaTypeDeclId, LuaTypeNode,
    SemanticModel, TypeDef, TypeDefKind, VariadicType, humanize_semantic_type,
};
use emmylua_parser::{
    LuaAst, LuaAstNode, LuaCallExpr, LuaExpr, LuaIndexExpr, LuaSyntaxId, LuaTokenKind,
};

struct Mismatch {
    range: rowan::TextRange,
    message: String,
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_param_type(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for node in root.descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            let LuaAst::LuaCallExpr(call) = node else {
                continue;
            };
            self.pt_check_call(context, &call)?;
            self.binding_finish_call_check(&call, "argument_check")?;
        }
        Ok(())
    }

    fn pt_name_initializer_managed(&mut self, callee: &LuaExpr) -> QueryResult<bool, E> {
        let LuaExpr::NameExpr(name) = callee else {
            return Ok(false);
        };
        self.checkpoint()?;
        match self.model.resolve_name(name.get_position()) {
            Some(decl) => self.owner_initializer_managed(&decl),
            None => Ok(false),
        }
    }

    fn pt_check_pcall_forward(
        &mut self,
        context: &mut CheckContext<'_>,
        args: &[LuaExpr],
    ) -> QueryResult<bool, E> {
        let Some(callback) = args.first() else {
            return Ok(false);
        };
        let candidates = self.callable_candidates_cached(callback)?;
        if candidates.is_empty() {
            return Ok(false);
        }
        let mut best: Option<Vec<Mismatch>> = None;
        for candidate in &candidates {
            self.checkpoint()?;
            let mismatches = self.pt_check_candidate(
                candidate,
                &args[1..],
                false,
                &OwnedValue::plain(LuaType::Unknown),
                &[],
                None,
            )?;
            if mismatches.is_empty() {
                return Ok(true);
            }
            if best
                .as_ref()
                .is_none_or(|current| mismatches.len() < current.len())
            {
                best = Some(mismatches);
            }
        }
        if let Some(mismatches) = best {
            for mismatch in mismatches {
                context.add_diagnostic(
                    DiagnosticCode::ParamTypeMismatch,
                    mismatch.range,
                    mismatch.message,
                );
            }
        }
        Ok(true)
    }

    fn pt_check_call(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        if !self.binding_call_allowed(call, "argument_check")? {
            return Ok(());
        }
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(());
        };
        let analysis = self.call_site_analysis(call)?;
        let mut candidates = analysis.candidates;
        if !self.binding_manages(callee.get_syntax_id())?
            && let LuaExpr::NameExpr(name) = &callee
            && let Some(decl) = self.model.resolve_name(name.get_position())
            && !self.owner_initializer_managed(&decl)?
            && let Some(facts) = self.model.file_facts()
            && let Some(decl) = facts.decl_by_id(&decl)
            && let Some(syntax) = decl.doc_type_syntax
        {
            let ty = OwnedValue::plain(self.model.doc_type_lua(syntax));
            if let Some(function) = self.pt_doc_function(&ty)? {
                candidates = vec![function];
            }
        }
        let args = call
            .get_args_list()
            .map(|list| list.get_args().collect::<Vec<_>>())
            .unwrap_or_default();
        if !self.binding_manages(callee.get_syntax_id())?
            && let LuaExpr::NameExpr(name) = &callee
            && name.get_name_text().as_deref() == Some("pcall")
            && !self.pt_name_initializer_managed(&callee)?
            && !args.is_empty()
            && self.pt_check_pcall_forward(context, &args)?
        {
            return Ok(());
        }
        if candidates.is_empty() {
            return Ok(());
        }
        let mut ordered: Vec<&OwnedValue> = candidates.iter().collect();
        ordered.sort_by_key(|function| {
            quick_candidate_score(
                function,
                &analysis.arg_types,
                analysis.colon_call,
                &analysis.receiver_ty,
            )
        });
        let mut best: Option<Vec<Mismatch>> = None;
        for candidate in ordered {
            let mismatches = self.pt_check_candidate(
                candidate,
                &args,
                analysis.colon_call,
                &analysis.receiver_ty,
                &analysis.explicit_generics,
                Some(&analysis.arg_types),
            )?;
            if mismatches.is_empty() {
                return Ok(());
            }
            if best
                .as_ref()
                .is_none_or(|current| mismatches.len() < current.len())
            {
                best = Some(mismatches);
            }
        }
        if let Some(mismatches) = best {
            let literal_names = if self.binding_manages(callee.get_syntax_id())?
                || self.pt_name_initializer_managed(&callee)?
            {
                Vec::new()
            } else {
                doc_overload_literal_names(&self.model, &callee)
            };
            for mismatch in mismatches {
                let message = if literal_names.len() >= 2 {
                    let found = mismatch
                        .message
                        .split_once("but found")
                        .map(|(_, found)| format!("but found{found}"))
                        .unwrap_or_else(|| mismatch.message.clone());
                    format!("expected `{}` {found}", literal_names.join(" | "))
                } else {
                    mismatch.message
                };
                context.add_diagnostic(DiagnosticCode::ParamTypeMismatch, mismatch.range, message);
            }
        }
        Ok(())
    }

    fn pt_check_candidate(
        &mut self,
        function: &OwnedValue,
        args: &[LuaExpr],
        colon_call: bool,
        receiver: &OwnedValue,
        explicit_generics: &[LuaSyntaxId],
        arg_types: Option<&[OwnedValue]>,
    ) -> QueryResult<Vec<Mismatch>, E> {
        let LuaType::DocFunction(meta) = function.structural_type() else {
            unreachable!("call candidate")
        };
        let params = meta
            .get_params()
            .iter()
            .map(|(name, _)| name.clone())
            .zip(function.function_parameters().expect("function parameters"))
            .collect::<Vec<_>>();
        let self_param = first_param_is_self(function);
        let param_start = usize::from(colon_call && self_param).min(params.len());
        let dot_receiver = !colon_call && !self_param && meta.is_colon_define();
        let arg_offset = usize::from(dot_receiver).min(args.len());
        let mut bindings = Bindings::new();
        for (index, syntax) in explicit_generics.iter().enumerate() {
            self.checkpoint()?;
            if let Some(tpl) = meta.get_generic_params().get(index) {
                let ty = self
                    .model
                    .doc_type_lua_in(self.model.file_id(), *syntax, &[]);
                bindings.insert(tpl.get_tpl_id(), OwnedValue::plain(ty));
            }
        }
        if colon_call && !self_param && !meta.is_colon_define() {
            let mut mismatches = Vec::new();
            if let Some((name, Some(param))) = params.first() {
                let param = effective_param(param, receiver);
                let _ = self.unify_call_bindings(&param, receiver, &mut bindings)?;
                if !self.type_check(receiver, &param)? {
                    mismatches.push(Mismatch {
                        range: receiver_range(args),
                        message: self.pt_mismatch_message(receiver, &param, name),
                    });
                }
            }
            let remainder = if params.is_empty() {
                params.as_slice()
            } else {
                &params[1..]
            };
            mismatches.extend(self.pt_check_arg_pairs(
                remainder,
                args,
                meta.is_variadic(),
                &mut bindings,
                arg_types,
            )?);
            return Ok(mismatches);
        }
        let dot_types = arg_types.map(|types| &types[arg_offset.min(types.len())..]);
        self.pt_check_arg_pairs(
            &params[param_start..],
            &args[arg_offset..],
            meta.is_variadic(),
            &mut bindings,
            dot_types,
        )
    }

    pub(super) fn pt_mismatch_message(
        &self,
        arg: &OwnedValue,
        param: &OwnedValue,
        name: &str,
    ) -> String {
        let _ = name;
        t!(
            "expected `%{source}` but found `%{found}`. %{reason}",
            source = humanize_semantic_type(&self.model, param.structural_type()),
            found = humanize_semantic_type(&self.model, arg.structural_type()),
            reason = ""
        )
    }
}

fn receiver_range(args: &[LuaExpr]) -> rowan::TextRange {
    args.first().map(|arg| arg.get_range()).unwrap_or_default()
}

use shapes::has_table_literal_shape;
use types::{effective_param, skip_arg};

fn doc_overload_literal_names(semantic_model: &SemanticModel<'_>, callee: &LuaExpr) -> Vec<String> {
    let LuaExpr::NameExpr(name_expr) = callee else {
        return Vec::new();
    };
    let Some(decl) = semantic_model.resolve_name(name_expr.get_position()) else {
        return Vec::new();
    };
    let Some(facts) = semantic_model.file_facts() else {
        return Vec::new();
    };
    let Some(decl_facts) = facts.decl_by_id(&decl) else {
        return Vec::new();
    };
    let Some(doc_syntax) = decl_facts.doc_type_syntax else {
        return Vec::new();
    };
    let Some(tree) = semantic_model.syntax_tree() else {
        return Vec::new();
    };
    let Some(node) = doc_syntax.to_node_from_root(&tree.get_red_root()) else {
        return Vec::new();
    };
    node.descendants_with_tokens()
        .filter_map(|item| item.into_token())
        .filter(|token| token.kind() == LuaTokenKind::TkString.into())
        .map(|token| token.text().trim_matches(['"', '\'']).to_string())
        .collect()
}

fn quick_candidate_score(
    function: &OwnedValue,
    arg_types: &[OwnedValue],
    colon_call: bool,
    receiver_ty: &OwnedValue,
) -> usize {
    let LuaType::DocFunction(func) = function.structural_type() else {
        unreachable!("call candidate")
    };
    let params = func.get_params();
    let self_param = first_param_is_self(function);
    let param_start = usize::from(colon_call && self_param);
    let dot_method_receiver = !colon_call && !self_param && func.is_colon_define();
    let arg_offset = usize::from(dot_method_receiver);
    let mut score = 0usize;

    let mut check_pair = |arg_ty: &LuaType, param_ty: &LuaType| {
        if obvious_scalar_mismatch(arg_ty, param_ty) {
            score += 1;
        }
    };

    if colon_call && !self_param && !func.is_colon_define() {
        if let Some((_, Some(param_ty))) = params.first() {
            check_pair(receiver_ty.structural_type(), param_ty);
        }
        let remainder = if params.is_empty() {
            params
        } else {
            &params[1..]
        };
        for (index, (_, param_ty)) in remainder.iter().enumerate() {
            let Some(param_ty) = param_ty else {
                continue;
            };
            let Some(arg_ty) = arg_types.get(index) else {
                break;
            };
            check_pair(arg_ty.structural_type(), param_ty);
        }
    } else {
        for (index, (_, param_ty)) in params[param_start..].iter().enumerate() {
            let Some(param_ty) = param_ty else {
                continue;
            };
            let Some(arg_ty) = arg_types.get(index + arg_offset) else {
                break;
            };
            check_pair(arg_ty.structural_type(), param_ty);
        }
    }
    score
}

fn obvious_scalar_mismatch(arg_ty: &LuaType, param_ty: &LuaType) -> bool {
    match param_ty {
        LuaType::Any | LuaType::Unknown | LuaType::Never => return false,
        LuaType::Union(union) => {
            return !union
                .into_vec()
                .iter()
                .any(|component| !obvious_scalar_mismatch(arg_ty, component));
        }
        _ => {}
    }
    match (arg_ty, param_ty) {
        (
            LuaType::StringConst(_) | LuaType::DocStringConst(_),
            LuaType::String
            | LuaType::StringConst(_)
            | LuaType::DocStringConst(_)
            | LuaType::StrTplRef(_),
        ) => false,
        (LuaType::StringConst(_) | LuaType::DocStringConst(_), _) => true,
        (
            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
            LuaType::Integer
            | LuaType::Number
            | LuaType::IntegerConst(_)
            | LuaType::DocIntegerConst(_),
        ) => false,
        (LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_), _) => true,
        (
            LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_),
            LuaType::Boolean | LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_),
        ) => false,
        (LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_), _) => true,
        _ => false,
    }
}
