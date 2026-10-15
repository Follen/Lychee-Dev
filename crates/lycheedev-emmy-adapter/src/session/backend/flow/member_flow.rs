//! Flow-sensitive member values, assignment skipping and branch merging.
use super::*;
use emmylua_parser::{BinaryOperator, LuaAstPtr, LuaDocTagCast, LuaLiteralToken};

impl<E> OwnedQuery<'_, '_, E> {
    fn member_file(&self, member: &SemanticId) -> emmylua_code_analysis::FileId {
        match member {
            SemanticId::Member(key) => key.file_id,
            _ => self.model.file_id(),
        }
    }
    pub(super) fn flow_member_value(
        &mut self,
        member: &SemanticId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let file = self.member_file(member);
        let Some(def) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.member_by_id(member))
        else {
            return Ok(None);
        };
        if matches!(def.owner, SemanticId::Member(_)) {
            let Some(syntax) = def.value_syntax else {
                return Ok(None);
            };
            let is_name = self
                .model
                .syntax_tree_of(file)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                .and_then(LuaExpr::cast)
                .is_some_and(|expr| matches!(expr, LuaExpr::NameExpr(_)));
            if is_name {
                // The fixed producer queries in its current model, including
                // foreign field syntax. Do not silently create another scope.
                let value = self.type_of_expr_at(syntax, syntax.get_range().start().into())?;
                if !value.structural_type().is_unknown() {
                    return self.flow_field_nullable(member, value).map(Some);
                }
            }
        }
        match self.type_of_member(member)? {
            Some(value) => self.flow_field_nullable(member, value).map(Some),
            None => Ok(None),
        }
    }
    fn flow_field_nullable(
        &mut self,
        member: &SemanticId,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if self
            .model
            .file_facts_of(self.member_file(member))
            .and_then(|facts| facts.member_by_id(member))
            .is_some_and(|def| def.is_nullable)
            && !value.structural_type().is_nullable()
        {
            self.flow_merge(value, OwnedValue::plain(LuaType::Nil))
        } else {
            Ok(value)
        }
    }
    fn flow_member_fallback(&mut self, member: &SemanticId) -> QueryResult<OwnedValue, E> {
        Ok(self
            .flow_member_value(member)?
            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)))
    }
    pub fn type_of_member_at(
        &mut self,
        member: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        let Some(tree) = self.owned_flow_tree()? else {
            return self.flow_member_fallback(member);
        };
        let offset = offset.into();
        let Some(flow) = tree.get_flow_id_at(offset) else {
            return self.flow_member_fallback(member);
        };
        let start = skip_member_assignment(member, &tree, flow, offset);
        if start == flow && !tree.has_complex_control() {
            let latest = tree.latest_member_assignment(member, offset);
            if let Some((position, assignment)) = latest
                && !tree.has_flow_event_between(position, offset, false)
            {
                if let Some(value) = self.trace_member_flow(
                    member,
                    &tree,
                    assignment,
                    &mut HashSet::new(),
                    &mut PathState::default(),
                )? {
                    return Ok(value);
                }
            } else if latest.is_none() && !tree.has_condition() && !tree.has_tag_cast() {
                return self.flow_member_fallback(member);
            }
        }
        match self.trace_member_flow(
            member,
            &tree,
            start,
            &mut HashSet::new(),
            &mut PathState::default(),
        )? {
            Some(value) => Ok(value),
            None => self.flow_member_fallback(member),
        }
    }
    pub fn type_of_member_before_at(
        &mut self,
        member: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        let Some(tree) = self.owned_flow_tree()? else {
            return self.flow_member_fallback(member);
        };
        let Some(node) = tree
            .get_flow_id_at(offset.into())
            .and_then(|flow| tree.get_flow_node(flow))
        else {
            return self.flow_member_fallback(member);
        };
        match self.walk_member_flow(
            member,
            &tree,
            node.antecedent.as_ref(),
            &mut HashSet::new(),
            &mut PathState::default(),
        )? {
            Some(value) => Ok(value),
            None => self.flow_member_fallback(member),
        }
    }
    fn trace_member_flow(
        &mut self,
        member: &SemanticId,
        tree: &FlowTree,
        mut flow: FlowId,
        visited: &mut HashSet<FlowId>,
        path: &mut PathState,
    ) -> QueryResult<Option<OwnedValue>, E> {
        loop {
            self.checkpoint()?;
            if !visited.insert(flow) {
                return Ok(None);
            }
            let Some(node) = tree.get_flow_node(flow) else {
                return Ok(None);
            };
            match &node.kind {
                FlowNodeKind::Assignment(_) => {
                    for effect in tree.get_flow_effects(flow) {
                        self.checkpoint()?;
                        if let FlowEffect::AssignMember {
                            member: assigned,
                            value_syntax,
                            ..
                        } = effect
                            && assigned == member
                        {
                            let assignment = PathState {
                                narrowings: Vec::new(),
                                casts: path.casts.clone(),
                            };
                            let mut value = self.type_of_expr(*value_syntax)?;
                            let file = self.member_file(member);
                            if let Some(syntax) = self
                                .model
                                .file_facts_of(file)
                                .and_then(|facts| facts.member_by_id(member))
                                .and_then(|member| member.doc_type_syntax)
                            {
                                let annotation = OwnedValue::plain(self.model.doc_type_lua_in(
                                    file,
                                    syntax,
                                    &[],
                                ));
                                value = if self.type_check(&annotation, &value)? {
                                    self.widen_flow_const(value)?
                                } else {
                                    annotation
                                };
                            }
                            return self.finalize_flow(value, &assignment).map(Some);
                        }
                    }
                }
                FlowNodeKind::TagCast(ptr) => self.collect_member_flow_cast(member, ptr, path)?,
                FlowNodeKind::AsCast(ptr) => self.collect_decl_as(member, ptr, path)?,
                FlowNodeKind::TrueCondition(ptr) | FlowNodeKind::FalseCondition(ptr) => {
                    if let Some(narrowing) = self.member_flow_narrowing(
                        member,
                        ptr,
                        matches!(node.kind, FlowNodeKind::TrueCondition(_)),
                    )? {
                        path.narrowings.push(narrowing);
                    }
                }
                FlowNodeKind::Start | FlowNodeKind::Unreachable => {
                    return self.final_member_base(member, path);
                }
                _ => {}
            }
            match node.antecedent.as_ref() {
                None => return self.final_member_base(member, path),
                Some(FlowAntecedent::Single(next)) => flow = *next,
                Some(FlowAntecedent::Multiple(_)) => {
                    if let Some(common) =
                        self.pure_member_join(member, tree, node.antecedent.as_ref())?
                    {
                        flow = common;
                    } else {
                        return self.walk_member_flow(
                            member,
                            tree,
                            node.antecedent.as_ref(),
                            visited,
                            path,
                        );
                    }
                }
            }
        }
    }
    fn final_member_base(
        &mut self,
        member: &SemanticId,
        path: &PathState,
    ) -> QueryResult<Option<OwnedValue>, E> {
        match self.flow_member_value(member)? {
            Some(value) => self.finalize_flow(value, path).map(Some),
            None => Ok(None),
        }
    }
    fn walk_member_flow(
        &mut self,
        member: &SemanticId,
        tree: &FlowTree,
        antecedent: Option<&FlowAntecedent>,
        visited: &mut HashSet<FlowId>,
        path: &mut PathState,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match antecedent {
            None => self.final_member_base(member, path),
            Some(FlowAntecedent::Single(next)) => {
                self.trace_member_flow(member, tree, *next, visited, path)
            }
            Some(FlowAntecedent::Multiple(index)) => {
                let Some(branches) = tree.get_multi_antecedents(*index) else {
                    return Ok(None);
                };
                if let Some(common) = self.pure_member_join(member, tree, antecedent)? {
                    return self.trace_member_flow(member, tree, common, visited, path);
                }
                let mut merged = OwnedValue::plain(LuaType::Unknown);
                for branch in branches {
                    self.checkpoint()?;
                    if let Some(value) = self.trace_member_flow(
                        member,
                        tree,
                        *branch,
                        &mut visited.clone(),
                        &mut path.clone(),
                    )? {
                        merged = self.flow_merge(merged, value)?;
                    }
                }
                Ok(Some(merged))
            }
        }
    }
    fn pure_member_join(
        &mut self,
        member: &SemanticId,
        tree: &FlowTree,
        antecedent: Option<&FlowAntecedent>,
    ) -> QueryResult<Option<FlowId>, E> {
        let Some(FlowAntecedent::Multiple(index)) = antecedent else {
            return Ok(None);
        };
        let Some(branches) = tree.get_multi_antecedents(*index) else {
            return Ok(None);
        };
        let mut common = None;
        for branch in branches {
            let mut current = *branch;
            let mut visited = HashSet::new();
            let candidate = loop {
                self.checkpoint()?;
                if !visited.insert(current) {
                    return Ok(None);
                }
                let Some(node) = tree.get_flow_node(current) else {
                    return Ok(None);
                };
                match node.antecedent.as_ref() {
                    Some(FlowAntecedent::Single(next)) => current = *next,
                    Some(FlowAntecedent::Multiple(_)) => break current,
                    None => return Ok(None),
                }
            };
            if common.is_some_and(|common| common != candidate) {
                return Ok(None);
            }
            common = Some(candidate);
        }
        let Some(common) = common else {
            return Ok(None);
        };
        for branch in branches {
            let mut current = *branch;
            let mut visited = HashSet::new();
            while current != common {
                self.checkpoint()?;
                if !visited.insert(current) {
                    return Ok(None);
                }
                let Some(node) = tree.get_flow_node(current) else {
                    return Ok(None);
                };
                match &node.kind {
                    FlowNodeKind::Assignment(_) => {
                        for effect in tree.get_flow_effects(current) {
                            self.checkpoint()?;
                            if matches!(effect, FlowEffect::AssignMember { member: assigned, .. } if assigned == member)
                            {
                                return Ok(None);
                            }
                        }
                    }
                    FlowNodeKind::TagCast(_) | FlowNodeKind::AsCast(_) => return Ok(None),
                    _ => {}
                }
                match node.antecedent.as_ref() {
                    Some(FlowAntecedent::Single(next)) => current = *next,
                    _ => return Ok(None),
                }
            }
        }
        Ok(Some(common))
    }
    fn collect_member_flow_cast(
        &mut self,
        member: &SemanticId,
        ptr: &LuaAstPtr<LuaDocTagCast>,
        path: &mut PathState,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(cast) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(());
        };
        let Some(LuaExpr::IndexExpr(index)) = cast.get_key_expr() else {
            return Ok(());
        };
        if self
            .resolve_owned_member(&index)?
            .and_then(|resolved| resolved.member_id)
            .as_ref()
            != Some(member)
        {
            return Ok(());
        }
        for op in cast.get_op_types() {
            self.checkpoint()?;
            let Some(doc) = op.get_type() else {
                if op
                    .get_op()
                    .is_some_and(|token| token.get_op() == BinaryOperator::OpSub)
                {
                    path.casts
                        .push(CastOp::Remove(OwnedValue::plain(LuaType::Nil)));
                }
                continue;
            };
            let value = if doc.get_text().trim() == "?" {
                LuaType::Nil
            } else {
                self.model.doc_type_lua(doc.get_syntax_id())
            };
            if value.is_unknown() {
                continue;
            }
            let value = OwnedValue::plain(value);
            match op.get_op().map(|token| token.get_op()) {
                None => path.casts.push(CastOp::Replace(value)),
                Some(BinaryOperator::OpAdd) => path.casts.push(CastOp::Add(value)),
                Some(BinaryOperator::OpSub) => path.casts.push(CastOp::Remove(value)),
                _ => {}
            }
        }
        Ok(())
    }
    fn member_flow_narrowing(
        &mut self,
        member: &SemanticId,
        ptr: &LuaAstPtr<LuaExpr>,
        truthy: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        let Some(expr) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(None);
        };
        if let LuaExpr::CallExpr(call) = &expr {
            if let Some((LuaExpr::IndexExpr(index), guard)) = self.type_guard_target(call)?
                && self
                    .resolve_owned_member(&index)?
                    .and_then(|resolved| resolved.member_id)
                    .as_ref()
                    == Some(member)
            {
                return Ok(Some(Narrowing::Replace(guard)));
            }
            if truthy {
                let args = call
                    .get_args_list()
                    .map(|list| list.get_args().collect::<Vec<_>>())
                    .unwrap_or_default();
                if let (Some(LuaExpr::NameExpr(receiver)), Some(LuaExpr::LiteralExpr(key))) =
                    (args.first(), args.get(1))
                    && let Some(declaration) = self.model.resolve_name(receiver.get_position())
                    && let Some(LuaLiteralToken::String(key)) = key.get_literal()
                    && self
                        .model
                        .file_facts_of(self.member_file(member))
                        .and_then(|facts| facts.member_by_id(member))
                        .is_some_and(|member| member.key.name() == Some(key.get_value().as_str()))
                {
                    let value = self
                        .type_of_decl(&declaration)?
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                    if !matches!(value.structural_type(), LuaType::Unknown | LuaType::Any)
                        && self
                            .member_type(
                                &value,
                                &emmylua_code_analysis::LuaMemberKey::Name(key.get_value().into()),
                            )?
                            .is_some()
                    {
                        return Ok(Some(Narrowing::NotNil));
                    }
                }
            }
        }
        if truthy {
            if let LuaExpr::IndexExpr(index) = &expr
                && self
                    .resolve_owned_member(index)?
                    .and_then(|resolved| resolved.member_id)
                    .as_ref()
                    == Some(member)
            {
                return Ok(Some(Narrowing::Truthy));
            }
            if let LuaExpr::NameExpr(name) = &expr
                && let Some(initializer) = self.local_initializer(name)
                && !matches!(initializer, LuaExpr::NameExpr(_))
                && let Some(narrowing) =
                    self.direct_member_flow_narrowing(member, &initializer, true)?
            {
                return Ok(Some(narrowing));
            }
        }
        self.direct_member_flow_narrowing(member, &expr, truthy)
    }
    fn direct_member_flow_narrowing(
        &mut self,
        member: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        let LuaExpr::BinaryExpr(binary) = expr else {
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
        let (index, literal) = match (left, right) {
            (LuaExpr::IndexExpr(index), LuaExpr::LiteralExpr(literal))
            | (LuaExpr::LiteralExpr(literal), LuaExpr::IndexExpr(index)) => (index, literal),
            _ => return Ok(None),
        };
        if self
            .resolve_owned_member(&index)?
            .and_then(|resolved| resolved.member_id)
            .as_ref()
            != Some(member)
        {
            return Ok(None);
        }
        Ok(guards::literal_type(&literal).map(|literal| {
            if (op == BinaryOperator::OpEq) == truthy {
                Narrowing::Replace(OwnedValue::plain(literal))
            } else {
                Narrowing::Remove(OwnedValue::plain(literal))
            }
        }))
    }
}
fn skip_member_assignment(
    member: &SemanticId,
    tree: &FlowTree,
    flow: FlowId,
    offset: TextSize,
) -> FlowId {
    let Some(node) = tree.get_flow_node(flow) else {
        return flow;
    };
    let FlowNodeKind::Assignment(ptr) = &node.kind else {
        return flow;
    };
    if ptr.get_syntax_id().get_range().contains(offset)
        && tree.get_flow_effects(flow).iter().any(|effect| matches!(effect, FlowEffect::AssignMember { member: assigned, .. } if assigned == member))
        && let Some(next) = first_antecedent(tree, node.antecedent.as_ref())
    { next } else { flow }
}
