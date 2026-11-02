//! The fixed expression compiler's producer order, outside the upstream VM.
//!
//! Selected projections happen during compilation, before deferred name/member/
//! call operations execute. Eager recursive evaluation would reorder evidence.
//! This module constructs a plan only; it never claims its operations executed.
use super::{BindingSite, SelectedExpressionBackend, SelectedProjector};
use emmylua_code_analysis::{FileId, InFiled, LuaMemberKey, LuaType};
use emmylua_parser::{
    BinaryOperator, LuaAstNode, LuaExpr, LuaIndexKey, LuaLiteralToken, LuaSyntaxId, UnaryOperator,
};

#[derive(Debug)]
pub struct ExpressionPlan<V> {
    pub file_id: FileId,
    pub instructions: Vec<ExpressionInstruction<V>>,
}

/// Values retain the owning query's carrier. Other instructions are work still
/// to execute through owned context-aware operations, never raw fallback claims.
#[derive(Debug)]
pub enum ExpressionInstruction<V> {
    PushValue(V),
    PushClosure {
        syntax: LuaSyntaxId,
    },
    LoadName {
        name: String,
        offset: u32,
    },
    IndexMember {
        key: Option<LuaMemberKey>,
        dynamic: bool,
    },
    Call {
        arg_count: usize,
        colon_call: bool,
        generic_type_syntaxes: Vec<LuaSyntaxId>,
        arg_syntaxes: Vec<LuaSyntaxId>,
    },
    Unary {
        op: UnaryOperator,
    },
    Binary {
        op: BinaryOperator,
        left_syntax: Option<LuaSyntaxId>,
        right_syntax: Option<LuaSyntaxId>,
    },
    Result,
}

enum Task {
    Expr(LuaExpr),
    Index {
        key: Option<LuaMemberKey>,
        dynamic: bool,
    },
    Call {
        arg_count: usize,
        colon_call: bool,
        generic_type_syntaxes: Vec<LuaSyntaxId>,
        arg_syntaxes: Vec<LuaSyntaxId>,
    },
    Unary {
        op: UnaryOperator,
    },
    BinaryRight {
        op: BinaryOperator,
        right: LuaExpr,
        left_syntax: LuaSyntaxId,
    },
    BinaryEmit {
        op: BinaryOperator,
        left_syntax: LuaSyntaxId,
        right_syntax: LuaSyntaxId,
    },
}

