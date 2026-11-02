//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/generic_constraint_mismatch.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
//! Runtime arguments and inferred substitutions retain their owned carriers.
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult, call_access::first_param_is_self};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, DocGenericParam, FileId, GenericTpl, GenericTplId, LuaFunctionType,
    LuaStringTplType, LuaType, SemanticId, SemanticModel, humanize_semantic_type,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaDocType, LuaExpr, LuaSyntaxId};
use std::collections::HashMap;
type OwnerConstraints = HashMap<String, (usize, OwnedValue)>;

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_generic_constraint_mismatch(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.constraint_definitions(context)?;
        self.constraint_instantiations(context)?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for call in root.descendants().filter_map(LuaCallExpr::cast) {
            self.constraint_call(context, &call)?;
        }
        Ok(())
    }
    fn constraint_definitions(&mut self, context: &mut CheckContext<'_>) -> QueryResult<(), E> {
        let Some(facts) = self.model.file_facts() else {
            return Ok(());
        };
        for def in &facts.type_defs {
            for param in &def.generic_params {
                self.checkpoint()?;
                let (Some(constraint), Some(default)) = (param.constraint, param.default) else {
                    continue;
                };
                let constraint_ty = self.constraint_doc_type(
                    def.file_id,
                    constraint,
                    &def.generic_params,
                    &HashMap::new(),
                )?;
                let default_ty = self.constraint_doc_type(
                    def.file_id,
                    default,
                    &def.generic_params,
                    &HashMap::new(),
                )?;
                if self.constraint_dependent_default(
                    &def.generic_params,
                    param,
                    &default_ty,
                    &constraint_ty,
                )? {
                    continue;
                }
                let range =
                    doc_syntax_range(&self.model, def.file_id, default).unwrap_or(def.name_range);
                self.constraint_diagnostic(context, range, &default_ty, &constraint_ty);
            }
        }
        for signature in &facts.signatures {
            let Some(docs) = &signature.docs else {
                continue;
            };
            for param in &docs.generic_params {
                self.checkpoint()?;
                let (Some(constraint), Some(default)) = (param.constraint, param.default) else {
                    continue;
                };
                let constraint_ty = self.constraint_doc_type(
                    signature.file_id,
                    constraint,
                    &docs.generic_params,
                    &HashMap::new(),
                )?;
                let default_ty = self.constraint_doc_type(
                    signature.file_id,
                    default,
                    &docs.generic_params,
                    &HashMap::new(),
                )?;
                if self.constraint_dependent_default(
                    &docs.generic_params,
                    param,
                    &default_ty,
                    &constraint_ty,
                )? {
                    continue;
                }
                let range = doc_syntax_range(&self.model, signature.file_id, default)
                    .unwrap_or(signature.closure_syntax.get_range());
                self.constraint_diagnostic(context, range, &default_ty, &constraint_ty);
            }
        }
        Ok(())
    }
    fn constraint_instantiations(&mut self, context: &mut CheckContext<'_>) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for generic in root.descendants().filter_map(LuaDocType::cast) {
            self.checkpoint()?;
            let LuaDocType::Generic(generic) = generic else {
                continue;
            };
            let Some(name) = generic.get_name_type().and_then(|n| n.get_name_text()) else {
                continue;
            };
            let Some(def) = self.model.resolve_type_def(&name) else {
                continue;
            };
            let Some(args) = generic.get_generic_types() else {
                continue;
            };
            let mut values = Vec::new();
            for arg in args.get_types() {
                values.push(self.constraint_doc_type(
                    self.model.file_id(),
                    arg.get_syntax_id(),
                    &[],
                    &HashMap::new(),
                )?);
            }
            if values.is_empty() {
                continue;
            }
            let mut substitutions = HashMap::new();
            for (index, param) in def.generic_params.iter().enumerate() {
                if let Some(value) = values.get(index) {
                    substitutions.insert(param.name.to_string(), value.clone());
                }
            }
            for (index, (value, arg)) in values.iter().zip(args.get_types()).enumerate() {
                let Some(param) = def.generic_params.get(index) else {
                    break;
                };
                let Some(constraint) = param.constraint else {
                    continue;
                };
                let constraint = self.constraint_doc_type(
                    def.file_id,
                    constraint,
                    &def.generic_params,
                    &substitutions,
                )?;
                if !self.constraint_compatible_owned(value, &constraint)? {
                    self.constraint_diagnostic(context, arg.get_range(), value, &constraint);
                }
            }
        }
        Ok(())
    }
    fn constraint_call(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(());
        };
        let analysis = self.call_site_analysis(call)?;
        // Keep this second owner-constraint producer even when analysis was cached.
        let owner_constraints = self.access_owner_constraints(&callee)?;
        if owner_constraints.is_empty() && analysis.candidates.iter().all(|candidate| matches!(candidate.structural_type(), LuaType::DocFunction(fun) if fun.get_generic_params().is_empty())) { return Ok(()) }
        let signatures = self.call_site_signatures(call)?;
        for (function, bindings) in &signatures {
            let LuaType::DocFunction(fun) = function.structural_type() else {
                continue;
            };
            let mut constraints = Vec::new();
            let docs = self
                .constraint_signature_docs(&callee)?
                .and_then(|(file, syntax)| {
                    self.model
                        .file_facts_of(file)
                        .and_then(|facts| facts.signature_by_closure(syntax))
                        .and_then(|signature| signature.docs.clone())
                        .map(|docs| (file, docs))
                });
            if let Some((file, docs)) = docs {
                let mut substitutions = HashMap::new();
                for (index, param) in docs.generic_params.iter().enumerate() {
                    let mut bound = fun
                        .get_generic_params()
                        .get(index)
                        .and_then(|generic| bindings.get(&generic.get_tpl_id()).cloned());
                    if bound.is_none()
                        && let Some(default) = param.default
                    {
                        bound = Some(self.constraint_doc_type(
                            file,
                            default,
                            &docs.generic_params,
                            &HashMap::new(),
                        )?);
                    }
                    if let Some(bound) = bound {
                        substitutions.insert(param.name.to_string(), bound);
                    }
                }
                for (index, param) in docs.generic_params.iter().enumerate() {
                    if let Some(constraint) = param.constraint {
                        constraints.push((
                            GenericTplId::Type(index as u32),
                            self.constraint_doc_type(
                                file,
                                constraint,
                                &docs.generic_params,
                                &substitutions,
                            )?,
                        ));
                    }
                }
            } else {
                // Generic metadata originates in signature doc projection; argument
                // substitutions above are the identity-bearing values.
                constraints.extend(fun.get_generic_params().iter().filter_map(|param| {
                    param.get_constraint().map(|constraint| {
                        (param.get_tpl_id(), OwnedValue::plain(constraint.clone()))
                    })
                }));
            }
            for (_, param) in fun.get_params() {
                let Some(param) = param else { continue };
                for tpl in tpl_refs_in(param) {
                    if constraints.iter().any(|(id, _)| *id == tpl.get_tpl_id()) {
                        continue;
                    }
                    if let Some((_, constraint)) = owner_constraints.get(tpl.get_name()) {
                        constraints.push((tpl.get_tpl_id(), constraint.clone()));
                    }
                }
            }
            let range = call
                .get_args_list()
                .map(|list| list.get_range())
                .unwrap_or_else(|| call.get_range());
            for (id, constraint) in constraints {
                let Some(actual) = bindings.get(&id) else {
                    continue;
                };
                if matches!(
                    actual.structural_type(),
                    LuaType::Unknown | LuaType::Any | LuaType::Never
                ) {
                    continue;
                }
                let actual = unbound_generic_actual(actual, &owner_constraints, fun);
                let resolved = match constraint.structural_type() {
                    LuaType::TplRef(tpl) => bindings
                        .get(&tpl.get_tpl_id())
                        .cloned()
                        .or_else(|| {
                            fun.get_generic_params()
                                .iter()
                                .find(|param| param.get_tpl_id() == tpl.get_tpl_id())
                                .and_then(|param| param.get_default_type().cloned())
                                .map(OwnedValue::plain)
                        })
                        .unwrap_or(constraint),
                    _ => constraint.clone(),
                };
                if !self.constraint_call_compatible(&actual, &resolved)? {
                    self.constraint_diagnostic(context, range, &actual, &resolved);
                }
            }
            self.constraint_string_params(
                context,
                call,
                function,
                &owner_constraints,
                &analysis.arg_types,
            )?;
        }
        Ok(())
    }
    fn constraint_signature_docs(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<Option<(FileId, LuaSyntaxId)>, E> {
        if self.binding_manages(callee.get_syntax_id())? {
            return Ok(None);
        }
        match callee {
            LuaExpr::NameExpr(name) => {
                let Some(id @ SemanticId::Decl(_)) = self.model.resolve_name(name.get_position())
                else {
                    return Ok(None);
                };
                if self.owner_initializer_managed(&id)? {
                    return Ok(None);
                }
                let SemanticId::Decl(key) = &id else {
                    unreachable!()
                };
                Ok((|| {
                    let facts = self.model.file_facts_of(key.file_id)?;
                    let decl = facts.decl_by_id(&id)?;
                    Some((decl.file_id, decl.value_expr_syntax?))
                })())
            }
            LuaExpr::IndexExpr(index) => {
                let Some(resolved) = self.resolve_owned_member(index)? else {
                    return Ok(None);
                };
                if resolved.binding_token.is_some() {
                    return Ok(None);
                }
                if let Some(id) = &resolved.member_id
                    && self.owner_initializer_managed(id)?
                {
                    return Ok(None);
                }
                Ok((|| {
                    let file = resolved.file_id?;
                    let facts = self.model.file_facts_of(file)?;
                    let member = facts.member_by_id(&resolved.member_id?)?;
                    Some((file, member.value_syntax?))
                })())
            }
            _ => Ok(None),
        }
    }
    fn constraint_string_params(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
        function: &OwnedValue,
        owners: &OwnerConstraints,
        arg_types: &[OwnedValue],
    ) -> QueryResult<(), E> {
        let args = call
            .get_args_list()
            .map(|list| list.get_args().collect::<Vec<_>>())
            .unwrap_or_default();
        if args.is_empty() {
            return Ok(());
        }
        let LuaType::DocFunction(fun) = function.structural_type() else {
            return Ok(());
        };
        let start = usize::from(call.is_colon_call() && first_param_is_self(function));
        let params = function.function_parameters().expect("function");
        for (index, param) in params[start..].iter().enumerate() {
            let Some(param) = param else { continue };
            let Some(arg) = args.get(index) else { continue };
            for tpl in str_tpl_refs_in(param.structural_type()) {
                let actual = match arg_types.get(index) {
                    Some(value) => value.clone(),
                    None => self.type_of_expr(arg.get_syntax_id())?,
                };
                if matches!(param.structural_type(), LuaType::Union(_))
                    && !matches!(
                        actual.structural_type(),
                        LuaType::String
                            | LuaType::StringConst(_)
                            | LuaType::DocStringConst(_)
                            | LuaType::StrTplRef(_)
                    )
                {
                    continue;
                }
                let text = match actual.structural_type() {
                    LuaType::StringConst(s) | LuaType::DocStringConst(s) => s.as_str(),
                    LuaType::String | LuaType::Any | LuaType::Unknown | LuaType::StrTplRef(_) => {
                        continue;
                    }
                    _ => {
                        self.constraint_diagnostic(
                            context,
                            arg.get_range(),
                            &actual,
                            &OwnedValue::plain(LuaType::String),
                        );
                        continue;
                    }
                };
                let name = format!("{}{}{}", tpl.get_prefix(), text, tpl.get_suffix());
                let Some(def) = self.model.resolve_type_def(&name) else {
                    context.add_diagnostic(DiagnosticCode::GenericConstraintMismatch, arg.get_range(), t!("the string template type `%{name}` does not match any type declaration", name = name));
                    continue;
                };
                let constraint = tpl
                    .get_constraint()
                    .cloned()
                    .map(OwnedValue::plain)
                    .or_else(|| {
                        owners
                            .get(tpl.get_name())
                            .map(|(_, constraint)| constraint.clone())
                    })
                    .or_else(|| {
                        fun.get_generic_params()
                            .iter()
                            .find(|param| param.get_name() == tpl.get_name())
                            .and_then(|param| param.get_constraint().cloned())
                            .map(OwnedValue::plain)
                    });
                if let Some(constraint) = constraint {
                    let resolved = OwnedValue::plain(self.model.type_def_ref(&def));
                    if !self.constraint_compatible_owned(&resolved, &constraint)? {
                        self.constraint_diagnostic(
                            context,
                            arg.get_range(),
                            &resolved,
                            &constraint,
                        );
                    }
                }
            }
        }
        Ok(())
    }
    fn constraint_dependent_default(
        &mut self,
        params: &[DocGenericParam],
        current: &DocGenericParam,
        actual: &OwnedValue,
        constraint: &OwnedValue,
    ) -> QueryResult<bool, E> {
        let actual_ref = generic_param_name(actual.structural_type());
        let constraint_ref = generic_param_name(constraint.structural_type());
        if let Some(name) = constraint_ref {
            return Ok(actual_ref == Some(name));
        }
        if let Some(name) = actual_ref {
            if let Some(previous) = params
                .iter()
                .take_while(|p| p.name != current.name)
                .find(|p| p.name.as_str() == name)
                && let Some(previous_constraint) = previous.constraint
            {
                let previous = self.constraint_doc_type(
                    self.model.file_id(),
                    previous_constraint,
                    params,
                    &HashMap::new(),
                )?;
                return self.constraint_compatible_owned(&previous, constraint);
            }
            return Ok(false);
        }
        self.constraint_compatible_owned(actual, constraint)
    }
    fn constraint_call_compatible(
        &mut self,
        actual: &OwnedValue,
        constraint: &OwnedValue,
    ) -> QueryResult<bool, E> {
        if matches!(
            actual.structural_type(),
            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_)
        ) && matches!(constraint.structural_type(), LuaType::Number)
        {
            return Ok(true);
        }
        if let Some(components) = actual.union_components() {
            for component in components {
                if self.constraint_compatible_owned(&component, constraint)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        self.constraint_compatible_owned(actual, constraint)
    }
    fn constraint_diagnostic(
        &self,
        context: &mut CheckContext<'_>,
        range: rowan::TextRange,
        actual: &OwnedValue,
        constraint: &OwnedValue,
    ) {
        let actual_name = humanize_semantic_type(&self.model, actual.structural_type());
        context.add_diagnostic(
            DiagnosticCode::GenericConstraintMismatch,
            range,
            t!(
                "type `%{found}` does not satisfy the constraint `%{source}`. %{reason}",
                found = actual_name,
                source = humanize_semantic_type(&self.model, constraint.structural_type()),
                reason = ""
            ),
        );
    }
}

fn unbound_generic_actual(
    actual: &OwnedValue,
    owners: &OwnerConstraints,
    fun: &LuaFunctionType,
) -> OwnedValue {
    let (LuaType::Ref(id) | LuaType::Def(id)) = actual.structural_type() else {
        return actual.clone();
    };
    let name = id.get_name();
    if let Some((_, constraint)) = owners.get(name) {
        return constraint.clone();
    }
    if let Some(param) = fun
        .get_generic_params()
        .iter()
        .find(|param| param.get_name() == name)
        && let Some(constraint) = param.get_constraint()
    {
        return OwnedValue::plain(constraint.clone());
    }
    actual.clone()
}

fn tpl_refs_in(ty: &LuaType) -> Vec<std::sync::Arc<GenericTpl>> {
    let mut out = Vec::new();
    collect_tpl_refs(ty, &mut out, 0);
    out
}

fn collect_tpl_refs(ty: &LuaType, out: &mut Vec<std::sync::Arc<GenericTpl>>, depth: usize) {
    if depth > 16 {
        return;
    }
    match ty {
        LuaType::TplRef(tpl) => out.push(tpl.clone()),
        LuaType::Union(union) => {
            for component in union.into_vec() {
                collect_tpl_refs(&component, out, depth + 1);
            }
        }
        LuaType::Intersection(intersection) => {
            for component in intersection.get_types() {
                collect_tpl_refs(component, out, depth + 1);
            }
        }
        LuaType::Array(array) => collect_tpl_refs(array.get_base(), out, depth + 1),
        _ => {}
    }
}

fn str_tpl_refs_in(ty: &LuaType) -> Vec<std::sync::Arc<LuaStringTplType>> {
    let mut out = Vec::new();
    collect_str_tpl_refs(ty, &mut out, 0);
    out
}

fn collect_str_tpl_refs(
    ty: &LuaType,
    out: &mut Vec<std::sync::Arc<LuaStringTplType>>,
    depth: usize,
) {
    if depth > 16 {
        return;
    }
    match ty {
        LuaType::StrTplRef(str_tpl) => out.push(str_tpl.clone()),
        LuaType::Union(union) => {
            for component in union.into_vec() {
                collect_str_tpl_refs(&component, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn generic_param_name(ty: &LuaType) -> Option<&str> {
    match ty {
        LuaType::Ref(id) | LuaType::Def(id) => {
            if id.get_name().len() == 1 {
                Some(id.get_name())
            } else {
                None
            }
        }
        LuaType::TplRef(tpl) => Some(tpl.get_name()),
        LuaType::StrTplRef(str_tpl) => Some(str_tpl.get_name()),
        _ => None,
    }
}

fn doc_syntax_range(
    semantic_model: &SemanticModel<'_>,
    file_id: FileId,
    syntax: emmylua_parser::LuaSyntaxId,
) -> Option<rowan::TextRange> {
    semantic_model
        .syntax_tree_of(file_id)
        .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
        .map(|node| node.text_range())
}
