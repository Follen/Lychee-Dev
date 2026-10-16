//! Adapted from fixed Emmy return_type_mismatch.rs; Copyright (c) CppCXY, MIT.
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, LuaType, SemanticId, SemanticModel, SignatureDoc, TypeDef, TypeScope,
    humanize_semantic_type,
};
use emmylua_parser::{LuaAstNode, LuaClosureExpr, LuaReturnStat};

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_return_type_mismatch(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(signatures) = self.model.signatures() else {
            return Ok(());
        };
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        let default_docs = SignatureDoc::default();
        for signature in signatures {
            self.checkpoint()?;
            let Some(node) = signature.closure_syntax.to_node_from_root(&root) else {
                continue;
            };
            let Some(closure) = LuaClosureExpr::cast(node) else {
                continue;
            };
            let (expected, docs) = if let Some(docs) = signature.docs.as_deref() {
                if docs.returns.is_empty() {
                    continue;
                }
                (
                    self.contextual_signature_return(self.model.file_id(), signature)?,
                    docs,
                )
            } else {
                let Some(function) = self
                    .expected_member_signatures_for_closure(signature.closure_syntax)?
                    .and_then(|functions| functions.into_iter().next())
                else {
                    continue;
                };
                let Some(expected) = function.function_return() else {
                    continue;
                };
                (expected, &default_docs)
            };
            for ret in own_return_stats(&closure) {
                self.check_return_stat(context, &ret, &expected, docs)?;
            }
        }
        Ok(())
    }

    fn check_return_stat(
        &mut self,
        context: &mut CheckContext<'_>,
        ret: &LuaReturnStat,
        expected: &OwnedValue,
        docs: &SignatureDoc,
    ) -> QueryResult<(), E> {
        for expression in ret.get_expr_list() {
            self.checkpoint()?;
            let mut raw = self.type_of_expr_at(
                expression.get_syntax_id(),
                expression.get_range().start().into(),
            )?;
            if let Some(ty) = inline_as_type(&self.model, ret, &expression) {
                raw = OwnedValue::plain(ty);
            }
            if is_unresolved_generic_ref(&self.model, raw.structural_type(), docs) {
                if let Some(constraint) =
                    generic_constraint_type(&self.model, raw.structural_type())
                {
                    let constraint = OwnedValue::plain(constraint);
                    if !self.return_compatible(&constraint, expected)? {
                        add_mismatch(context, expression.get_range(), expected, &constraint);
                    }
                    self.finish_return_quality(&expression, &constraint, expected, true)?;
                } else {
                    self.finish_return_quality(&expression, &raw, expected, false)?;
                }
                continue;
            }
            let actual = generic_constraint_type(&self.model, raw.structural_type())
                .map(OwnedValue::plain)
                .unwrap_or(raw);
            if matches!(
                actual.structural_type(),
                LuaType::Unknown | LuaType::Any | LuaType::Never
            ) {
                self.finish_return_quality(&expression, &actual, expected, false)?;
                continue;
            }
            if !self.return_compatible(&actual, expected)? {
                add_mismatch(context, expression.get_range(), expected, &actual);
            }
            self.finish_return_quality(&expression, &actual, expected, true)?;
        }
        Ok(())
    }

    fn return_compatible(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        if !self.structure_allowed(source)? || !self.structure_allowed(target)? {
            return Ok(false);
        }
        if source == target {
            return Ok(true);
        }
        if let LuaType::Union(union) = source.structural_type() {
            let components = union.into_vec();
            let target_is_named =
                matches!(target.structural_type(), LuaType::Ref(_) | LuaType::Def(_));
            let has_function = components.iter().any(|component| {
                matches!(
                    component,
                    LuaType::Function | LuaType::DocFunction(_) | LuaType::Signature(_)
                )
            });
            let require_all = target_is_named && has_function;
            for (index, component) in components.iter().enumerate() {
                let component = self.owned_child(source, index, component);
                let compatible = self.return_compatible(&component, target)?;
                if require_all && !compatible {
                    return Ok(false);
                }
                if !require_all && compatible {
                    return Ok(true);
                }
            }
            return Ok(require_all);
        }
        if matches!(
            source.structural_type(),
            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_)
        ) && matches!(target.structural_type(), LuaType::Number)
        {
            return Ok(true);
        }
        if let (LuaType::Generic(generic), LuaType::Array(array)) =
            (source.structural_type(), target.structural_type())
            && generic.get_base_type_id().get_name() == "table"
            && let Some(value) = generic.get_params().get(1)
        {
            let value = self.owned_child(source, 1, value);
            let base = self.owned_child(target, 0, array.get_base());
            if self.return_compatible(&value, &base)? {
                return Ok(true);
            }
        }
        if self.type_check(source, target)? {
            return Ok(true);
        }
        if let LuaType::Union(union) = target.structural_type() {
            for (index, component) in union.into_vec().iter().enumerate() {
                let component = self.owned_child(target, index, component);
                if self.return_compatible(source, &component)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        self.checkpoint()?;
        Ok(generic_extends_named(
            &self.model,
            source.structural_type(),
            target.structural_type(),
        ))
    }
}

fn own_return_stats(closure: &LuaClosureExpr) -> Vec<LuaReturnStat> {
    closure
        .descendants::<LuaReturnStat>()
        .filter(|stat| {
            stat.ancestors::<LuaClosureExpr>()
                .next()
                .is_some_and(|expr| &expr == closure)
        })
        .collect()
}
fn add_mismatch(
    context: &mut CheckContext<'_>,
    range: rowan::TextRange,
    expected: &OwnedValue,
    actual: &OwnedValue,
) {
    context.add_diagnostic(
        DiagnosticCode::ReturnTypeMismatch,
        range,
        t!(
            "expected `%{source}` but found `%{found}`. %{reason}",
            source = humanize_semantic_type(context.semantic_model, expected.structural_type()),
            found = humanize_semantic_type(context.semantic_model, actual.structural_type()),
            reason = ""
        ),
    );
}

