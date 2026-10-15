use super::*;
use emmylua_code_analysis::{LuaMemberKey, TypeDefKind};
use emmylua_parser::{
    BinaryOperator, LuaAstPtr, LuaCallExprStat, LuaDocTagAs, LuaDocTagCast, LuaLiteralToken,
    UnaryOperator,
};
#[derive(Clone)]
pub(super) enum CastOp {
    Add(OwnedValue),
    Remove(OwnedValue),
    Replace(OwnedValue),
}
#[derive(Clone)]
pub(super) enum Narrowing {
    Replace(OwnedValue),
    Remove(OwnedValue),
    Truthy,
    Falsy,
    NotNil,
    Correlated {
        matching: Vec<OwnedValue>,
        candidates: Vec<OwnedValue>,
    },
    Primitive {
        primitive: LuaType,
        keep: bool,
    },
    Member {
        key: LuaMemberKey,
        literal: OwnedValue,
        keep: bool,
    },
    MemberTruthy {
        key: LuaMemberKey,
        keep: bool,
    },
    ArrayLen(i64),
    ArrayMinLen(i64),
}
#[derive(Clone, Default)]
pub(super) struct PathState {
    pub(super) narrowings: Vec<Narrowing>,
    pub(super) casts: Vec<CastOp>,
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn finalize_flow(
        &mut self,
        mut value: OwnedValue,
        path: &PathState,
    ) -> QueryResult<OwnedValue, E> {
        for narrowing in path.narrowings.iter().rev() {
            self.checkpoint()?;
            value = match narrowing {
                Narrowing::Replace(other) => other.clone(),
                Narrowing::Remove(other) => {
                    let out = self.flow_remove(value.clone(), other)?;
                    if out.structural_type().is_unknown()
                        && matches!(
                            value.structural_type(),
                            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_)
                        )
                    {
                        OwnedValue::plain(LuaType::Integer)
                    } else {
                        out
                    }
                }
                Narrowing::Truthy => self.flow_truthy(value, true)?,
                Narrowing::Falsy => self.flow_truthy(value, false)?,
                Narrowing::NotNil => self.flow_remove(value, &OwnedValue::plain(LuaType::Nil))?,
                Narrowing::Primitive { primitive, keep } => {
                    self.filter_flow_primitive(value, primitive, *keep)?
                }
                Narrowing::ArrayLen(max) | Narrowing::ArrayMinLen(max) => {
                    if let LuaType::Array(array) = value.structural_type() {
                        let len = if matches!(narrowing, Narrowing::ArrayLen(_)) {
                            emmylua_code_analysis::LuaArrayLen::Max(*max)
                        } else {
                            emmylua_code_analysis::LuaArrayLen::Min(*max)
                        };
                        OwnedValue::plain(LuaType::Array(Arc::new(
                            emmylua_code_analysis::LuaArrayType::new(array.get_base().clone(), len),
                        )))
                    } else {
                        value
                    }
                }
                _ => {
                    return self.continuation(
                        OwnedQueryCapability::DeclarationFlow,
                        None,
                        None,
                        None,
                    );
                }
            };
        }
        for cast in path.casts.iter().rev() {
            self.checkpoint()?;
            value = match cast {
                CastOp::Replace(other) => other.clone(),
                CastOp::Add(other) => self.flow_merge(value, other.clone())?,
                CastOp::Remove(other) => self.flow_remove(value, other)?,
            };
        }
        Ok(value)
    }
    fn filter_flow_primitive(
        &mut self,
        value: OwnedValue,
        primitive: &LuaType,
        keep: bool,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if matches!(value.structural_type(), LuaType::Any | LuaType::Unknown) {
            return Ok(if keep {
                OwnedValue::plain(primitive.clone())
            } else {
                value
            });
        }
        let mut kept = Vec::new();
        for component in algebra::components(value) {
            self.checkpoint()?;
            if let LuaType::Ref(id) | LuaType::Def(id) = component.structural_type()
                && let Some(def) = self.model.type_def_of(id)
                && def.kind == TypeDefKind::Alias
                && let Some(target) = self.model.alias_target(&def)
            {
                if !matches!(target, LuaType::Union(_) | LuaType::MultiLineUnion(_))
                    && self.flow_matches_primitive(&component, primitive)?
                {
                    kept.push(component);
                    continue;
                }
                let expanded =
                    self.filter_flow_primitive(OwnedValue::plain(target), primitive, keep)?;
                if !expanded.structural_type().is_unknown() {
                    kept.extend(algebra::components(expanded));
                }
                continue;
            }
            if self.flow_matches_primitive(&component, primitive)? == keep {
                kept.push(component);
            }
        }
        if kept.is_empty() {
            Ok(OwnedValue::plain(LuaType::Never))
        } else {
            Ok(algebra::from_components(kept))
        }
    }
    fn flow_matches_primitive(
        &mut self,
        value: &OwnedValue,
        primitive: &LuaType,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let ty = value.structural_type();
        if ty == primitive {
            return Ok(true);
        }
        let generic = match ty {
            LuaType::Ref(id) | LuaType::Def(id) if self.model.type_def_of(id).is_none() => {
                Some(id.get_name())
            }
            LuaType::TplRef(tpl) => Some(tpl.get_name()),
            _ => None,
        };
        if let Some(name) = generic
            && let Some(facts) = self.model.file_facts()
        {
            for (signature, param) in facts.generic_param_constraints(name) {
                self.checkpoint()?;
                if let (Some(syntax), Some(docs)) = (param.constraint, signature.docs.as_ref()) {
                    let target = self.model.doc_type_lua_in(
                        self.model.file_id(),
                        syntax,
                        &docs.generic_params,
                    );
                    return self.flow_matches_primitive(&OwnedValue::plain(target), primitive);
                }
            }
        }
        if matches!(
            (ty, primitive),
            (
                LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                LuaType::Number | LuaType::Integer
            ) | (
                LuaType::StringConst(_) | LuaType::DocStringConst(_),
                LuaType::String
            ) | (
                LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_),
                LuaType::Boolean
            )
        ) {
            return Ok(true);
        }
        if *primitive == LuaType::Table
            && let LuaType::Ref(id) | LuaType::Def(id) = ty
            && self
                .model
                .type_def_of(id)
                .is_some_and(|def| def.kind == TypeDefKind::Enum)
        {
            return Ok(false);
        }
        self.type_check(value, &OwnedValue::plain(primitive.clone()))
    }
    pub(super) fn collect_decl_cast(
        &mut self,
        declaration: &SemanticId,
        ptr: &LuaAstPtr<LuaDocTagCast>,
        path: &mut PathState,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(cast) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(());
        };
        let Some(LuaExpr::NameExpr(name)) = cast.get_key_expr() else {
            return Ok(());
        };
        if self.model.resolve_name(name.get_position()).as_ref() != Some(declaration) {
            return Ok(());
        }
        for op in cast.get_op_types() {
            self.checkpoint()?;
            let Some(doc) = op.get_type() else {
                if op
                    .get_op()
                    .is_some_and(|op| op.get_op() == BinaryOperator::OpSub)
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
            match op.get_op().map(|op| op.get_op()) {
                None => path.casts.push(CastOp::Replace(value)),
                Some(BinaryOperator::OpAdd) => path.casts.push(CastOp::Add(value)),
                Some(BinaryOperator::OpSub) => path.casts.push(CastOp::Remove(value)),
                _ => {}
            }
        }
        Ok(())
    }
    pub(super) fn collect_decl_as(
        &mut self,
        _declaration: &SemanticId,
        ptr: &LuaAstPtr<LuaDocTagAs>,
        path: &mut PathState,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if let Some(doc) = self
            .model
            .chunk()
            .and_then(|chunk| ptr.to_node(&chunk))
            .and_then(|cast| cast.get_type())
        {
            let value = self.model.doc_type_lua(doc.get_syntax_id());
            if !value.is_unknown() {
                path.casts.push(CastOp::Replace(OwnedValue::plain(value)));
            }
        }
        Ok(())
    }
    pub(super) fn apply_expr_casts(
        &mut self,
        syntax: LuaSyntaxId,
        mut value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        let Some(tree) = self.owned_flow_tree()? else {
            return Ok(value);
        };
        let mut flows = Vec::new();
        if let Some(id) = tree.get_flow_id(syntax) {
            flows.push(id);
        }
        if let Some(node) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
        {
            for ancestor in node.ancestors() {
                self.checkpoint()?;
                if let Some(id) = tree.get_flow_id(LuaSyntaxId::from_node(&ancestor))
                    && !flows.contains(&id)
                {
                    flows.push(id);
                }
            }
        }
        for id in flows {
            for effect in tree.get_flow_effects(id) {
                self.checkpoint()?;
                match effect {
                    FlowEffect::TagCast(ptr) => {
                        let Some(cast) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk))
                        else {
                            continue;
                        };
                        for op in cast.get_op_types() {
                            self.checkpoint()?;
                            let Some(doc) = op.get_type() else {
                                if op
                                    .get_op()
                                    .is_some_and(|op| op.get_op() == BinaryOperator::OpSub)
                                {
                                    value =
                                        self.flow_remove(value, &OwnedValue::plain(LuaType::Nil))?;
                                }
                                continue;
                            };
                            let target = OwnedValue::plain(if doc.get_text().trim() == "?" {
                                LuaType::Nil
                            } else {
                                self.model.doc_type_lua(doc.get_syntax_id())
                            });
                            value = match op.get_op().map(|op| op.get_op()) {
                                None => target,
                                Some(BinaryOperator::OpAdd) => self.flow_merge(value, target)?,
                                Some(BinaryOperator::OpSub) => self.flow_remove(value, &target)?,
                                _ => value,
                            };
                        }
                    }
                    FlowEffect::AsCast(ptr) => {
                        if let Some(doc) = self
                            .model
                            .chunk()
                            .and_then(|chunk| ptr.to_node(&chunk))
                            .and_then(|tag| tag.get_type())
                        {
                            value = OwnedValue::plain(self.model.doc_type_lua(doc.get_syntax_id()));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(value)
    }
    pub(super) fn decl_narrowing(
        &mut self,
        declaration: &SemanticId,
        tree: &FlowTree,
        ptr: &LuaAstPtr<LuaExpr>,
        flow: FlowId,
        truthy: bool,
        prior: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        self.checkpoint()?;
        let Some(expr) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(None);
        };
        if let Some(value) = self.direct_decl_narrowing(declaration, &expr, truthy, prior)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.array_len_narrowing(declaration, &expr, truthy)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.alias_narrowing(declaration, &expr, truthy, prior)? {
            return Ok(Some(value));
        }
        // The aliases and correlated multi-return stage must use this same CFG
        // and owned inference. Until that implementation lands, only a real
        // correlation/alias dependency leaves through the development boundary.
        let mut names = Vec::new();
        self.condition_names(&expr, &mut names)?;
        if names
            .iter()
            .any(|name| tree.has_shared_multi_return_refs(name, declaration))
        {
            return self.continuation(
                OwnedQueryCapability::DeclarationFlow,
                Some(self.site(ptr.get_syntax_id())),
                Some(flow.0),
                Some(declaration.clone()),
            );
        }
        Ok(None)
    }
    pub(super) fn direct_decl_narrowing(
        &mut self,
        declaration: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
        prior: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        self.checkpoint()?;
        let result = match expr {
            LuaExpr::NameExpr(name)
                if self.model.resolve_name(name.get_position()).as_ref() == Some(declaration) =>
            {
                Some(if truthy {
                    Narrowing::Truthy
                } else {
                    Narrowing::Falsy
                })
            }
            LuaExpr::UnaryExpr(unary) => {
                let (Some(op), Some(inner)) = (unary.get_op_token(), unary.get_expr()) else {
                    return Ok(None);
                };
                match (op.get_op(), inner) {
                    (UnaryOperator::OpNot, LuaExpr::NameExpr(name))
                        if self.model.resolve_name(name.get_position()).as_ref()
                            == Some(declaration) =>
                    {
                        Some(if truthy {
                            Narrowing::Falsy
                        } else {
                            Narrowing::Truthy
                        })
                    }
                    (UnaryOperator::OpLen, LuaExpr::NameExpr(name))
                        if truthy
                            && self.model.resolve_name(name.get_position()).as_ref()
                                == Some(declaration) =>
                    {
                        Some(Narrowing::Truthy)
                    }
                    (
                        UnaryOperator::OpNot,
                        inner @ (LuaExpr::CallExpr(_) | LuaExpr::IndexExpr(_)),
                    ) => return self.direct_decl_narrowing(declaration, &inner, !truthy, prior),
                    _ => None,
                }
            }
            LuaExpr::BinaryExpr(binary) => {
                let (Some(op), Some((left, right))) = (binary.get_op_token(), binary.get_exprs())
                else {
                    return Ok(None);
                };
                let op = op.get_op();
                let equals = (op == BinaryOperator::OpEq) == truthy;
                if let LuaExpr::CallExpr(call) = &left
                    && let Some(LuaExpr::NameExpr(callee)) = call.get_prefix_expr()
                    && callee.get_name_text().as_deref() == Some("type")
                    && let Some(LuaExpr::NameExpr(arg)) =
                        call.get_args_list().and_then(|list| list.get_args().next())
                    && self.model.resolve_name(arg.get_position()).as_ref() == Some(declaration)
                    && let LuaExpr::LiteralExpr(literal) = &right
                    && let Some(LuaLiteralToken::String(name)) = literal.get_literal()
                {
                    return Ok(primitive(&name.get_value()).map(|primitive| {
                        Narrowing::Primitive {
                            primitive,
                            keep: equals,
                        }
                    }));
                }
                let other = if self.is_decl_name(&left, declaration) {
                    Some(&right)
                } else if self.is_decl_name(&right, declaration) {
                    Some(&left)
                } else {
                    None
                };
                if let Some(other) = other {
                    if let LuaExpr::LiteralExpr(literal) = other
                        && let Some(value) = literal_type(literal)
                    {
                        if value.is_nil() && truthy {
                            return Ok(Some(if op == BinaryOperator::OpEq {
                                Narrowing::Replace(OwnedValue::plain(value))
                            } else {
                                Narrowing::NotNil
                            }));
                        }
                        if !matches!(op, BinaryOperator::OpEq | BinaryOperator::OpNe) {
                            return Ok(None);
                        }
                        return Ok(Some(if equals {
                            Narrowing::Replace(OwnedValue::plain(value))
                        } else if value.is_nil() {
                            Narrowing::NotNil
                        } else {
                            Narrowing::Remove(OwnedValue::plain(value))
                        }));
                    }
                    if matches!(op, BinaryOperator::OpEq | BinaryOperator::OpNe) {
                        let value = match other {
                            LuaExpr::NameExpr(name) => self.type_of_expr(name.get_syntax_id())?,
                            LuaExpr::IndexExpr(index) => self.type_of_expr_at(
                                index.get_syntax_id(),
                                index.get_range().start().into(),
                            )?,
                            _ => return Ok(None),
                        };
                        if !value.structural_type().is_unknown() {
                            if equals {
                                let value = if truthy
                                    && value.structural_type().is_nullable()
                                    && !self
                                        .declared_or_unknown(declaration)?
                                        .structural_type()
                                        .is_nullable()
                                {
                                    self.flow_remove(value, &OwnedValue::plain(LuaType::Nil))?
                                } else {
                                    value
                                };
                                return Ok(Some(Narrowing::Replace(value)));
                            }
                            if singleton(value.structural_type()) {
                                return Ok(Some(Narrowing::Remove(value)));
                            }
                        }
                    }
                }
                None
            }
            LuaExpr::CallExpr(call) => {
                let mut names = Vec::new();
                self.condition_names(expr, &mut names)?;
                if names.contains(declaration) {
                    return self.continuation(
                        OwnedQueryCapability::DeclarationFlow,
                        Some(self.site(call.get_syntax_id())),
                        None,
                        Some(declaration.clone()),
                    );
                }
                None
            }
            LuaExpr::IndexExpr(index) => {
                if index
                    .get_prefix_expr()
                    .as_ref()
                    .is_some_and(|prefix| self.is_decl_name(prefix, declaration))
                {
                    return self.continuation(
                        OwnedQueryCapability::DeclarationFlow,
                        Some(self.site(index.get_syntax_id())),
                        None,
                        Some(declaration.clone()),
                    );
                }
                None
            }
            _ => None,
        };
        Ok(result)
    }
    pub(super) fn is_decl_name(&self, expr: &LuaExpr, declaration: &SemanticId) -> bool {
        matches!(expr,LuaExpr::NameExpr(name) if self.model.resolve_name(name.get_position()).as_ref()==Some(declaration))
    }
    fn condition_names(&mut self, expr: &LuaExpr, out: &mut Vec<SemanticId>) -> QueryResult<(), E> {
        let mut stack = vec![expr.clone()];
        while let Some(expr) = stack.pop() {
            self.checkpoint()?;
            match expr {
                LuaExpr::NameExpr(name) => {
                    if let Some(decl) = self.model.resolve_name(name.get_position())
                        && !out.contains(&decl)
                    {
                        out.push(decl);
                    }
                }
                LuaExpr::UnaryExpr(unary) => {
                    if let Some(inner) = unary.get_expr() {
                        stack.push(inner);
                    }
                }
                LuaExpr::BinaryExpr(binary) => {
                    if let Some((left, right)) = binary.get_exprs() {
                        stack.push(right);
                        stack.push(left);
                    }
                }
                LuaExpr::CallExpr(call) => {
                    if let Some(args) = call.get_args_list() {
                        let args: Vec<_> = args.get_args().collect();
                        stack.extend(args.into_iter().rev());
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(super) fn pure_decl_join(
        &mut self,
        declaration: &SemanticId,
        tree: &FlowTree,
        antecedent: Option<&FlowAntecedent>,
        options: Options,
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
                    break None;
                }
                let Some(node) = tree.get_flow_node(current) else {
                    break None;
                };
                match node.antecedent.as_ref() {
                    Some(FlowAntecedent::Single(next)) => current = *next,
                    Some(FlowAntecedent::Multiple(_)) => break Some(current),
                    None => break None,
                }
            };
            let Some(candidate) = candidate else {
                return Ok(None);
            };
            if let Some(common) = common {
                if common != candidate {
                    return Ok(None);
                }
            } else {
                common = Some(candidate);
            }
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
                match &node.kind{
                FlowNodeKind::Assignment(_) if options.assignments=>{if tree.get_flow_effects(current).iter().any(|effect|matches!(effect,FlowEffect::AssignDecl{decl,..} if decl==declaration)){return Ok(None);}},
                FlowNodeKind::TrueCondition(ptr)|FlowNodeKind::FalseCondition(ptr) if options.guards=>{if let Some(expr)=self.model.chunk().and_then(|chunk|ptr.to_node(&chunk)){let mut names=Vec::new();self.condition_names(&expr,&mut names)?;if names.iter().any(|decl|decl==declaration||tree.has_shared_multi_return_refs(decl,declaration)){return Ok(None);}}},
                FlowNodeKind::CallExprStat(call)=>{if self.flow_call_never(call)?{return Ok(None);}},
                FlowNodeKind::TagCast(_)|FlowNodeKind::AsCast(_) if options.casts=>return Ok(None),_=>{}
            }
                match node.antecedent.as_ref() {
                    Some(FlowAntecedent::Single(next)) => current = *next,
                    _ => return Ok(None),
                }
            }
        }
        Ok(Some(common))
    }
    pub(super) fn flow_call_never(
        &mut self,
        ptr: &LuaAstPtr<LuaCallExprStat>,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let Some(call) = self
            .model
            .chunk()
            .and_then(|chunk| ptr.to_node(&chunk))
            .and_then(|stat| stat.get_call_expr())
        else {
            return Ok(false);
        };
        Ok(self
            .flow_signature_return(&call)?
            .is_some_and(|v| v.structural_type().is_never()))
    }
    pub(super) fn flow_condition_unreachable(
        &mut self,
        ptr: &LuaAstPtr<LuaExpr>,
        truthy: bool,
    ) -> QueryResult<bool, E> {
        let Some(expr) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk)) else {
            return Ok(false);
        };
        let mut current = expr;
        let mut truthy = truthy;
        loop {
            self.checkpoint()?;
            match current {
                LuaExpr::ParenExpr(paren) => {
                    let Some(inner) = paren.get_expr() else {
                        return Ok(false);
                    };
                    current = inner;
                }
                LuaExpr::UnaryExpr(unary) => {
                    if !unary
                        .get_op_token()
                        .is_some_and(|op| op.get_op() == UnaryOperator::OpNot)
                    {
                        return Ok(false);
                    }
                    let Some(inner) = unary.get_expr() else {
                        return Ok(false);
                    };
                    current = inner;
                    truthy = !truthy;
                }
                LuaExpr::CallExpr(call) => {
                    return Ok(self.flow_signature_return(&call)?.is_some_and(|value| {
                        value.structural_type().is_never()
                            || if truthy {
                                value.structural_type().is_always_falsy()
                            } else {
                                value.structural_type().is_always_truthy()
                            }
                    }));
                }
                _ => return Ok(false),
            }
        }
    }
    pub(super) fn branch_reachable(
        &mut self,
        tree: &FlowTree,
        start: Option<&FlowAntecedent>,
    ) -> QueryResult<bool, E> {
        let mut stack = vec![start.cloned()];
        let mut visited = HashSet::new();
        while let Some(antecedent) = stack.pop() {
            self.checkpoint()?;
            match antecedent {
                None => return Ok(true),
                Some(FlowAntecedent::Single(flow)) => {
                    if !visited.insert(flow) {
                        return Ok(true);
                    }
                    let Some(node) = tree.get_flow_node(flow) else {
                        return Ok(false);
                    };
                    match &node.kind {
                        FlowNodeKind::Unreachable
                        | FlowNodeKind::Return
                        | FlowNodeKind::Break
                        | FlowNodeKind::Continue => continue,
                        FlowNodeKind::TrueCondition(ptr) => {
                            if self.flow_condition_unreachable(ptr, true)? {
                                continue;
                            }
                        }
                        FlowNodeKind::FalseCondition(ptr) => {
                            if self.flow_condition_unreachable(ptr, false)? {
                                continue;
                            }
                        }
                        FlowNodeKind::CallExprStat(ptr) => {
                            if self.flow_call_never(ptr)? {
                                continue;
                            }
                        }
                        _ => {}
                    }
                    match &node.antecedent {
                        None => return Ok(true),
                        Some(next) => stack.push(Some(next.clone())),
                    }
                }
                Some(FlowAntecedent::Multiple(index)) => {
                    let Some(branches) = tree.get_multi_antecedents(index) else {
                        return Ok(false);
                    };
                    for branch in branches {
                        stack.push(Some(FlowAntecedent::Single(*branch)));
                    }
                }
            }
        }
        Ok(false)
    }
}
pub(super) fn primitive(name: &str) -> Option<LuaType> {
    Some(match name {
        "nil" => LuaType::Nil,
        "boolean" => LuaType::Boolean,
        "number" => LuaType::Number,
        "integer" => LuaType::Integer,
        "string" => LuaType::String,
        "table" => LuaType::Table,
        "function" => LuaType::Function,
        "userdata" => LuaType::Userdata,
        "thread" => LuaType::Thread,
        _ => return None,
    })
}
pub(super) fn literal_type(lit: &emmylua_parser::LuaLiteralExpr) -> Option<LuaType> {
    match lit.get_literal()? {
        LuaLiteralToken::Nil(_) => Some(LuaType::Nil),
        LuaLiteralToken::Bool(token) => Some(LuaType::BooleanConst(token.is_true())),
        LuaLiteralToken::String(token) => {
            Some(match LuaMemberKey::Name(token.get_value().into()) {
                LuaMemberKey::Name(value) => LuaType::StringConst(value.into()),
                _ => unreachable!("constructed named key"),
            })
        }
        LuaLiteralToken::Number(token) => Some(match token.get_number_value() {
            emmylua_parser::NumberResult::Int(v) => LuaType::IntegerConst(v),
            _ => LuaType::Number,
        }),
        _ => None,
    }
}
fn singleton(ty: &LuaType) -> bool {
    matches!(
        ty,
        LuaType::Nil
            | LuaType::StringConst(_)
            | LuaType::DocStringConst(_)
            | LuaType::IntegerConst(_)
            | LuaType::DocIntegerConst(_)
            | LuaType::BooleanConst(_)
            | LuaType::DocBooleanConst(_)
    )
}
