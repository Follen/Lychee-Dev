//! Two-phase expression execution, borrowing the existing analysis/session.
//!
//! This owns the fixed stack schedule and unary operation. Context-dependent
//! name/member/call/flow/binary operations are required on the same backend;
//! implementing this trait does not itself prove those operations equivalent.
use super::{BindingSite, ExpressionInstruction, SelectedExpressionBackend, SelectedProjector};
use emmylua_code_analysis::{FileId, LuaMemberKey, LuaType, SemanticId};
use emmylua_parser::{BinaryOperator, LuaSyntaxId, UnaryOperator};

/// VM metadata is separate from the selected-value carrier. Clearing an owner,
/// closure or receiver must not erase an object's session token/epoch/provenance
/// retained inside `value`. No Clone bound or structural identity shortcut is
/// imposed on the carrier.
#[derive(Debug)]
pub struct ExpressionValue<V> {
    pub value: V,
    pub owner: Option<SemanticId>,
    pub closure_syntax: Option<LuaSyntaxId>,
    pub receiver: Option<V>,
}

impl<V> ExpressionValue<V> {
    pub fn plain(value: V) -> Self {
        Self {
            value,
            owner: None,
            closure_syntax: None,
            receiver: None,
        }
    }
}

/// Every recursive operation must use the supplied projector and its original
/// deadline. There are deliberately no default raw-inference implementations.
/// The query owner must also preserve its expression/decl in-progress caches;
/// `evaluate_expression` is the uncached compiler/executor body, not that cache.
pub trait ExpressionExecutionBackend: SelectedExpressionBackend {
    fn value_type<'a>(&self, value: &'a Self::Value) -> &'a LuaType;

    /// Semantic type equality includes selected object identity/token/epoch and
    /// nested carrier contents. Comparing only the public LuaType is inadequate
    /// if that public type cannot represent the owned provenance.
    fn same_value_type(&self, left: &Self::Value, right: &Self::Value) -> bool;

    fn load_name(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        name: &str,
        offset: u32,
    ) -> Result<ExpressionValue<Self::Value>, Self::Error>;

    /// Includes the fixed integer/array fast path and strict-array Nil union.
    /// All selected object/member operations retain the same carrier/session.
    fn index_member(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        owner: ExpressionValue<Self::Value>,
        key: &LuaMemberKey,
    ) -> Result<ExpressionValue<Self::Value>, Self::Error>;

    fn index_dynamic(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        owner: ExpressionValue<Self::Value>,
        key: ExpressionValue<Self::Value>,
    ) -> Result<ExpressionValue<Self::Value>, Self::Error>;

    /// The actual flow-sensitive query, including context-aware recursion. It
    /// runs for each supplied argument syntax before the callee is popped.
    fn expression_at(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        offset: u32,
    ) -> Result<Self::Value, Self::Error>;

    fn call(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        callee: ExpressionValue<Self::Value>,
        arguments: Vec<ExpressionValue<Self::Value>>,
        colon_call: bool,
        generic_type_syntaxes: &[LuaSyntaxId],
    ) -> Result<ExpressionValue<Self::Value>, Self::Error>;

    /// The old logical_left_nullable_extra query, not unconditional raw flow.
    fn logical_nullable_extra(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> Result<Option<Self::Value>, Self::Error>;

    /// Preserve the same union rules and provenance through a type merge.
    fn merge_value(
        &mut self,
        left: Self::Value,
        extra: Self::Value,
    ) -> Result<Self::Value, Self::Error>;

    fn binary(
        &mut self,
        projector: &SelectedProjector<'_>,
        file_id: FileId,
        op: BinaryOperator,
        left: Self::Value,
        right: Self::Value,
    ) -> Result<Self::Value, Self::Error>;
}

