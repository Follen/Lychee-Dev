//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/check_return_count.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
//! Return values and condition inference use the same owned query/session.
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, FileId, LuaType, SemanticId, SemanticModel, Signature, VariadicType,
};
use emmylua_parser::{
    BinaryOperator, LuaAstNode, LuaAstToken, LuaBlock, LuaCallExpr, LuaCallExprStat,
    LuaClosureExpr, LuaExpr, LuaIfClauseStat, LuaIfStat, LuaReturnStat, LuaStat, LuaTokenKind,
};
use std::sync::Arc;

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_return_count(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(signatures) = self.model.signatures() else {
            return Ok(());
        };
        let signatures = signatures.to_vec();
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for signature in &signatures {
            self.checkpoint()?;
            let Some(node) = signature.closure_syntax.to_node_from_root(&root) else {
                continue;
            };
            let Some(closure) = LuaClosureExpr::cast(node) else {
                continue;
            };
            let expected = self.return_expectation(signature, &closure)?;
            self.return_stats(context, &closure, &expected)?;
            self.return_missing(context, &closure, &expected)?;
        }
        Ok(())
    }
    fn return_expectation(
        &mut self,
        signature: &Signature,
        closure: &LuaClosureExpr,
    ) -> QueryResult<ReturnExpectation, E> {
        let mut syntaxes = Vec::new();
        if let Some(docs) = &signature.docs {
            syntaxes.extend(
                docs.returns
                    .iter()
                    .map(|syntax| (syntax.get_range().start(), *syntax)),
            );
            syntaxes.extend(
                docs.return_overloads
                    .iter()
                    .filter(|(name, _)| name.is_some())
                    .map(|(_, syntax)| (syntax.get_range().start(), *syntax)),
            );
        }
        if !syntaxes.is_empty() {
            syntaxes.sort_by_key(|(start, _)| *start);
            let types = syntaxes
                .into_iter()
                .map(|(_, syntax)| project_return_annotation(&self.model, syntax))
                .collect::<Vec<_>>();
            return Ok(expectation_from_types(&types));
        }
        Ok(self
            .return_contextual_expectation(closure)?
            .unwrap_or_else(ReturnExpectation::none))
    }
    fn return_contextual_expectation(
        &mut self,
        closure: &LuaClosureExpr,
    ) -> QueryResult<Option<ReturnExpectation>, E> {
        let Some((owner, key)) = self
            .model
            .file_facts()
            .and_then(|facts| {
                facts
                    .members
                    .iter()
                    .find(|member| member.value_syntax == Some(closure.get_syntax_id()))
            })
            .and_then(|member| {
                member
                    .key
                    .name()
                    .map(|key| (member.owner.clone(), key.to_string()))
            })
        else {
            return Ok(None);
        };
        let Some(owner_ty) = self.type_of_decl(&owner)? else {
            return Ok(None);
        };
        let (LuaType::Ref(id) | LuaType::Def(id)) = owner_ty.structural_type() else {
            return Ok(None);
        };
        let result = (|| {
            let def = self.model.type_def_of(id)?;
            let members = self.model.members_of_owner(&def.id);
            let field_ref = members.iter().find(|field| field.name.as_str() == key)?;
            let facts = self.model.file_facts_of(field_ref.file_id)?;
            let field = facts.member_by_id(&field_ref.id)?;
            let syntax = field.value_syntax?;
            let tree = self.model.syntax_tree_of(field_ref.file_id)?;
            let doc =
                emmylua_parser::LuaDocType::cast(syntax.to_node_from_root(&tree.get_red_root())?)?;
            let emmylua_parser::LuaDocType::Func(fun) = doc else {
                return None;
            };
            let Some(returns) = fun.get_return_type_list() else {
                return Some(ReturnExpectation::fixed(0));
            };
            let types = returns
                .get_return_type_list()
                .filter_map(|ret| ret.get_name_and_type().1)
                .map(|ty| {
                    project_return_annotation_in(&self.model, field_ref.file_id, ty.get_syntax_id())
                })
                .collect::<Vec<_>>();
            Some(expectation_from_types(&types))
        })();
        Ok(result)
    }
    fn return_stats(
        &mut self,
        context: &mut CheckContext<'_>,
        closure: &LuaClosureExpr,
        expected: &ReturnExpectation,
    ) -> QueryResult<(), E> {
        if expected.max.is_none() && expected.min == 0 {
            return Ok(());
        }
        for stat in own_return_stats(closure) {
            let Some((total, redundant)) = self.return_values(&stat, expected.max)? else {
                continue;
            };
            if total < expected.min {
                context.add_diagnostic(DiagnosticCode::MissingReturnValue, stat.get_range(), t!("Annotations specify that at least %{min} return value(s) are required, found %{rmin} returned here instead.", min = expected.min, rmin = total));
            }
            if let Some(max) = expected.max {
                for range in redundant {
                    context.add_diagnostic(DiagnosticCode::RedundantReturnValue, range, t!("Annotations specify that at most %{max} return value(s) are required, found %{rmax} returned here instead.", max = max, rmax = total));
                }
            }
        }
        Ok(())
    }
    fn return_values(
        &mut self,
        stat: &LuaReturnStat,
        expected_max: Option<usize>,
    ) -> QueryResult<Option<(usize, Vec<rowan::TextRange>)>, E> {
        let exprs = stat.get_expr_list().collect::<Vec<_>>();
        let mut total = 0;
        let mut redundant = Vec::new();
        let mut tail_nil = false;
        for (index, expr) in exprs.iter().enumerate() {
            let ty = self.return_full_type(expr)?;
            match ty.structural_type() {
                LuaType::Variadic(variadic) => {
                    let Some(len) = variadic.get_max_len() else {
                        return Ok(None);
                    };
                    total += len;
                }
                LuaType::Nil => {
                    if index == exprs.len() - 1 {
                        tail_nil = true;
                    }
                    total += 1;
                }
                _ => total += 1,
            }
            if let Some(max) = expected_max
                && total > max
            {
                if tail_nil && total - 1 == max {
                    continue;
                }
                redundant.push(expr.get_range());
            }
        }
        Ok(Some((total, redundant)))
    }
    fn return_full_type(&mut self, expr: &LuaExpr) -> QueryResult<OwnedValue, E> {
        if let Some(ty) = self.binding_type_of_expr(expr.get_syntax_id())? {
            return Ok(
                if matches!(ty.structural_type(), LuaType::Unknown)
                    && matches!(expr, LuaExpr::CallExpr(_))
                {
                    OwnedValue::plain(LuaType::Variadic(Arc::new(VariadicType::Base(
                        LuaType::Unknown,
                    ))))
                } else {
                    ty
                },
            );
        }
        if let LuaExpr::CallExpr(call) = expr {
            let managed = match call.get_prefix_expr() {
                Some(callee) => self.binding_manages(callee.get_syntax_id())?,
                None => false,
            };
            if !managed && let Some(types) = self.return_call_annotations(call)? {
                return Ok(match types.len() {
                    0 => OwnedValue::plain(LuaType::Nil),
                    1 => types.into_iter().next().expect("len checked"),
                    _ => OwnedValue::pack(types),
                });
            }
        }
        self.type_of_expr_at(expr.get_syntax_id(), expr.get_range().start().into())
    }
    fn return_call_annotations(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(None);
        };
        let location = match &callee {
            LuaExpr::NameExpr(name) => {
                let Some(id @ SemanticId::Decl(_)) = self.model.resolve_name(name.get_position())
                else {
                    return Ok(None);
                };
                if self.owner_initializer_managed(&id)? {
                    return Ok(None);
                }
                let SemanticId::Decl(key) = &id else {
                    unreachable!()
                };
                (|| {
                    let facts = self.model.file_facts_of(key.file_id)?;
                    let decl = facts.decl_by_id(&id)?;
                    Some((decl.file_id, decl.value_expr_syntax?))
                })()
            }
            LuaExpr::IndexExpr(index) => {
                let Some(resolved) = self.resolve_owned_member(index)? else {
                    return Ok(None);
                };
                if resolved.binding_token.is_some() {
                    return Ok(None);
                }
                if let Some(id) = &resolved.member_id
                    && self.owner_initializer_managed(id)?
                {
                    return Ok(None);
                }
                (|| {
                    let file = resolved.file_id?;
                    let facts = self.model.file_facts_of(file)?;
                    let member = facts.member_by_id(&resolved.member_id?)?;
                    Some((file, member.value_syntax?))
                })()
            }
            _ => None,
        };
        let Some((file, syntax)) = location else {
            return Ok(None);
        };
        let Some(docs) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.signature_by_closure(syntax))
            .and_then(|signature| signature.docs.as_ref())
        else {
            return Ok(None);
        };
        let mut syntaxes = docs
            .returns
            .iter()
            .map(|syntax| (syntax.get_range().start(), *syntax))
            .chain(
                docs.return_overloads
                    .iter()
                    .filter(|(name, _)| name.is_some())
                    .map(|(_, syntax)| (syntax.get_range().start(), *syntax)),
            )
            .collect::<Vec<_>>();
        if syntaxes.is_empty() {
            return Ok(Some(Vec::new()));
        }
        syntaxes.sort_by_key(|(start, _)| *start);
        Ok(Some(
            syntaxes
                .into_iter()
                .map(|(_, syntax)| {
                    OwnedValue::plain(project_return_annotation_in(&self.model, file, syntax))
                })
                .collect(),
        ))
    }
    fn return_missing(
        &mut self,
        context: &mut CheckContext<'_>,
        closure: &LuaClosureExpr,
        expected: &ReturnExpectation,
    ) -> QueryResult<(), E> {
        if expected.min == 0 {
            return Ok(());
        }
        let flow = match closure_body_block(closure) {
            Some(block) => self.return_block_flow(&block)?,
            None => ReturnFlow::fallthrough(),
        };
        if !flow.can_fall_through && !flow.can_break {
            return Ok(());
        }
        let range = closure_body_block(closure)
            .and_then(|block| {
                block
                    .token_by_kind(LuaTokenKind::TkEnd)
                    .map(|t| t.syntax().text_range())
            })
            .unwrap_or_else(|| closure.get_range());
        context.add_diagnostic(
            DiagnosticCode::MissingReturn,
            range,
            t!("Annotations specify that a return value is required here."),
        );
        Ok(())
    }
    fn return_block_flow(&mut self, block: &LuaBlock) -> QueryResult<ReturnFlow, E> {
        let mut flow = ReturnFlow::default();
        let mut fallthrough = true;
        for stat in block.get_stats() {
            self.checkpoint()?;
            if !fallthrough {
                break;
            }
            let branch = self.return_stat_flow(&stat)?;
            flow.can_break |= branch.can_break;
            flow.is_infinite |= branch.is_infinite;
            flow.may_diverge |= branch.may_diverge;
            fallthrough = branch.can_fall_through;
        }
        flow.can_fall_through = fallthrough;
        Ok(flow)
    }
    fn return_optional_block_flow(
        &mut self,
        block: Option<LuaBlock>,
    ) -> QueryResult<ReturnFlow, E> {
        match block {
            Some(block) => self.return_block_flow(&block),
            None => Ok(ReturnFlow::fallthrough()),
        }
    }
    fn return_stat_flow(&mut self, stat: &LuaStat) -> QueryResult<ReturnFlow, E> {
        Ok(match stat {
            LuaStat::DoStat(stat) => self.return_optional_block_flow(stat.get_block())?,
            LuaStat::WhileStat(stat) => match self
                .return_condition(stat.get_condition_expr().as_ref())?
            {
                ConditionState::Falsy => ReturnFlow::fallthrough(),
                ConditionState::Never => ReturnFlow::default(),
                ConditionState::Truthy => {
                    let body = self.return_optional_block_flow(stat.get_block())?;
                    ReturnFlow {
                        can_fall_through: body.can_break,
                        can_break: false,
                        is_infinite: (body.can_fall_through && !body.can_break) || body.is_infinite,
                        may_diverge: (body.can_fall_through && body.can_break) || body.may_diverge,
                    }
                }
                ConditionState::Dynamic => {
                    let body = self.return_optional_block_flow(stat.get_block())?;
                    ReturnFlow {
                        can_fall_through: true,
                        can_break: false,
                        is_infinite: body.is_infinite,
                        may_diverge: body.can_fall_through || body.may_diverge,
                    }
                }
            },
            LuaStat::RepeatStat(stat) => {
                let body = self.return_optional_block_flow(stat.get_block())?;
                let mut flow = ReturnFlow {
                    can_fall_through: body.can_break,
                    can_break: false,
                    is_infinite: body.is_infinite,
                    may_diverge: body.may_diverge,
                };
                if !body.can_fall_through {
                    return Ok(flow);
                }
                match self.return_condition(stat.get_condition_expr().as_ref())? {
                    ConditionState::Truthy => flow.can_fall_through = true,
                    ConditionState::Falsy => {
                        if body.can_break {
                            flow.may_diverge = true;
                        } else {
                            flow.is_infinite = true;
                        }
                    }
                    ConditionState::Dynamic => {
                        flow.can_fall_through = true;
                        flow.may_diverge = true;
                    }
                    ConditionState::Never => {}
                }
                flow
            }
            LuaStat::IfStat(stat) => self.return_if_flow(stat)?,
            LuaStat::ForStat(stat) => {
                let mut flow = self.return_optional_block_flow(stat.get_block())?;
                flow.can_fall_through = true;
                flow.can_break = false;
                flow
            }
            LuaStat::ForRangeStat(stat) => {
                let mut flow = self.return_optional_block_flow(stat.get_block())?;
                flow.can_fall_through = true;
                flow.can_break = false;
                flow
            }
            LuaStat::CallExprStat(stat) => analyze_call_expr_stat_returns(stat),
            LuaStat::BreakStat(_) => ReturnFlow {
                can_break: true,
                ..Default::default()
            },
            LuaStat::ReturnStat(_) => ReturnFlow::default(),
            _ => ReturnFlow::fallthrough(),
        })
    }
    fn return_if_flow(&mut self, stat: &LuaIfStat) -> QueryResult<ReturnFlow, E> {
        let mut flow = ReturnFlow::default();
        let mut reach_next = true;
        match self.return_condition(stat.get_condition_expr().as_ref())? {
            ConditionState::Truthy => return self.return_optional_block_flow(stat.get_block()),
            ConditionState::Falsy => {}
            ConditionState::Dynamic => {
                flow.merge_choice(self.return_optional_block_flow(stat.get_block())?)
            }
            ConditionState::Never => return Ok(flow),
        }
        for clause in stat.get_all_clause() {
            if !reach_next {
                break;
            }
            match clause {
                LuaIfClauseStat::ElseIf(clause) => {
                    match self.return_condition(clause.get_condition_expr().as_ref())? {
                        ConditionState::Truthy => {
                            flow.merge_choice(self.return_optional_block_flow(clause.get_block())?);
                            reach_next = false;
                        }
                        ConditionState::Falsy => {}
                        ConditionState::Dynamic => {
                            flow.merge_choice(self.return_optional_block_flow(clause.get_block())?)
                        }
                        ConditionState::Never => reach_next = false,
                    }
                }
                LuaIfClauseStat::Else(clause) => {
                    flow.merge_choice(self.return_optional_block_flow(clause.get_block())?);
                    reach_next = false;
                }
            }
        }
        if reach_next {
            flow.can_fall_through = true;
        }
        Ok(flow)
    }
    fn return_condition(&mut self, condition: Option<&LuaExpr>) -> QueryResult<ConditionState, E> {
        let Some(condition) = condition else {
            return Ok(ConditionState::Dynamic);
        };
        // Preserve short-circuit producer order across all descendants, including
        // managed child values of not/comparison/parenthesized conditions.
        let mut managed = false;
        for expr in condition.syntax().descendants().filter_map(LuaExpr::cast) {
            if self.binding_manages(expr.get_syntax_id())?
                || matches!(expr, LuaExpr::CallExpr(_))
                    && self.binding_manages_call(expr.get_syntax_id())?
            {
                managed = true;
                break;
            }
        }
        if !managed {
            if let Some(state) = static_condition_state(condition) {
                return Ok(state);
            }
            if !can_analyze_condition(condition) {
                return Ok(ConditionState::Dynamic);
            }
        }
        let ty = self.type_of_expr(condition.get_syntax_id())?;
        Ok(if ty.structural_type().is_never() {
            ConditionState::Never
        } else if ty.structural_type().is_always_truthy() {
            ConditionState::Truthy
        } else if ty.structural_type().is_always_falsy() {
            ConditionState::Falsy
        } else {
            ConditionState::Dynamic
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct ReturnExpectation {
    min: usize,
    max: Option<usize>,
}

impl ReturnExpectation {
    fn none() -> Self {
        Self { min: 0, max: None }
    }

    fn fixed(count: usize) -> Self {
        Self {
            min: count,
            max: Some(count),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConditionState {
    Dynamic,
    Truthy,
    Falsy,
    Never,
}

#[derive(Debug, Default)]
struct ReturnFlow {
    can_fall_through: bool,
    can_break: bool,
    is_infinite: bool,
    may_diverge: bool,
}

impl ReturnFlow {
    fn fallthrough() -> Self {
        Self {
            can_fall_through: true,
            ..Default::default()
        }
    }

    fn merge_choice(&mut self, other: Self) {
        self.can_fall_through |= other.can_fall_through;
        self.can_break |= other.can_break;
        self.is_infinite |= other.is_infinite;
        self.may_diverge |= other.may_diverge;
    }
}

#[derive(Debug, Clone, PartialEq)]
enum StaticValue {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
}

fn expectation_from_types(types: &[LuaType]) -> ReturnExpectation {
    if types.is_empty() {
        return ReturnExpectation::fixed(0);
    }
    if types.len() == 1 {
        let ty = &types[0];
        if let LuaType::Variadic(variadic) = ty {
            return expectation_from_variadic(variadic);
        }
        if matches!(ty, LuaType::Any | LuaType::Unknown) {
            return ReturnExpectation {
                min: 0,
                max: Some(1),
            };
        }
        if matches!(ty, LuaType::Nil) {
            return ReturnExpectation {
                min: 0,
                max: Some(0),
            };
        }
        let min = usize::from(!ty.is_optional());
        return ReturnExpectation { min, max: Some(1) };
    }

    let mut min = types.len();
    // Trailing consecutive optionals are not required to be returned.
    for ty in types.iter().rev() {
        if ty.is_optional() {
            min -= 1;
        } else {
            break;
        }
    }
    let mut max = Some(0usize);
    for ty in types {
        match ty {
            LuaType::Variadic(variadic) => {
                let Some(len) = variadic.get_max_len() else {
                    return ReturnExpectation { min, max: None };
                };
                *max.as_mut().expect("max is Some") += len;
            }
            _ => *max.as_mut().expect("max is Some") += 1,
        }
    }
    ReturnExpectation { min, max }
}

fn expectation_from_variadic(variadic: &VariadicType) -> ReturnExpectation {
    let Some(min_len) = variadic.get_min_len() else {
        return ReturnExpectation::none();
    };
    let mut min = min_len;
    if min_len > 0 {
        for idx in (0..min_len).rev() {
            if let Some(ty) = variadic.get_type(idx)
                && ty.is_optional()
            {
                min -= 1;
            } else {
                break;
            }
        }
    }
    ReturnExpectation {
        min,
        max: variadic.get_max_len(),
    }
}

fn project_return_annotation(
    semantic_model: &SemanticModel<'_>,
    syntax: emmylua_parser::LuaSyntaxId,
) -> LuaType {
    project_return_annotation_in(semantic_model, semantic_model.file_id(), syntax)
}

fn project_return_annotation_in(
    semantic_model: &SemanticModel<'_>,
    file_id: FileId,
    syntax: emmylua_parser::LuaSyntaxId,
) -> LuaType {
    if let Some(tree) = semantic_model.syntax_tree_of(file_id)
        && let Some(node) = syntax.to_node_from_root(&tree.get_red_root())
        && let Some(doc_ty) = emmylua_parser::LuaDocType::cast(node)
    {
        if let emmylua_parser::LuaDocType::Variadic(variadic) = &doc_ty {
            let inner = variadic
                .get_type()
                .map(|ty| project_return_annotation_in(semantic_model, file_id, ty.get_syntax_id()))
                .unwrap_or(LuaType::Any);
            return LuaType::Variadic(Arc::new(VariadicType::Base(inner)));
        }
    }
    semantic_model.doc_type_lua_rich_in(file_id, syntax)
}

fn own_return_stats(closure: &LuaClosureExpr) -> Vec<LuaReturnStat> {
    closure
        .descendants::<LuaReturnStat>()
        .filter(|stat| {
            stat.ancestors::<LuaClosureExpr>()
                .next()
                .is_some_and(|expr| &expr == closure)
        })
        .collect()
}

fn closure_body_block(closure: &LuaClosureExpr) -> Option<LuaBlock> {
    if let Some(block) = closure
        .get_block()
        .or_else(|| closure.descendants::<LuaBlock>().next())
    {
        return Some(block);
    }
    let parent = closure.syntax().parent()?;
    parent.descendants().filter_map(LuaBlock::cast).next()
}

fn analyze_call_expr_stat_returns(call_expr_stat: &LuaCallExprStat) -> ReturnFlow {
    if call_expr_stat
        .get_call_expr()
        .is_some_and(|call| call.is_error())
    {
        return ReturnFlow::default();
    }
    ReturnFlow::fallthrough()
}

fn static_condition_state(expr: &LuaExpr) -> Option<ConditionState> {
    match expr {
        LuaExpr::LiteralExpr(literal) => match literal.get_literal() {
            Some(emmylua_parser::LuaLiteralToken::Nil(_)) => Some(ConditionState::Falsy),
            Some(emmylua_parser::LuaLiteralToken::Bool(token)) => Some(if token.is_true() {
                ConditionState::Truthy
            } else {
                ConditionState::Falsy
            }),
            Some(emmylua_parser::LuaLiteralToken::String(_))
            | Some(emmylua_parser::LuaLiteralToken::Number(_)) => Some(ConditionState::Truthy),
            _ => None,
        },
        LuaExpr::TableExpr(_) | LuaExpr::ClosureExpr(_) => Some(ConditionState::Truthy),
        LuaExpr::ParenExpr(paren) => paren
            .get_expr()
            .and_then(|inner| static_condition_state(&inner)),
        LuaExpr::UnaryExpr(unary) => {
            if unary
                .get_op_token()
                .is_some_and(|op| op.get_op() == emmylua_parser::UnaryOperator::OpNot)
            {
                unary
                    .get_expr()
                    .and_then(|inner| static_condition_state(&inner).map(not_condition_state))
            } else {
                None
            }
        }
        LuaExpr::BinaryExpr(binary) => {
            let (left, right) = binary.get_exprs()?;
            let op = binary.get_op_token()?.get_op();
            match op {
                BinaryOperator::OpAnd => match static_condition_state(&left) {
                    Some(ConditionState::Falsy) => Some(ConditionState::Falsy),
                    Some(ConditionState::Truthy) => static_condition_state(&right),
                    _ => None,
                },
                BinaryOperator::OpOr => match static_condition_state(&left) {
                    Some(ConditionState::Truthy) => Some(ConditionState::Truthy),
                    Some(ConditionState::Falsy) => static_condition_state(&right),
                    _ => None,
                },
                BinaryOperator::OpEq | BinaryOperator::OpNe => {
                    let (left_value, right_value) = (static_value(&left)?, static_value(&right)?);
                    let equal = left_value == right_value;
                    let truthy = if op == BinaryOperator::OpEq {
                        equal
                    } else {
                        !equal
                    };
                    Some(if truthy {
                        ConditionState::Truthy
                    } else {
                        ConditionState::Falsy
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn not_condition_state(state: ConditionState) -> ConditionState {
    match state {
        ConditionState::Truthy => ConditionState::Falsy,
        ConditionState::Falsy => ConditionState::Truthy,
        other => other,
    }
}

fn static_value(expr: &LuaExpr) -> Option<StaticValue> {
    match expr {
        LuaExpr::LiteralExpr(literal) => match literal.get_literal() {
            Some(emmylua_parser::LuaLiteralToken::Nil(_)) => Some(StaticValue::Nil),
            Some(emmylua_parser::LuaLiteralToken::Bool(token)) => {
                Some(StaticValue::Bool(token.is_true()))
            }
            Some(emmylua_parser::LuaLiteralToken::String(token)) => {
                Some(StaticValue::String(token.get_value()))
            }
            Some(emmylua_parser::LuaLiteralToken::Number(token)) => {
                match token.get_number_value() {
                    emmylua_parser::NumberResult::Int(i) => Some(StaticValue::Number(i as f64)),
                    emmylua_parser::NumberResult::Uint(u) => Some(StaticValue::Number(u as f64)),
                    emmylua_parser::NumberResult::Float(f) => Some(StaticValue::Number(f)),
                    emmylua_parser::NumberResult::Number => None,
                }
            }
            _ => None,
        },
        LuaExpr::ParenExpr(paren) => paren.get_expr().and_then(|inner| static_value(&inner)),
        _ => None,
    }
}

fn can_analyze_condition(expr: &LuaExpr) -> bool {
    match expr {
        LuaExpr::LiteralExpr(_) | LuaExpr::TableExpr(_) | LuaExpr::ClosureExpr(_) => true,
        LuaExpr::CallExpr(_) => false,
        LuaExpr::ParenExpr(paren_expr) => paren_expr
            .get_expr()
            .is_some_and(|expr| can_analyze_condition(&expr)),
        LuaExpr::UnaryExpr(unary_expr) => unary_expr
            .get_expr()
            .is_some_and(|expr| can_analyze_condition(&expr)),
        LuaExpr::BinaryExpr(binary_expr) => binary_expr.get_exprs().is_some_and(|(left, right)| {
            can_analyze_condition(&left) && can_analyze_condition(&right)
        }),
        LuaExpr::NameExpr(_) | LuaExpr::IndexExpr(_) => false,
        _ => true,
    }
}
