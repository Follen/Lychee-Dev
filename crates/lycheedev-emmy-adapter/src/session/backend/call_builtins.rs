//! Fixed VM builtin dispatch and pcall return producers, preserving owned children.
use super::{ExpressionValue, OwnedQuery, QueryResult, call_bindings::widen_call_const};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DeclKind, LuaMemberKey, LuaType, SemanticId, TypeDefKind, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaSyntaxId};
use std::{collections::HashSet, sync::Arc};

impl<E> OwnedQuery<'_, '_, E> {
    /// The owner check intentionally does not infer the callee again. Alias
    /// resolution uses the current model, including the original foreign-local
    /// name-position behavior.
    fn owned_callee_builtin_name(
        &mut self,
        callee: &ExpressionValue<OwnedValue>,
    ) -> QueryResult<Option<String>, E> {
        match &callee.owner {
            Some(SemanticId::Name(name)) => Ok(Some(name.to_string())),
            Some(owner @ SemanticId::Decl(_)) => {
                let mut visited = HashSet::new();
                let mut current = owner.clone();
                loop {
                    self.checkpoint()?;
                    if !visited.insert(current.clone()) {
                        return Ok(None);
                    }
                    let SemanticId::Decl(key) = &current else {
                        return Ok(None);
                    };
                    let Some(facts) = self.model.file_facts_of(key.file_id) else {
                        return Ok(None);
                    };
                    let Some(decl) = facts.decl_by_id(&current) else {
                        return Ok(None);
                    };
                    match decl.kind {
                        DeclKind::Global => return Ok(Some(decl.name.to_string())),
                        DeclKind::Local { .. } => {
                            let Some(syntax) = decl.value_expr_syntax else {
                                return Ok(None);
                            };
                            let Some(tree) = self.model.syntax_tree_of(key.file_id) else {
                                return Ok(None);
                            };
                            let Some(node) = syntax.to_node_from_root(&tree.get_red_root()) else {
                                return Ok(None);
                            };
                            let Some(LuaExpr::NameExpr(name)) = LuaExpr::cast(node) else {
                                return Ok(None);
                            };
                            let Some(next) = self.model.resolve_name(name.get_position()) else {
                                return Ok(None);
                            };
                            current = next;
                        }
                        _ => return Ok(None),
                    }
                }
            }
            _ => Ok(None),
        }
    }

    fn owned_callee_member_name(
        &mut self,
        callee: &ExpressionValue<OwnedValue>,
    ) -> QueryResult<Option<String>, E> {
        self.checkpoint()?;
        let Some(owner @ SemanticId::Member(key)) = callee.owner.as_ref() else {
            return Ok(None);
        };
        let Some(facts) = self.model.file_facts_of(key.file_id) else {
            return Ok(None);
        };
        let Some(member) = facts.member_by_id(owner) else {
            return Ok(None);
        };
        Ok(matches!(member.owner, SemanticId::Name(_)).then(|| member.key.to_path()))
    }

