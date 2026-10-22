use super::*;
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn count_generic_fallback(
        &mut self,
        callee: &LuaExpr,
        value: &OwnedValue,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaType::Generic(generic) = value.structural_type() else {
            return Ok(Vec::new());
        };
        let Some(def) = self.model.type_def_of(&generic.get_base_type_id()) else {
            return Ok(Vec::new());
        };
        if def.call_overloads.is_empty() {
            return Ok(Vec::new());
        }
        let Some(actuals) = self.count_generic_actuals(callee, &def)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for syntax in &def.call_overloads {
            if let Some(function) = self.count_expanded_overload(def.file_id, *syntax, &actuals)? {
                out.push(function);
            }
        }
        Ok(out)
    }
    fn count_generic_actuals(
        &mut self,
        callee: &LuaExpr,
        def: &TypeDef,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        self.checkpoint()?;
        let LuaExpr::NameExpr(name) = callee else {
            return Ok(None);
        };
        let Some(text) = name.get_name_text() else {
            return Ok(None);
        };
        let Some(id) = self
            .model
            .global_decl(&text)
            .or_else(|| self.model.resolve_name(name.get_position()))
        else {
            return Ok(None);
        };
        if self.owner_initializer_managed(&id)? {
            return Ok(None);
        }
        let SemanticId::Decl(key) = &id else {
            return Ok(None);
        };
        let Some(facts) = self.model.file_facts_of(key.file_id) else {
            return Ok(None);
        };
        let Some(syntax) = facts.decl_by_id(&id).and_then(|d| d.value_expr_syntax) else {
            return Ok(None);
        };
        let call = self
            .model
            .syntax_tree_of(key.file_id)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaCallExpr::cast);
        let Some(call) = call else { return Ok(None) };
        let Some(LuaExpr::NameExpr(inner)) = call.get_prefix_expr() else {
            return Ok(None);
        };
        let Some(inner) = inner.get_name_text() else {
            return Ok(None);
        };
        let Some(closure) = facts.decl_named(&inner).and_then(|d| d.value_expr_syntax) else {
            return Ok(None);
        };
        let Ok(function) = self.signature_in(key.file_id, closure)? else {
            return Ok(None);
        };
        let Some(arg) = call.get_args_list().and_then(|list| {
            list.get_args()
                .find_map(|arg| LuaClosureExpr::cast(arg.syntax().clone()))
        }) else {
            return Ok(None);
        };
        let Ok(arg) = self.signature_in(key.file_id, arg.get_syntax_id())? else {
            return Ok(None);
        };
        let mut bindings = Bindings::new();
        for param in function.function_parameters().expect("function parameters") {
            if let Some(param) = param {
                let _ = self.unify_structural(&param, &arg, &mut bindings)?;
            }
        }
        let LuaType::DocFunction(meta) = function.structural_type() else {
            unreachable!("function")
        };
        let mut actuals = Vec::new();
        for (index, _) in def.generic_params.iter().enumerate() {
            self.checkpoint()?;
            let Some(tpl) = meta.get_generic_params().get(index) else {
                return Ok(None);
            };
            let Some(value) = bindings.get(&tpl.get_tpl_id()) else {
                return Ok(None);
            };
            actuals.push(value.clone());
        }
        Ok(Some(actuals))
    }
    fn count_expanded_overload(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        actuals: &[OwnedValue],
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let doc = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast);
        let Some(LuaDocType::Func(fun)) = doc else {
            return Ok(None);
        };
        let mut total = 0;
        for (index, param) in fun.get_params().enumerate() {
            self.checkpoint()?;
            let Some(ty) = param.get_type() else {
                return Ok(None);
            };
            if let LuaDocType::Variadic(pack) = ty {
                let Some(inner) = pack.get_type() else {
                    return Ok(None);
                };
                let Some(len) = self.count_tuple_doc_len(file, &inner, actuals, 0)? else {
                    return Ok(None);
                };
                total = index + len;
                break;
            } else {
                total = index + 1;
            }
        }
        if total == 0 {
            return Ok(None);
        }
        let params = (0..total)
            .map(|_| (String::new(), Some(LuaType::Any)))
            .collect();
        Ok(Some(OwnedValue::plain(LuaType::DocFunction(Arc::new(
            LuaFunctionType::new(
                AsyncState::None,
                false,
                false,
                params,
                LuaType::Unknown,
                None,
            ),
        )))))
    }
    fn count_tuple_doc_len(
        &mut self,
        file: FileId,
        ty: &LuaDocType,
        actuals: &[OwnedValue],
        depth: usize,
    ) -> QueryResult<Option<usize>, E> {
        self.checkpoint()?;
        if depth > 8 {
            return Ok(None);
        }
        let LuaDocType::Generic(generic) = ty else {
            return Ok(None);
        };
        let Some(name) = generic.get_name_type().and_then(|n| n.get_name_text()) else {
            return Ok(None);
        };
        let Some(def) = self.model.resolve_type_def_in(file, &name) else {
            return Ok(None);
        };
        self.count_alias_tuple_len(&def, actuals, depth + 1)
    }
    fn count_alias_tuple_len(
        &mut self,
        def: &TypeDef,
        actuals: &[OwnedValue],
        depth: usize,
    ) -> QueryResult<Option<usize>, E> {
        self.checkpoint()?;
        if depth > 8 {
            return Ok(None);
        }
        let doc = def
            .alias_type
            .and_then(|syntax| {
                self.model
                    .syntax_tree_of(def.file_id)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            })
            .and_then(LuaDocType::cast);
        let Some(LuaDocType::Conditional(conditional)) = doc else {
            return Ok(None);
        };
        let Some((condition, yes, _)) = conditional.get_types() else {
            return Ok(None);
        };
        let LuaDocType::Binary(binary) = condition else {
            return Ok(None);
        };
        if binary.get_op_token().map(|op| op.get_op()) != Some(LuaTypeBinaryOperator::Extends) {
            return Ok(None);
        }
        let Some((_, right)) = binary.get_types() else {
            return Ok(None);
        };
        match right {
            LuaDocType::Name(name) if name.get_name_text().as_deref() == Some("Procedure") => {
                if !actuals
                    .first()
                    .is_some_and(|v| matches!(v.structural_type(), LuaType::DocFunction(_)))
                {
                    return Ok(None);
                }
                self.count_tuple_doc_len(def.file_id, &yes, actuals, depth)
            }
            LuaDocType::Func(_) => {
                let Some(actual) = actuals.first() else {
                    return Ok(None);
                };
                let LuaType::DocFunction(fun) = actual.structural_type() else {
                    return Ok(None);
                };
                if matches!(&yes,LuaDocType::Name(n) if n.get_name_text().as_deref()==Some("P")) {
                    Ok(Some(fun.get_params().len()))
                } else {
                    self.count_tuple_doc_len(def.file_id, &yes, actuals, depth)
                }
            }
            _ => Ok(None),
        }
    }
}
