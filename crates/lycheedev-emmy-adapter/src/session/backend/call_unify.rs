//! The call-specific structural binder. This deliberately differs from general
//! type compatibility: e.g. absent Object fields are skipped and unions are not
//! distributively matched. Failed binding keeps the original partial mutations.
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{GenericTplId, LuaType, VariadicType};
use std::collections::HashMap;
pub(super) type Bindings = HashMap<GenericTplId, OwnedValue>;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn unify_structural(
        &mut self,
        parameter: &OwnedValue,
        argument: &OwnedValue,
        bindings: &mut Bindings,
    ) -> QueryResult<bool, E> {
        let mut pending = vec![(parameter.clone(), argument.clone())];
        while let Some((parameter, argument)) = pending.pop() {
            self.checkpoint()?;
            let p = parameter.structural_type();
            let a = argument.structural_type();
            let mut pairs = Vec::new();
            match p {
                LuaType::TplRef(tpl) => {
                    if matches!(a, LuaType::Any) {
                        continue;
                    }
                    let id = tpl.get_tpl_id();
                    if let Some(existing) = bindings.get(&id) {
                        if existing != &argument {
                            return Ok(false);
                        }
                    } else {
                        bindings.insert(id, argument);
                    }
                    continue;
                }
                LuaType::Array(p) => {
                    let LuaType::Array(a) = a else {
                        return Ok(false);
                    };
                    pairs.push((
                        self.owned_child(&parameter, 0, p.get_base()),
                        self.owned_child(&argument, 0, a.get_base()),
                    ));
                }
                LuaType::TableGeneric(p) => {
                    let LuaType::TableGeneric(a) = a else {
                        return Ok(false);
                    };
                    if p.len() != a.len() {
                        return Ok(false);
                    }
                    for (index, (p, a)) in p.iter().zip(a.iter()).enumerate() {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Tuple(p) => {
                    let LuaType::Tuple(a) = a else {
                        return Ok(false);
                    };
                    if p.get_types().len() != a.get_types().len() {
                        return Ok(false);
                    }
                    for (index, (p, a)) in p.get_types().iter().zip(a.get_types()).enumerate() {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Object(p) => {
                    let LuaType::Object(a) = a else {
                        return Ok(false);
                    };
                    // Index-length mismatch is checked after field binding in
                    // the original. Resolve fields now to preserve mutations.
                    for (key, expected) in p.get_fields() {
                        self.checkpoint()?;
                        if let Some(actual) = a.get_fields().get(key)
                            && !self.unify_structural(
                                &self.owned_field(&parameter, key, expected),
                                &self.owned_field(&argument, key, actual),
                                bindings,
                            )?
                        {
                            return Ok(false);
                        }
                    }
                    if p.get_index_access().len() != a.get_index_access().len() {
                        return Ok(false);
                    }
                    for (index, ((pk, pv), (ak, av))) in p
                        .get_index_access()
                        .iter()
                        .zip(a.get_index_access())
                        .enumerate()
                    {
                        pairs.push((
                            self.owned_index(&parameter, index, true, pk),
                            self.owned_index(&argument, index, true, ak),
                        ));
                        pairs.push((
                            self.owned_index(&parameter, index, false, pv),
                            self.owned_index(&argument, index, false, av),
                        ));
                    }
                }
                LuaType::Generic(p) => {
                    let LuaType::Generic(a) = a else {
                        return Ok(false);
                    };
                    if p.get_base_type_id() != a.get_base_type_id()
                        || p.get_params().len() != a.get_params().len()
                    {
                        return Ok(false);
                    }
                    for (index, (p, a)) in p.get_params().iter().zip(a.get_params()).enumerate() {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Intersection(p) => {
                    let LuaType::Intersection(a) = a else {
                        return Ok(false);
                    };
                    if p.get_types().len() != a.get_types().len() {
                        return Ok(false);
                    }
                    for (index, (p, a)) in p.get_types().iter().zip(a.get_types()).enumerate() {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Call(p) => {
                    let LuaType::Call(a) = a else {
                        return Ok(false);
                    };
                    if p.get_call_kind() != a.get_call_kind()
                        || p.get_operands().len() != a.get_operands().len()
                    {
                        return Ok(false);
                    }
                    for (index, (p, a)) in p.get_operands().iter().zip(a.get_operands()).enumerate()
                    {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Conditional(p) => {
                    let LuaType::Conditional(a) = a else {
                        return Ok(false);
                    };
                    for (index, (p, a)) in [
                        p.get_checked_type(),
                        p.get_extends_type(),
                        p.get_true_type(),
                        p.get_false_type(),
                    ]
                    .into_iter()
                    .zip([
                        a.get_checked_type(),
                        a.get_extends_type(),
                        a.get_true_type(),
                        a.get_false_type(),
                    ])
                    .enumerate()
                    {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Mapped(p) => {
                    let LuaType::Mapped(a) = a else {
                        return Ok(false);
                    };
                    pairs.push((
                        parameter
                            .structural_children()
                            .map(|children| children[0].clone())
                            .unwrap_or_else(|| OwnedValue::plain(p.value.clone())),
                        argument
                            .structural_children()
                            .map(|children| children[0].clone())
                            .unwrap_or_else(|| OwnedValue::plain(a.value.clone())),
                    ));
                }
                LuaType::TypeGuard(p) => {
                    let LuaType::TypeGuard(a) = a else {
                        return Ok(false);
                    };
                    pairs.push((
                        self.owned_child(&parameter, 0, p),
                        self.owned_child(&argument, 0, a),
                    ));
                }
                LuaType::MultiLineUnion(p) => {
                    let LuaType::MultiLineUnion(a) = a else {
                        return Ok(false);
                    };
                    if p.get_unions().len() != a.get_unions().len() {
                        return Ok(false);
                    }
                    for (index, ((p, _), (a, _))) in
                        p.get_unions().iter().zip(a.get_unions()).enumerate()
                    {
                        pairs.push((
                            self.owned_child(&parameter, index, p),
                            self.owned_child(&argument, index, a),
                        ));
                    }
                }
                LuaType::Variadic(p) => match (p.as_ref(), a) {
                    (VariadicType::Base(p), LuaType::Variadic(a)) => match a.as_ref() {
                        VariadicType::Base(a) => pairs.push((
                            self.owned_child(&parameter, 0, p),
                            self.owned_child(&argument, 0, a),
                        )),
                        VariadicType::Multi(_) => return Ok(false),
                    },
                    (VariadicType::Base(_), LuaType::Unknown | LuaType::Any) => {}
                    (VariadicType::Base(p), _) => {
                        pairs.push((self.owned_child(&parameter, 0, p), argument.clone()))
                    }
                    (VariadicType::Multi(p), LuaType::Variadic(a)) => {
                        let VariadicType::Multi(a) = a.as_ref() else {
                            return Ok(false);
                        };
                        if p.len() != a.len() {
                            return Ok(false);
                        }
                        for (index, (p, a)) in p.iter().zip(a).enumerate() {
                            pairs.push((
                                self.owned_child(&parameter, index, p),
                                self.owned_child(&argument, index, a),
                            ));
                        }
                    }
                    (VariadicType::Multi(_), LuaType::Unknown | LuaType::Any) => {}
                    _ => return Ok(false),
                },
                _ => {
                    if !matches!(a, LuaType::Unknown | LuaType::Any) && parameter != argument {
                        return Ok(false);
                    }
                }
            }
            for pair in pairs.into_iter().rev() {
                self.checkpoint()?;
                pending.push(pair);
            }
        }
        Ok(true)
    }
}
