//! Ordered overload matching adapted from fixed EmmyLua infer/overload.rs.
//! Every candidate owns its partial bindings; ties keep original producer order.
use super::{
    ExpressionValue, OwnedQuery, QueryResult,
    call_bindings::{widen_call_const, widen_variadic_const},
    call_unify::Bindings,
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    LuaArrayType, LuaFunctionType, LuaGenericType, LuaMemberKey, LuaObjectType, LuaTupleStatus,
    LuaType, SemanticId, TypeDefKind, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaSyntaxId};
use std::{collections::HashMap, sync::Arc};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn select_owned_callables(
        &mut self,
        candidates: &[OwnedValue],
        args: &[ExpressionValue<OwnedValue>],
        colon: bool,
        receiver: Option<&OwnedValue>,
        partial: bool,
        all: bool,
    ) -> QueryResult<Vec<(OwnedValue, Bindings)>, E> {
        let mut best = Vec::new();
        let mut best_score = None;
        for candidate in candidates {
            self.checkpoint()?;
            if let Some((bindings, score)) =
                self.match_owned_callable(candidate, args, colon, receiver, partial)?
            {
                if best_score.is_some_and(|best| score < best || (!all && score == best)) {
                    continue;
                }
                if best_score.is_none_or(|best| score > best) {
                    best.clear();
                    best_score = Some(score);
                }
                best.push((candidate.clone(), bindings));
            }
        }
        Ok(best)
    }

    pub(super) fn match_owned_callable(
        &mut self,
        value: &OwnedValue,
        args: &[ExpressionValue<OwnedValue>],
        colon: bool,
        receiver: Option<&OwnedValue>,
        partial: bool,
    ) -> QueryResult<Option<(Bindings, i32)>, E> {
        self.checkpoint()?;
        let LuaType::DocFunction(function) = value.structural_type() else {
            return Ok(None);
        };
        let params = value.function_parameters().expect("function parameters");
        let has_self = function
            .get_params()
            .first()
            .is_some_and(|(name, ty)| name == "self" || matches!(ty, Some(LuaType::SelfInfer)));
        let param_start = usize::from(has_self);
        let mut adjusted = Vec::new();
        let args = if colon && !has_self && !function.is_colon_define() {
            if let Some(receiver) = receiver {
                adjusted.push(ExpressionValue::plain(receiver.clone()));
                adjusted.extend_from_slice(args);
                adjusted.as_slice()
            } else {
                args
            }
        } else {
            args
        };
        let arg_start = if has_self && colon {
            0
        } else if !has_self && function.is_colon_define() && !colon {
            1
        } else {
            param_start
        };
        let mut bindings = Bindings::new();
        let mut score = 0;
        self.prebind_call_variadics(value, args, param_start, arg_start, &mut bindings)?;
        for (pi, param) in params.iter().enumerate().skip(param_start) {
            self.checkpoint()?;
            let index = arg_start + (pi - param_start);
            let Some(param) = param else {
                continue;
            };
            let raw =
                self.bind_owned_signature_generics(param.clone(), function.get_generic_params())?;
            let mut parameter = self.substitute_owned(raw.clone(), &bindings)?;
            let expanded = if matches!(raw.structural_type(), LuaType::Ref(_) | LuaType::Def(_)) {
                self.expand_call_named_alias(&parameter)?
            } else {
                self.expand_owned_aliases(parameter.clone())?
            };
            if (matches!(raw.structural_type(), LuaType::Ref(_) | LuaType::Def(_))
                && expanded != raw)
                || matches!(expanded.structural_type(), LuaType::TplRef(_))
            {
                parameter = expanded;
            } else if self.call_contains_template(expanded.structural_type(), true)? {
                parameter = self.eval_owned_conditionals(expanded)?;
            }
            if function.get_params()[pi].0 == "..."
                || matches!(parameter.structural_type(), LuaType::Variadic(_))
            {
                let base = self
                    .call_variadic_base(&parameter)
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                if matches!(parameter.structural_type(), LuaType::Variadic(_)) {
                    if self.call_variadic_base(&raw).is_some_and(|v|matches!(v.structural_type(),LuaType::TplRef(tpl) if bindings.contains_key(&tpl.get_tpl_id()))) {score+=2;break;}
                    if let LuaType::TplRef(tpl) = base.structural_type() {
                        if bindings.contains_key(&tpl.get_tpl_id()) {
                            score += 2;
                            break;
                        }
                        let mut rest = Vec::new();
                        for argument in args.get(index..).unwrap_or(&[]) {
                            self.checkpoint()?;
                            rest.push(widen_variadic_const(&argument.value));
                        }
                        let tuple =
                            OwnedValue::tuple_checked(rest, LuaTupleStatus::InferResolve, || {
                                self.checkpoint()
                            })?;
                        if !self.unify_call_bindings(
                            &OwnedValue::plain(LuaType::TplRef(tpl.clone())),
                            &tuple,
                            &mut bindings,
                        )? {
                            return Ok(None);
                        }
                        score += 2;
                        break;
                    }
                }
                for arg in args.get(index..).unwrap_or(&[]) {
                    let argument = self.call_structural_table(&arg.value)?;
                    if !self.unify_call_bindings(&base, &argument, &mut bindings)? {
                        if partial {
                            break;
                        }
                        return Ok(None);
                    }
                }
                score += 2;
                break;
            }
            let arg = args.get(index);
            let argument = if let Some(arg) = arg {
                if matches!(parameter.structural_type(), LuaType::TplRef(_)) {
                    if let Some(closure) = arg.closure_syntax {
                        self.shallow_call_closure(closure)?
                    } else {
                        arg.value.clone()
                    }
                } else if arg.closure_syntax.is_some()
                    && matches!(parameter.structural_type(), LuaType::DocFunction(_))
                {
                    let variadic = matches!(parameter.structural_type(),LuaType::DocFunction(f) if f.get_params().iter().any(|(_,ty)|matches!(ty,Some(LuaType::Variadic(_)))));
                    if variadic {
                        self.signature_local(arg.closure_syntax.expect("closure"))?
                            .unwrap_or_else(|| parameter.clone())
                    } else {
                        parameter.clone()
                    }
                } else if matches!(parameter.structural_type(), LuaType::Array(_)) {
                    self.call_table_array(&arg.value)?
                        .unwrap_or_else(|| arg.value.clone())
                } else {
                    self.call_structural_table(&arg.value)?
                }
            } else {
                OwnedValue::plain(LuaType::Unknown)
            };
            let unify_parameter = if matches!(raw.structural_type(), LuaType::DocFunction(_)) {
                &raw
            } else {
                &parameter
            };
            let argument = self.normalize_call_generic(argument, &parameter)?;
            let accepts = if arg.is_none()
                && self.call_contains_template(parameter.structural_type(), false)?
            {
                true
            } else if matches!(argument.structural_type(), LuaType::Object(_))
                && matches!(
                    raw.structural_type(),
                    LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_)
                )
            {
                true
            } else {
                self.unify_call_bindings(unify_parameter, &argument, &mut bindings)?
            };
            if !accepts {
                return Ok(None);
            }
            score += if arg.is_some() {
                score_call_param(&parameter, &argument)
            } else {
                -50
            };
        }
        Ok(Some((bindings, score)))
    }

    fn prebind_call_variadics(
        &mut self,
        value: &OwnedValue,
        args: &[ExpressionValue<OwnedValue>],
        param_start: usize,
        arg_start: usize,
        bindings: &mut Bindings,
    ) -> QueryResult<(), E> {
        let LuaType::DocFunction(function) = value.structural_type() else {
            unreachable!();
        };
        let params = value.function_parameters().expect("function parameters");
        for (i, param) in params.iter().enumerate().skip(param_start) {
            self.checkpoint()?;
            let Some(param) = param else {
                continue;
            };
            if !matches!(param.structural_type(), LuaType::Variadic(_)) {
                continue;
            }
            let Some(base) = self.call_variadic_base(param) else {
                continue;
            };
            let LuaType::TplRef(tpl) = base.structural_type() else {
                continue;
            };
            if bindings.contains_key(&tpl.get_tpl_id()) {
                continue;
            }
            let mut rest = Vec::new();
            for arg in args.get(arg_start + i - param_start..).unwrap_or(&[]) {
                self.checkpoint()?;
                rest.push(widen_variadic_const(&arg.value));
            }
            if rest.is_empty()
                || rest
                    .iter()
                    .all(|v| matches!(v.structural_type(), LuaType::Unknown | LuaType::Any))
            {
                continue;
            }
            let bound = if rest.len() == 1 {
                rest.remove(0)
            } else {
                OwnedValue::tuple_checked(rest, LuaTupleStatus::InferResolve, || self.checkpoint())?
            };
            let _ = self.unify_structural(&base, &bound, bindings)?;
        }
        let _ = function;
        Ok(())
    }

    pub(super) fn call_variadic_base(&self, value: &OwnedValue) -> Option<OwnedValue> {
        match value.structural_type() {
            LuaType::Variadic(v) => match v.as_ref() {
                VariadicType::Base(base) => Some(self.owned_child(value, 0, base)),
                VariadicType::Multi(values) => {
                    values.first().map(|v| self.owned_child(value, 0, v))
                }
            },
            _ => Some(value.clone()),
        }
    }

    pub(super) fn call_structural_table(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let LuaType::TableConst(table) = value.structural_type() else {
            return Ok(value.clone());
        };
        let owner = SemanticId::member(table.file_id, table.value);
        let mut fields = HashMap::new();
        for member in self.model.members_of_owner(&owner) {
            self.checkpoint()?;
            let key = LuaMemberKey::Name(member.name.clone());
            let mut ty = self
                .type_of_member(&member.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if matches!(ty.structural_type(), LuaType::Unknown)
                && let Some(facts) = self.model.file_facts_of(member.file_id)
                && let Some(def) = facts.member_by_id(&member.id)
                && let Some(syntax) = def.value_syntax
            {
                ty = self.type_of_expr(syntax)?;
                if matches!(ty.structural_type(), LuaType::Unknown)
                    && let Some(node) = self
                        .model
                        .syntax_tree()
                        .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    && let Some(LuaExpr::NameExpr(name)) = LuaExpr::cast(node)
                    && let Some(name) = name.get_name_text()
                    && let Some(def) = self.model.resolve_type_def(&name)
                {
                    ty = OwnedValue::plain(self.model.type_def_ref(&def));
                }
            }
            fields.insert(key, ty);
        }
        let object = LuaObjectType::new_with_fields(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), value.structural_type().clone()))
                .collect(),
            Vec::new(),
        );
        let children = object
            .get_fields()
            .keys()
            .map(|key| fields[key].clone())
            .collect();
        Ok(OwnedValue::with_structural_children(
            LuaType::Object(Arc::new(object)),
            children,
        ))
    }

    fn call_table_array(&mut self, value: &OwnedValue) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaType::TableConst(table) = value.structural_type() else {
            return Ok(None);
        };
        let owner = SemanticId::member(table.file_id, table.value);
        let mut values = Vec::new();
        for reference in self.model.members_of_owner(&owner) {
            self.checkpoint()?;
            let Some(member) = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
            else {
                continue;
            };
            if !matches!(member.key,LuaMemberKey::Integer(key) if key>0) {
                continue;
            }
            let ty = if let Some(syntax) = member.value_syntax {
                self.type_of_expr(syntax)?
            } else {
                self.type_of_member(&reference.id)?
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown))
            };
            let ty = widen_call_const(&ty);
            if !matches!(ty.structural_type(), LuaType::Unknown | LuaType::Any)
                && !values.contains(&ty)
            {
                values.push(ty);
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        let base = OwnedValue::union_checked(values, || self.checkpoint())?;
        Ok(Some(OwnedValue::with_structural_children(
            LuaType::Array(Arc::new(LuaArrayType::from_base_type(
                base.structural_type().clone(),
            ))),
            vec![base],
        )))
    }

    fn expand_call_named_alias(&mut self, value: &OwnedValue) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let (LuaType::Ref(id) | LuaType::Def(id)) = value.structural_type() else {
            return Ok(value.clone());
        };
        Ok(self
            .model
            .type_def_of(id)
            .filter(|def| def.kind == TypeDefKind::Alias)
            .and_then(|def| self.model.alias_target(&def))
            .map(OwnedValue::plain)
            .unwrap_or_else(|| value.clone()))
    }

    fn normalize_call_generic(
        &mut self,
        value: OwnedValue,
        param: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let (LuaType::Generic(generic), LuaType::Ref(id) | LuaType::Def(id)) =
            (param.structural_type(), value.structural_type())
        else {
            return Ok(value);
        };
        if generic.get_base_type_id() != *id {
            return Ok(value);
        }
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(value);
        };
        let mut params = Vec::new();
        for param in &def.generic_params {
            self.checkpoint()?;
            params.push(
                param
                    .default
                    .map(|s| self.model.doc_type_lua_in(def.file_id, s, &[]))
                    .or_else(|| {
                        param
                            .constraint
                            .map(|s| self.model.doc_type_lua_in(def.file_id, s, &[]))
                    })
                    .unwrap_or(LuaType::Unknown),
            );
        }
        Ok(OwnedValue::plain(LuaType::Generic(Arc::new(
            LuaGenericType::new(id.clone(), params),
        ))))
    }

    fn shallow_call_closure(&mut self, syntax: LuaSyntaxId) -> QueryResult<OwnedValue, E> {
        let Some(value) = self.signature_local(syntax)? else {
            return Ok(OwnedValue::plain(LuaType::Unknown));
        };
        let LuaType::DocFunction(function) = value.structural_type() else {
            unreachable!();
        };
        let Some(facts) = self.model.file_facts() else {
            return Ok(value);
        };
        let explicit = facts
            .signature_by_closure(syntax)
            .and_then(|sig| sig.docs.as_ref())
            .is_some_and(|docs| !docs.returns.is_empty() || !docs.return_overloads.is_empty());
        let mut params = value.function_parameters().expect("function parameters");
        for param in &mut params {
            self.checkpoint()?;
            if param
                .as_ref()
                .is_some_and(|ty| matches!(ty.structural_type(), LuaType::Unknown))
            {
                *param = None;
            }
        }
        let ret = if explicit {
            value.function_return().expect("function return")
        } else {
            OwnedValue::plain(LuaType::Any)
        };
        let metadata = LuaFunctionType::new(
            function.get_async_state(),
            function.is_colon_define(),
            function.is_variadic(),
            function
                .get_params()
                .iter()
                .zip(&params)
                .map(|((name, _), value)| {
                    (
                        name.clone(),
                        value.as_ref().map(|v| v.structural_type().clone()),
                    )
                })
                .collect(),
            ret.structural_type().clone(),
            Some(function.get_generic_params().to_vec()),
        );
        Ok(OwnedValue::function(&metadata, params, ret))
    }

    // These two walkers intentionally use the old distinct supported child
    // sets: Mapped participates only in ordinary generic detection; TypeGuard,
    // Instance and StrTpl are not searched by either predicate.
    fn call_contains_template(&mut self, root: &LuaType, infer: bool) -> QueryResult<bool, E> {
        use LuaType::*;
        let mut pending = vec![root];
        while let Some(value) = pending.pop() {
            self.checkpoint()?;
            match value {
                TplRef(tpl) => {
                    if !infer || tpl.get_tpl_id().is_conditional_infer() {
                        return Ok(true);
                    }
                }
                Array(a) => pending.push(a.get_base()),
                Tuple(t) => pending.extend(t.get_types().iter().rev()),
                Union(u) => {
                    for part in u.into_vec() {
                        if self.call_contains_template(&part, infer)? {
                            return Ok(true);
                        }
                    }
                }
                Intersection(t) => pending.extend(t.get_types().iter().rev()),
                Generic(t) => pending.extend(t.get_params().iter().rev()),
                TableGeneric(t) => pending.extend(t.iter().rev()),
                Variadic(t) => match t.as_ref() {
                    VariadicType::Base(t) => pending.push(t),
                    VariadicType::Multi(t) => pending.extend(t.iter().rev()),
                },
                DocFunction(f) => {
                    pending.push(f.get_ret());
                    for (_, ty) in f.get_params().iter().rev() {
                        if let Some(ty) = ty {
                            pending.push(ty);
                        }
                    }
                }
                Object(o) => {
                    for (k, v) in o.get_index_access().iter().rev() {
                        pending.push(v);
                        pending.push(k);
                    }
                    for ty in o.get_fields().values() {
                        pending.push(ty);
                    }
                }
                Call(c) => pending.extend(c.get_operands().iter().rev()),
                Mapped(m) if !infer => pending.push(&m.value),
                Conditional(c) => {
                    pending.push(c.get_false_type());
                    pending.push(c.get_true_type());
                    pending.push(c.get_extends_type());
                    pending.push(c.get_checked_type());
                }
                _ => {}
            }
        }
        Ok(false)
    }
}

fn score_call_param(p: &OwnedValue, a: &OwnedValue) -> i32 {
    use LuaType::*;
    match (p.structural_type(), a.structural_type()) {
        (DocFunction(_), DocFunction(_)) => 100,
        (TplRef(_), _) => 50,
        (Any | Unknown, _) => 0,
        (IntegerConst(a) | DocIntegerConst(a), IntegerConst(b) | DocIntegerConst(b)) if a == b => {
            200
        }
        (StringConst(a) | DocStringConst(a), StringConst(b) | DocStringConst(b)) if a == b => 200,
        (Variadic(_), _) => 2,
        _ if p == a => 80,
        _ => 20,
    }
}