    pub(super) fn call_builtin(
        &mut self,
        callee: &ExpressionValue<OwnedValue>,
        args: &[ExpressionValue<OwnedValue>],
    ) -> QueryResult<Option<ExpressionValue<OwnedValue>>, E> {
        self.checkpoint()?;
        let name = match self.owned_callee_builtin_name(callee)? {
            Some(name) => Some(name),
            None => self.owned_callee_member_name(callee)?,
        };
        let Some(name) = name else { return Ok(None) };
        let value = match name.as_str() {
            "setmetatable" => {
                let Some(first) = args.first() else {
                    return Ok(Some(plain(LuaType::Unknown)));
                };
                if let Some(meta) = args.get(1)
                    && let Some(info) = self
                        .member_info_uncached(&meta.value, &LuaMemberKey::Name("__index".into()))?
                {
                    let index = match info.typ.structural_type() {
                        LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_) => {
                            Some(info.typ.clone())
                        }
                        LuaType::TableConst(table) => {
                            // The original helper is a pure facts/type identity projection;
                            // its foreign-model hop does not evaluate the initializer.
                            self.model.file_facts_of(table.file_id).and_then(|facts| {
                                let decl = facts.decl_by_value_range(table.value)?;
                                let def = facts.type_def_by_owner_syntax(decl.owner_syntax?)?;
                                Some(OwnedValue::plain(self.model.type_def_ref(def)))
                            })
                        }
                        _ => None,
                    };
                    if let Some(value) = index {
                        return Ok(Some(ExpressionValue {
                            value,
                            owner: first.owner.clone(),
                            closure_syntax: None,
                            receiver: first.receiver.clone(),
                        }));
                    }
                }
                return Ok(Some(ExpressionValue {
                    value: first.value.clone(),
                    owner: first.owner.clone(),
                    closure_syntax: first.closure_syntax,
                    receiver: first.receiver.clone(),
                }));
            }
            "require" => {
                let module = args
                    .first()
                    .and_then(|arg| match arg.value.structural_type() {
                        LuaType::StringConst(name) | LuaType::DocStringConst(name) => {
                            Some(name.to_string())
                        }
                        _ => None,
                    });
                let Some(module) = module else {
                    return Ok(None);
                };
                // This original producer deliberately projects a public module
                // TypeShell, rather than invoking contextual semantic inference.
                self.checkpoint()?;
                OwnedValue::plain(self.model.require_module_type(&module))
            }
            "assert" => match args.first() {
                Some(arg) => self.builtin_remove_nil(arg.value.clone())?,
                None => OwnedValue::plain(LuaType::Unknown),
            },
            "pcall" | "xpcall" => {
                // xpcall intentionally passes args[1..] just like the fixed VM.
                let ret = if let Some(arg) = args.first() {
                    self.owned_pcall_callback_ret(
                        &arg.value,
                        arg.owner.as_ref(),
                        arg.closure_syntax,
                        &args[1..],
                    )?
                } else {
                    None
                };
                match ret {
                    Some((value, include_error)) => {
                        self.owned_pcall_return(value, include_error)?
                    }
                    None => OwnedValue::plain(LuaType::Boolean),
                }
            }
            "unpack" => {
                let Some(arg) = args.first() else {
                    return Ok(Some(plain(LuaType::Unknown)));
                };
                match arg.value.structural_type() {
                    LuaType::TableConst(table) => {
                        let owner = SemanticId::member(table.file_id, table.value);
                        let members = self.model.members_of_owner(&owner);
                        let mut indexed = Vec::new();
                        for reference in members.iter() {
                            self.checkpoint()?;
                            let Some(facts) = self.model.file_facts_of(reference.file_id) else {
                                continue;
                            };
                            let Some(member) = facts.member_by_id(&reference.id) else {
                                continue;
                            };
                            let LuaMemberKey::Integer(index) = &member.key else {
                                continue;
                            };
                            let index = *index;
                            let value = if let Some(syntax) = member.value_syntax {
                                self.owned_expr_inherit(syntax)?
                            } else {
                                self.type_of_member(&reference.id)?
                                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown))
                            };
                            indexed.push((index, value));
                        }
                        indexed.sort_by_key(|(index, _)| *index);
                        OwnedValue::pack(indexed.into_iter().map(|(_, value)| value).collect())
                    }
                    LuaType::Array(array) => {
                        let base = self.owned_child(&arg.value, 0, array.get_base());
                        let element = if matches!(callee.owner, Some(SemanticId::Member(_))) {
                            OwnedValue::union_checked(
                                vec![base, OwnedValue::plain(LuaType::Nil)],
                                || self.checkpoint(),
                            )?
                        } else {
                            base
                        };
                        base_pack(element)
                    }
                    _ => OwnedValue::plain(LuaType::Unknown),
                }
            }
            "select" => self.owned_select_builtin(args)?,
            _ => return Ok(None),
        };
        self.checkpoint()?;
        Ok(Some(ExpressionValue::plain(value)))
    }

    fn owned_select_builtin(
        &mut self,
        args: &[ExpressionValue<OwnedValue>],
    ) -> QueryResult<OwnedValue, E> {
        if args.first().is_some_and(|arg| matches!(arg.value.structural_type(), LuaType::StringConst(s) | LuaType::DocStringConst(s) if s.as_str() == "#")) {
            let mut count = 0usize;
            let mut unbounded = false;
            for arg in args.iter().skip(1) {
                self.checkpoint()?;
                match arg.value.structural_type() {
                    LuaType::Variadic(pack) => match pack.as_ref() {
                        VariadicType::Multi(types) => count += types.len(),
                        VariadicType::Base(_) => unbounded = true,
                    },
                    _ => count += 1,
                }
            }
            return Ok(OwnedValue::plain(if unbounded { LuaType::Integer } else { LuaType::IntegerConst(count as i64) }));
        }
        let n = args
            .first()
            .and_then(|arg| match arg.value.structural_type() {
                LuaType::IntegerConst(i) | LuaType::DocIntegerConst(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(1)
            .max(1);
        let mut tail = Vec::new();
        let mut unbounded = false;
        let mut remaining = n.saturating_sub(1);
        for arg in args.iter().skip(1) {
            self.checkpoint()?;
            match arg.value.structural_type() {
                LuaType::Variadic(pack) => match pack.as_ref() {
                    VariadicType::Multi(types) => {
                        for (index, raw) in types.iter().enumerate() {
                            self.checkpoint()?;
                            if remaining > 0 {
                                remaining -= 1;
                            } else {
                                tail.push(widen_call_const(
                                    &self.owned_child(&arg.value, index, raw),
                                ));
                            }
                        }
                    }
                    VariadicType::Base(base) => {
                        remaining = 0;
                        tail.push(widen_call_const(&self.owned_child(&arg.value, 0, base)));
                        unbounded = true;
                    }
                },
                _ => {
                    if remaining > 0 {
                        remaining -= 1;
                    } else {
                        tail.push(widen_call_const(&arg.value));
                    }
                }
            }
        }
        Ok(if unbounded {
            base_pack(
                tail.into_iter()
                    .next()
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)),
            )
        } else if tail.is_empty() {
            OwnedValue::plain(LuaType::Nil)
        } else {
            OwnedValue::pack(tail)
        })
    }

    fn builtin_remove_nil(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Union(_) => OwnedValue::union_checked(
                value
                    .union_components()
                    .expect("union components")
                    .into_iter()
                    .filter(|value| !matches!(value.structural_type(), LuaType::Nil))
                    .collect(),
                || self.checkpoint(),
            ),
            LuaType::Nil => Ok(OwnedValue::plain(LuaType::Unknown)),
            LuaType::Variadic(pack) => match pack.as_ref() {
                VariadicType::Multi(types) => {
                    let mut slots = Vec::new();
                    for (index, raw) in types.iter().enumerate() {
                        let child = self.owned_child(&value, index, raw);
                        slots.push(if index == 0 {
                            self.builtin_remove_nil(child)?
                        } else {
                            child
                        });
                    }
                    Ok(OwnedValue::pack(slots))
                }
                VariadicType::Base(base) => {
                    let base = self.builtin_remove_nil(self.owned_child(&value, 0, base))?;
                    Ok(base_pack(base))
                }
            },
            _ => Ok(value),
        }
    }

    fn owned_pcall_callback_ret(
        &mut self,
        value: &OwnedValue,
        owner: Option<&SemanticId>,
        closure: Option<LuaSyntaxId>,
        args: &[ExpressionValue<OwnedValue>],
    ) -> QueryResult<Option<(OwnedValue, bool)>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::DocFunction(function) => {
                let ret = value.function_return().expect("function return");
                let ret = if !args.is_empty() {
                    if let Some((bindings, _)) =
                        self.match_owned_callable(value, args, false, None, false)?
                    {
                        let ret =
                            self.bind_owned_signature_generics(ret, function.get_generic_params())?;
                        let ret = self.substitute_owned(ret, &bindings)?;
                        let ret = self.expand_owned_aliases(ret)?;
                        self.eval_owned_conditionals(ret)?
                    } else {
                        ret
                    }
                } else {
                    ret
                };
                Ok(Some((ret, true)))
            }
            LuaType::Any | LuaType::Unknown => {
                Ok(Some((OwnedValue::plain(LuaType::Unknown), true)))
            }
            LuaType::Union(_) | LuaType::Intersection(_) => {
                let components = if let Some(components) = value.union_components() {
                    components
                } else if let LuaType::Intersection(intersection) = value.structural_type() {
                    intersection
                        .get_types()
                        .iter()
                        .enumerate()
                        .map(|(index, raw)| self.owned_child(value, index, raw))
                        .collect()
                } else {
                    unreachable!("union or intersection")
                };
                let mut types = Vec::new();
                for component in components {
                    self.checkpoint()?;
                    // Keep the precheck and the recursive DocFunction match as
                    // separate producers, exactly as the fixed implementation.
                    if matches!(component.structural_type(), LuaType::DocFunction(_))
                        && !args.is_empty()
                        && self
                            .match_owned_callable(&component, args, false, None, false)?
                            .is_none()
                    {
                        continue;
                    }
                    if let Some((ret, _)) =
                        self.owned_pcall_callback_ret(&component, owner, closure, args)?
                        && !types.contains(&ret)
                    {
                        types.push(ret);
                    }
                }
                if types.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some((
                        OwnedValue::union_checked(types, || self.checkpoint())?,
                        true,
                    )))
                }
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                if def.kind != TypeDefKind::Alias {
                    return Ok(None);
                }
                let Some(target) = self.model.alias_target(&def) else {
                    return Ok(None);
                };
                let target = OwnedValue::plain(target);
                if matches!(target.structural_type(), LuaType::DocFunction(_))
                    && !args.is_empty()
                    && self
                        .match_owned_callable(&target, args, false, None, false)?
                        .is_none()
                {
                    return Ok(None);
                }
                Ok(self
                    .owned_pcall_callback_ret(&target, owner, closure, args)?
                    .map(|(ret, _)| (ret, true)))
            }
            LuaType::Function | LuaType::Signature(_) => {
                if let Some(closure) = closure
                    && let Some(function) = self.signature_local(closure)?
                {
                    return Ok(Some((
                        function.function_return().expect("signature return"),
                        false,
                    )));
                }
                let Some(SemanticId::Member(key)) = owner else {
                    return Ok(None);
                };
                let Some(facts) = self.model.file_facts_of(key.file_id) else {
                    return Ok(None);
                };
                let Some(member) = facts.member_by_id(&SemanticId::Member(key.clone())) else {
                    return Ok(None);
                };
                let Some(closure) = member.value_syntax else {
                    return Ok(None);
                };
                Ok(self
                    .signature_in(key.file_id, closure)?
                    .ok()
                    .map(|function| (function.function_return().expect("signature return"), false)))
            }
            _ => Ok(None),
        }
    }

    fn owned_pcall_return(
        &mut self,
        callback: OwnedValue,
        include_error: bool,
    ) -> QueryResult<OwnedValue, E> {
        let mut slots = vec![OwnedValue::plain(LuaType::Boolean)];
        match callback.structural_type() {
            LuaType::Variadic(pack) => match pack.as_ref() {
                VariadicType::Multi(types) => {
                    for (index, raw) in types.iter().enumerate() {
                        self.checkpoint()?;
                        let child = self.owned_child(&callback, index, raw);
                        slots.push(self.pcall_retained_slot(child, include_error && index == 0)?);
                    }
                }
                VariadicType::Base(base) => slots.push(
                    self.pcall_retained_slot(self.owned_child(&callback, 0, base), include_error)?,
                ),
            },
            _ => slots.push(self.pcall_retained_slot(callback, include_error)?),
        }
        Ok(OwnedValue::pack(slots))
    }

    fn pcall_retained_slot(
        &mut self,
        value: OwnedValue,
        include_error: bool,
    ) -> QueryResult<OwnedValue, E> {
        let value = if include_error {
            OwnedValue::union_checked(vec![value, OwnedValue::plain(LuaType::String)], || {
                self.checkpoint()
            })?
        } else {
            value
        };
        if value.structural_type().contain_multi_return() {
            OwnedValue::union_checked(vec![value, OwnedValue::plain(LuaType::Nil)], || {
                self.checkpoint()
            })
        } else {
            Ok(value)
        }
    }
}

fn plain(ty: LuaType) -> ExpressionValue<OwnedValue> {
    ExpressionValue::plain(OwnedValue::plain(ty))
}
fn base_pack(base: OwnedValue) -> OwnedValue {
    OwnedValue::with_structural_children(
        LuaType::Variadic(Arc::new(VariadicType::Base(base.structural_type().clone()))),
        vec![base],
    )
}
