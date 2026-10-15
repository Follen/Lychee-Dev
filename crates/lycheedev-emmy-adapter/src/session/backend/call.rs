//! Complete call execution schedule, adapted from fixed EmmyLua infer/vm.rs.
//! Discovery, generic matching, callback environments and returns all share the
//! current owned query and append-only binding ledger.
use super::{ExpressionValue, OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaMemberKey, LuaType, SemanticId, VariadicType};
use emmylua_parser::{LuaAstNode, LuaClosureExpr, LuaSyntaxId};
use std::{cell::RefCell, collections::HashSet, rc::Rc};

struct CallbackDepth(Rc<RefCell<super::environment::Environment>>);
impl Drop for CallbackDepth {
    fn drop(&mut self) {
        self.0.borrow_mut().callback_depth -= 1;
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn call_plain_signature(
        &mut self,
        callee: ExpressionValue<OwnedValue>,
        arguments: Vec<ExpressionValue<OwnedValue>>,
        colon: bool,
        generic_syntaxes: &[LuaSyntaxId],
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.call_owned(callee, &arguments, colon, generic_syntaxes)
    }

    pub(super) fn call_owned(
        &mut self,
        callee: ExpressionValue<OwnedValue>,
        args: &[ExpressionValue<OwnedValue>],
        colon: bool,
        generic_syntaxes: &[LuaSyntaxId],
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        if matches!(callee.value.structural_type(), LuaType::Any) {
            return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Any)));
        }
        if let Some(value) = self.call_builtin(&callee, args)? {
            return Ok(value);
        }
        let candidates = self.owned_callable_candidates(&callee)?;
        let receiver = match &callee.receiver {
            Some(value) => Some(
                self.call_receiver_class(value)?
                    .unwrap_or_else(|| value.clone()),
            ),
            None => None,
        };
        let mut selected =
            self.select_owned_callables(&candidates, args, colon, receiver.as_ref(), false, false)?;
        if selected.is_empty() {
            selected = self.select_owned_callables(
                &candidates,
                args,
                colon,
                receiver.as_ref(),
                true,
                false,
            )?;
        }
        let Some((function, mut bindings)) = selected.into_iter().next() else {
            return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Unknown)));
        };
        let LuaType::DocFunction(metadata) = function.structural_type() else {
            unreachable!("callable candidate");
        };
        for (i, syntax) in generic_syntaxes.iter().enumerate() {
            self.checkpoint()?;
            if let Some(tpl) = metadata.get_generic_params().get(i) {
                let value = self
                    .model
                    .doc_type_lua_in(self.model.file_id(), *syntax, &[]);
                bindings.insert(tpl.get_tpl_id(), OwnedValue::plain(value));
            }
        }
        let class_map = match &callee.receiver {
            Some(value) => self.owned_class_generic_map(value)?.unwrap_or_default(),
            None => Default::default(),
        };
        for parameter in metadata.get_generic_params() {
            self.checkpoint()?;
            if bindings.contains_key(&parameter.get_tpl_id()) {
                continue;
            }
            if let Some(value) = parameter
                .get_default_type()
                .or_else(|| parameter.get_constraint())
            {
                let value =
                    self.substitute_owned_names(OwnedValue::plain(value.clone()), &class_map)?;
                bindings.insert(parameter.get_tpl_id(), value);
            }
        }
        for _ in 0..metadata.get_generic_params().len().max(1) {
            self.checkpoint()?;
            let mut changed = false;
            let mut names = class_map.clone();
            for parameter in metadata.get_generic_params() {
                self.checkpoint()?;
                if let Some(value) = bindings.get(&parameter.get_tpl_id()) {
                    names.insert(parameter.get_name().to_string(), value.clone());
                }
            }
            for parameter in metadata.get_generic_params() {
                self.checkpoint()?;
                if let Some(value) = bindings.get(&parameter.get_tpl_id()).cloned() {
                    let replacement = self.substitute_owned_names(value.clone(), &names)?;
                    let replacement = self.substitute_owned(replacement, &bindings)?;
                    changed |= replacement != value;
                    bindings.insert(parameter.get_tpl_id(), replacement);
                }
            }
            if !changed {
                break;
            }
        }
        let callback_offset = usize::from(
            colon
                && metadata.get_params().first().is_some_and(|(name, ty)| {
                    name == "self" || matches!(ty, Some(LuaType::SelfInfer))
                }),
        );
        let parameters = function.function_parameters().expect("function parameters");
        // Populate every callback parameter before any return back-inference.
        for (index, arg) in args.iter().enumerate() {
            self.checkpoint()?;
            let Some(closure) = arg.closure_syntax else {
                continue;
            };
            let Some(Some(parameter)) = parameters.get(index + callback_offset) else {
                continue;
            };
            let parameter = self
                .bind_owned_signature_generics(parameter.clone(), metadata.get_generic_params())?;
            let parameter = self.substitute_owned(parameter, &bindings)?;
            let Some(callback) = self.callback_function(parameter)? else {
                continue;
            };
            for (i, parameter) in callback
                .function_parameters()
                .expect("callback parameters")
                .into_iter()
                .enumerate()
            {
                self.checkpoint()?;
                let Some(parameter) = parameter else {
                    continue;
                };
                let mut value = self.substitute_owned(parameter, &bindings)?;
                if let Some(receiver) = &callee.receiver {
                    value = self.replace_owned_self(value, receiver)?;
                }
                let values = match value.structural_type() {
                    LuaType::Variadic(v) => Some(match v.as_ref() {
                        VariadicType::Base(base) => {
                            let base = self.owned_child(&value, 0, base);
                            if let LuaType::Tuple(tuple) = base.structural_type() {
                                tuple
                                    .get_types()
                                    .iter()
                                    .enumerate()
                                    .map(|(i, v)| self.owned_child(&base, i, v))
                                    .collect()
                            } else {
                                vec![base]
                            }
                        }
                        VariadicType::Multi(types) => types
                            .iter()
                            .enumerate()
                            .map(|(i, v)| self.owned_child(&value, i, v))
                            .collect(),
                    }),
                    _ => None,
                };
                let env = self.environment();
                if let Some(values) = values {
                    for (offset, value) in values.into_iter().enumerate() {
                        self.checkpoint()?;
                        env.borrow_mut()
                            .parameters
                            .insert((closure, i + offset), value);
                    }
                } else {
                    env.borrow_mut().parameters.insert((closure, i), value);
                }
            }
        }
        // Object-argument closures are a separate producer pass in the VM.
        for (index, arg) in args.iter().enumerate() {
            self.checkpoint()?;
            if !matches!(arg.value.structural_type(), LuaType::TableConst(_)) {
                continue;
            }
            let Some(Some(parameter)) = parameters.get(index + callback_offset) else {
                continue;
            };
            let parameter = self
                .bind_owned_signature_generics(parameter.clone(), metadata.get_generic_params())?;
            let parameter = self.substitute_owned(parameter, &bindings)?;
            if !matches!(parameter.structural_type(), LuaType::Unknown | LuaType::Any) {
                self.bind_owned_table_callbacks(&arg.value, &parameter, &bindings)?;
            }
        }
        for (index, arg) in args.iter().enumerate() {
            self.checkpoint()?;
            let Some(closure) = arg.closure_syntax else {
                continue;
            };
            let Some(Some(parameter)) = parameters.get(index + callback_offset) else {
                continue;
            };
            let parameter = self
                .bind_owned_signature_generics(parameter.clone(), metadata.get_generic_params())?;
            let parameter = self.substitute_owned(parameter, &bindings)?;
            if matches!(parameter.structural_type(), LuaType::DocFunction(_)) {
                let ret = parameter.function_return().expect("callback return");
                if matches!(ret.structural_type(), LuaType::Variadic(_)) {
                    continue;
                }
                let actual = self.closure_return_with_env(closure)?;
                if !matches!(actual.structural_type(), LuaType::Unknown | LuaType::Any) {
                    let _ = self.unify_call_bindings(&ret, &actual, &mut bindings)?;
                }
            }
        }
        for (index, arg) in args.iter().enumerate() {
            self.checkpoint()?;
            let Some(Some(parameter)) = parameters.get(index + callback_offset) else {
                continue;
            };
            let parameter = self
                .bind_owned_signature_generics(parameter.clone(), metadata.get_generic_params())?;
            if !matches!(parameter.structural_type(), LuaType::DocFunction(_)) {
                continue;
            }
            let original_ret = parameter.function_return().expect("callback return");
            let parameter = self.substitute_owned(parameter, &bindings)?;
            if !matches!(parameter.structural_type(), LuaType::DocFunction(_)) {
                continue;
            }
            let substituted_ret = parameter.function_return().expect("callback return");
            let value = if let Some(closure) = arg.closure_syntax {
                self.signature_local(closure)?
                    .unwrap_or_else(|| arg.value.clone())
            } else {
                arg.value.clone()
            };
            let callback = ExpressionValue {
                value,
                owner: arg.owner.clone(),
                closure_syntax: None,
                receiver: arg.receiver.clone(),
            };
            let later = args.get(index + 1..).unwrap_or(&[]);
            if self.environment().borrow().callback_depth >= 8 {
                continue;
            }
            let actual = if let Some(closure) = arg.closure_syntax
                && !matches!(original_ret.structural_type(), LuaType::Variadic(_))
            {
                let ret = self.closure_return_with_env(closure)?;
                if !matches!(ret.structural_type(), LuaType::Unknown | LuaType::Any) {
                    ret
                } else {
                    self.nested_owned_callback(callback.clone(), later)?
                }
            } else {
                self.nested_owned_callback(callback.clone(), later)?
            };
            if !matches!(actual.structural_type(), LuaType::Unknown | LuaType::Any) {
                self.bind_owned_callback_return(
                    &original_ret,
                    &substituted_ret,
                    &actual,
                    &mut bindings,
                )?;
            } else {
                let mut structural = false;
                for candidate in self.owned_callable_candidates(&callback)? {
                    self.checkpoint()?;
                    if matches!(candidate.structural_type(),LuaType::DocFunction(fun) if fun.get_params().iter().any(|(_,ty)|matches!(ty,Some(LuaType::DocFunction(_)))))
                        && self
                            .match_owned_callable(&candidate, later, false, None, false)?
                            .is_some()
                    {
                        structural = true;
                        break;
                    }
                }
                if structural {
                    self.bind_owned_callback_return(
                        &original_ret,
                        &substituted_ret,
                        &OwnedValue::plain(LuaType::Unknown),
                        &mut bindings,
                    )?;
                }
            }
        }
        let mut ret = self.substituted_owned_return(&callee, &function, &bindings)?;
        if (self.environment().borrow().callback_depth > 0
            || matches!(callee.value.structural_type(), LuaType::Union(_)))
            && candidates.len() > 1
        {
            let selected = self.select_owned_callables(
                &candidates,
                args,
                colon,
                receiver.as_ref(),
                false,
                true,
            )?;
            if selected.len() > 1 {
                let mut returns = Vec::new();
                for (function, bindings) in selected {
                    let value = self.substituted_owned_return(&callee, &function, &bindings)?;
                    if !returns.contains(&value) {
                        returns.push(value);
                    }
                }
                ret = OwnedValue::union_checked(returns, || self.checkpoint())?;
            }
        }
        if let Some(receiver) = &callee.receiver
            && let Some(map) = self.owned_class_generic_map(receiver)?
        {
            ret = self.substitute_owned_names(ret, &map)?;
        }
        let self_type = if colon {
            receiver
        } else {
            if let Some(value) = self.call_operator_self(&callee.value)? {
                Some(value)
            } else if matches!(callee.value.structural_type(), LuaType::TableConst(_)) {
                Some(callee.value.clone())
            } else {
                receiver
            }
        };
        if let Some(owner) = self_type {
            let owner = self.instantiate_call_self(&owner, &function, &bindings)?;
            ret = self.replace_owned_self(ret, &owner)?;
        }
        ret = self.owned_sanitize_generics(ret, &HashSet::new(), true)?;
        ret = self.expand_owned_unpack(ret)?;
        ret = self.expand_returned_call_function(ret, args, &function)?;
        Ok(ExpressionValue::plain(ret))
    }

    fn nested_owned_callback(
        &mut self,
        callee: ExpressionValue<OwnedValue>,
        args: &[ExpressionValue<OwnedValue>],
    ) -> QueryResult<OwnedValue, E> {
        let environment = self.environment();
        environment.borrow_mut().callback_depth += 1;
        let _depth = CallbackDepth(environment);
        self.call_owned(callee, args, false, &[])
            .map(|value| value.value)
    }

    fn bind_owned_table_callbacks(
        &mut self,
        table: &OwnedValue,
        expected: &OwnedValue,
        bindings: &Bindings,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let LuaType::TableConst(table) = table.structural_type() else {
            return Ok(());
        };
        let owner = SemanticId::member(table.file_id, table.value);
        let Some(tree) = self.model.syntax_tree_of(table.file_id) else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for reference in self.model.members_of_owner(&owner) {
            self.checkpoint()?;
            let Some(member) = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
            else {
                continue;
            };
            let Some(syntax) = member.value_syntax else {
                continue;
            };
            let Some(closure) = syntax
                .to_node_from_root(&root)
                .and_then(LuaClosureExpr::cast)
            else {
                continue;
            };
            let Some(field) = self.member_type(expected, &member.key)? else {
                continue;
            };
            let field = self.expand_owned_aliases(field)?;
            let Some(function) = self.callback_function(field)? else {
                continue;
            };
            for (i, param) in function
                .function_parameters()
                .expect("function parameters")
                .into_iter()
                .enumerate()
            {
                self.checkpoint()?;
                if let Some(param) = param {
                    let mut value = self.substitute_owned(param, bindings)?;
                    if !matches!(expected.structural_type(), LuaType::Unknown | LuaType::Any) {
                        value = self.replace_owned_self(value, expected)?;
                    }
                    self.environment()
                        .borrow_mut()
                        .parameters
                        .insert((closure.get_syntax_id(), i), value);
                }
            }
        }
        Ok(())
    }
}

pub(super) fn plain_atom(value: &LuaType) -> bool {
    matches!(
        value,
        LuaType::Unknown
            | LuaType::Any
            | LuaType::Nil
            | LuaType::Table
            | LuaType::Userdata
            | LuaType::Function
            | LuaType::Thread
            | LuaType::Boolean
            | LuaType::String
            | LuaType::Integer
            | LuaType::Number
            | LuaType::Io
            | LuaType::Never
            | LuaType::BooleanConst(_)
            | LuaType::StringConst(_)
            | LuaType::IntegerConst(_)
            | LuaType::FloatConst(_)
            | LuaType::DocStringConst(_)
            | LuaType::DocIntegerConst(_)
            | LuaType::DocBooleanConst(_)
    )
}
