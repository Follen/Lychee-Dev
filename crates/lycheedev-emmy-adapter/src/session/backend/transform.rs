//! Identity-preserving structural transformations and conditional inference.
//! Contextual relation and member work stays in the owned query throughout.
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    GenericParam, GenericTpl, GenericTplId, LuaAliasCallType, LuaArrayType, LuaConditionalType,
    LuaFunctionType, LuaGenericType, LuaInstanceType, LuaIntersectionType, LuaMappedType,
    LuaMemberKey, LuaMultiLineUnion, LuaObjectType, LuaStringTplType, LuaTupleStatus, LuaType,
    LuaTypeDeclId, TypeDefKind, VariadicType,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Copy)]
enum Transform<'a> {
    Substitute(&'a HashMap<GenericTplId, OwnedValue>),
    Named(&'a HashMap<String, OwnedValue>),
    ReturnNames(&'a HashMap<String, OwnedValue>),
    Bind(&'a [GenericTpl]),
    Alias,
    Conditional,
    MappedBind(GenericTplId, &'a str),
    SelfType(&'a OwnedValue),
    Sanitize(&'a HashSet<GenericTplId>),
    SanitizeModel(&'a HashSet<GenericTplId>),
    SanitizeParam,
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
        // Union evidence is a dedicated carrier, not Structural. Consumers
        // enumerate LuaUnionType::into_vec in this same semantic order, including
        // Nullable's trailing Nil. Preserve identity before any relation/gate.
        if let Some(members) = value.carried_union() {
            return members[index].clone();
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
    pub(super) fn owned_sanitize_generics(
        &mut self,
        value: OwnedValue,
        allowed: &HashSet<GenericTplId>,
        with_model: bool,
    ) -> QueryResult<OwnedValue, E> {
        let op = if with_model {
            Transform::SanitizeModel(allowed)
        } else {
            Transform::Sanitize(allowed)
        };
        self.transform_owned(value, op, &mut Vec::new())
    }
    pub(super) fn owned_class_generic_map(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<HashMap<String, OwnedValue>>, E> {
        self.checkpoint()?;
        let LuaType::Generic(generic) = value.structural_type() else {
            return Ok(None);
        };
        let Some(def) = self.model.type_def_of(&generic.get_base_type_id()) else {
            return Ok(None);
        };
        let mut names = HashMap::new();
        for (index, param) in def.generic_params.iter().enumerate() {
            self.checkpoint()?;
            if let Some(raw) = generic.get_params().get(index) {
                names.insert(param.name.to_string(), self.owned_child(value, index, raw));
            }
        }
        Ok(Some(names))
    }
    pub(super) fn replace_owned_self(
        &mut self,
        value: OwnedValue,
        owner: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.transform_owned(value, Transform::SelfType(owner), &mut Vec::new())
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
    /// Bind declared return names with the same unconstrained Type(index)
    /// identity as the public doc shell projection. Rich object/index shapes
    /// remain intact; using the shell alone would reduce nonempty objects to
    /// broad Table. Existing nested function template identities stay local.
    pub(super) fn bind_owned_return_generics(
        &mut self,
        value: OwnedValue,
        generics: &[emmylua_code_analysis::DocGenericParam],
    ) -> QueryResult<OwnedValue, E> {
        let mut names = HashMap::new();
        for (index, generic) in generics.iter().enumerate() {
            self.checkpoint()?;
            names.entry(generic.name.to_string()).or_insert_with(|| {
                OwnedValue::plain(LuaType::TplRef(Arc::new(GenericTpl::new(
                    GenericTplId::Type(index as u32),
                    generic.name.clone(),
                    None,
                    None,
                    false,
                    None,
                ))))
            });
        }
        self.transform_owned(value, Transform::ReturnNames(&names), &mut Vec::new())
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
        if value.nominal_identity().is_some() {
            return Ok(value);
        }
        let value = if let Transform::SanitizeModel(allowed) = op {
            self.transform_owned(value, Transform::Sanitize(allowed), visiting)?
        } else {
            value
        };
        if matches!(
            op,
            Transform::Sanitize(_) | Transform::SanitizeModel(_) | Transform::SanitizeParam
        ) {
            let descend = matches!(
                value.structural_type(),
                LuaType::TplRef(_)
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Array(_)
                    | LuaType::Tuple(_)
                    | LuaType::Union(_)
                    | LuaType::Object(_)
                    | LuaType::TableGeneric(_)
                    | LuaType::Variadic(_)
                    | LuaType::Call(_)
            ) || (!matches!(op, Transform::Sanitize(_))
                && matches!(value.structural_type(), LuaType::Generic(_)));
            if !descend {
                return Ok(value);
            }
        }
        if matches!(op, Transform::MappedBind(_, _))
            && !matches!(
                value.structural_type(),
                LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Array(_)
                    | LuaType::Tuple(_)
                    | LuaType::Union(_)
                    | LuaType::Object(_)
                    | LuaType::Variadic(_)
                    | LuaType::Call(_)
                    | LuaType::Conditional(_)
            )
        {
            return Ok(value);
        }
        match (op, value.structural_type()) {
            (Transform::Sanitize(allowed), LuaType::TplRef(tpl))
                if !allowed.contains(&tpl.get_tpl_id()) =>
            {
                return Ok(OwnedValue::plain(LuaType::Unknown));
            }
            (Transform::SanitizeParam, LuaType::Ref(id) | LuaType::Def(id))
                if self.model.type_def_of(id).is_none() =>
            {
                return Ok(OwnedValue::plain(LuaType::Unknown));
            }
            (Transform::SelfType(owner), LuaType::SelfInfer) => return Ok(owner.clone()),
            (Transform::MappedBind(id, name), LuaType::Ref(found) | LuaType::Def(found))
                if found.get_name() == name =>
            {
                return Ok(OwnedValue::plain(LuaType::TplRef(Arc::new(
                    GenericTpl::new(id, name.into(), None, None, false, None),
                ))));
            }
            (Transform::Substitute(bindings), LuaType::TplRef(tpl)) => {
                return Ok(bindings.get(&tpl.get_tpl_id()).cloned().unwrap_or(value));
            }
            (
                Transform::Named(names) | Transform::ReturnNames(names),
                LuaType::Ref(id) | LuaType::Def(id),
            ) => {
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
                if matches!(op, Transform::Bind(_) | Transform::ReturnNames(_))
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
                let op = if matches!(op, Transform::SanitizeModel(_)) {
                    Transform::SanitizeParam
                } else {
                    op
                };
                if matches!(op, Transform::Alias) {
                    let base = generic.get_base_type_id();
                    if matches!(base.get_name(), "RawGet" | "std.RawGet")
                        && generic.get_params().len() == 2
                    {
                        let owner = self.transform_owned(
                            self.owned_child(&value, 0, &generic.get_params()[0]),
                            op,
                            visiting,
                        )?;
                        let key = self.transform_owned(
                            self.owned_child(&value, 1, &generic.get_params()[1]),
                            op,
                            visiting,
                        )?;
                        if let Some(found) = self.eval_owned_index_access(&owner, &key)? {
                            return Ok(found);
                        }
                    }
                    if base.get_name() == "Merge" {
                        let mut operands = Vec::new();
                        for (index, ty) in generic.get_params().iter().enumerate() {
                            operands.push(self.transform_owned(
                                self.owned_child(&value, index, ty),
                                op,
                                visiting,
                            )?);
                        }
                        if let Some(merged) = self.eval_owned_merge(&operands)? {
                            return Ok(merged);
                        }
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
                        if let LuaType::Conditional(conditional) = &alias {
                            if generic
                                .get_params()
                                .iter()
                                .any(|ty| conditional_has_template(ty, false))
                            {
                                visiting.pop();
                                let mut params = Vec::new();
                                for (index, ty) in generic.get_params().iter().enumerate() {
                                    params.push(self.transform_owned(
                                        self.owned_child(&value, index, ty),
                                        op,
                                        visiting,
                                    )?);
                                }
                                let ty = LuaType::Generic(Arc::new(LuaGenericType::new(
                                    base,
                                    params.iter().map(|v| v.structural_type().clone()).collect(),
                                )));
                                params.reverse();
                                return Ok(OwnedValue::with_structural_children(ty, params));
                            }
                            let evaluated = self.evaluate_owned_conditional_alias(
                                conditional,
                                &bindings,
                                &names,
                            )?;
                            let expanded = self.transform_owned(evaluated, op, visiting)?;
                            visiting.pop();
                            return Ok(expanded);
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
                    if let Some(merged) = self.eval_owned_merge(&values)? {
                        return Ok(merged);
                    }
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
                let scoped_names = match op {
                    Transform::ReturnNames(names)
                        if names.contains_key(mapped.param.1.name.as_str()) =>
                    {
                        let mut names = names.clone();
                        names.remove(mapped.param.1.name.as_str());
                        Some(names)
                    }
                    _ => None,
                };
                let op = scoped_names.as_ref().map_or(op, Transform::ReturnNames);
                if matches!(op, Transform::Alias)
                    && let Some(expanded) = self.expand_owned_mapped(&value)?
                {
                    return self.transform_owned(expanded, op, visiting);
                }
                let old = value.structural_children();
                let mut next = 1;
                let constraint = if let Some(ty) = &mapped.param.1.constraint {
                    let child = old
                        .map(|c| c[next].clone())
                        .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                    next += 1;
                    Some(if matches!(op, Transform::SelfType(_)) {
                        child
                    } else {
                        self.transform_owned(child, op, visiting)?
                    })
                } else {
                    None
                };
                let default = if let Some(ty) = &mapped.param.1.default {
                    let child = old
                        .map(|c| c[next].clone())
                        .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                    Some(if matches!(op, Transform::SelfType(_)) {
                        child
                    } else {
                        self.transform_owned(child, op, visiting)?
                    })
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
            LuaType::StrTplRef(tpl)
                if matches!(op, Transform::Substitute(_) | Transform::ReturnNames(_)) =>
            {
                let constraint = if let Some(ty) = tpl.get_constraint() {
                    Some(self.transform_owned(self.owned_child(&value, 0, ty), op, visiting)?)
                } else {
                    None
                };
                let id = match op {
                    Transform::ReturnNames(names)
                        if matches!(tpl.get_tpl_id(), GenericTplId::Type(_)) =>
                    {
                        names
                            .get(tpl.get_name())
                            .and_then(|value| match value.structural_type() {
                                LuaType::TplRef(generic) => Some(generic.get_tpl_id()),
                                _ => None,
                            })
                            .unwrap_or_else(|| tpl.get_tpl_id())
                    }
                    _ => tpl.get_tpl_id(),
                };
                let ty = LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                    tpl.get_prefix(),
                    tpl.get_name(),
                    id,
                    tpl.get_suffix(),
                    constraint.as_ref().map(|v| v.structural_type().clone()),
                )));
                Ok(OwnedValue::with_structural_children(
                    ty,
                    constraint.into_iter().collect(),
                ))
            }
            LuaType::MultiLineUnion(union)
                if matches!(
                    op,
                    Transform::Substitute(_) | Transform::SelfType(_) | Transform::ReturnNames(_)
                ) =>
            {
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
            LuaType::TypeGuard(inner)
                if matches!(
                    op,
                    Transform::Substitute(_) | Transform::SelfType(_) | Transform::ReturnNames(_)
                ) =>
            {
                let inner =
                    self.transform_owned(self.owned_child(&value, 0, inner), op, visiting)?;
                Ok(OwnedValue::with_structural_children(
                    LuaType::TypeGuard(Arc::new(inner.structural_type().clone())),
                    vec![inner],
                ))
            }
            LuaType::Instance(instance)
                if matches!(
                    op,
                    Transform::Substitute(_) | Transform::SelfType(_) | Transform::ReturnNames(_)
                ) =>
            {
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
            let source = if conditional.has_new {
                self.owned_constructor_function(&checked)?
                    .unwrap_or(checked)
            } else {
                checked
            };
            let source = self.expand_owned_index_call(&source)?.unwrap_or(source);
            let mut assignments = HashMap::new();
            if self.collect_owned_infer(
                &source,
                &extends,
                &mut assignments,
                InferVariance::Covariant,
            )? {
                let mut bindings = HashMap::new();
                for (id, candidates) in assignments {
                    self.checkpoint()?;
                    if let Some(value) = candidates.covariant {
                        bindings.insert(id, value);
                    } else if let Some(value) = candidates.contravariant {
                        bindings.insert(id, self.simplify_owned_infer_intersection(value)?);
                    }
                }
                let yes = self.substitute_owned(yes, &bindings)?;
                return self.eval_owned_conditionals(yes);
            }
            return self.eval_owned_conditionals(no);
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
            LuaType::Mapped(_) if !infer => ty.push_direct_children(&mut stack),
            _ => {}
        }
    }
    false
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn eval_owned_index_access(
        &mut self,
        base: &OwnedValue,
        key: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        use emmylua_code_analysis::LuaAliasCallKind;
        self.checkpoint()?;
        match key.structural_type() {
            LuaType::Call(call) if call.get_call_kind() == LuaAliasCallKind::KeyOf => {
                let Some(first) = call.get_operands().first() else {
                    return Ok(None);
                };
                let infos = self.member_infos(&self.owned_child(key, 0, first))?;
                if infos.is_empty() {
                    return Ok(None);
                }
                OwnedValue::union_checked(infos.into_iter().map(|info| info.typ).collect(), || {
                    self.checkpoint()
                })
                .map(Some)
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                let Some(alias) = self.model.alias_target(&def) else {
                    return Ok(None);
                };
                self.eval_owned_index_access(base, &OwnedValue::plain(alias))
            }
            LuaType::Union(_) => {
                let mut types = Vec::new();
                for member in key.union_components().expect("union components") {
                    if let Some(found) = self.eval_owned_index_access(base, &member)?
                        && !types.contains(&found)
                    {
                        types.push(found);
                    }
                }
                if types.is_empty() {
                    Ok(None)
                } else {
                    OwnedValue::union_checked(types, || self.checkpoint()).map(Some)
                }
            }
            ty => match literal_member_key(ty) {
                Some(key) => self.member_type(base, &key),
                None => Ok(None),
            },
        }
    }
    pub(super) fn expand_owned_index_call(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        use emmylua_code_analysis::LuaAliasCallKind;
        let LuaType::Call(call) = value.structural_type() else {
            return Ok(None);
        };
        if call.get_call_kind() != LuaAliasCallKind::Index || call.get_operands().len() != 2 {
            return Ok(None);
        }
        let base = self.owned_child(value, 0, &call.get_operands()[0]);
        let key = self.owned_child(value, 1, &call.get_operands()[1]);
        if let LuaType::Call(key_call) = key.structural_type()
            && key_call.get_call_kind() == LuaAliasCallKind::KeyOf
            && let Some(first) = key_call.get_operands().first()
        {
            let infos = self.member_infos(&self.owned_child(&key, 0, first))?;
            if infos.is_empty() {
                return Ok(None);
            }
            return OwnedValue::union_checked(
                infos.into_iter().map(|info| info.typ).collect(),
                || self.checkpoint(),
            )
            .map(Some);
        }
        let mut types = Vec::new();
        for (key, _) in owned_literal_keys(&key) {
            self.checkpoint()?;
            let found = if let LuaType::Tuple(tuple) = base.structural_type() {
                if let LuaMemberKey::Integer(index) = key {
                    tuple
                        .get_types()
                        .get(index as usize)
                        .map(|ty| self.owned_child(&base, index as usize, ty))
                } else {
                    None
                }
            } else {
                self.member_type(&base, &key)?
            };
            if let Some(found) = found
                && !types.contains(&found)
            {
                types.push(found);
            }
        }
        if types.is_empty() {
            Ok(None)
        } else {
            OwnedValue::union_checked(types, || self.checkpoint()).map(Some)
        }
    }
    fn eval_owned_merge(&mut self, operands: &[OwnedValue]) -> QueryResult<Option<OwnedValue>, E> {
        if operands.len() < 2 {
            return Ok(None);
        }
        let mut fields = Vec::new();
        let mut has_info = false;
        for operand in operands {
            let infos = self.member_infos(operand)?;
            if infos.is_empty() {
                if !matches!(
                    operand.structural_type(),
                    LuaType::Object(_)
                        | LuaType::Ref(_)
                        | LuaType::Def(_)
                        | LuaType::Generic(_)
                        | LuaType::TableConst(_)
                ) {
                    return Ok(None);
                }
            } else {
                has_info = true;
            }
            for info in infos {
                self.checkpoint()?;
                if matches!(info.key, LuaMemberKey::Name(_)) {
                    replace_owned_field(&mut fields, info.key, info.typ);
                }
            }
        }
        Ok(has_info.then(|| owned_fields_object(fields)))
    }
    fn owned_keyof_keys(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Vec<(LuaMemberKey, OwnedValue)>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Union(_) => {
                let mut keys = Vec::new();
                for member in value.union_components().expect("union components") {
                    keys.extend(self.owned_keyof_keys(&member)?);
                }
                Ok(keys)
            }
            LuaType::Tuple(tuple) => Ok(tuple
                .get_types()
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let key = LuaMemberKey::Integer(index as i64);
                    (key.clone(), owned_key_literal(&key))
                })
                .collect()),
            LuaType::Object(object) => {
                let mut keys = Vec::new();
                for key in object.get_fields().keys() {
                    self.checkpoint()?;
                    keys.push((key.clone(), owned_key_literal(key)));
                }
                for (key, _) in object.get_index_access() {
                    self.checkpoint()?;
                    if let Some(key) = literal_member_key(key) {
                        keys.push((key.clone(), owned_key_literal(&key)));
                    }
                }
                Ok(keys)
            }
            LuaType::Ref(_)
            | LuaType::Def(_)
            | LuaType::Generic(_)
            | LuaType::TableConst(_)
            | LuaType::Array(_) => Ok(self
                .member_infos(value)?
                .into_iter()
                .map(|info| (info.key.clone(), owned_key_literal(&info.key)))
                .collect()),
            _ => Ok(Vec::new()),
        }
    }
    fn expand_owned_mapped(&mut self, value: &OwnedValue) -> QueryResult<Option<OwnedValue>, E> {
        let LuaType::Mapped(mapped) = value.structural_type() else {
            return Ok(None);
        };
        let Some(constraint) = &mapped.param.1.constraint else {
            return Ok(None);
        };
        let constraint = value
            .structural_children()
            .map(|children| children[1].clone())
            .unwrap_or_else(|| OwnedValue::plain(constraint.clone()));
        let keys = if let LuaType::Call(call) = constraint.structural_type()
            && call.get_call_kind() == emmylua_code_analysis::LuaAliasCallKind::KeyOf
        {
            let Some(first) = call.get_operands().first() else {
                return Ok(None);
            };
            self.owned_keyof_keys(&self.owned_child(&constraint, 0, first))?
        } else {
            owned_literal_keys(&constraint)
        };
        if keys.is_empty() {
            return Ok(None);
        }
        let body = value
            .structural_children()
            .map(|children| children[0].clone())
            .unwrap_or_else(|| OwnedValue::plain(mapped.value.clone()));
        let mut fields = Vec::new();
        for (key, key_value) in keys {
            let bound = self.transform_owned(
                body.clone(),
                Transform::MappedBind(mapped.param.0, mapped.param.1.name.as_str()),
                &mut Vec::new(),
            )?;
            let bound =
                self.substitute_owned(bound, &HashMap::from([(mapped.param.0, key_value)]))?;
            let bound = self.expand_owned_index_call(&bound)?.unwrap_or(bound);
            replace_owned_field(&mut fields, key, bound);
        }
        Ok(Some(owned_fields_object(fields)))
    }
    fn evaluate_owned_conditional_alias(
        &mut self,
        conditional: &LuaConditionalType,
        bindings: &HashMap<GenericTplId, OwnedValue>,
        names: &HashMap<String, OwnedValue>,
    ) -> QueryResult<OwnedValue, E> {
        let naked = match conditional.get_checked_type() {
            LuaType::TplRef(tpl) => Some((tpl.get_tpl_id(), tpl.get_name().to_string())),
            LuaType::Ref(id) | LuaType::Def(id) if names.contains_key(id.get_name()) => {
                Some((GenericTplId::Type(0), id.get_name().to_string()))
            }
            _ => None,
        };
        if let Some((id, name)) = naked
            && let Some(mut bound) = bindings.get(&id).or_else(|| names.get(&name)).cloned()
        {
            if let LuaType::Ref(id) | LuaType::Def(id) = bound.structural_type()
                && let Some(def) = self.model.type_def_of(id)
                && def.kind == TypeDefKind::Alias
                && let Some(alias) = self.model.alias_target(&def)
            {
                bound = OwnedValue::plain(alias);
            }
            let bound = self.expand_owned_aliases(bound)?;
            if bound.structural_type().is_never() {
                return Ok(OwnedValue::plain(LuaType::Never));
            }
            if let Some(members) = bound.union_components() {
                let mut results = Vec::new();
                for member in members {
                    let mut bindings = bindings.clone();
                    bindings.insert(id, member.clone());
                    let mut names = names.clone();
                    names.insert(name.clone(), member);
                    let raw =
                        OwnedValue::plain(LuaType::Conditional(Arc::new(conditional.clone())));
                    let substituted = self.substitute_owned(raw, &bindings)?;
                    let substituted = self.substitute_owned_names(substituted, &names)?;
                    let result = self.evaluate_owned_conditional(substituted)?;
                    if !result.structural_type().is_never() && !results.contains(&result) {
                        results.push(result);
                    }
                }
                return OwnedValue::union_checked(results, || self.checkpoint());
            }
            let mut bindings = bindings.clone();
            bindings.insert(id, bound.clone());
            let mut names = names.clone();
            names.insert(name, bound);
            let raw = OwnedValue::plain(LuaType::Conditional(Arc::new(conditional.clone())));
            let substituted = self.substitute_owned(raw, &bindings)?;
            let substituted = self.substitute_owned_names(substituted, &names)?;
            return self.evaluate_owned_conditional(substituted);
        }
        let raw = OwnedValue::plain(LuaType::Conditional(Arc::new(conditional.clone())));
        let substituted = self.substitute_owned(raw, bindings)?;
        let substituted = self.substitute_owned_names(substituted, names)?;
        self.evaluate_owned_conditional(substituted)
    }
}
fn literal_member_key(ty: &LuaType) -> Option<LuaMemberKey> {
    match ty {
        LuaType::StringConst(value) | LuaType::DocStringConst(value) => {
            Some(LuaMemberKey::Name(value.as_str().into()))
        }
        LuaType::IntegerConst(value) | LuaType::DocIntegerConst(value) => {
            Some(LuaMemberKey::Integer(*value))
        }
        _ => None,
    }
}
fn owned_literal_keys(value: &OwnedValue) -> Vec<(LuaMemberKey, OwnedValue)> {
    if let Some(key) = literal_member_key(value.structural_type()) {
        return vec![(key, value.clone())];
    }
    value
        .union_components()
        .into_iter()
        .flatten()
        .flat_map(|member| owned_literal_keys(&member))
        .collect()
}
fn owned_key_literal(key: &LuaMemberKey) -> OwnedValue {
    OwnedValue::plain(match key {
        LuaMemberKey::Name(name) => LuaType::StringConst(name.clone().into()),
        LuaMemberKey::Integer(index) => LuaType::IntegerConst(*index),
        _ => LuaType::Unknown,
    })
}
fn replace_owned_field(
    fields: &mut Vec<(LuaMemberKey, OwnedValue)>,
    key: LuaMemberKey,
    value: OwnedValue,
) {
    if let Some((_, existing)) = fields.iter_mut().find(|(found, _)| found == &key) {
        *existing = value;
    } else {
        fields.push((key, value));
    }
}
fn owned_fields_object(fields: Vec<(LuaMemberKey, OwnedValue)>) -> OwnedValue {
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
        .map(|key| {
            fields
                .iter()
                .find(|(found, _)| found == key)
                .expect("rebuilt field")
                .1
                .clone()
        })
        .collect();
    OwnedValue::with_structural_children(LuaType::Object(Arc::new(object)), children)
}

#[derive(Clone, Copy)]
enum InferVariance {
    Covariant,
    Contravariant,
}
impl InferVariance {
    fn flip(self) -> Self {
        match self {
            Self::Covariant => Self::Contravariant,
            Self::Contravariant => Self::Covariant,
        }
    }
}
#[derive(Default)]
struct OwnedInferCandidates {
    covariant: Option<OwnedValue>,
    contravariant: Option<OwnedValue>,
}

impl<E> OwnedQuery<'_, '_, E> {
    fn owned_constructor_function(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let def = match value.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => self.model.type_def_of(id),
            LuaType::TableConst(table) => {
                self.model.file_facts_of(table.file_id).and_then(|facts| {
                    let decl = facts.decl_by_value_range(table.value)?;
                    facts.type_def_by_owner_syntax(decl.owner_syntax?).cloned()
                })
            }
            LuaType::StringConst(name) | LuaType::DocStringConst(name) => {
                self.model.resolve_type_def(name.as_ref())
            }
            _ => None,
        };
        let Some(def) = def else {
            return Ok(None);
        };
        for syntax in &def.call_overloads {
            self.checkpoint()?;
            let ty = self
                .model
                .doc_type_lua_in(def.file_id, *syntax, &def.generic_params);
            if matches!(ty, LuaType::DocFunction(_)) {
                return Ok(Some(OwnedValue::plain(ty)));
            }
        }
        Ok(None)
    }
    fn collect_owned_infer(
        &mut self,
        source: &OwnedValue,
        pattern: &OwnedValue,
        assignments: &mut HashMap<GenericTplId, OwnedInferCandidates>,
        variance: InferVariance,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if let LuaType::TplRef(tpl) = pattern.structural_type()
            && tpl.get_tpl_id().is_conditional_infer()
        {
            let candidate = assignments.entry(tpl.get_tpl_id()).or_default();
            match variance {
                InferVariance::Covariant => {
                    candidate.covariant = Some(match candidate.covariant.take() {
                        Some(existing) => {
                            OwnedValue::union_checked(vec![existing, source.clone()], || {
                                self.checkpoint()
                            })?
                        }
                        None => source.clone(),
                    });
                }
                InferVariance::Contravariant => {
                    candidate.contravariant = Some(match candidate.contravariant.take() {
                        Some(existing) => owned_intersection(vec![existing, source.clone()]),
                        None => source.clone(),
                    });
                }
            }
            return Ok(true);
        }
        match pattern.structural_type() {
            LuaType::Array(pattern_array) => {
                let LuaType::Array(source_array) = source.structural_type() else {
                    return Ok(false);
                };
                self.collect_owned_infer(
                    &self.owned_child(source, 0, source_array.get_base()),
                    &self.owned_child(pattern, 0, pattern_array.get_base()),
                    assignments,
                    variance,
                )
            }
            LuaType::Tuple(pattern_tuple) => {
                let LuaType::Tuple(source_tuple) = source.structural_type() else {
                    return Ok(false);
                };
                if source_tuple.get_types().len() != pattern_tuple.get_types().len() {
                    return Ok(false);
                }
                for (index, (p, s)) in pattern_tuple
                    .get_types()
                    .iter()
                    .zip(source_tuple.get_types())
                    .enumerate()
                {
                    if !self.collect_owned_infer(
                        &self.owned_child(source, index, s),
                        &self.owned_child(pattern, index, p),
                        assignments,
                        variance,
                    )? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LuaType::Variadic(pattern_variadic) => {
                let LuaType::Variadic(source_variadic) = source.structural_type() else {
                    return Ok(false);
                };
                match (pattern_variadic.as_ref(), source_variadic.as_ref()) {
                    (VariadicType::Base(p), VariadicType::Base(s)) => self.collect_owned_infer(
                        &self.owned_child(source, 0, s),
                        &self.owned_child(pattern, 0, p),
                        assignments,
                        variance,
                    ),
                    (VariadicType::Base(p), VariadicType::Multi(types)) => {
                        let mut types = types
                            .iter()
                            .enumerate()
                            .map(|(index, ty)| self.owned_child(source, index, ty))
                            .collect::<Vec<_>>();
                        let source = if types.len() == 1 {
                            types.pop().expect("single source")
                        } else {
                            OwnedValue::tuple_checked(types, LuaTupleStatus::InferResolve, || {
                                self.checkpoint()
                            })?
                        };
                        self.collect_owned_infer(
                            &source,
                            &self.owned_child(pattern, 0, p),
                            assignments,
                            variance,
                        )
                    }
                    _ => Ok(false),
                }
            }
            LuaType::Generic(p) => {
                let LuaType::Generic(s) = source.structural_type() else {
                    return Ok(false);
                };
                if p.get_base_type_id() != s.get_base_type_id()
                    || p.get_params().len() != s.get_params().len()
                {
                    return Ok(false);
                }
                for (index, (p, s)) in p.get_params().iter().zip(s.get_params()).enumerate() {
                    if !self.collect_owned_infer(
                        &self.owned_child(source, index, s),
                        &self.owned_child(pattern, index, p),
                        assignments,
                        variance,
                    )? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LuaType::Object(object) => {
                if !matches!(
                    source.structural_type(),
                    LuaType::Object(_) | LuaType::Ref(_) | LuaType::Def(_) | LuaType::TableConst(_)
                ) {
                    return Ok(false);
                }
                for (key, raw_pattern) in object.get_fields() {
                    self.checkpoint()?;
                    let source_field = if let LuaType::Object(object) = source.structural_type() {
                        object
                            .get_fields()
                            .get(key)
                            .map(|ty| self.owned_field(source, key, ty))
                    } else {
                        self.member_type(source, key)?
                    };
                    if let Some(source_field) = source_field {
                        if !self.collect_owned_infer(
                            &source_field,
                            &self.owned_field(pattern, key, raw_pattern),
                            assignments,
                            variance,
                        )? {
                            return Ok(false);
                        }
                    } else if conditional_has_template(raw_pattern, true) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LuaType::DocFunction(_) => {
                if !matches!(source.structural_type(), LuaType::DocFunction(_)) {
                    return Ok(false);
                }
                self.collect_owned_function_infer(source, pattern, assignments, variance)
            }
            _ => {
                if conditional_has_template(pattern.structural_type(), true) {
                    Ok(false)
                } else {
                    Ok(source == pattern || self.type_check(source, pattern)?)
                }
            }
        }
    }
    fn collect_owned_function_infer(
        &mut self,
        source: &OwnedValue,
        pattern: &OwnedValue,
        assignments: &mut HashMap<GenericTplId, OwnedInferCandidates>,
        variance: InferVariance,
    ) -> QueryResult<bool, E> {
        let LuaType::DocFunction(source_fun) = source.structural_type() else {
            unreachable!()
        };
        let LuaType::DocFunction(pattern_fun) = pattern.structural_type() else {
            unreachable!()
        };
        let source_params = source
            .function_parameters()
            .expect("source function parameters");
        let pattern_params = pattern
            .function_parameters()
            .expect("pattern function parameters");
        let has_variadic = pattern_fun.get_params().last().is_some_and(|(name, ty)| {
            name == "..." || ty.as_ref().is_some_and(LuaType::is_variadic)
        });
        let normal_len = pattern_params
            .len()
            .saturating_sub(usize::from(has_variadic));
        if !has_variadic && source_params.len() > normal_len {
            return Ok(false);
        }
        for (index, pattern_param) in pattern_params.iter().take(normal_len).enumerate() {
            self.checkpoint()?;
            match (
                source_params.get(index).and_then(Option::as_ref),
                pattern_param.as_ref(),
            ) {
                (Some(source), Some(pattern)) => {
                    if !self.collect_owned_infer(source, pattern, assignments, variance.flip())? {
                        return Ok(false);
                    }
                }
                (None, Some(pattern))
                    if conditional_has_template(pattern.structural_type(), true) =>
                {
                    return Ok(false);
                }
                _ => {}
            }
        }
        if has_variadic
            && let Some(Some(pattern)) = pattern_params.last()
            && conditional_has_template(pattern.structural_type(), true)
        {
            let rest = source_params
                .iter()
                .skip(normal_len)
                .map(|ty| {
                    ty.clone()
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Any))
                })
                .collect::<Vec<_>>();
            let source = match rest.len() {
                0 => OwnedValue::plain(LuaType::Never),
                1 if source_fun.is_variadic() => rest.into_iter().next().expect("single rest"),
                _ => OwnedValue::tuple_checked(rest, LuaTupleStatus::InferResolve, || {
                    self.checkpoint()
                })?,
            };
            return self.collect_owned_infer(&source, pattern, assignments, variance.flip());
        }
        let pattern = pattern.function_return().expect("pattern return");
        if conditional_has_template(pattern.structural_type(), true) {
            self.collect_owned_infer(
                &source.function_return().expect("source return"),
                &pattern,
                assignments,
                variance,
            )
        } else {
            Ok(true)
        }
    }
    fn simplify_owned_infer_intersection(
        &mut self,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        if !matches!(value.structural_type(), LuaType::Intersection(_)) {
            return Ok(value);
        }
        let mut stack = vec![value];
        let mut components = Vec::new();
        while let Some(value) = stack.pop() {
            self.checkpoint()?;
            if let LuaType::Intersection(intersection) = value.structural_type() {
                for (index, ty) in intersection.get_types().iter().enumerate().rev() {
                    stack.push(self.owned_child(&value, index, ty));
                }
            } else {
                components.push(value);
            }
        }
        let mut seen = Vec::new();
        for component in components {
            self.checkpoint()?;
            if component.structural_type().is_never() {
                return Ok(OwnedValue::plain(LuaType::Never));
            }
            if !seen.contains(&component) {
                seen.push(component);
            }
        }
        for (index, left) in seen.iter().enumerate() {
            for right in seen.iter().skip(index + 1) {
                self.checkpoint()?;
                if let (Some(left), Some(right)) = (
                    owned_primitive_group(left.structural_type()),
                    owned_primitive_group(right.structural_type()),
                ) && left != right
                {
                    return Ok(OwnedValue::plain(LuaType::Never));
                }
            }
        }
        Ok(if seen.len() == 1 {
            seen.pop().expect("single component")
        } else {
            owned_intersection(seen)
        })
    }
}
fn owned_intersection(values: Vec<OwnedValue>) -> OwnedValue {
    let ty = LuaType::Intersection(Arc::new(LuaIntersectionType::new(
        values
            .iter()
            .map(|value| value.structural_type().clone())
            .collect(),
    )));
    OwnedValue::with_structural_children(ty, values.into_iter().rev().collect())
}
fn owned_primitive_group(ty: &LuaType) -> Option<u8> {
    match ty {
        LuaType::String | LuaType::StringConst(_) | LuaType::DocStringConst(_) => Some(0),
        LuaType::Number
        | LuaType::Integer
        | LuaType::IntegerConst(_)
        | LuaType::DocIntegerConst(_)
        | LuaType::FloatConst(_) => Some(1),
        LuaType::Boolean | LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_) => Some(2),
        LuaType::Nil => Some(3),
        _ => None,
    }
}
