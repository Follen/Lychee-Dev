use super::*;
use emmylua_parser::{BinaryOperator, UnaryOperator};
use rowan::{TextRange, TextSize};

enum ShellFrame {
    Call(LuaCallExpr, LuaExpr),
    Index(LuaIndexExpr),
    Paren,
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn shell_expression(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let key = (file, syntax);
        if !state.expressions.insert(key) {
            return Ok(TypeShell::unknown());
        }
        let expr = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast);
        let result = match expr {
            Some(expr) => self.shell_expr(file, expr, state),
            None => Ok(TypeShell::unknown()),
        };
        state.expressions.remove(&key);
        result
    }
    pub(super) fn shell_expr(
        &mut self,
        file: FileId,
        expr: LuaExpr,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        let mut current = expr;
        let mut frames = Vec::new();
        loop {
            self.checkpoint()?;
            match current {
                LuaExpr::CallExpr(call) => {
                    let Some(prefix) = call.get_prefix_expr() else {
                        return Ok(TypeShell::unknown());
                    };
                    frames.push(ShellFrame::Call(call, prefix.clone()));
                    current = prefix;
                }
                LuaExpr::IndexExpr(index) => {
                    let Some(prefix) = index.get_prefix_expr() else {
                        return Ok(TypeShell::unknown());
                    };
                    frames.push(ShellFrame::Index(index));
                    current = prefix;
                }
                LuaExpr::ParenExpr(paren) => {
                    let Some(inner) = paren.get_expr() else {
                        return Ok(TypeShell::unknown());
                    };
                    frames.push(ShellFrame::Paren);
                    current = inner;
                }
                _ => break,
            }
        }
        let mut shell = self.shell_expr_node(file, current, state)?;
        for frame in frames.into_iter().rev() {
            self.checkpoint()?;
            shell = match frame {
                ShellFrame::Call(call, prefix) => {
                    self.shell_call(file, &call, prefix, shell, state)?
                }
                ShellFrame::Index(index) => self.shell_index(file, &index, shell, state)?,
                ShellFrame::Paren => shell,
            };
        }
        Ok(shell)
    }
    fn shell_expr_node(
        &mut self,
        file: FileId,
        expr: LuaExpr,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        Ok(match expr {
            LuaExpr::LiteralExpr(literal) => match literal.get_literal() {
                Some(LuaLiteralToken::String(_)) => {
                    TypeShell::from_primitive(PrimitiveType::String)
                }
                Some(LuaLiteralToken::Number(n)) => match n.get_number_value() {
                    NumberResult::Float(f) => {
                        TypeShell::from_literal(LiteralShell::Float(f.to_bits()))
                    }
                    _ => TypeShell::from_primitive(PrimitiveType::Number),
                },
                Some(LuaLiteralToken::Bool(_)) => TypeShell::from_primitive(PrimitiveType::Boolean),
                Some(LuaLiteralToken::Nil(_)) => TypeShell::from_primitive(PrimitiveType::Nil),
                _ => TypeShell::unknown(),
            },
            LuaExpr::TableExpr(table) => {
                TypeShell::from_table(TableId::from_range(file, table.get_range()))
            }
            LuaExpr::ClosureExpr(_) => TypeShell::from_primitive(PrimitiveType::Function),
            LuaExpr::NameExpr(name) => {
                let Some(text) = name.get_name_text() else {
                    return Ok(TypeShell::unknown());
                };
                if text == "nil" {
                    return Ok(TypeShell::from_primitive(PrimitiveType::Nil));
                }
                if let Some(decl) = self.model.file_facts_of(file).and_then(|facts| {
                    facts.find_visible_decl_before_offset(&text, name.get_position())
                }) {
                    return self.shell_decl(file, &decl.id, state);
                }
                if let Some(decl) = self.model.global_decl(&text)
                    && let SemanticId::Decl(key) = &decl
                {
                    let shell = self.shell_decl(key.file_id, &decl, state)?;
                    if !shell.is_unknown() {
                        return Ok(shell);
                    }
                }
                if let Some(def) = self
                    .model
                    .type_defs_in_scope(TypeScope::Global, &text)
                    .first()
                {
                    TypeShell::from_name(&def.full_name)
                } else {
                    TypeShell::unknown()
                }
            }
            LuaExpr::BinaryExpr(binary) => match binary.get_op_token().map(|t| t.get_op()) {
                Some(BinaryOperator::OpOr | BinaryOperator::OpAnd) => {
                    if let Some((left, right)) = binary.get_exprs() {
                        let mut shell = self.shell_expr(file, left, state)?;
                        shell.merge(&self.shell_expr(file, right, state)?);
                        shell
                    } else {
                        TypeShell::unknown()
                    }
                }
                Some(BinaryOperator::OpConcat) => TypeShell::from_primitive(PrimitiveType::String),
                Some(
                    BinaryOperator::OpLt
                    | BinaryOperator::OpLe
                    | BinaryOperator::OpGt
                    | BinaryOperator::OpGe
                    | BinaryOperator::OpEq
                    | BinaryOperator::OpNe,
                ) => TypeShell::from_primitive(PrimitiveType::Boolean),
                Some(
                    BinaryOperator::OpAdd
                    | BinaryOperator::OpSub
                    | BinaryOperator::OpMul
                    | BinaryOperator::OpDiv
                    | BinaryOperator::OpIDiv
                    | BinaryOperator::OpMod
                    | BinaryOperator::OpPow,
                ) => TypeShell::from_primitive(PrimitiveType::Number),
                _ => TypeShell::unknown(),
            },
            LuaExpr::UnaryExpr(unary) => {
                if unary.get_op_token().map(|t| t.get_op()) == Some(UnaryOperator::OpNot) {
                    TypeShell::from_primitive(PrimitiveType::Boolean)
                } else if let Some(expr) = unary.get_expr() {
                    self.shell_expr(file, expr, state)?
                } else {
                    TypeShell::unknown()
                }
            }
            // Chain nodes are handled by the explicit stack above.
            _ => TypeShell::unknown(),
        })
    }
    fn shell_index(
        &mut self,
        file: FileId,
        index: &LuaIndexExpr,
        prefix: TypeShell,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let Some((owner, name)) = self.shell_member_reference(file, index)? else {
            return Ok(TypeShell::unknown());
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(TypeShell::unknown());
        };
        if let Some(member) = facts
            .members_of_owner(&owner)
            .find(|m| m.key.name() == Some(name.as_str()))
        {
            return self.shell_member(file, &member.id, state);
        }
        if let Some(shell) = self.shell_member_via_owner(&owner, &name, false, state)? {
            return Ok(shell);
        }
        for candidate in &prefix.candidates {
            self.checkpoint()?;
            match candidate {
                TypeCandidate::GenericInstance(instance) => {
                    if let Some(def) = self.model.resolve_type_def_in(file, &instance.name)
                        && let Some(shell) =
                            self.shell_member_via_owner(&def.id, &name, false, state)?
                    {
                        let substituted =
                            substitute_shell(&shell, &def.generic_params, &instance.args);
                        if !substituted.is_unknown() {
                            return Ok(substituted);
                        }
                    }
                }
                TypeCandidate::Table(table) => {
                    let owner = SemanticId::member(
                        FileId::new(table.file_id),
                        TextRange::new(TextSize::from(table.start), TextSize::from(table.end)),
                    );
                    if let Some(shell) = self.shell_member_via_owner(&owner, &name, false, state)? {
                        return Ok(shell);
                    }
                }
                TypeCandidate::Named(type_name) => {
                    let class_table=index.get_prefix_expr().is_some_and(|p|matches!(p,LuaExpr::NameExpr(n) if n.get_name_text().as_deref()==Some(type_name.as_str())));
                    let class = self
                        .model
                        .resolve_type_def_in(file, type_name)
                        .is_some_and(|d| d.kind == TypeDefKind::Class);
                    if let Some(shell) = self.shell_member_via_owner(
                        &SemanticId::name(type_name.clone()),
                        &name,
                        !class_table && class,
                        state,
                    )? {
                        return Ok(shell);
                    }
                    let def = facts
                        .type_def_by_name(type_name)
                        .or_else(|| facts.type_def_by_full_name(type_name));
                    if let Some(def) = def
                        && let Some(shell) =
                            self.shell_type_member(file, &def.id, &name, &mut Vec::new())?
                    {
                        return Ok(shell);
                    }
                }
                _ => {}
            }
        }
        Ok(TypeShell::unknown())
    }
    fn shell_call(
        &mut self,
        file: FileId,
        call: &LuaCallExpr,
        prefix: LuaExpr,
        prefix_shell: TypeShell,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        if call.is_require()
            && let Some(LuaExpr::LiteralExpr(literal)) =
                call.get_args_list().and_then(|a| a.get_args().next())
            && let Some(LuaLiteralToken::String(name)) = literal.get_literal()
            && let Some(module) = self.model.module_file_of(&name.get_value())
        {
            let shell = self.shell_module(module, state)?;
            if !shell.is_unknown() {
                return Ok(shell);
            }
        }
        if let Some(closure) = self.shell_callee_closure(file, prefix)? {
            return self.shell_signature_return(file, closure, state);
        }
        for candidate in prefix_shell.candidates {
            self.checkpoint()?;
            if let TypeCandidate::Function(fun) = candidate {
                return Ok(fun.returns.clone());
            }
        }
        Ok(TypeShell::unknown())
    }
    fn shell_callee_closure(
        &mut self,
        file: FileId,
        prefix: LuaExpr,
    ) -> QueryResult<Option<LuaSyntaxId>, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        Ok(match prefix {
            LuaExpr::NameExpr(name) => name
                .get_name_text()
                .and_then(|text| facts.find_visible_decl_before_offset(&text, name.get_position()))
                .and_then(|d| d.value_expr_syntax),
            LuaExpr::IndexExpr(index) => {
                let Some((owner, name)) = self.shell_member_reference(file, &index)? else {
                    return Ok(None);
                };
                facts
                    .members_of_owner(&owner)
                    .find(|m| m.key.name() == Some(name.as_str()))
                    .and_then(|m| m.value_syntax)
            }
            LuaExpr::ClosureExpr(closure) => Some(closure.get_syntax_id()),
            _ => None,
        })
    }
    fn shell_member_reference(
        &mut self,
        file: FileId,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<(SemanticId, String)>, E> {
        let Some(key) = index.get_index_key() else {
            return Ok(None);
        };
        let Some(mut current) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let mut segments = Vec::new();
        loop {
            self.checkpoint()?;
            match current {
                LuaExpr::ParenExpr(paren) => {
                    let Some(expr) = paren.get_expr() else {
                        return Ok(None);
                    };
                    current = expr;
                }
                LuaExpr::IndexExpr(parent) => {
                    let Some(key) = parent.get_index_key() else {
                        return Ok(None);
                    };
                    segments.push(key.get_path_part());
                    let Some(expr) = parent.get_prefix_expr() else {
                        return Ok(None);
                    };
                    current = expr;
                }
                _ => break,
            }
        }
        let LuaExpr::NameExpr(name) = current else {
            return Ok(None);
        };
        let Some(text) = name.get_name_text() else {
            return Ok(None);
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let owner = if text != "_ENV"
            && text != "_G"
            && let Some(decl) = facts.find_visible_decl_before_offset(&text, name.get_position())
            && !matches!(decl.kind, DeclKind::Global)
        {
            decl.id.clone()
        } else {
            SemanticId::name(text.into())
        };
        let owner = if let SemanticId::Name(root) = owner {
            let mut path = root.to_string();
            for segment in segments.into_iter().rev() {
                path.push('.');
                path.push_str(&segment);
            }
            SemanticId::name(path.into())
        } else {
            owner
        };
        Ok(Some((owner, key.get_path_part().to_string())))
    }
}
