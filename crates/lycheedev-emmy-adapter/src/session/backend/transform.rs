//! Identity-preserving structural transformations. Public plain-type transforms
//! are used only where their inputs contain no owned selected descendants.
use super::{OwnedQuery, OwnedQueryCapability, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    GenericParam, GenericTpl, GenericTplId, LuaAliasCallType, LuaArrayType, LuaConditionalType,
    LuaFunctionType, LuaGenericType, LuaInstanceType, LuaIntersectionType, LuaMappedType,
    LuaMemberKey, LuaMultiLineUnion, LuaObjectType, LuaStringTplType, LuaTupleStatus, LuaType,
    LuaTypeDeclId, TypeDefKind, VariadicType,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy)]
enum Transform<'a> {
    Substitute(&'a HashMap<GenericTplId, OwnedValue>),
    Named(&'a HashMap<String, OwnedValue>),
    Bind(&'a [GenericTpl]),
    Alias,
    Conditional,
}

impl<E> OwnedQuery<'_, '_, E> {
    /// Semantic child order (array base / sequence index / conditional checked,
    /// extends, true, false); Structural stores the reversed push-stack order.
    pub(super) fn owned_child(
        &self,
        value: &OwnedValue,
        index: usize,
        fallback: &LuaType,
    ) -> OwnedValue {
        if let Some(slots) = value.carried_slots() {
            return slots[index].clone();
        }
        if let Some(children) = value.structural_children() {
            let position = match value.structural_type() {
                LuaType::Array(_)
                | LuaType::Instance(_)
                | LuaType::TypeGuard(_)
                | LuaType::StrTplRef(_) => index,
                LuaType::Variadic(variadic)
                    if matches!(variadic.as_ref(), VariadicType::Base(_)) =>
                {
                    index
                }
                _ => children.len() - index - 1,
            };
            return children[position].clone();
        }
        OwnedValue::plain(fallback.clone())
    }
    pub(super) fn owned_field(
        &self,
        value: &OwnedValue,
        key: &LuaMemberKey,
        fallback: &LuaType,
    ) -> OwnedValue {
        if let Some(fields) = value.carried_fields()
            && let Some(field) = fields.get(key)
        {
            return field.clone();
        }
        if let Some(children) = value.structural_children()
            && let LuaType::Object(object) = value.structural_type()
            && let Some(index) = object.get_fields().keys().position(|found| found == key)
        {
            return children[object.get_index_access().len() * 2 + index].clone();
        }
        OwnedValue::plain(fallback.clone())
    }
    pub(super) fn owned_index(
        &self,
        value: &OwnedValue,
        index: usize,
        is_key: bool,
        fallback: &LuaType,
    ) -> OwnedValue {
        if let Some(children) = value.structural_children()
            && let LuaType::Object(object) = value.structural_type()
        {
            return children
                [(object.get_index_access().len() - index - 1) * 2 + usize::from(is_key)]
            .clone();
        }
        OwnedValue::plain(fallback.clone())
    }
    pub(super) fn substitute_owned(
        &mut self,
        value: OwnedValue,
        bindings: &HashMap<GenericTplId, OwnedValue>,
    ) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::Substitute(bindings), &mut Vec::new())
    }
    pub(super) fn substitute_owned_names(
        &mut self,
        value: OwnedValue,
        names: &HashMap<String, OwnedValue>,
    ) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::Named(names), &mut Vec::new())
    }
    pub(super) fn bind_owned_signature_generics(
        &mut self,
        value: OwnedValue,
        generics: &[GenericTpl],
    ) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::Bind(generics), &mut Vec::new())
    }
    pub(super) fn expand_owned_aliases(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::Alias, &mut Vec::new())
    }
    pub(super) fn eval_owned_conditionals(
        &mut self,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::Conditional, &mut Vec::new())
    }
    pub(super) fn reassign_owned_function_generics(
        &mut self,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        let LuaType::DocFunction(function) = value.structural_type() else {
            return Ok(value);
        };
        if function.get_generic_params().is_empty() {
            return Ok(value);
        }
        let mut generics = Vec::new();
        for (index, old) in function.get_generic_params().iter().enumerate() {
            self.checkpoint()?;
            let param = old.get_param();
            generics.push(GenericTpl::new(
                GenericTplId::Func(index as u32),
                param.name.clone(),
                param.constraint.clone(),
                param.default.clone(),
                param.is_const,
                param.attributes.clone(),
            ));
        }
        let mut params = Vec::new();
        for param in value.function_parameters().expect("function parameters") {
            params.push(match param {
                Some(param) => Some(self.bind_owned_signature_generics(param, &generics)?),
                None => None,
            });
        }
        let ret = self.bind_owned_signature_generics(
            value.function_return().expect("function return"),
            &generics,
        )?;
        let metadata = LuaFunctionType::new(
            function.get_async_state(),
            function.is_colon_define(),
            function.is_variadic(),
            function.get_params().to_vec(),
            ret.structural_type().clone(),
            Some(generics),
        );
        Ok(OwnedValue::function(&metadata, params, ret))
    }

    fn transform_owned(
        &mut self,
        value: OwnedValue,
        op: Transform<'_>,
        visiting: &mut Vec<LuaTypeDeclId>,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if value.is_context_free() {
            match op {
                Transform::Alias => {
                    let ty = self.model.expand_alias_for_hover(value.structural_type());
                    self.checkpoint()?;
                    return Ok(OwnedValue::plain(ty));
                }
                Transform::Conditional => {
                    let ty = self
                        .model
                        .eval_conditionals_for_hover(value.structural_type());
                    self.checkpoint()?;
                    return Ok(OwnedValue::plain(ty));
                }
                _ => {}
            }
        }
        match (op, value.structural_type()) {
            (Transform::Substitute(bindings), LuaType::TplRef(tpl)) => {
                return Ok(bindings.get(&tpl.get_tpl_id()).cloned().unwrap_or(value));
            }
            (Transform::Named(names), LuaType::Ref(id) | LuaType::Def(id)) => {
                return Ok(names.get(id.get_name()).cloned().unwrap_or(value));
            }
            (Transform::Bind(generics), LuaType::TplRef(tpl)) => {
                return Ok(generics
                    .iter()
                    .find(|generic| {
                        generic.get_tpl_id() == tpl.get_tpl_id()
                            || generic.get_name() == tpl.get_name()
                    })
                    .map(|generic| OwnedValue::plain(LuaType::TplRef(Arc::new(generic.clone()))))
                    .unwrap_or(value));
            }
            (Transform::Bind(generics), LuaType::Ref(id) | LuaType::Def(id)) => {
                return Ok(generics
                    .iter()
                    .enumerate()
                    .find(|(_, generic)| generic.get_name() == id.get_name())
                    .map(|(index, generic)| {
                        OwnedValue::plain(LuaType::TplRef(Arc::new(GenericTpl::new(
                            GenericTplId::Type(index as u32),
                            id.get_name().into(),
                            generic.get_constraint().cloned(),
                            generic.get_default_type().cloned(),
                            generic.is_const(),
                            None,
                        ))))
                    })
                    .unwrap_or(value));
            }
            _ => {}
        }
        let ty = value.structural_type();
        match ty {
            LuaType::Array(array) => {
                let base = self.transform_owned(
                    self.owned_child(&value, 0, array.get_base()),
                    op,
                    visiting,
                )?;
                Ok(OwnedValue::with_structural_children(
                    LuaType::Array(Arc::new(LuaArrayType::from_base_type(
                        base.structural_type().clone(),
                    ))),
                    vec![base],
                ))
            }
            LuaType::Tuple(tuple) if !matches!(op, Transform::Bind(_)) => {
                let mut slots = Vec::new();
                for (index, ty) in tuple.get_types().iter().enumerate() {
                    slots.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                let status = if matches!(op, Transform::Substitute(_)) {
                    LuaTupleStatus::DocResolve
                } else {
                    tuple.status
                };
                OwnedValue::tuple_checked(slots, status, || self.checkpoint())
            }
            LuaType::DocFunction(function)
                if !matches!(op, Transform::Alias | Transform::Conditional) =>
            {
                if matches!(op, Transform::Bind(_))
                    && function
                        .get_generic_params()
                        .iter()
                        .any(|param| param.get_tpl_id().is_func())
                {
                    return Ok(value);
                }
                let mut params = Vec::new();
                for param in value.function_parameters().expect("function parameters") {
                    params.push(match param {
                        Some(param) => Some(self.transform_owned(param, op, visiting)?),
                        None => None,
                    });
                }
                let ret = self.transform_owned(
                    value.function_return().expect("function return"),
                    op,
                    visiting,
                )?;
                Ok(OwnedValue::function(function, params, ret))
            }
            LuaType::Object(object) => {
                let mut fields = Vec::new();
                for (key, ty) in object.get_fields() {
                    fields.push((
                        key.clone(),
                        self.transform_owned(self.owned_field(&value, key, ty), op, visiting)?,
                    ));
                }
                let mut indices = Vec::new();
                for (index, (key, ty)) in object.get_index_access().iter().enumerate() {
                    let key = self.owned_index(&value, index, true, key);
                    let ty = self.owned_index(&value, index, false, ty);
                    if matches!(op, Transform::Bind(_)) {
                        indices.push((key, ty));
                    } else {
                        indices.push((
                            self.transform_owned(key, op, visiting)?,
                            self.transform_owned(ty, op, visiting)?,
                        ));
                    }
                }
                if value.object_info().is_some() {
                    return Ok(value.rebuild_object_fields(fields));
                }
                let object = LuaObjectType::new_with_fields(
                    fields
                        .iter()
                        .map(|(key, value)| (key.clone(), value.structural_type().clone()))
                        .collect(),
                    indices
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.structural_type().clone(),
                                value.structural_type().clone(),
                            )
                        })
                        .collect(),
                );
                let mut children = Vec::new();
                for (key, ty) in indices.into_iter().rev() {
                    children.push(ty);
                    children.push(key);
                }
                for key in object.get_fields().keys() {
                    children.push(
                        fields
                            .iter()
                            .find(|(found, _)| found == key)
                            .expect("rebuilt object field")
                            .1
                            .clone(),
                    );
                }
                Ok(OwnedValue::with_structural_children(
                    LuaType::Object(Arc::new(object)),
                    children,
                ))
            }
            LuaType::Union(_) => {
                let mut members = Vec::new();
                for member in value.union_components().expect("union components") {
                    members.push(self.transform_owned(member, op, visiting)?);
                }
                Ok(OwnedValue::raw_union(members))
            }
            LuaType::Variadic(variadic) => match variadic.as_ref() {
                VariadicType::Base(base) => {
                    let base =
                        self.transform_owned(self.owned_child(&value, 0, base), op, visiting)?;
                    if matches!(op, Transform::Substitute(_))
                        && matches!(base.structural_type(), LuaType::Variadic(_))
                    {
                        return Ok(base);
                    }
                    Ok(OwnedValue::with_structural_children(
                        LuaType::Variadic(Arc::new(VariadicType::Base(
                            base.structural_type().clone(),
                        ))),
                        vec![base],
                    ))
                }
                VariadicType::Multi(types) => {
                    let mut slots = Vec::new();
                    for (index, ty) in types.iter().enumerate() {
                        slots.push(self.transform_owned(
                            self.owned_child(&value, index, ty),
                            op,
                            visiting,
                        )?);
                    }
                    Ok(OwnedValue::pack(slots))
                }
            },
            LuaType::Generic(generic) => {
                if matches!(op, Transform::Alias) {
                    let base = generic.get_base_type_id();
                    if matches!(base.get_name(), "RawGet" | "std.RawGet" | "Merge") {
                        return self.continuation(
                            OwnedQueryCapability::TypeRelation,
                            None,
                            None,
                            None,
                        );
                    }
                    if let Some(def) = self.model.type_def_of(&base)
                        && def.kind == TypeDefKind::Alias
                        && !visiting.contains(&base)
                        && let Some(alias) = self.model.alias_target(&def)
                    {
                        visiting.push(base.clone());
                        let mut bindings = HashMap::new();
                        let mut names = HashMap::new();
                        for (index, ty) in generic.get_params().iter().enumerate() {
                            let param = self.owned_child(&value, index, ty);
                            bindings.insert(GenericTplId::Type(index as u32), param.clone());
                            if let Some(generic) = def.generic_params.get(index) {
                                names.insert(generic.name.to_string(), param);
                            }
                        }
                        if matches!(alias, LuaType::Conditional(_)) {
                            return self.continuation(
                                OwnedQueryCapability::TypeRelation,
                                None,
                                None,
                                None,
                            );
                        }
                        let alias =
                            self.substitute_owned_names(OwnedValue::plain(alias), &names)?;
                        let alias = self.substitute_owned(alias, &bindings)?;
                        let expanded = self.transform_owned(alias, op, visiting)?;
                        visiting.pop();
                        return Ok(expanded);
                    }
                }
                let mut params = Vec::new();
                for (index, ty) in generic.get_params().iter().enumerate() {
                    params.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                let ty = LuaType::Generic(Arc::new(LuaGenericType::new(
                    generic.get_base_type_id(),
                    params
                        .iter()
                        .map(|value| value.structural_type().clone())
                        .collect(),
                )));
                params.reverse();
                Ok(OwnedValue::with_structural_children(ty, params))
            }
            LuaType::Intersection(intersection) => {
                let mut types = Vec::new();
                for (index, ty) in intersection.get_types().iter().enumerate() {
                    types.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                let ty = LuaType::Intersection(Arc::new(LuaIntersectionType::new(
                    types
                        .iter()
                        .map(|value| value.structural_type().clone())
                        .collect(),
                )));
                types.reverse();
                Ok(OwnedValue::with_structural_children(ty, types))
            }
            LuaType::TableGeneric(types)
                if !matches!(op, Transform::Alias | Transform::Conditional) =>
            {
                let mut values = Vec::new();
                for (index, ty) in types.iter().enumerate() {
                    values.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                let ty = LuaType::TableGeneric(Arc::new(
                    values
                        .iter()
                        .map(|value| value.structural_type().clone())
                        .collect(),
                ));
                values.reverse();
                Ok(OwnedValue::with_structural_children(ty, values))
            }
            LuaType::Call(call) => {
                let mut values = Vec::new();
                for (index, ty) in call.get_operands().iter().enumerate() {
                    values.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                if matches!(op, Transform::Alias)
                    && call.get_call_kind() == emmylua_code_analysis::LuaAliasCallKind::Merge
                {
                    return self.continuation(OwnedQueryCapability::TypeRelation, None, None, None);
                }
                let ty = LuaType::Call(Arc::new(LuaAliasCallType::new(
                    call.get_call_kind(),
                    values
                        .iter()
                        .map(|value| value.structural_type().clone())
                        .collect(),
                )));
                values.reverse();
                Ok(OwnedValue::with_structural_children(ty, values))
            }
            LuaType::Conditional(conditional) => {
                if matches!(op, Transform::Conditional) {
                    return self.evaluate_owned_conditional(value);
                }
                let mut values = Vec::new();
                for (index, ty) in [
                    conditional.get_checked_type(),
                    conditional.get_extends_type(),
                    conditional.get_true_type(),
                    conditional.get_false_type(),
                ]
                .into_iter()
                .enumerate()
                {
                    values.push(self.transform_owned(
                        self.owned_child(&value, index, ty),
                        op,
                        visiting,
                    )?);
                }
                let ty = LuaType::Conditional(Arc::new(LuaConditionalType::new(
                    values[0].structural_type().clone(),
                    values[1].structural_type().clone(),
                    values[2].structural_type().clone(),
                    values[3].structural_type().clone(),
                    conditional.get_infer_params().to_vec(),
                    conditional.has_new,
                )));
                values.reverse();
                let value = OwnedValue::with_structural_children(ty, values);
                if matches!(op, Transform::Alias) {
                    self.eval_owned_conditionals(value)
                } else {
                    Ok(value)
                }
            }
            LuaType::Mapped(mapped) if !matches!(op, Transform::Conditional) => {
                if matches!(op, Transform::Alias) {
                    return self.continuation(OwnedQueryCapability::TypeRelation, None, None, None);
                }
                let old = value.structural_children();
                let mut next = 1;
                let constraint = if let Some(ty) = &mapped.param.1.constraint {
                    let child = old
                        .map(|c| c[next].clone())
                        .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                    next += 1;
                    Some(self.transform_owned(child, op, visiting)?)
                } else {
                    None
                };
                let default = if let Some(ty) = &mapped.param.1.default {
                    let child = old
                        .map(|c| c[next].clone())
                        .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                    Some(self.transform_owned(child, op, visiting)?)
                } else {
                    None
                };
                let body = self.transform_owned(
                    old.map(|c| c[0].clone())
                        .unwrap_or_else(|| OwnedValue::plain(mapped.value.clone())),
                    op,
                    visiting,
                )?;
                let ty = LuaType::Mapped(Arc::new(LuaMappedType::new(
                    (
                        mapped.param.0,
                        GenericParam::new(
                            mapped.param.1.name.clone(),
                            constraint.as_ref().map(|v| v.structural_type().clone()),
                            default.as_ref().map(|v| v.structural_type().clone()),
                            mapped.param.1.is_const,
                            mapped.param.1.attributes.clone(),
                        ),
                    ),
                    body.structural_type().clone(),
                    mapped.is_readonly,
                    mapped.is_optional,
                )));
                let mut children = vec![body];
                children.extend(constraint);
                children.extend(default);
                Ok(OwnedValue::with_structural_children(ty, children))
            }
            LuaType::StrTplRef(tpl) if matches!(op, Transform::Substitute(_)) => {
                let constraint = if let Some(ty) = tpl.get_constraint() {
                    Some(self.transform_owned(self.owned_child(&value, 0, ty), op, visiting)?)
                } else {
                    None
                };
                let ty = LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                    tpl.get_prefix(),
                    tpl.get_name(),
                    tpl.get_tpl_id(),
                    tpl.get_suffix(),
                    constraint.as_ref().map(|v| v.structural_type().clone()),
                )));
                Ok(OwnedValue::with_structural_children(
                    ty,
                    constraint.into_iter().collect(),
                ))
            }
            LuaType::MultiLineUnion(union) if matches!(op, Transform::Substitute(_)) => {
                let mut values = Vec::new();
                let mut types = Vec::new();
                for (index, (ty, description)) in union.get_unions().iter().enumerate() {
                    let child =
                        self.transform_owned(self.owned_child(&value, index, ty), op, visiting)?;
                    types.push((child.structural_type().clone(), description.clone()));
                    values.push(child);
                }
                values.reverse();
                Ok(OwnedValue::with_structural_children(
                    LuaType::MultiLineUnion(Arc::new(LuaMultiLineUnion::new(types))),
                    values,
                ))
            }
            LuaType::TypeGuard(inner) if matches!(op, Transform::Substitute(_)) => {
                let inner =
                    self.transform_owned(self.owned_child(&value, 0, inner), op, visiting)?;
                Ok(OwnedValue::with_structural_children(
                    LuaType::TypeGuard(Arc::new(inner.structural_type().clone())),
                    vec![inner],
                ))
            }
            LuaType::Instance(instance) if matches!(op, Transform::Substitute(_)) => {
                let base = self.transform_owned(
                    self.owned_child(&value, 0, instance.get_base()),
                    op,
                    visiting,
                )?;
                Ok(OwnedValue::with_structural_children(
                    LuaType::Instance(Arc::new(LuaInstanceType::new(
                        base.structural_type().clone(),
                        instance.get_range().clone(),
                    ))),
                    vec![base],
                ))
            }
            _ => Ok(value),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConditionalChoice {
    True,
    False,
    Both,
}
impl<E> OwnedQuery<'_, '_, E> {
    fn evaluate_owned_conditional(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        let LuaType::Conditional(conditional) = value.structural_type() else {
            return Ok(value);
        };
        self.checkpoint()?;
        if conditional_has_template(conditional.get_checked_type(), false)
            || conditional_has_template(conditional.get_extends_type(), false)
        {
            return Ok(value);
        }
        let checked = self.owned_child(&value, 0, conditional.get_checked_type());
        let extends = self.owned_child(&value, 1, conditional.get_extends_type());
        let yes = self.owned_child(&value, 2, conditional.get_true_type());
        let no = self.owned_child(&value, 3, conditional.get_false_type());
        if let Some(members) = checked.union_components() {
            let mut results = Vec::new();
            for member in members {
                let ty = LuaType::Conditional(Arc::new(LuaConditionalType::new(
                    member.structural_type().clone(),
                    extends.structural_type().clone(),
                    yes.structural_type().clone(),
                    no.structural_type().clone(),
                    conditional.get_infer_params().to_vec(),
                    conditional.has_new,
                )));
                let result =
                    self.evaluate_owned_conditional(OwnedValue::with_structural_children(
                        ty,
                        vec![no.clone(), yes.clone(), extends.clone(), member],
                    ))?;
                if !result.structural_type().is_never() && !results.contains(&result) {
                    results.push(result);
                }
            }
            return OwnedValue::union_checked(results, || self.checkpoint());
        }
        if checked.structural_type().is_never() {
            return Ok(OwnedValue::plain(LuaType::Never));
        }
        if conditional_has_template(extends.structural_type(), true) {
            // The remaining inference/constructor/index evaluator must preserve
            // owned candidates and variance; never project selected children away.
            return self.continuation(OwnedQueryCapability::TypeRelation, None, None, None);
        }
        match self.owned_conditional_choice(&checked, &extends)? {
            ConditionalChoice::True => self.eval_owned_conditionals(yes),
            ConditionalChoice::False => self.eval_owned_conditionals(no),
            ConditionalChoice::Both => {
                let yes = self.eval_owned_conditionals(yes)?;
                let no = self.eval_owned_conditionals(no)?;
                OwnedValue::union_checked(vec![yes, no], || self.checkpoint())
            }
        }
    }

    fn owned_conditional_choice(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<ConditionalChoice, E> {
        self.checkpoint()?;
        let s = source.structural_type();
        let t = target.structural_type();
        let literal = match (s, t) {
            (
                LuaType::StringConst(a) | LuaType::DocStringConst(a),
                LuaType::StringConst(b) | LuaType::DocStringConst(b),
            ) => Some(a == b),
            (
                LuaType::IntegerConst(a) | LuaType::DocIntegerConst(a),
                LuaType::IntegerConst(b) | LuaType::DocIntegerConst(b),
            ) => Some(a == b),
            (
                LuaType::BooleanConst(a) | LuaType::DocBooleanConst(a),
                LuaType::BooleanConst(b) | LuaType::DocBooleanConst(b),
            ) => Some(a == b),
            _ => None,
        };
        if let Some(equal) = literal {
            return Ok(if equal {
                ConditionalChoice::True
            } else {
                ConditionalChoice::False
            });
        }
        if s.is_any() {
            return Ok(ConditionalChoice::Both);
        }
        if t.is_any() || matches!(t, LuaType::Unknown) {
            return Ok(ConditionalChoice::True);
        }
        if s.is_unknown() {
            return Ok(ConditionalChoice::False);
        }
        if s.is_never() {
            return Ok(ConditionalChoice::True);
        }
        if let Some(members) = source.union_components() {
            let mut choice = ConditionalChoice::False;
            for member in members {
                let other = self.owned_conditional_choice(&member, target)?;
                choice = if choice == other {
                    choice
                } else {
                    ConditionalChoice::Both
                };
                if choice == ConditionalChoice::Both {
                    break;
                }
            }
            return Ok(choice);
        }
        if let Some(members) = target.union_components() {
            for member in members {
                if self.owned_conditional_choice(source, &member)? != ConditionalChoice::False {
                    return Ok(ConditionalChoice::True);
                }
            }
            return Ok(ConditionalChoice::False);
        }
        Ok(if self.type_check(source, target)? {
            ConditionalChoice::True
        } else {
            ConditionalChoice::False
        })
    }
}
fn conditional_has_template(ty: &LuaType, infer: bool) -> bool {
    use emmylua_code_analysis::LuaTypeNode;
    let mut stack = vec![ty];
    while let Some(ty) = stack.pop() {
        match ty {
            LuaType::TplRef(tpl) if tpl.get_tpl_id().is_conditional_infer() == infer => {
                return true;
            }
            LuaType::Array(_)
            | LuaType::Tuple(_)
            | LuaType::Union(_)
            | LuaType::Intersection(_)
            | LuaType::Object(_)
            | LuaType::DocFunction(_)
            | LuaType::Generic(_)
            | LuaType::TableGeneric(_)
            | LuaType::Variadic(_)
            | LuaType::Call(_)
            | LuaType::Conditional(_) => ty.push_direct_children(&mut stack),
            _ => {}
        }
    }
    false
}
