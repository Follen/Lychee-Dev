//! Correlated return-overload narrowing preserves source sets, row order and
//! uncorrelated assignments. No selected call is treated as an ordinary row.
use super::*;
use crate::session::BindingDecision;
use emmylua_code_analysis::{FileId, Signature};
use emmylua_parser::{BinaryOperator, LuaAstPtr, LuaCallExpr, LuaClosureExpr, LuaLiteralToken};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn flow_callable_signature(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<(FileId, LuaSyntaxId, Signature)>, E> {
        self.checkpoint()?;
        let site = self.site(call.get_syntax_id());
        let managed = !matches!(
            self.session.context().call(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        let managed = if managed {
            true
        } else {
            let managed = !matches!(
                self.session.context().resolve(site),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            managed
        };
        if managed {
            return Ok(None);
        }
        let callee = call.get_prefix_expr();
        if let Some(callee) = &callee {
            self.checkpoint()?;
            let managed = !matches!(
                self.session
                    .context()
                    .resolve(self.site(callee.get_syntax_id())),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            if managed {
                return Ok(None);
            }
        }
        let Some(LuaExpr::NameExpr(name)) = callee else {
            return Ok(None);
        };
        let Some(mut declaration) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let mut visited = HashSet::new();
        loop {
            self.checkpoint()?;
            let SemanticId::Decl(key) = &declaration else {
                return Ok(None);
            };
            let Some(facts) = self.model.file_facts_of(key.file_id) else {
                return Ok(None);
            };
            let Some(syntax) = facts
                .decl_by_id(&declaration)
                .and_then(|decl| decl.value_expr_syntax)
            else {
                return Ok(None);
            };
            let Some(node) = self
                .model
                .syntax_tree_of(key.file_id)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            else {
                return Ok(None);
            };
            if LuaClosureExpr::cast(node.clone()).is_some() {
                return Ok(facts
                    .signature_by_closure(syntax)
                    .map(|signature| (key.file_id, syntax, signature.clone())));
            }
            let Some(LuaExpr::NameExpr(alias)) = LuaExpr::cast(node) else {
                return Ok(None);
            };
            // The original resolver deliberately uses the current model here.
            let Some(next) = self.model.resolve_name(alias.get_position()) else {
                return Ok(None);
            };
            if !visited.insert(next.clone()) {
                return Ok(None);
            }
            declaration = next;
        }
    }
    fn overload_flow_rows(
        &mut self,
        ptr: &LuaAstPtr<LuaCallExpr>,
    ) -> QueryResult<Option<Vec<Vec<OwnedValue>>>, E> {
        self.checkpoint()?;
        let Some(call) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(None);
        };
        let Some((file, _, signature)) = self.flow_callable_signature(&call)? else {
            return Ok(None);
        };
        let Some(docs) = &signature.docs else {
            return Ok(None);
        };
        if docs.return_overload_rows.is_empty() {
            return Ok(None);
        }
        let mut rows = Vec::new();
        let mut index = 0usize;
        let mut generic = false;
        for len in &docs.return_overload_rows {
            self.checkpoint()?;
            let end = (index + len).min(docs.return_overloads.len());
            let mut row = Vec::new();
            for (_, syntax) in &docs.return_overloads[index..end] {
                self.checkpoint()?;
                let value = self
                    .model
                    .doc_type_lua_in(file, *syntax, &docs.generic_params);
                generic |= value.contain_tpl();
                row.push(OwnedValue::plain(value));
            }
            rows.push(row);
            index = end;
        }
        if generic
            && let Some((_, bindings)) = self.infer_owned_call_bindings(call.get_syntax_id())?
        {
            let mut instantiated = Vec::new();
            for row in rows {
                let mut values = Vec::new();
                for value in row {
                    self.checkpoint()?;
                    values.push(if value.structural_type().contain_tpl() {
                        let value = self.substitute_owned(value, &bindings)?;
                        self.widen_flow_const(value)?
                    } else {
                        value
                    });
                }
                instantiated.push(values);
            }
            return Ok(Some(instantiated));
        }
        Ok(Some(rows))
    }
    pub(super) fn correlated_flow_narrowing(
        &mut self,
        tree: &FlowTree,
        target: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
        flow: FlowId,
    ) -> QueryResult<Option<Narrowing>, E> {
        let Some((discriminant, narrowing)) = self.flow_discriminant(tree, target, expr, truthy)?
        else {
            return Ok(None);
        };
        let position = expr.get_position();
        self.checkpoint()?;
        let (disc_refs, _) =
            tree.get_decl_multi_return_ref_summary_at(&discriminant, position, flow);
        let (target_refs, _) = tree.get_decl_multi_return_ref_summary_at(target, position, flow);
        self.checkpoint()?;
        let mut disc_calls = HashSet::new();
        let mut target_calls = HashSet::new();
        for item in &disc_refs {
            self.checkpoint()?;
            disc_calls.insert(item.call_expr.get_syntax_id());
        }
        for item in &target_refs {
            self.checkpoint()?;
            target_calls.insert(item.call_expr.get_syntax_id());
        }
        if disc_calls.intersection(&target_calls).count() != disc_calls.len() {
            return Ok(None);
        }
        let mut matching = Vec::new();
        let mut candidates = Vec::new();
        for disc in &disc_refs {
            self.checkpoint()?;
            let Some(rows) = self.overload_flow_rows(&disc.call_expr)? else {
                continue;
            };
            let mut base = Vec::new();
            for row in &rows {
                self.checkpoint()?;
                base.push(
                    row.get(disc.return_index)
                        .cloned()
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil)),
                );
            }
            let base = OwnedValue::union_checked(base, || self.checkpoint())?;
            let narrowed = self.finalize_flow(
                base,
                &PathState {
                    narrowings: vec![narrowing.clone()],
                    casts: Vec::new(),
                },
            )?;
            for target in &target_refs {
                self.checkpoint()?;
                if target.call_expr.get_syntax_id() != disc.call_expr.get_syntax_id() {
                    continue;
                }
                for row in &rows {
                    self.checkpoint()?;
                    let disc_value = row
                        .get(disc.return_index)
                        .cloned()
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil));
                    let target = row
                        .get(target.return_index)
                        .cloned()
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil));
                    let target = target.result_slot(0).unwrap_or(target);
                    if !candidates.contains(&target) {
                        candidates.push(target.clone());
                    }
                    if self.flow_overlap(disc_value, narrowed.clone())?
                        && !matching.contains(&target)
                    {
                        matching.push(target);
                    }
                }
            }
        }
        Ok((!matching.is_empty()).then_some(Narrowing::Correlated {
            matching,
            candidates,
        }))
    }
    fn flow_discriminant(
        &mut self,
        tree: &FlowTree,
        target: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
    ) -> QueryResult<Option<(SemanticId, Narrowing)>, E> {
        if let Some(decl) = self.find_flow_discriminant(tree, target, expr)? {
            return Ok(self
                .direct_decl_narrowing(&decl, expr, truthy, false)?
                .map(|narrowing| (decl, narrowing)));
        }
        if let Some(value) = self.boolean_alias_discriminant(tree, target, expr, truthy)? {
            return Ok(Some(value));
        }
        self.type_alias_discriminant(tree, target, expr, truthy)
    }
    fn find_flow_discriminant(
        &mut self,
        tree: &FlowTree,
        target: &SemanticId,
        expr: &LuaExpr,
    ) -> QueryResult<Option<SemanticId>, E> {
        let mut stack = match expr {
            LuaExpr::NameExpr(_) | LuaExpr::CallExpr(_) => vec![expr.clone()],
            LuaExpr::UnaryExpr(unary) => unary.get_expr().into_iter().collect(),
            LuaExpr::BinaryExpr(binary) => binary
                .get_exprs()
                .map(|(left, right)| vec![right, left])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        while let Some(expr) = stack.pop() {
            self.checkpoint()?;
            match expr {
                LuaExpr::NameExpr(name) => {
                    if let Some(decl) = self.model.resolve_name(name.get_position())
                        && &decl != target
                        && tree.has_shared_multi_return_refs(&decl, target)
                    {
                        return Ok(Some(decl));
                    }
                }
                LuaExpr::CallExpr(call) => {
                    let mut args = Vec::new();
                    if let Some(list) = call.get_args_list() {
                        for arg in list.get_args() {
                            self.checkpoint()?;
                            args.push(arg);
                        }
                    }
                    stack.extend(args.into_iter().rev());
                }
                _ => {}
            }
        }
        Ok(None)
    }
    fn boolean_alias_discriminant(
        &mut self,
        tree: &FlowTree,
        target: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
    ) -> QueryResult<Option<(SemanticId, Narrowing)>, E> {
        let LuaExpr::NameExpr(name) = expr else {
            return Ok(None);
        };
        let Some(alias) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        if &alias == target || tree.has_shared_multi_return_refs(&alias, target) {
            return Ok(None);
        }
        let Some(LuaExpr::BinaryExpr(binary)) = self.flow_alias_initializer(&alias) else {
            return Ok(None);
        };
        let Some(op) = binary
            .get_op_token()
            .map(|token| token.get_op())
            .filter(|op| matches!(op, BinaryOperator::OpEq | BinaryOperator::OpNe))
        else {
            return Ok(None);
        };
        let Some((left, right)) = binary.get_exprs() else {
            return Ok(None);
        };
        let (name, literal) = match (left, right) {
            (LuaExpr::NameExpr(name), LuaExpr::LiteralExpr(literal))
            | (LuaExpr::LiteralExpr(literal), LuaExpr::NameExpr(name)) => (name, literal),
            _ => return Ok(None),
        };
        let Some(LuaLiteralToken::Bool(value)) = literal.get_literal() else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        if !tree.has_shared_multi_return_refs(&decl, target) {
            return Ok(None);
        }
        let state = ((op == BinaryOperator::OpEq) == value.is_true()) == truthy;
        Ok(Some((
            decl,
            if state {
                Narrowing::Truthy
            } else {
                Narrowing::Falsy
            },
        )))
    }
    fn flow_alias_initializer(&self, decl: &SemanticId) -> Option<LuaExpr> {
        let syntax = self
            .model
            .file_facts()?
            .decl_by_id(decl)?
            .value_expr_syntax?;
        let node = syntax.to_node_from_root(&self.model.syntax_tree()?.get_red_root())?;
        LuaExpr::cast(node)
    }
    fn type_alias_discriminant(
        &mut self,
        tree: &FlowTree,
        target: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
    ) -> QueryResult<Option<(SemanticId, Narrowing)>, E> {
        let mut aliases = Vec::new();
        self.condition_names(expr, &mut aliases)?;
        for alias in aliases {
            self.checkpoint()?;
            if &alias == target || tree.has_shared_multi_return_refs(&alias, target) {
                continue;
            }
            let Some(LuaExpr::CallExpr(call)) = self.flow_alias_initializer(&alias) else {
                continue;
            };
            let Some(LuaExpr::NameExpr(callee)) = call.get_prefix_expr() else {
                continue;
            };
            if callee.get_name_text().as_deref() != Some("type") {
                continue;
            }
            let Some(LuaExpr::NameExpr(arg)) =
                call.get_args_list().and_then(|list| list.get_args().next())
            else {
                continue;
            };
            let Some(decl) = self.model.resolve_name(arg.get_position()) else {
                continue;
            };
            if !tree.has_shared_multi_return_refs(&decl, target) {
                continue;
            }
            let LuaExpr::BinaryExpr(binary) = expr else {
                continue;
            };
            let (Some(op), Some((left, right))) = (binary.get_op_token(), binary.get_exprs())
            else {
                continue;
            };
            let literal = if self.is_decl_name(&left, &alias) {
                right
            } else if self.is_decl_name(&right, &alias) {
                left
            } else {
                continue;
            };
            let LuaExpr::LiteralExpr(literal) = literal else {
                continue;
            };
            let Some(LuaLiteralToken::String(value)) = literal.get_literal() else {
                continue;
            };
            let Some(primitive) = guards::primitive(&value.get_value()) else {
                continue;
            };
            let narrowing = match (truthy, op.get_op()) {
                (true, BinaryOperator::OpEq) | (false, BinaryOperator::OpNe) => {
                    Narrowing::Replace(OwnedValue::plain(primitive))
                }
                (true, BinaryOperator::OpNe) | (false, BinaryOperator::OpEq) => {
                    Narrowing::Remove(OwnedValue::plain(primitive))
                }
                _ => continue,
            };
            return Ok(Some((decl, narrowing)));
        }
        Ok(None)
    }
}