impl SelectedProjector<'_> {
    /// Compile without executing deferred inference. The caller retains its
    /// original absolute deadline. Interrupted plans are dropped, while already
    /// completed selected producers remain in the same session ledger.
    pub fn compile_expression<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        backend: &mut B,
    ) -> Result<ExpressionPlan<B::Value>, B::Error> {
        self.compile_expression_inner(site, backend, false)
    }

    /// The caller has already authenticated this call's permit. Only the root
    /// selected lookup is skipped; every selected child and deferred operation
    /// retains the normal compiler/executor schedule and the same session.
    pub(super) fn compile_permitted_call<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        backend: &mut B,
    ) -> Result<ExpressionPlan<B::Value>, B::Error> {
        self.compile_expression_inner(site, backend, true)
    }

    fn compile_expression_inner<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        backend: &mut B,
        permitted_call_root: bool,
    ) -> Result<ExpressionPlan<B::Value>, B::Error> {
        backend.checkpoint()?;
        let model = self.analysis().semantic_model(site.file_id);
        let expr = model
            .syntax_tree()
            .and_then(|tree| site.syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast);
        backend.checkpoint()?;
        let mut instructions = Vec::new();
        let mut tasks = Vec::new();
        if let Some(expr) = expr {
            tasks.push(Task::Expr(expr));
        } else {
            instructions.push(ExpressionInstruction::PushValue(
                backend.plain(LuaType::Unknown),
            ));
        }
        while let Some(task) = tasks.pop() {
            backend.checkpoint()?;
            match task {
                Task::Expr(expr) => {
                    let current = BindingSite {
                        file_id: site.file_id,
                        syntax: expr.get_syntax_id(),
                    };
                    if !(permitted_call_root && current == site)
                        && let Some(value) = self.project_selected(current, backend)?
                    {
                        instructions.push(ExpressionInstruction::PushValue(value));
                        continue;
                    }
                    match expr {
                        LuaExpr::LiteralExpr(literal) => {
                            let token = literal.get_literal();
                            if matches!(&token, Some(LuaLiteralToken::Dots(_))) {
                                instructions.push(ExpressionInstruction::LoadName {
                                    name: "...".into(),
                                    offset: literal.syntax().text_range().start().into(),
                                });
                            } else {
                                // Public token primitives and member-key string
                                // conversion preserve the fixed literal rules
                                // without a second inference query/cache.
                                let ty = match token {
                                    Some(LuaLiteralToken::String(string)) => {
                                        LuaMemberKey::Name(string.get_value().into())
                                            .to_index_type()
                                            .unwrap_or(LuaType::Unknown)
                                    }
                                    Some(LuaLiteralToken::Number(number)) => {
                                        match number.get_number_value() {
                                            emmylua_parser::NumberResult::Int(value) => {
                                                LuaType::IntegerConst(value)
                                            }
                                            emmylua_parser::NumberResult::Uint(value) => {
                                                LuaType::IntegerConst(value as i64)
                                            }
                                            emmylua_parser::NumberResult::Float(value) => {
                                                LuaType::FloatConst(value)
                                            }
                                            emmylua_parser::NumberResult::Number => LuaType::Number,
                                        }
                                    }
                                    Some(LuaLiteralToken::Bool(value)) => {
                                        LuaType::BooleanConst(value.is_true())
                                    }
                                    Some(LuaLiteralToken::Nil(_)) => LuaType::Nil,
                                    _ => LuaType::Unknown,
                                };
                                instructions
                                    .push(ExpressionInstruction::PushValue(backend.plain(ty)));
                            }
                        }
                        LuaExpr::NameExpr(name) => {
                            if let Some(text) = name.get_name_text() {
                                instructions.push(ExpressionInstruction::LoadName {
                                    name: text.to_string(),
                                    offset: name.get_position().into(),
                                });
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        LuaExpr::IndexExpr(index) => {
                            let index_key = index.get_index_key();
                            let key = index_key.as_ref().and_then(member_key);
                            if let Some(prefix) = index.get_prefix_expr() {
                                if let Some(LuaIndexKey::Expr(expr)) = index_key {
                                    tasks.push(Task::Index { key, dynamic: true });
                                    tasks.push(Task::Expr(expr));
                                    tasks.push(Task::Expr(prefix));
                                } else {
                                    tasks.push(Task::Index {
                                        key,
                                        dynamic: false,
                                    });
                                    tasks.push(Task::Expr(prefix));
                                }
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        LuaExpr::CallExpr(call) => {
                            let arg_count = call.get_args_count().unwrap_or(0);
                            let mut args = Vec::new();
                            let mut arg_syntaxes = Vec::new();
                            if let Some(list) = call.get_args_list() {
                                for arg in list.get_args() {
                                    backend.checkpoint()?;
                                    arg_syntaxes.push(arg.get_syntax_id());
                                    args.push(arg);
                                }
                            }
                            if let Some(prefix) = call.get_prefix_expr() {
                                let mut generic_type_syntaxes = Vec::new();
                                if let Some(list) = call.get_call_generic_type_list() {
                                    for ty in list.get_types() {
                                        backend.checkpoint()?;
                                        generic_type_syntaxes.push(ty.get_syntax_id());
                                    }
                                }
                                tasks.push(Task::Call {
                                    arg_count,
                                    colon_call: call.is_colon_call(),
                                    generic_type_syntaxes,
                                    arg_syntaxes,
                                });
                                for arg in args.into_iter().rev() {
                                    backend.checkpoint()?;
                                    tasks.push(Task::Expr(arg));
                                }
                                tasks.push(Task::Expr(prefix));
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        LuaExpr::BinaryExpr(binary) => {
                            if let (Some(op), Some((left, right))) = (
                                binary.get_op_token().map(|token| token.get_op()),
                                binary.get_exprs(),
                            ) {
                                tasks.push(Task::BinaryRight {
                                    op,
                                    right,
                                    left_syntax: left.get_syntax_id(),
                                });
                                tasks.push(Task::Expr(left));
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        LuaExpr::ClosureExpr(closure) => {
                            instructions.push(ExpressionInstruction::PushClosure {
                                syntax: closure.get_syntax_id(),
                            })
                        }
                        LuaExpr::ParenExpr(paren) => {
                            if let Some(inner) = paren.get_expr() {
                                tasks.push(Task::Expr(inner));
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        LuaExpr::TableExpr(table) => {
                            instructions.push(ExpressionInstruction::PushValue(backend.plain(
                                LuaType::TableConst(InFiled::new(site.file_id, table.get_range())),
                            )))
                        }
                        LuaExpr::UnaryExpr(unary) => {
                            let op = unary.get_op_token().map(|token| token.get_op());
                            // Preserve the fixed compiler: `not` does not visit
                            // its operand and cannot fabricate selected receipts.
                            if op == Some(UnaryOperator::OpNot) {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Boolean),
                                ));
                            } else if let Some(inner) = unary.get_expr() {
                                tasks.push(Task::Unary {
                                    op: op.unwrap_or(UnaryOperator::OpUnm),
                                });
                                tasks.push(Task::Expr(inner));
                            } else {
                                instructions.push(ExpressionInstruction::PushValue(
                                    backend.plain(LuaType::Unknown),
                                ));
                            }
                        }
                        _ => instructions.push(ExpressionInstruction::PushValue(
                            backend.plain(LuaType::Unknown),
                        )),
                    }
                }
                Task::Index { key, dynamic } => {
                    instructions.push(ExpressionInstruction::IndexMember { key, dynamic })
                }
                Task::Call {
                    arg_count,
                    colon_call,
                    generic_type_syntaxes,
                    arg_syntaxes,
                } => instructions.push(ExpressionInstruction::Call {
                    arg_count,
                    colon_call,
                    generic_type_syntaxes,
                    arg_syntaxes,
                }),
                Task::Unary { op } => instructions.push(ExpressionInstruction::Unary { op }),
                Task::BinaryRight {
                    op,
                    right,
                    left_syntax,
                } => {
                    tasks.push(Task::BinaryEmit {
                        op,
                        left_syntax,
                        right_syntax: right.get_syntax_id(),
                    });
                    tasks.push(Task::Expr(right));
                }
                Task::BinaryEmit {
                    op,
                    left_syntax,
                    right_syntax,
                } => instructions.push(ExpressionInstruction::Binary {
                    op,
                    left_syntax: Some(left_syntax),
                    right_syntax: Some(right_syntax),
                }),
            }
        }
        backend.checkpoint()?;
        instructions.push(ExpressionInstruction::Result);
        Ok(ExpressionPlan {
            file_id: site.file_id,
            instructions,
        })
    }
}

fn member_key(key: &LuaIndexKey) -> Option<LuaMemberKey> {
    match key {
        LuaIndexKey::Name(name) => Some(LuaMemberKey::Name(name.get_name_text().into())),
        LuaIndexKey::String(string) => Some(LuaMemberKey::Name(string.get_value().into())),
        LuaIndexKey::Integer(integer) => match integer.get_number_value() {
            emmylua_parser::NumberResult::Int(value) => Some(LuaMemberKey::Integer(value)),
            emmylua_parser::NumberResult::Uint(value) => Some(LuaMemberKey::Integer(value as i64)),
            _ => None,
        },
        LuaIndexKey::Idx(index) => Some(LuaMemberKey::Integer(*index as i64)),
        LuaIndexKey::Expr(_) => None,
    }
}
