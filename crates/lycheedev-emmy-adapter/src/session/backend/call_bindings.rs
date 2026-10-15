//! Call-site generic inference, adapted from fixed EmmyLua's infer/vm.rs.
//! Unlike structural matching, a failed alternative retains partial bindings.
use super::{OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    GenericTplId, LuaFunctionType, LuaGenericType, LuaTupleStatus, LuaType, LuaTypeDeclId,
    LuaTypeNode, TypeDefKind, TypeVisibility, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaDocTag, LuaDocType};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn unify_call_bindings(
        &mut self,
        parameter: &OwnedValue,
        argument: &OwnedValue,
        bindings: &mut Bindings,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        use LuaType::*;
        let p = parameter.structural_type();
        let a = argument.structural_type();
        match p {
            Unknown | Any => Ok(true),
            TplRef(tpl) => {
                if matches!(a, TplRef(actual) if actual.get_tpl_id() == tpl.get_tpl_id()) {
                    return Ok(true);
                }
                let id = tpl.get_tpl_id();
                if let Some(existing) = bindings.get(&id).cloned() {
                    if existing == *argument
                        || matches!(a, Unknown | Any | Never)
                        || self.type_check_subtype(argument, &existing)?
                    {
                        return Ok(true);
                    }
                    return self.unify_call_bindings(&existing, argument, bindings);
                }
                let arg = if tpl.is_const() || tpl.get_constraint().is_some() {
                    argument.clone()
                } else {
                    widen_call_const(argument)
                };
                if !self.unify_structural(parameter, &arg, bindings)? {
                    return Ok(false);
                }
                if let Some(constraint) = tpl.get_constraint()
                    && !matches!(constraint, Unknown | Call(_))
                    && !self.type_check(&arg, &OwnedValue::plain(constraint.clone()))?
                {
                    return Ok(false);
                }
                Ok(true)
            }
            Variadic(pv) => match pv.as_ref() {
                VariadicType::Base(base) => {
                    let base = self.owned_child(parameter, 0, base);
                    match a {
                        Variadic(av) => match av.as_ref() {
                            VariadicType::Base(ab) => self.unify_call_bindings(
                                &base,
                                &self.owned_child(argument, 0, ab),
                                bindings,
                            ),
                            VariadicType::Multi(types) => {
                                if matches!(base.structural_type(), TplRef(_)) {
                                    return self.unify_call_bindings(&base, argument, bindings);
                                }
                                for (i, ty) in types.iter().enumerate() {
                                    if !self.unify_call_bindings(
                                        &base,
                                        &self.owned_child(argument, i, ty),
                                        bindings,
                                    )? {
                                        return Ok(false);
                                    }
                                }
                                Ok(true)
                            }
                        },
                        Unknown | Any => Ok(true),
                        _ => self.unify_call_bindings(&base, argument, bindings),
                    }
                }
                VariadicType::Multi(types) => match a {
                    Variadic(av) => {
                        let VariadicType::Multi(actual) = av.as_ref() else {
                            return Ok(false);
                        };
                        if types.len() != actual.len() {
                            return Ok(false);
                        }
                        for (i, (p, a)) in types.iter().zip(actual).enumerate() {
                            if !self.unify_call_bindings(
                                &self.owned_child(parameter, i, p),
                                &self.owned_child(argument, i, a),
                                bindings,
                            )? {
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    }
                    Unknown | Any => Ok(true),
                    _ if types.len() == 1
                        || (types.len() == 2
                            && matches!(types.last(), Some(Variadic(v)) if matches!(v.as_ref(), VariadicType::Base(_)))) =>
                    {
                        self.unify_call_bindings(
                            &self.owned_child(parameter, 0, &types[0]),
                            argument,
                            bindings,
                        )
                    }
                    _ => Ok(false),
                },
            },
            Generic(pg) => {
                if let Generic(ag) = a
                    && pg.get_base_type_id() == ag.get_base_type_id()
                    && pg.get_params().len() == ag.get_params().len()
                {
                    for (i, (p, a)) in pg.get_params().iter().zip(ag.get_params()).enumerate() {
                        if !self.unify_call_bindings(
                            &self.owned_child(parameter, i, p),
                            &self.owned_child(argument, i, a),
                            bindings,
                        )? {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                }
                let expanded = match a {
                    Ref(id) | Def(id) => self
                        .model
                        .type_def_of(id)
                        .filter(|def| def.kind == TypeDefKind::Alias)
                        .and_then(|def| self.model.alias_target(&def))
                        .map(OwnedValue::plain)
                        .unwrap_or_else(|| argument.clone()),
                    _ => argument.clone(),
                };
                let expanded = self.expand_owned_aliases(expanded)?;
                if expanded != *argument {
                    return self.unify_call_bindings(parameter, &expanded, bindings);
                }
                if let Some(parent) = self.call_super_generic(argument, &pg.get_base_type_id())? {
                    return self.unify_call_bindings(parameter, &parent, bindings);
                }
                self.unify_structural(parameter, argument, bindings)
            }
            Table | TableConst(_) | TableGeneric(_) => Ok(true),
            Object(_) => {
                if matches!(a, Object(_)) {
                    self.unify_structural(parameter, argument, bindings)
                } else {
                    Ok(false)
                }
            }
            DocFunction(pf) => {
                let DocFunction(af) = a else {
                    return Ok(matches!(a, Function));
                };
                let pp = parameter
                    .function_parameters()
                    .expect("function parameters");
                let ap = argument.function_parameters().expect("function parameters");
                let mut i = 0;
                let mut ai = 0usize;
                while i < pp.len() {
                    self.checkpoint()?;
                    let Some(pt) = &pp[i] else {
                        i += 1;
                        continue;
                    };
                    if matches!(pt.structural_type(), Unknown | Any) {
                        i += 1;
                        continue;
                    }
                    if pf.get_params()[i].0 == "..." || matches!(pt.structural_type(), Variadic(_))
                    {
                        let trailing = pf.get_params()[i + 1..]
                            .iter()
                            .filter(|(name, ty)| name != "..." && !matches!(ty, Some(Variadic(_))))
                            .count();
                        let tpl = match pt.structural_type() {
                            Variadic(v) => match v.as_ref() {
                                VariadicType::Base(TplRef(tpl)) => Some(tpl),
                                _ => None,
                            },
                            _ => None,
                        };
                        let consume = tpl
                            .and_then(|tpl| bindings.get(&tpl.get_tpl_id()))
                            .map(|bound| match bound.structural_type() {
                                Tuple(t) => t.get_types().len(),
                                Variadic(v) => match v.as_ref() {
                                    VariadicType::Multi(v) => v.len(),
                                    VariadicType::Base(_) => 1,
                                },
                                _ => 1,
                            })
                            .unwrap_or_else(|| ap.len().saturating_sub(ai).saturating_sub(trailing))
                            .min(ap.len().saturating_sub(ai));
                        let remaining: Vec<_> =
                            ap[ai..ai + consume].iter().flatten().cloned().collect();
                        if let Some(tpl) = tpl
                            && !bindings.contains_key(&tpl.get_tpl_id()) && !remaining.is_empty()
                            && remaining.iter().any(|v| !matches!(v.structural_type(),Unknown|Any))
                            && !remaining.iter().any(|v| matches!(v.structural_type(),Variadic(v) if matches!(v.as_ref(),VariadicType::Base(TplRef(t)) if t.get_tpl_id() == tpl.get_tpl_id()))) {
                            let bound = if remaining.len() == 1 { remaining[0].clone() } else { OwnedValue::tuple_checked(remaining,LuaTupleStatus::InferResolve,||self.checkpoint())? };
                            if !self.unify_call_bindings(&OwnedValue::plain(TplRef(tpl.clone())),&bound,bindings)? { return Ok(false); }
                        }
                        i += 1;
                        ai += consume.max(1);
                        continue;
                    }
                    let at = ap
                        .get(ai)
                        .and_then(Clone::clone)
                        .unwrap_or_else(|| OwnedValue::plain(Unknown));
                    if !self.unify_call_bindings(pt, &at, bindings)? {
                        return Ok(false);
                    }
                    i += 1;
                    ai += 1;
                }
                let actual_ids: HashSet<_> = af
                    .get_generic_params()
                    .iter()
                    .map(|p| p.get_tpl_id())
                    .collect();
                if !actual_ids.is_empty() {
                    if self.call_contains_ids(pf.get_ret(), &actual_ids)? {
                        return Ok(true);
                    }
                    for (_, ty) in pf.get_params() {
                        if let Some(ty) = ty
                            && self.call_contains_ids(ty, &actual_ids)?
                        {
                            return Ok(true);
                        }
                    }
                }
                let ret = self.call_return_defaults(
                    &argument.function_return().expect("function return"),
                    af,
                )?;
                self.unify_call_bindings(
                    &parameter.function_return().expect("function return"),
                    &ret,
                    bindings,
                )
            }
            Union(_) => {
                let mut pv = parameter.union_components().expect("union");
                if matches!(a, Union(_)) {
                    let mut av = argument.union_components().expect("union");
                    if pv.iter().any(|v| matches!(v.structural_type(), Nil))
                        && av.iter().any(|v| matches!(v.structural_type(), Nil))
                    {
                        pv.retain(|v| !matches!(v.structural_type(), Nil));
                        av.retain(|v| !matches!(v.structural_type(), Nil));
                    }
                    if pv.len() == av.len() {
                        for (p, a) in pv.iter().zip(&av) {
                            if !self.unify_call_bindings(p, a, bindings)? {
                                return Ok(false);
                            }
                        }
                        return Ok(true);
                    }
                    if pv.len() == 1 {
                        for a in &av {
                            if self.unify_call_bindings(&pv[0], a, bindings)? {
                                return Ok(true);
                            }
                        }
                    }
                    return Ok(false);
                }
                let any = matches!(a, Function | DocFunction(_) | Signature(_));
                if !any
                    && let Some(string) = pv
                        .iter()
                        .find(|v| matches!(v.structural_type(), StrTplRef(_)))
                {
                    if self.unify_call_bindings(string, argument, bindings)? {
                        return Ok(true);
                    }
                    for p in &pv {
                        if self.unify_call_bindings(p, argument, bindings)? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
                if any || pv.iter().any(|v| matches!(v.structural_type(), TplRef(_))) {
                    for p in &pv {
                        if self.unify_call_bindings(p, argument, bindings)? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
                if pv.iter().any(|v| matches!(v.structural_type(), Nil)) {
                    for p in &pv {
                        if !matches!(p.structural_type(), Nil)
                            && !self.unify_call_bindings(p, argument, bindings)?
                        {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                }
                Ok(false)
            }
            SelfInfer => Ok(matches!(
                a,
                SelfInfer
                    | Ref(_)
                    | Def(_)
                    | Generic(_)
                    | Table
                    | TableConst(_)
                    | TableGeneric(_)
                    | Object(_)
                    | Intersection(_)
                    | Unknown
                    | Any
            )),
            StrTplRef(tpl) => {
                let full = match a {
                    StringConst(s) | DocStringConst(s) => {
                        format!("{}{}{}", tpl.get_prefix(), s.as_str(), tpl.get_suffix())
                    }
                    _ => return Ok(false),
                };
                self.checkpoint()?;
                let id = if let Some(def) = self.model.resolve_type_def(&full) {
                    if matches!(def.visibility, TypeVisibility::Public) {
                        LuaTypeDeclId::global(&def.full_name)
                    } else {
                        LuaTypeDeclId::file(def.file_id, &def.full_name)
                    }
                } else {
                    LuaTypeDeclId::global(&full)
                };
                bindings.insert(tpl.get_tpl_id(), OwnedValue::plain(Ref(id)));
                Ok(true)
            }
            _ => {
                if matches!(a, Unknown | Any)
                    || parameter == argument
                    || primitive_accepts_const(p, a)
                    || (matches!(p, Function) && matches!(a, DocFunction(_) | Signature(_)))
                {
                    Ok(true)
                } else {
                    self.unify_structural(parameter, argument, bindings)
                }
            }
        }
    }

    fn call_contains_ids(
        &mut self,
        root: &LuaType,
        ids: &HashSet<GenericTplId>,
    ) -> QueryResult<bool, E> {
        let mut pending = vec![root];
        while let Some(ty) = pending.pop() {
            self.checkpoint()?;
            if matches!(ty,LuaType::TplRef(tpl) if ids.contains(&tpl.get_tpl_id())) {
                return Ok(true);
            }
            ty.push_direct_children(&mut pending);
        }
        Ok(false)
    }

    fn call_return_defaults(
        &mut self,
        value: &OwnedValue,
        function: &LuaFunctionType,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::TplRef(tpl) => {
                for generic in function.get_generic_params() {
                    self.checkpoint()?;
                    if generic.get_tpl_id() == tpl.get_tpl_id() {
                        if let Some(default) = generic
                            .get_default_type()
                            .or_else(|| generic.get_constraint())
                        {
                            return Ok(OwnedValue::plain(default.clone()));
                        }
                    }
                }
                Ok(value.clone())
            }
            LuaType::Union(_) => {
                let mut parts = Vec::new();
                for part in value.union_components().expect("union") {
                    parts.push(self.call_return_defaults(&part, function)?);
                }
                OwnedValue::union_checked(parts, || self.checkpoint())
            }
            LuaType::Variadic(variadic) => match variadic.as_ref() {
                VariadicType::Base(base) => {
                    let child =
                        self.call_return_defaults(&self.owned_child(value, 0, base), function)?;
                    Ok(OwnedValue::with_structural_children(
                        LuaType::Variadic(Arc::new(VariadicType::Base(
                            child.structural_type().clone(),
                        ))),
                        vec![child],
                    ))
                }
                VariadicType::Multi(parts) => {
                    let mut result = Vec::new();
                    for (i, part) in parts.iter().enumerate() {
                        result.push(
                            self.call_return_defaults(&self.owned_child(value, i, part), function)?,
                        );
                    }
                    Ok(OwnedValue::pack(result))
                }
            },
            _ => Ok(value.clone()),
        }
    }

    fn call_super_generic(
        &mut self,
        arg: &OwnedValue,
        target: &LuaTypeDeclId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let (definition, parameters) = match arg.structural_type() {
            LuaType::Generic(g) => {
                let Some(def) = self.model.type_def_of(&g.get_base_type_id()) else {
                    return Ok(None);
                };
                let params = g
                    .get_params()
                    .iter()
                    .enumerate()
                    .map(|(i, v)| self.owned_child(arg, i, v))
                    .collect::<Vec<_>>();
                (def, Some(params))
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                (def, None)
            }
            _ => return Ok(None),
        };
        if let Some(tree) = self.model.syntax_tree_of(definition.file_id) {
            for node in tree.get_red_root().descendants() {
                self.checkpoint()?;
                let Some(LuaDocTag::Class(class)) = LuaDocTag::cast(node) else {
                    continue;
                };
                let Some(name) = class.get_name_token() else {
                    continue;
                };
                if name.get_name_text() != definition.name.as_str() {
                    continue;
                }
                let Some(supers) = class.get_supers() else {
                    continue;
                };
                for doc in supers.get_types() {
                    self.checkpoint()?;
                    let (name, args) = match doc {
                        LuaDocType::Name(name) => {
                            let Some(name) = name.get_name_text() else {
                                return Ok(None);
                            };
                            (name, Vec::new())
                        }
                        LuaDocType::Generic(g) => {
                            let Some(name) =
                                g.get_name_type().and_then(|name| name.get_name_text())
                            else {
                                return Ok(None);
                            };
                            (
                                name,
                                g.get_generic_types()
                                    .map(|list| list.get_types().collect::<Vec<_>>())
                                    .unwrap_or_default(),
                            )
                        }
                        _ => continue,
                    };
                    let id = LuaTypeDeclId::global(&name);
                    let Some(parent_def) = self.model.resolve_type_def(name.as_str()) else {
                        return Ok(None);
                    };
                    let parent = if args.is_empty() || parent_def.generic_params.is_empty() {
                        OwnedValue::plain(LuaType::Ref(id.clone()))
                    } else {
                        let mut bindings = Bindings::new();
                        let mut names = HashMap::new();
                        if let Some(parameters) = &parameters {
                            for (i, value) in parameters.iter().enumerate() {
                                self.checkpoint()?;
                                bindings.insert(GenericTplId::Type(i as u32), value.clone());
                                if let Some(generic) = definition.generic_params.get(i) {
                                    names.insert(generic.name.to_string(), value.clone());
                                }
                            }
                        }
                        let mut values = Vec::new();
                        for doc in args {
                            self.checkpoint()?;
                            let mut value = OwnedValue::plain(self.model.doc_type_lua_in(
                                definition.file_id,
                                doc.get_syntax_id(),
                                &definition.generic_params,
                            ));
                            if parameters.is_some() {
                                value = self.substitute_owned(value, &bindings)?;
                                value = self.substitute_owned_names(value, &names)?;
                            }
                            values.push(value);
                        }
                        let ty = LuaType::Generic(Arc::new(LuaGenericType::new(
                            id.clone(),
                            values.iter().map(|v| v.structural_type().clone()).collect(),
                        )));
                        values.reverse();
                        OwnedValue::with_structural_children(ty, values)
                    };
                    if id.get_simple_name() == target.get_simple_name() {
                        return Ok(Some(parent));
                    }
                    if let Some(result) = self.call_super_generic(&parent, target)? {
                        return Ok(Some(result));
                    }
                }
            }
        }
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        for name in &definition.super_names {
            self.checkpoint()?;
            let id = LuaTypeDeclId::global(name);
            if &id != target && id.get_simple_name() != target.get_simple_name() {
                continue;
            }
            let Some(parent) = self.model.resolve_type_def(name) else {
                return Ok(None);
            };
            if parent.generic_params.is_empty() {
                return Ok(Some(OwnedValue::plain(LuaType::Ref(id))));
            }
            if parent.generic_params.len() == parameters.len() {
                let ty = LuaType::Generic(Arc::new(LuaGenericType::new(
                    id,
                    parameters
                        .iter()
                        .map(|v| v.structural_type().clone())
                        .collect(),
                )));
                let mut children = parameters.clone();
                children.reverse();
                return Ok(Some(OwnedValue::with_structural_children(ty, children)));
            }
        }
        Ok(None)
    }
}

pub(super) fn widen_call_const(value: &OwnedValue) -> OwnedValue {
    match value.structural_type() {
        LuaType::StringConst(_) | LuaType::DocStringConst(_) => OwnedValue::plain(LuaType::String),
        LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
            OwnedValue::plain(LuaType::Integer)
        }
        _ => value.clone(),
    }
}
pub(super) fn widen_variadic_const(value: &OwnedValue) -> OwnedValue {
    if matches!(
        value.structural_type(),
        LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_)
    ) {
        OwnedValue::plain(LuaType::Boolean)
    } else {
        widen_call_const(value)
    }
}
fn primitive_accepts_const(p: &LuaType, a: &LuaType) -> bool {
    use LuaType::*;
    matches!(
        (p, a),
        (Integer, IntegerConst(_) | DocIntegerConst(_))
            | (Number, IntegerConst(_) | DocIntegerConst(_) | FloatConst(_))
            | (String, StringConst(_) | DocStringConst(_))
            | (Boolean, BooleanConst(_) | DocBooleanConst(_))
    )
}