/// `---@class B<T>: A` (where `A` is an alias/class): assign a `B<string>` instance to `A`.
fn generic_extends_named(
    semantic_model: &SemanticModel<'_>,
    source: &LuaType,
    target: &LuaType,
) -> bool {
    let LuaType::Generic(generic) = source else {
        return false;
    };
    let expected_id = match target {
        LuaType::Ref(id) | LuaType::Def(id) => id,
        _ => return false,
    };
    let Some(source_def) = semantic_model.type_def_of(&generic.get_base_type_id()) else {
        return false;
    };
    let Some(target_def) = semantic_model.type_def_of(expected_id) else {
        return false;
    };
    let mut visited = Vec::new();
    def_extends(semantic_model, &source_def, &target_def, &mut visited)
}

fn def_extends(
    semantic_model: &SemanticModel<'_>,
    source: &TypeDef,
    target: &TypeDef,
    visited: &mut Vec<SemanticId>,
) -> bool {
    if visited.contains(&source.id) {
        return false;
    }
    visited.push(source.id.clone());
    if source.id == target.id {
        return true;
    }
    for super_name in &source.super_names {
        let super_def = semantic_model
            .resolve_type_def_in(source.file_id, super_name.as_str())
            .or_else(|| {
                semantic_model
                    .type_defs_in_scope(TypeScope::Global, super_name)
                    .iter()
                    .next()
                    .cloned()
            });
        if let Some(super_def) = super_def
            && def_extends(semantic_model, &super_def, target, visited)
        {
            return true;
        }
    }
    false
}

/// Uninstantiated `Ref("T")` / `Ref("E")`: the signature doc declares a generic parameter with the same name.
fn is_unresolved_generic_ref(
    semantic_model: &SemanticModel<'_>,
    ty: &LuaType,
    docs: &SignatureDoc,
) -> bool {
    let (LuaType::Ref(id) | LuaType::Def(id)) = ty else {
        return false;
    };
    if semantic_model.type_def_of(id).is_some() {
        return false;
    }
    docs.generic_params
        .iter()
        .any(|param| param.name.as_str() == id.get_name())
}

/// `Ref("T")` when the signature declares `---@generic T: Animal` -> project to the constraint `Animal`.
fn generic_constraint_type(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> Option<LuaType> {
    let (LuaType::Ref(id) | LuaType::Def(id)) = ty else {
        return None;
    };
    if semantic_model.type_def_of(id).is_some() {
        return None;
    }
    let name = id.get_name();
    for (_signature, param) in semantic_model.file_facts()?.generic_param_constraints(name) {
        let Some(constraint) = param.constraint else {
            continue;
        };
        let constraint_ty = semantic_model.doc_type_lua(constraint);
        if !matches!(constraint_ty, LuaType::Unknown) {
            return Some(constraint_ty);
        }
    }
    None
}

/// Inline type for `return expr ---@as integer` / `--[[@as integer]]`.
fn inline_as_type(
    semantic_model: &SemanticModel<'_>,
    ret: &LuaReturnStat,
    expr: &emmylua_parser::LuaExpr,
) -> Option<LuaType> {
    let expr_siblings = expr.syntax().siblings_with_tokens(rowan::Direction::Next);
    let ret_siblings = ret.syntax().siblings_with_tokens(rowan::Direction::Next);
    for sibling in expr_siblings.chain(ret_siblings) {
        if let Some(ty) = as_type_from_token(semantic_model, sibling.as_token()) {
            return Some(ty);
        }
    }
    // The parser may not attach the end-of-line comment as a sibling of ReturnStat; fall back to searching by position inside the closure.
    if let Some(closure) = ret.ancestors::<LuaClosureExpr>().next() {
        let mut prev_text = String::new();
        let mut pending_as = false;
        for item in closure.syntax().descendants_with_tokens() {
            let Some(token) = item.into_token() else {
                continue;
            };
            let text = token.text();
            if pending_as {
                let name: String = text
                    .trim()
                    .trim_start_matches(['[', '-'])
                    .chars()
                    .take_while(|c| {
                        c.is_alphanumeric() || matches!(c, '.' | '_' | '?' | '|' | '<' | '>')
                    })
                    .collect();
                let name = name.trim_end_matches([']', '-']);
                if !name.is_empty() {
                    return Some(semantic_model.type_from_name(name));
                }
                if !text.trim().is_empty() {
                    pending_as = false;
                }
            }
            if prev_text.ends_with("---@") && text == "as" {
                pending_as = true;
            }
            prev_text = text.to_string();
            if token.text_range().start() >= expr.get_range().end() {
                if let Some(ty) = as_type_from_token(semantic_model, Some(&token)) {
                    return Some(ty);
                }
            }
        }
    }
    None
}

fn as_type_from_token(
    semantic_model: &SemanticModel<'_>,
    token: Option<&emmylua_parser::LuaSyntaxToken>,
) -> Option<LuaType> {
    let token = token?;
    let text = token.text();
    let mark = text.find("@as")?;
    let rest = &text[mark + 3..];
    let name: String = rest
        .trim_start_matches(|c: char| c.is_whitespace() || c == '[' || c == '-')
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '?' | '|' | '<' | '>'))
        .collect();
    let name = name.trim_end_matches([']', '-']);
    if name.is_empty() {
        return None;
    }
    Some(semantic_model.type_from_name(name))
}