impl SelectedProjector<'_> {
    /// Compile all selected producers first, then consume that same plan on one
    /// stack. The plan cannot be replayed here or executed against another
    /// session. No provider/DB/ledger clone is made. This does not install a
    /// root-query cache or claim any deferred backend operation is implemented.
    pub fn evaluate_expression<B: ExpressionExecutionBackend>(
        &self,
        site: BindingSite,
        backend: &mut B,
    ) -> Result<B::Value, B::Error> {
        let plan = self.compile_expression(site, backend)?;
        let model = self.analysis().semantic_model(plan.file_id);
        let mut stack = Vec::new();
        for instruction in plan.instructions {
            backend.checkpoint()?;
            match instruction {
                ExpressionInstruction::PushValue(value) => {
                    stack.push(ExpressionValue::plain(value));
                }
                ExpressionInstruction::PushClosure { syntax } => {
                    let mut value = ExpressionValue::plain(backend.plain(LuaType::Function));
                    value.closure_syntax = Some(syntax);
                    stack.push(value);
                }
                ExpressionInstruction::LoadName { name, offset } => {
                    let value = backend.load_name(self, plan.file_id, &name, offset)?;
                    backend.checkpoint()?;
                    stack.push(value);
                }
                ExpressionInstruction::IndexMember { key, dynamic } => {
                    let value = if dynamic {
                        let key = pop_or_unknown(&mut stack, backend);
                        let owner = pop_or_unknown(&mut stack, backend);
                        backend.index_dynamic(self, plan.file_id, owner, key)?
                    } else {
                        let owner = pop_or_unknown(&mut stack, backend);
                        let key = key.unwrap_or_else(|| LuaMemberKey::Name("".into()));
                        backend.index_member(self, plan.file_id, owner, &key)?
                    };
                    backend.checkpoint()?;
                    stack.push(value);
                }
                ExpressionInstruction::Call {
                    arg_count,
                    colon_call,
                    generic_type_syntaxes,
                    arg_syntaxes,
                } => {
                    // Preserve filter_map(pop), reversal, flow overrides, then
                    // callee pop: malformed instruction stacks follow the same
                    // Unknown behavior without copying values or metadata.
                    let mut arguments = Vec::new();
                    for _ in 0..arg_count {
                        backend.checkpoint()?;
                        if let Some(value) = stack.pop() {
                            arguments.push(value);
                        }
                    }
                    let count = arguments.len();
                    for index in 0..count / 2 {
                        backend.checkpoint()?;
                        arguments.swap(index, count - index - 1);
                    }
                    for (index, argument) in arguments.iter_mut().enumerate() {
                        backend.checkpoint()?;
                        if let Some(syntax) = arg_syntaxes.get(index) {
                            let flow = backend.expression_at(
                                self,
                                BindingSite {
                                    file_id: plan.file_id,
                                    syntax: *syntax,
                                },
                                syntax.get_range().start().into(),
                            )?;
                            backend.checkpoint()?;
                            if !backend.same_value_type(&flow, &argument.value)
                                && !matches!(
                                    backend.value_type(&flow),
                                    LuaType::Unknown | LuaType::Any
                                )
                            {
                                // Upstream changes ty only. Owner/closure/
                                // receiver metadata must remain untouched.
                                argument.value = flow;
                            }
                        }
                    }
                    let callee = pop_or_unknown(&mut stack, backend);
                    let value = backend.call(
                        self,
                        plan.file_id,
                        callee,
                        arguments,
                        colon_call,
                        &generic_type_syntaxes,
                    )?;
                    backend.checkpoint()?;
                    stack.push(value);
                }
                ExpressionInstruction::Unary { op } => {
                    let operand = pop_or_unknown(&mut stack, backend);
                    let name = match op {
                        UnaryOperator::OpUnm => Some("unm"),
                        UnaryOperator::OpLen => Some("len"),
                        _ => None,
                    };
                    backend.checkpoint()?;
                    let overload = name.and_then(|name| {
                        model.operator_type(name, backend.value_type(&operand.value))
                    });
                    backend.checkpoint()?;
                    let value = match overload {
                        Some(ty) => backend.plain(ty),
                        // The pinned engine returns the operand unchanged. In
                        // particular it does NOT negate numeric constants here.
                        None => operand.value,
                    };
                    stack.push(ExpressionValue::plain(value));
                }
                ExpressionInstruction::Binary {
                    op,
                    left_syntax,
                    right_syntax,
                } => {
                    let mut right = pop_or_unknown(&mut stack, backend);
                    let mut left = pop_or_unknown(&mut stack, backend);
                    if matches!(op, BinaryOperator::OpAnd | BinaryOperator::OpOr) {
                        if let Some(syntax) = left_syntax {
                            backend.checkpoint()?;
                            let extra = backend.logical_nullable_extra(
                                self,
                                BindingSite {
                                    file_id: plan.file_id,
                                    syntax,
                                },
                            )?;
                            backend.checkpoint()?;
                            if let Some(extra) = extra {
                                left.value = backend.merge_value(left.value, extra)?;
                                backend.checkpoint()?;
                            }
                        }
                        if let Some(syntax) = right_syntax {
                            backend.checkpoint()?;
                            let extra = backend.logical_nullable_extra(
                                self,
                                BindingSite {
                                    file_id: plan.file_id,
                                    syntax,
                                },
                            )?;
                            backend.checkpoint()?;
                            if let Some(extra) = extra {
                                right.value = backend.merge_value(right.value, extra)?;
                                backend.checkpoint()?;
                            }
                        }
                    }
                    let value = backend.binary(self, plan.file_id, op, left.value, right.value)?;
                    backend.checkpoint()?;
                    stack.push(ExpressionValue::plain(value));
                }
                ExpressionInstruction::Result => break,
            }
        }
        backend.checkpoint()?;
        Ok(pop_or_unknown(&mut stack, backend).value)
    }
}

fn pop_or_unknown<B: ExpressionExecutionBackend>(
    stack: &mut Vec<ExpressionValue<B::Value>>,
    backend: &mut B,
) -> ExpressionValue<B::Value> {
    stack
        .pop()
        .unwrap_or_else(|| ExpressionValue::plain(backend.plain(LuaType::Unknown)))
}
