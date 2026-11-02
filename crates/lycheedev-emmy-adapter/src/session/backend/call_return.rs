//! Call return projection and higher-order slot algebra adapted from fixed VM.
use super::{ExpressionValue, OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    LuaAliasCallKind, LuaFunctionType, LuaGenericType, LuaMemberKey, LuaType, SemanticId,
    VariadicType,
};
use std::{collections::HashSet, sync::Arc};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn substituted_owned_return(
        &mut self,
        callee: &ExpressionValue<OwnedValue>,
        function: &OwnedValue,
        bindings: &Bindings,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let owner_info = callee.owner.as_ref().and_then(|owner| match owner {
            SemanticId::Decl(key) => self
                .model
                .file_facts_of(key.file_id)?
                .decl_by_id(owner)?
                .value_expr_syntax
                .map(|syntax| (key.file_id, syntax)),
            SemanticId::Member(key) => self
                .model
                .file_facts_of(key.file_id)?
                .member_by_id(owner)?
                .value_syntax
                .map(|syntax| (key.file_id, syntax)),
            _ => None,
        });
        if let Some((file, syntax)) = owner_info
            && let Some(signature) = self
                .model
                .file_facts_of(file)
                .and_then(|facts| facts.signature_by_closure(syntax))
            && let Some(docs) = signature.docs.as_ref()
            && !docs.return_overload_rows.is_empty()
        {
            return self.callable_return_overloads(
                file,
                docs,
                &docs.generic_params,
                Some(bindings),
            );
        }
        let LuaType::DocFunction(metadata) = function.structural_type() else {
            unreachable!();
        };
        let ret = self.bind_owned_signature_generics(
            function.function_return().expect("function return"),
            metadata.get_generic_params(),
        )?;
        let ret = self.substitute_owned(ret, bindings)?;
        let ret = self.expand_owned_aliases(ret)?;
        let ret = self.eval_owned_conditionals(ret)?;
        if variadic_tuple_return(metadata)
            && let LuaType::Tuple(tuple) = ret.structural_type()
            && !tuple.get_types().is_empty()
        {
            let slots = tuple
                .get_types()
                .iter()
                .enumerate()
                .map(|(i, value)| self.owned_child(&ret, i, value))
                .collect();
            return Ok(OwnedValue::pack(slots));
        }
        Ok(ret)
    }

    pub(super) fn bind_owned_callback_return(
        &mut self,
        original: &OwnedValue,
        substituted: &OwnedValue,
        actual: &OwnedValue,
        bindings: &mut Bindings,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        match original.structural_type() {
            LuaType::TplRef(tpl) => {
                bindings.insert(tpl.get_tpl_id(), actual.clone());
            }
            LuaType::Variadic(v) => match v.as_ref() {
                VariadicType::Base(LuaType::TplRef(tpl)) => {
                    bindings.insert(tpl.get_tpl_id(), actual.clone());
                }
                VariadicType::Multi(types) => {
                    let tail = types.last().and_then(|t| match t {
                        LuaType::Variadic(v) => match v.as_ref() {
                            VariadicType::Base(base) => Some(base),
                            _ => None,
                        },
                        _ => None,
                    });
                    let Some(base) = tail else {
                        let _ = self.unify_call_bindings(substituted, actual, bindings)?;
                        return Ok(());
                    };
                    let actuals = self.call_return_slots(actual);
                    let fixed = types.len() - 1;
                    let mut ok = true;
                    for (i, (param, actual)) in types[..fixed].iter().zip(&actuals).enumerate() {
                        if !self.unify_call_bindings(
                            &self.owned_child(original, i, param),
                            actual,
                            bindings,
                        )? {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        let rest = if actuals.len() > fixed {
                            if actuals.len() == fixed + 1 {
                                actuals[fixed].clone()
                            } else {
                                OwnedValue::pack(actuals[fixed..].to_vec())
                            }
                        } else {
                            base_variadic(OwnedValue::plain(LuaType::Nil))
                        };
                        let tail = self.owned_child(original, fixed, &types[fixed]);
                        let base = self.owned_child(&tail, 0, base);
                        let _ = self.unify_call_bindings(&base, &rest, bindings)?;
                    }
                }
                _ => {
                    let _ = self.unify_call_bindings(substituted, actual, bindings)?;
                }
            },
            _ => {
                let _ = self.unify_call_bindings(substituted, actual, bindings)?;
            }
        }
        Ok(())
    }
    pub(super) fn call_return_slots(&self, value: &OwnedValue) -> Vec<OwnedValue> {
        match value.structural_type() {
            LuaType::Variadic(v) => match v.as_ref() {
                VariadicType::Base(base) => vec![self.owned_child(value, 0, base)],
                VariadicType::Multi(values) => values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| self.owned_child(value, i, v))
                    .collect(),
            },
            _ => vec![value.clone()],
        }
    }

    pub(super) fn expand_owned_unpack(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if let LuaType::Variadic(v) = value.structural_type()
            && let VariadicType::Base(base) = v.as_ref()
        {
            let base = self.owned_child(&value, 0, base);
            let slots = match base.structural_type() {
                LuaType::Tuple(tuple) => Some(
                    tuple
                        .get_types()
                        .iter()
                        .enumerate()
                        .map(|(i, v)| self.owned_child(&base, i, v))
                        .collect::<Vec<_>>(),
                ),
                LuaType::Object(object) => {
                    let mut slots = Vec::new();
                    for (key, v) in object.get_fields() {
                        self.checkpoint()?;
                        if let LuaMemberKey::Integer(i) = key {
                            slots.push((*i, self.owned_field(&base, key, v)));
                        }
                    }
                    slots.sort_by_key(|(i, _)| *i);
                    Some(slots.into_iter().map(|(_, v)| v).collect())
                }
                _ => None,
            };
            if let Some(slots) = slots
                && !slots.is_empty()
            {
                return Ok(OwnedValue::pack(slots));
            }
        }
        let LuaType::Call(call) = value.structural_type() else {
            return Ok(value);
        };
        if call.get_call_kind() != LuaAliasCallKind::Unpack {
            return Ok(value);
        }
        let Some(operand) = call.get_operands().first() else {
            return Ok(value);
        };
        let operand = self.owned_child(&value, 0, operand);
        match operand.structural_type() {
            LuaType::TableConst(table) => {
                let owner = SemanticId::member(table.file_id, table.value);
                let mut indexed = Vec::new();
                for reference in self.model.members_of_owner(&owner).iter() {
                    self.checkpoint()?;
                    if let Some(member) = self
                        .model
                        .file_facts_of(reference.file_id)
                        .and_then(|facts| facts.member_by_id(&reference.id))
                        && let LuaMemberKey::Integer(index) = member.key
                    {
                        let ty = if let Some(syntax) = member.value_syntax {
                            self.type_of_expr_inherit(syntax)?
                        } else {
                            self.type_of_member(&reference.id)?
                                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown))
                        };
                        indexed.push((index, ty));
                    }
                }
                indexed.sort_by_key(|(i, _)| *i);
                self.checkpoint()?;
                Ok(OwnedValue::pack(
                    indexed.into_iter().map(|(_, v)| v).collect(),
                ))
            }
            LuaType::Array(array) => {
                let base = self.owned_child(&operand, 0, array.get_base());
                let base =
                    OwnedValue::union_checked(vec![base, OwnedValue::plain(LuaType::Nil)], || {
                        self.checkpoint()
                    })?;
                Ok(base_variadic(base))
            }
            LuaType::Tuple(tuple) => Ok(OwnedValue::pack(
                tuple
                    .get_types()
                    .iter()
                    .enumerate()
                    .map(|(i, v)| self.owned_child(&operand, i, v))
                    .collect(),
            )),
            LuaType::Object(object) => {
                let mut slots = Vec::new();
                for (key, v) in object.get_fields() {
                    self.checkpoint()?;
                    if let LuaMemberKey::Name(name) = key {
                        let s = name.as_str();
                        if s.starts_with('[')
                            && s.ends_with(']')
                            && let Ok(i) = s[1..s.len() - 1].parse::<i64>()
                        {
                            slots.push((i, self.owned_field(&operand, key, v)));
                        }
                    }
                }
                slots.sort_by_key(|(i, _)| *i);
                self.checkpoint()?;
                Ok(OwnedValue::pack(
                    slots.into_iter().map(|(_, v)| v).collect(),
                ))
            }
            _ => Ok(value),
        }
    }

    pub(super) fn instantiate_call_self(
        &mut self,
        owner: &OwnedValue,
        function: &OwnedValue,
        bindings: &Bindings,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let (LuaType::Ref(id) | LuaType::Def(id)) = owner.structural_type() else {
            return Ok(owner.clone());
        };
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(owner.clone());
        };
        if def.generic_params.is_empty() {
            return Ok(owner.clone());
        }
        let LuaType::DocFunction(function) = function.structural_type() else {
            unreachable!();
        };
        let mut values = Vec::new();
        for param in &def.generic_params {
            self.checkpoint()?;
            let bound = bindings.iter().find_map(|(id, value)| {
                function
                    .get_generic_params()
                    .iter()
                    .find(|p| p.get_tpl_id() == *id && p.get_name() == param.name.as_str())
                    .map(|_| value.clone())
            });
            values.push(if let Some(value) = bound {
                value
            } else {
                OwnedValue::plain(
                    param
                        .default
                        .map(|syntax| {
                            self.model
                                .doc_type_lua_in(def.file_id, syntax, &def.generic_params)
                        })
                        .or_else(|| {
                            param.constraint.map(|syntax| {
                                self.model
                                    .doc_type_lua_in(def.file_id, syntax, &def.generic_params)
                            })
                        })
                        .unwrap_or(LuaType::Unknown),
                )
            });
        }
        let ty = LuaType::Generic(Arc::new(LuaGenericType::new(
            id.clone(),
            values.iter().map(|v| v.structural_type().clone()).collect(),
        )));
        values.reverse();
        Ok(OwnedValue::with_structural_children(ty, values))
    }

    pub(super) fn expand_returned_call_function(
        &mut self,
        value: OwnedValue,
        args: &[ExpressionValue<OwnedValue>],
        callee: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let LuaType::DocFunction(function) = value.structural_type() else {
            return Ok(value);
        };
        if function.get_params().len() != 1 {
            return Ok(value);
        }
        let parameter = value
            .function_parameters()
            .expect("function parameters")
            .remove(0);
        let Some(parameter) = parameter else {
            return Ok(value);
        };
        let LuaType::Variadic(variadic) = parameter.structural_type() else {
            return Ok(value);
        };
        let mut arg_function = None;
        for arg in args {
            self.checkpoint()?;
            if matches!(arg.value.structural_type(), LuaType::DocFunction(_)) {
                arg_function = Some(arg.value.clone());
                break;
            }
            if let Some(closure) = arg.closure_syntax
                && let Some(function) = self.signature_local(closure)?
            {
                arg_function = Some(function);
                break;
            }
        }
        let Some(arg_function) = arg_function else {
            return Ok(value);
        };
        let LuaType::DocFunction(arg_metadata) = arg_function.structural_type() else {
            unreachable!();
        };
        let LuaType::DocFunction(callee) = callee.structural_type() else {
            unreachable!();
        };
        let prefix = returned_variadic_prefix(callee);
        let (start, end) = match variadic.as_ref() {
            VariadicType::Base(base) => match base {
                LuaType::Tuple(tuple) => {
                    let len = tuple.get_types().len();
                    if arg_metadata.get_params().len() < len {
                        return Ok(value);
                    }
                    if prefix {
                        (0, len)
                    } else {
                        (
                            arg_metadata.get_params().len() - len,
                            arg_metadata.get_params().len(),
                        )
                    }
                }
                LuaType::TplRef(_) | LuaType::Unknown | LuaType::Any | LuaType::Never => {
                    return Ok(value);
                }
                _ => (0, arg_metadata.get_params().len()),
            },
            VariadicType::Multi(_) => return Ok(value),
        };
        let mut ret = self.owned_sanitize_generics(
            value.function_return().expect("function return"),
            &HashSet::new(),
            false,
        )?;
        if matches!(ret.structural_type(),LuaType::Variadic(v) if matches!(v.as_ref(),VariadicType::Base(LuaType::Unknown)))
        {
            ret = OwnedValue::plain(LuaType::Unknown);
        }
        let params = arg_function
            .function_parameters()
            .expect("function parameters")[start..end]
            .to_vec();
        let metadata = LuaFunctionType::new(
            function.get_async_state(),
            function.is_colon_define(),
            false,
            arg_metadata.get_params()[start..end].to_vec(),
            ret.structural_type().clone(),
            None,
        );
        Ok(OwnedValue::function(&metadata, params, ret))
    }
}
fn base_variadic(value: OwnedValue) -> OwnedValue {
    OwnedValue::with_structural_children(
        LuaType::Variadic(Arc::new(VariadicType::Base(
            value.structural_type().clone(),
        ))),
        vec![value],
    )
}
fn variadic_tuple_return(function: &LuaFunctionType) -> bool {
    let LuaType::TplRef(ret) = function.get_ret() else {
        return false;
    };
    function.get_params().iter().any(|(name,ty)|(name=="..."||matches!(ty,Some(LuaType::Variadic(_))))&&matches!(ty,Some(LuaType::Variadic(v)) if matches!(v.as_ref(),VariadicType::Base(LuaType::TplRef(p)) if p.get_tpl_id()==ret.get_tpl_id())))
}
fn returned_variadic_prefix(function: &LuaFunctionType) -> bool {
    let LuaType::DocFunction(ret) = function.get_ret() else {
        return false;
    };
    let Some((_, Some(LuaType::Variadic(v)))) = ret.get_params().first() else {
        return false;
    };
    let VariadicType::Base(LuaType::TplRef(ret)) = v.as_ref() else {
        return false;
    };
    for (_, parameter) in function.get_params() {
        if let Some(LuaType::DocFunction(source)) = parameter {
            for (i, (_, ty)) in source.get_params().iter().enumerate() {
                if let Some(LuaType::Variadic(v)) = ty
                    && let VariadicType::Base(LuaType::TplRef(tpl)) = v.as_ref()
                    && tpl.get_tpl_id() == ret.get_tpl_id()
                {
                    return i + 1 < source.get_params().len();
                }
            }
        }
    }
    false
}
