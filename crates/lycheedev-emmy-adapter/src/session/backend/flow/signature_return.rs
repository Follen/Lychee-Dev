//! Guard reachability uses the non-contextual signature-shell producer, not the
//! contextual closure-return VM. Selected call/callee ownership short-circuits
//! this producer exactly at the original two manages checks.
use super::*;
use crate::session::BindingDecision;
use emmylua_parser::{LuaCallExpr, LuaClosureExpr, LuaLiteralToken, LuaReturnStat};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn flow_signature_return(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let site = self.site(call.get_syntax_id());
        self.checkpoint()?;
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
            let file = key.file_id;
            let Some(facts) = self.model.file_facts_of(file) else {
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
                .syntax_tree_of(file)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            else {
                return Ok(None);
            };
            if let Some(closure) = LuaClosureExpr::cast(node.clone()) {
                let Some(signature) = facts.signature_by_closure(syntax) else {
                    return Ok(None);
                };
                if let Some(docs) = &signature.docs {
                    if !docs.returns.is_empty() {
                        let mut types = Vec::new();
                        for syntax in &docs.returns {
                            self.checkpoint()?;
                            let ty =
                                self.model
                                    .doc_type_lua_in(file, *syntax, &docs.generic_params);
                            if !ty.is_unknown() {
                                types.push(ty);
                            }
                        }
                        if !types.is_empty() {
                            return Ok(Some(OwnedValue::plain(LuaType::from_vec(types))));
                        }
                    }
                    if !docs.return_overload_rows.is_empty() {
                        let mut types = Vec::new();
                        let mut start = 0usize;
                        for len in &docs.return_overload_rows {
                            self.checkpoint()?;
                            let end = (start + len).min(docs.return_overloads.len());
                            for (_, syntax) in &docs.return_overloads[start..end] {
                                let ty =
                                    self.model
                                        .doc_type_lua_in(file, *syntax, &docs.generic_params);
                                if !ty.is_unknown() {
                                    types.push(ty);
                                }
                            }
                            start = end;
                        }
                        if !types.is_empty() {
                            return Ok(Some(OwnedValue::plain(LuaType::from_vec(types))));
                        }
                    }
                }
                // Literal-only return shells are complete without invoking the
                // semantic VM. The wider shell graph producer is implemented
                // separately, never substituted with selected inference.
                let mut count = 0usize;
                for _ in closure.descendants::<LuaReturnStat>() {
                    self.checkpoint()?;
                    count += 1;
                }
                let mut values = Vec::new();
                for row in closure.descendants::<LuaReturnStat>() {
                    for expr in row.get_expr_list() {
                        self.checkpoint()?;
                        let LuaExpr::LiteralExpr(literal) = expr else {
                            return self.continuation(
                                OwnedQueryCapability::FunctionReturnType,
                                Some(crate::session::BindingSite {
                                    file_id: file,
                                    syntax,
                                }),
                                None,
                                None,
                            );
                        };
                        let value = match literal.get_literal() {
                            Some(LuaLiteralToken::Nil(_)) => LuaType::Nil,
                            Some(LuaLiteralToken::Bool(_)) => LuaType::Boolean,
                            Some(LuaLiteralToken::String(_)) => LuaType::String,
                            Some(LuaLiteralToken::Number(token))
                                if count > 1
                                    && matches!(
                                        token.get_number_value(),
                                        emmylua_parser::NumberResult::Int(_)
                                            | emmylua_parser::NumberResult::Uint(_)
                                    ) =>
                            {
                                LuaType::Integer
                            }
                            Some(LuaLiteralToken::Number(_)) => LuaType::Number,
                            _ => LuaType::Unknown,
                        };
                        if !value.is_unknown() {
                            values.push(value);
                        }
                    }
                }
                if values.is_empty() && facts.member_by_value_syntax(syntax).is_some() {
                    return self.continuation(
                        OwnedQueryCapability::FunctionReturnType,
                        Some(crate::session::BindingSite {
                            file_id: file,
                            syntax,
                        }),
                        None,
                        None,
                    );
                }
                return Ok(Some(OwnedValue::plain(LuaType::from_vec(values))));
            }
            let Some(LuaExpr::NameExpr(alias)) = LuaExpr::cast(node) else {
                return Ok(None);
            };
            // Preserve the fixed model's current-file alias lookup quirk.
            let Some(next) = self.model.resolve_name(alias.get_position()) else {
                return Ok(None);
            };
            if !visited.insert(next.clone()) {
                return Ok(None);
            }
            declaration = next;
        }
    }
}
