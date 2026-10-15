//! Argument/vararg branches retain the original short-circuit producer order.
use super::*;

impl<E> OwnedQuery<'_, '_, E> {
    fn pt_cached_arg_type(
        &mut self,
        args: &[LuaExpr],
        types: Option<&[OwnedValue]>,
        index: usize,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if let Some(ty) = types.and_then(|types| types.get(index)) {
            return Ok(ty.clone());
        }
        match args.get(index) {
            Some(arg) => self.type_of_expr(arg.get_syntax_id()),
            None => Ok(OwnedValue::plain(LuaType::Unknown)),
        }
    }

    pub(super) fn pt_check_arg_pairs(
        &mut self,
        params: &[(String, Option<OwnedValue>)],
        args: &[LuaExpr],
        is_variadic: bool,
        bindings: &mut Bindings,
        arg_types: Option<&[OwnedValue]>,
    ) -> QueryResult<Vec<Mismatch>, E> {
        let mut out = Vec::new();
        for (index, (name, param)) in params.iter().enumerate() {
            self.checkpoint()?;
            if is_variadic && index + 1 == params.len() {
                let rest_start = index.min(args.len());
                let rest = &args[rest_start..];
                if let Some(param) = param {
                    let param = self.substitute_owned(param.clone(), bindings)?;
                    let param = self.expand_owned_aliases(param)?;
                    let param = self.eval_owned_conditionals(param)?;
                    if let LuaType::Variadic(variadic) = param.structural_type() {
                        if let VariadicType::Base(base) = variadic.as_ref()
                            && let LuaType::TplRef(tpl) = base
                            && !bindings.contains_key(&tpl.get_tpl_id())
                        {
                            let mut rest_types = Vec::new();
                            for (offset, _) in rest.iter().enumerate() {
                                let ty =
                                    self.pt_cached_arg_type(args, arg_types, rest_start + offset)?;
                                rest_types.push(self.pt_normalize_arg(&ty)?);
                            }
                            let tuple = OwnedValue::tuple_checked(
                                rest_types,
                                LuaTupleStatus::InferResolve,
                                || self.checkpoint(),
                            )?;
                            let tpl = self.owned_child(&param, 0, base);
                            let _ = self.unify_call_bindings(&tpl, &tuple, bindings)?;
                            break;
                        }
                        for (slot_index, arg) in rest.iter().enumerate() {
                            self.checkpoint()?;
                            // get_type supplies both repeated base slots and the terminal
                            // variadic tail of a multi-return slot list. Keep that shape.
                            let Some(_) = variadic.get_type(slot_index) else {
                                break;
                            };
                            let mut slot = self.pt_variadic_slot(&param, slot_index);
                            let arg_ty =
                                self.pt_cached_arg_type(args, arg_types, rest_start + slot_index)?;
                            let arg_ty = self.pt_normalize_arg(&arg_ty)?;
                            slot = self.substitute_owned(slot, bindings)?;
                            if slot.structural_type().contains_tpl_node()
                                && !self.unify_call_bindings(&slot, &arg_ty, bindings)?
                            {
                                out.push(Mismatch {
                                    range: arg.get_range(),
                                    message: self.pt_mismatch_message(&arg_ty, &slot, name),
                                });
                                continue;
                            }
                            slot = self.substitute_owned(slot, bindings)?;
                            slot = self.expand_owned_aliases(slot)?;
                            if self.structure_allowed(&arg_ty)?
                                && self.structure_allowed(&slot)?
                                && (arg_ty == slot
                                    || self.type_check(&arg_ty, &slot)?
                                    || self.type_check_assign(&arg_ty, &slot)?)
                            {
                                continue;
                            }
                            out.push(Mismatch {
                                range: arg.get_range(),
                                message: self.pt_mismatch_message(&arg_ty, &slot, name),
                            });
                        }
                    } else {
                        for (offset, arg) in rest.iter().enumerate() {
                            self.checkpoint()?;
                            let arg_ty =
                                self.pt_cached_arg_type(args, arg_types, rest_start + offset)?;
                            let arg_ty = self.pt_normalize_arg(&arg_ty)?;
                            let original = param.clone();
                            let unified = self.unify_call_bindings(&original, &arg_ty, bindings)?;
                            if original.structural_type().contains_tpl_node() && !unified {
                                out.push(Mismatch {
                                    range: arg.get_range(),
                                    message: self.pt_mismatch_message(&arg_ty, &original, name),
                                });
                                continue;
                            }
                            let expected = self.substitute_owned(original, bindings)?;
                            if self.structure_allowed(&arg_ty)?
                                && self.structure_allowed(&expected)?
                                && (arg_ty == expected
                                    || self.pt_generic_base_match(&arg_ty, &expected)?
                                    || self.type_check_assign(&arg_ty, &expected)?)
                            {
                                continue;
                            }
                            out.push(Mismatch {
                                range: arg.get_range(),
                                message: self.pt_mismatch_message(&arg_ty, &expected, name),
                            });
                        }
                    }
                }
                break;
            }
            let Some(param) = param else { continue };
            let Some(arg) = args.get(index) else { continue };
            let arg_ty = self.pt_cached_arg_type(args, arg_types, index)?;
            let arg_ty = self.pt_normalize_arg(&arg_ty)?;
            let call_arg = self.pt_call_argument_type(arg, &arg_ty)?;
            let call_arg = self.pt_normalize_arg(&call_arg)?;
            let param = self.substitute_owned(param.clone(), bindings)?;
            let param = self.expand_owned_aliases(param)?;
            let param = self.eval_owned_conditionals(param)?;
            if !self.binding_manages(arg.get_syntax_id())?
                && let LuaExpr::IndexExpr(index) = arg
                && self.pt_member_missing_in_union(index)?
            {
                out.push(Mismatch {
                    range: arg.get_range(),
                    message: self.pt_mismatch_message(
                        &OwnedValue::plain(LuaType::Nil),
                        &param,
                        name,
                    ),
                });
                continue;
            }
            if !self.binding_manages(arg.get_syntax_id())?
                && let LuaExpr::TableExpr(table) = arg
            {
                if has_table_literal_shape(param.structural_type()) {
                    if let Some(mismatch) = self.pt_table_literal_mismatch(&param, table)? {
                        out.push(mismatch);
                    }
                    continue;
                }
                if let Some(mismatch) = self.pt_generic_table_required_mismatch(&param, table)? {
                    out.push(mismatch);
                    continue;
                }
            }
            if !self.structure_allowed(&call_arg)? || !self.structure_allowed(&param)? {
                out.push(Mismatch {
                    range: arg.get_range(),
                    message: self.pt_mismatch_message(&call_arg, &param, name),
                });
                continue;
            }
            if skip_arg(call_arg.structural_type(), param.structural_type()) {
                continue;
            }
            let has_generic_union_tpl = matches!(param.structural_type(), LuaType::Union(union)
                if union.into_vec().iter().any(|ty| matches!(ty, LuaType::TplRef(_) | LuaType::StrTplRef(_))));
            if !has_generic_union_tpl
                && !matches!(
                    call_arg.structural_type(),
                    LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::Function
                )
                && let Some(components) = param.union_components()
            {
                let mut accepted = false;
                for component in components
                    .iter()
                    .filter(|ty| !matches!(ty.structural_type(), LuaType::Nil))
                {
                    if self.pt_union_component_accepts(&call_arg, component)? {
                        accepted = true;
                        break;
                    }
                }
                if accepted {
                    continue;
                }
            }
            let param = effective_param(&param, &call_arg);
            let _ = self.unify_call_bindings(&param, &call_arg, bindings)?;
            let param = self.substitute_owned(param, bindings)?;
            if let Some(components) = param.union_components() {
                let mut accepted = false;
                for component in &components {
                    if self.pt_union_component_accepts(&call_arg, component)? {
                        accepted = true;
                        break;
                    }
                }
                if accepted {
                    continue;
                }
            }
            if self.pt_object_array_compatible(&call_arg, &param, bindings)? {
                continue;
            }
            if self.pt_function_arg_compatible(&call_arg, &param, bindings)? {
                continue;
            }
            if call_arg == param {
                continue;
            }
            let exact_const = match (call_arg.structural_type(), param.structural_type()) {
                (LuaType::StringConst(a), LuaType::StringConst(b)) => a == b,
                (
                    LuaType::IntegerConst(a) | LuaType::DocIntegerConst(a),
                    LuaType::IntegerConst(b) | LuaType::DocIntegerConst(b),
                ) => a == b,
                _ => true,
            };
            let leaf = exact_const
                && !matches!(call_arg.structural_type(), LuaType::Function)
                && self.pt_generic_base_match(&call_arg, &param)?;
            if leaf
                || self.pt_literal_compatible(&call_arg, &param)?
                || self.pt_named_kind_compatible(&call_arg, &param)?
            {
                continue;
            }
            out.push(Mismatch {
                range: arg.get_range(),
                message: self.pt_mismatch_message(&arg_ty, &param, name),
            });
        }
        Ok(out)
    }

    fn pt_variadic_slot(&self, value: &OwnedValue, index: usize) -> OwnedValue {
        let LuaType::Variadic(variadic) = value.structural_type() else {
            unreachable!("variadic")
        };
        // Called only after get_type(index) succeeds; mirror its complete tuple
        // base and recursively nested terminal-tail behavior with owned children.
        match variadic.as_ref() {
            VariadicType::Base(base) => {
                let child = self.owned_child(value, 0, base);
                if let LuaType::Tuple(tuple) = base {
                    self.owned_child(&child, index, &tuple.get_types()[index])
                } else {
                    child
                }
            }
            VariadicType::Multi(types) => {
                if index + 1 < types.len() {
                    return self.owned_child(value, index, &types[index]);
                }
                let last = types.len() - 1;
                let child = self.owned_child(value, last, &types[last]);
                if matches!(types[last], LuaType::Variadic(_)) {
                    self.pt_variadic_slot(&child, index - last)
                } else {
                    child
                }
            }
        }
    }
}
