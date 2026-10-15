//! Owned flow-sensitive queries. CFG topology is built lazily from public facts
//! and shared only inside this immutable analysis/session borrow. Values never
//! leave their selected carrier while crossing an assignment, guard or join.
mod algebra;
mod assignments;
mod condition_alias;
mod correlation;
mod discriminants;
mod guards;
mod index_flow;
mod member_flow;
mod return_cast;
mod signature_return;
pub(super) use return_cast::ReturnCast;
mod type_guard;
use super::{OwnedQuery, OwnedQueryCapability, OwnedQueryFailure, QueryResult, cfg};
use crate::session::{OwnedValue, SelectedProjectionBackend, SelectedProjector};
use cfg::{FlowAntecedent, FlowEffect, FlowId, FlowNodeKind, FlowTree};
use emmylua_code_analysis::{DeclKind, LuaType, SemanticId};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaSyntaxId};
use guards::{CastOp, Narrowing, PathState};
use rowan::TextSize;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy)]
struct Options {
    guards: bool,
    casts: bool,
    assignments: bool,
}
impl Options {
    const READ: Self = Self {
        guards: true,
        casts: true,
        assignments: true,
    };
    const TARGET: Self = Self {
        guards: false,
        casts: true,
        assignments: false,
    };
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Point,
    Branch,
}
struct AssignmentFrame {
    antecedent: FlowAntecedent,
    flow_id: FlowId,
    position: TextSize,
    value: LuaSyntaxId,
    path: PathState,
    base: OwnedValue,
}
impl<E> OwnedQuery<'_, '_, E> {
    fn owned_flow_tree(&mut self) -> QueryResult<Option<Arc<FlowTree>>, E> {
        self.checkpoint()?;
        let file = self.model.file_id();
        let cached = self.flow_trees.borrow().get(&file).cloned();
        if let Some(tree) = cached {
            self.checkpoint()?;
            return Ok(Some(tree));
        }
        let (Some(facts), Some(chunk)) = (self.model.file_facts(), self.model.chunk()) else {
            return Ok(None);
        };
        let tree = Arc::new(
            cfg::build(file, facts, chunk, &mut *self.check).map_err(OwnedQueryFailure::Control)?,
        );
        self.checkpoint()?;
        self.flow_trees.borrow_mut().insert(file, Arc::clone(&tree));
        Ok(Some(tree))
    }
    fn declared_or_unknown(&mut self, declaration: &SemanticId) -> QueryResult<OwnedValue, E> {
        Ok(self
            .type_of_decl(declaration)?
            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)))
    }
    fn sanitize_global(
        &mut self,
        declaration: &SemanticId,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let SemanticId::Decl(key) = declaration else {
            return Ok(value);
        };
        let Some(facts) = self.model.file_facts_of(key.file_id) else {
            return Ok(value);
        };
        if !facts
            .decl_by_id(declaration)
            .is_some_and(|decl| decl.kind == DeclKind::Global)
        {
            return Ok(value);
        }
        if let LuaType::Ref(id) = value.structural_type() {
            for signature in &facts.signatures {
                self.checkpoint()?;
                if signature.docs.as_ref().is_some_and(|docs| {
                    docs.generic_params
                        .iter()
                        .any(|param| param.name.as_str() == id.get_name())
                }) {
                    return Ok(OwnedValue::plain(LuaType::Unknown));
                }
            }
        }
        Ok(value)
    }
    pub fn type_of_decl_at(
        &mut self,
        declaration: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let Some(tree) = self.owned_flow_tree()? else {
            let value = self.declared_or_unknown(declaration)?;
            return self.sanitize_global(declaration, value);
        };
        let offset: TextSize = offset.into();
        let Some(flow) = tree.get_flow_id_at(offset) else {
            let value = self.declared_or_unknown(declaration)?;
            return self.sanitize_global(declaration, value);
        };
        let start = skip_own_assignment(declaration, &tree, flow, offset);
        let key = (self.model.file_id(), declaration.clone(), start);
        let cached = self.flow_declarations.get(&key).cloned();
        if let Some(value) = cached {
            return self.sanitize_global(declaration, value);
        }
        let direct = match declaration {
            SemanticId::Decl(key) if key.file_id == self.model.file_id() => self
                .model
                .file_facts()
                .and_then(|facts| facts.decl_by_id(declaration))
                .and_then(|decl| decl.owner_syntax)
                .is_some_and(|owner| {
                    tree.latest_decl_assignment(declaration, offset).is_none()
                        && !tree.has_flow_event_between(owner.get_range().start(), offset, true)
                }),
            _ => false,
        };
        let value = if direct {
            self.declared_or_unknown(declaration)?
        } else {
            match self.trace_decl(
                declaration,
                &tree,
                start,
                Options::READ,
                Mode::Point,
                &mut HashSet::new(),
                &mut PathState::default(),
            )? {
                Some(value) => value,
                None => self.declared_or_unknown(declaration)?,
            }
        };
        self.checkpoint()?;
        self.flow_declarations.insert(key, value.clone());
        self.sanitize_global(declaration, value)
    }
    pub fn type_of_decl_assign_target_at(
        &mut self,
        declaration: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        let Some(tree) = self.owned_flow_tree()? else {
            return self.declared_or_unknown(declaration);
        };
        if !tree.has_tag_cast() {
            return self.declared_or_unknown(declaration);
        }
        let Some(flow) = tree.get_flow_id_at(offset.into()) else {
            return self.declared_or_unknown(declaration);
        };
        let start = skip_own_assignment(declaration, &tree, flow, offset.into());
        match self.trace_decl(
            declaration,
            &tree,
            start,
            Options::TARGET,
            Mode::Point,
            &mut HashSet::new(),
            &mut PathState::default(),
        )? {
            Some(value) => Ok(value),
            None => self.declared_or_unknown(declaration),
        }
    }
    pub fn type_of_decl_before_at(
        &mut self,
        declaration: &SemanticId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        let Some(tree) = self.owned_flow_tree()? else {
            return self.declared_or_unknown(declaration);
        };
        let Some(flow) = tree.get_flow_id_at(offset.into()) else {
            return self.declared_or_unknown(declaration);
        };
        let Some(node) = tree.get_flow_node(flow) else {
            return self.declared_or_unknown(declaration);
        };
        match self.walk_decl(
            declaration,
            &tree,
            node.antecedent.as_ref(),
            Options::READ,
            Mode::Point,
            &mut HashSet::new(),
            &mut PathState::default(),
        )? {
            Some(value) => Ok(value),
            None => self.declared_or_unknown(declaration),
        }
    }
    fn trace_decl(
        &mut self,
        declaration: &SemanticId,
        tree: &FlowTree,
        mut current: FlowId,
        options: Options,
        mut mode: Mode,
        visited: &mut HashSet<FlowId>,
        path: &mut PathState,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let root_mode = mode;
        let mut frames = Vec::new();
        loop {
            self.checkpoint()?;
            if !visited.insert(current) {
                return Ok(None);
            }
            let Some(node) = tree.get_flow_node(current) else {
                return Ok(None);
            };
            match &node.kind {
                FlowNodeKind::Assignment(assign) if options.assignments => {
                    let position = assign.get_syntax_id().get_range().start();
                    let assigned =
                        tree.get_flow_effects(current)
                            .iter()
                            .find_map(|effect| match effect {
                                FlowEffect::AssignDecl { decl, value_syntax }
                                    if decl == declaration =>
                                {
                                    Some(*value_syntax)
                                }
                                _ => None,
                            });
                    if let Some(value) = assigned {
                        let is_multi = tree
                            .get_decl_multi_return_ref_on_flow(declaration, position, current)
                            .and_then(|at| at.reference.as_ref())
                            .is_some();
                        let mut assign_path = path.clone();
                        if !is_multi {
                            assign_path
                                .narrowings
                                .retain(|n| !matches!(n, Narrowing::Correlated { .. }));
                        }
                        let base = self.declared_or_unknown(declaration)?;
                        if let Some(FlowAntecedent::Single(next)) = node.antecedent.as_ref() {
                            frames.push(AssignmentFrame {
                                antecedent: FlowAntecedent::Single(*next),
                                flow_id: current,
                                position,
                                value,
                                path: assign_path,
                                base,
                            });
                            *path = PathState::default();
                            mode = Mode::Point;
                            current = *next;
                            continue;
                        }
                        let before = self.walk_decl(
                            declaration,
                            tree,
                            node.antecedent.as_ref(),
                            Options::READ,
                            Mode::Point,
                            &mut visited.clone(),
                            &mut PathState::default(),
                        )?;
                        let before = match before {
                            Some(v) => v,
                            None => self.finalize_flow(base.clone(), path)?,
                        };
                        let value_ty = self.assigned_value(
                            declaration,
                            tree,
                            current,
                            position,
                            value,
                            &before,
                        )?;
                        let value_ty =
                            self.coerce_flow_assignment(declaration, value_ty, &before, &base)?;
                        let value_ty = if value_ty.structural_type().is_unknown()
                            && !matches!(before.structural_type(), LuaType::Unknown | LuaType::Any)
                        {
                            self.widen_flow_const(before.clone())?
                        } else {
                            value_ty
                        };
                        let result = self.finalize_flow(value_ty, &assign_path)?;
                        if mode == Mode::Branch
                            && !self.branch_reachable(tree, node.antecedent.as_ref())?
                        {
                            return Ok(Some(if before.structural_type().is_never() {
                                before
                            } else {
                                self.widen_flow_const(before)?
                            }));
                        }
                        return Ok(Some(result));
                    }
                }
                FlowNodeKind::CallExprStat(call) if mode == Mode::Branch => {
                    if self.flow_call_never(call)? {
                        return Ok(Some(OwnedValue::plain(LuaType::Never)));
                    }
                }
                FlowNodeKind::DeclPosition(position) => {
                    if self
                        .model
                        .file_facts()
                        .and_then(|facts| facts.decl_by_id(declaration))
                        .and_then(|decl| decl.owner_syntax)
                        .map(|owner| owner.get_range().start())
                        == Some(*position)
                    {
                        let base = self.declared_or_unknown(declaration)?;
                        let base = self.finalize_flow(base, path)?;
                        return self.finish_decl_frames(declaration, tree, base, frames, root_mode);
                    }
                }
                FlowNodeKind::TrueCondition(condition)
                | FlowNodeKind::FalseCondition(condition) => {
                    let truthy = matches!(node.kind, FlowNodeKind::TrueCondition(_));
                    if mode == Mode::Branch && self.flow_condition_unreachable(condition, truthy)? {
                        return Ok(Some(OwnedValue::plain(LuaType::Never)));
                    }
                    if options.guards {
                        let prior = antecedent_has_narrowing(tree, node.antecedent.clone());
                        if let Some(narrowing) = self.decl_narrowing(
                            declaration,
                            tree,
                            condition,
                            current,
                            truthy,
                            prior,
                        )? {
                            path.narrowings.push(narrowing);
                        }
                    }
                }
                FlowNodeKind::TagCast(cast) if options.casts => {
                    self.collect_decl_cast(declaration, cast, path)?
                }
                FlowNodeKind::AsCast(cast) if options.casts => {
                    self.collect_decl_as(declaration, cast, path)?
                }
                FlowNodeKind::Start | FlowNodeKind::Unreachable => {
                    if mode == Mode::Branch && matches!(node.kind, FlowNodeKind::Unreachable) {
                        return Ok(Some(OwnedValue::plain(LuaType::Never)));
                    }
                    let base = match self.type_of_decl(declaration)? {
                        Some(base) => self.finalize_flow(base, path)?,
                        None => OwnedValue::plain(LuaType::Unknown),
                    };
                    return self.finish_decl_frames(declaration, tree, base, frames, root_mode);
                }
                _ => {}
            }
            match &node.antecedent {
                None => {
                    let base = match self.type_of_decl(declaration)? {
                        Some(base) => self.finalize_flow(base, path)?,
                        None => OwnedValue::plain(LuaType::Unknown),
                    };
                    return self.finish_decl_frames(declaration, tree, base, frames, root_mode);
                }
                Some(FlowAntecedent::Single(next)) => current = *next,
                antecedent @ Some(FlowAntecedent::Multiple(_)) => {
                    if let Some(common) =
                        self.pure_decl_join(declaration, tree, antecedent.as_ref(), options)?
                    {
                        current = common;
                        continue;
                    }
                    let before = self.walk_decl(
                        declaration,
                        tree,
                        antecedent.as_ref(),
                        options,
                        mode,
                        visited,
                        path,
                    )?;
                    if frames.is_empty() {
                        return Ok(before);
                    }
                    let before = match before {
                        Some(v) => v,
                        None => {
                            let base = self.declared_or_unknown(declaration)?;
                            self.finalize_flow(base, path)?
                        }
                    };
                    return self.finish_decl_frames(declaration, tree, before, frames, root_mode);
                }
            }
        }
    }
    fn finish_decl_frames(
        &mut self,
        declaration: &SemanticId,
        tree: &FlowTree,
        mut result: OwnedValue,
        mut frames: Vec<AssignmentFrame>,
        mode: Mode,
    ) -> QueryResult<Option<OwnedValue>, E> {
        while let Some(frame) = frames.pop() {
            self.checkpoint()?;
            let before = result;
            let value = self.assigned_value(
                declaration,
                tree,
                frame.flow_id,
                frame.position,
                frame.value,
                &before,
            )?;
            let value = self.coerce_flow_assignment(declaration, value, &before, &frame.base)?;
            let value = if value.structural_type().is_unknown()
                && !matches!(before.structural_type(), LuaType::Unknown | LuaType::Any)
            {
                self.widen_flow_const(before.clone())?
            } else {
                value
            };
            let assigned = self.finalize_flow(value, &frame.path)?;
            if mode == Mode::Branch && !self.branch_reachable(tree, Some(&frame.antecedent))? {
                if before.structural_type().is_never() {
                    return Ok(Some(before));
                }
                result = self.widen_flow_const(before)?;
            } else {
                result = assigned;
            }
        }
        Ok(Some(result))
    }
    fn walk_decl(
        &mut self,
        declaration: &SemanticId,
        tree: &FlowTree,
        antecedent: Option<&FlowAntecedent>,
        options: Options,
        mode: Mode,
        visited: &mut HashSet<FlowId>,
        path: &mut PathState,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match antecedent {
            None => match self.type_of_decl(declaration)? {
                Some(value) => self.finalize_flow(value, path).map(Some),
                None => Ok(None),
            },
            Some(FlowAntecedent::Single(next)) => {
                self.trace_decl(declaration, tree, *next, options, mode, visited, path)
            }
            Some(FlowAntecedent::Multiple(index)) => {
                let Some(branches) = tree.get_multi_antecedents(*index) else {
                    return Ok(None);
                };
                if let Some(common) = self.pure_decl_join(declaration, tree, antecedent, options)? {
                    return self.trace_decl(
                        declaration,
                        tree,
                        common,
                        options,
                        mode,
                        visited,
                        path,
                    );
                }
                let mut merged = OwnedValue::plain(LuaType::Unknown);
                for branch in branches {
                    self.checkpoint()?;
                    if let Some(value) = self.trace_decl(
                        declaration,
                        tree,
                        *branch,
                        options,
                        Mode::Branch,
                        &mut visited.clone(),
                        &mut path.clone(),
                    )? {
                        if value.structural_type().is_never() {
                            continue;
                        }
                        merged = if options.assignments {
                            self.flow_merge(merged, value)?
                        } else {
                            self.flow_intersect(merged, value)?
                        };
                    }
                }
                Ok(Some(merged))
            }
        }
    }
    pub fn type_of_expr_at(
        &mut self,
        syntax: LuaSyntaxId,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let key = (syntax, offset);
        if let Some(value) = self.flow_expressions.get(&key).cloned() {
            self.checkpoint()?;
            return Ok(value);
        }
        let projector = SelectedProjector::new(self.analysis, self.session);
        let value = if let Some(value) = projector.project_selected(self.site(syntax), self)? {
            value
        } else {
            let expr = self
                .model
                .syntax_tree()
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                .and_then(LuaExpr::cast);
            match expr {
                Some(LuaExpr::NameExpr(name)) => match self.model.resolve_name(name.get_position())
                {
                    Some(decl) => self.type_of_decl_at(&decl, offset)?,
                    None => self.type_of_expr(syntax)?,
                },
                Some(LuaExpr::IndexExpr(index)) => self.flow_index_expr(&index, offset)?,
                Some(LuaExpr::ClosureExpr(closure)) => {
                    let value = match self.signature_local(closure.get_syntax_id())? {
                        Some(value) => value,
                        None => self.type_of_expr(syntax)?,
                    };
                    self.apply_expr_casts(syntax, value)?
                }
                Some(LuaExpr::BinaryExpr(binary)) => self.flow_binary(syntax, &binary, offset)?,
                Some(LuaExpr::CallExpr(_) | LuaExpr::TableExpr(_) | LuaExpr::LiteralExpr(_)) => {
                    let value = self.type_of_expr(syntax)?;
                    self.apply_expr_casts(syntax, value)?
                }
                _ => self.type_of_expr(syntax)?,
            }
        };
        self.checkpoint()?;
        self.flow_expressions.insert(key, value.clone());
        Ok(value)
    }
    fn flow_binary(
        &mut self,
        syntax: LuaSyntaxId,
        binary: &emmylua_parser::LuaBinaryExpr,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        use emmylua_parser::BinaryOperator;
        let (Some(op), Some((left, right))) = (binary.get_op_token(), binary.get_exprs()) else {
            let value = self.type_of_expr(syntax)?;
            return self.apply_expr_casts(syntax, value);
        };
        let op = op.get_op();
        if matches!(op, BinaryOperator::OpAnd | BinaryOperator::OpOr)
            && let Some(tuple) = self.flow_array_tuple(&right)?
        {
            let left = self.type_of_expr_at(left.get_syntax_id(), offset)?;
            let value = if matches!(left.structural_type(), LuaType::Unknown | LuaType::Any) {
                OwnedValue::plain(LuaType::Unknown)
            } else if op == BinaryOperator::OpAnd {
                if left.structural_type().is_always_falsy() {
                    left
                } else if left.structural_type().is_always_truthy() {
                    tuple
                } else {
                    let mut members = left.logical_components(false, || self.checkpoint())?;
                    members.push(tuple);
                    OwnedValue::union_checked(members, || self.checkpoint())?
                }
            } else if left.structural_type().is_always_truthy() {
                left
            } else if left.structural_type().is_always_falsy() {
                tuple
            } else {
                let mut members = left.logical_components(true, || self.checkpoint())?;
                members.push(tuple);
                OwnedValue::union_checked(members, || self.checkpoint())?
            };
            return self.apply_expr_casts(syntax, value);
        }
        let left = self.type_of_expr_at(left.get_syntax_id(), offset)?;
        let right = self.type_of_expr_at(right.get_syntax_id(), offset)?;
        if matches!(
            op,
            BinaryOperator::OpAdd
                | BinaryOperator::OpSub
                | BinaryOperator::OpMul
                | BinaryOperator::OpDiv
                | BinaryOperator::OpIDiv
                | BinaryOperator::OpMod
                | BinaryOperator::OpPow
        ) && (matches!(left.structural_type(), LuaType::Unknown | LuaType::Any)
            || matches!(right.structural_type(), LuaType::Unknown | LuaType::Any))
        {
            return self.apply_expr_casts(syntax, OwnedValue::plain(LuaType::Unknown));
        }
        let projector = SelectedProjector::new(self.analysis, self.session);
        let value = crate::session::ExpressionExecutionBackend::binary(
            self,
            &projector,
            self.model.file_id(),
            op,
            left,
            right,
        )?;
        self.apply_expr_casts(syntax, value)
    }
    fn flow_array_tuple(&mut self, expr: &LuaExpr) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaExpr::TableExpr(table) = expr else {
            return Ok(None);
        };
        let fields: Vec<_> = table.get_fields().collect();
        if fields.is_empty() {
            return Ok(None);
        }
        let mut values = Vec::with_capacity(fields.len());
        for (index, field) in fields.iter().enumerate() {
            self.checkpoint()?;
            if !field.is_value_field()
                || !matches!(field.get_field_key(),Some(emmylua_parser::LuaIndexKey::Idx(slot))if slot==index+1)
            {
                return Ok(None);
            }
            let Some(expr) = field.get_value_expr() else {
                return Ok(None);
            };
            values.push(self.type_of_expr(expr.get_syntax_id())?);
        }
        OwnedValue::tuple_checked(
            values,
            emmylua_code_analysis::LuaTupleStatus::InferResolve,
            || self.checkpoint(),
        )
        .map(Some)
    }
}
fn first_antecedent(tree: &FlowTree, antecedent: Option<&FlowAntecedent>) -> Option<FlowId> {
    match antecedent {
        Some(FlowAntecedent::Single(next)) => Some(*next),
        Some(FlowAntecedent::Multiple(index)) => {
            tree.get_multi_antecedents(*index)?.first().copied()
        }
        None => None,
    }
}
fn skip_own_assignment(
    declaration: &SemanticId,
    tree: &FlowTree,
    flow: FlowId,
    offset: TextSize,
) -> FlowId {
    let Some(node) = tree.get_flow_node(flow) else {
        return flow;
    };
    if let FlowNodeKind::Assignment(assign) = &node.kind
        && assign.get_syntax_id().get_range().contains(offset)
        && tree.get_flow_effects(flow).iter().any(
            |effect| matches!(effect, FlowEffect::AssignDecl { decl, .. } if decl == declaration),
        )
    {
        return first_antecedent(tree, node.antecedent.as_ref()).unwrap_or(flow);
    }
    flow
}
fn antecedent_has_narrowing(tree: &FlowTree, start: Option<FlowAntecedent>) -> bool {
    let mut stack = vec![(start, HashSet::new())];
    while let Some((antecedent, mut visited)) = stack.pop() {
        match antecedent {
            None => return false,
            Some(FlowAntecedent::Single(flow)) => {
                if !visited.insert(flow) {
                    return false;
                }
                let Some(node) = tree.get_flow_node(flow) else {
                    return false;
                };
                match node.kind {
                    FlowNodeKind::TrueCondition(_)
                    | FlowNodeKind::FalseCondition(_)
                    | FlowNodeKind::TagCast(_)
                    | FlowNodeKind::AsCast(_) => {}
                    FlowNodeKind::DeclPosition(_)
                    | FlowNodeKind::Start
                    | FlowNodeKind::Unreachable => return false,
                    _ => match node.antecedent.clone() {
                        Some(next) => stack.push((Some(next), visited)),
                        None => return false,
                    },
                }
            }
            Some(FlowAntecedent::Multiple(index)) => {
                let Some(branches) = tree.get_multi_antecedents(index) else {
                    return false;
                };
                for branch in branches {
                    stack.push((Some(FlowAntecedent::Single(*branch)), HashSet::new()));
                }
            }
        }
    }
    true
}
