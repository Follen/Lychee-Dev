#[path = "support/expression_execution_cases.rs"]
mod support;
use emmylua_code_analysis::LuaType;
use emmylua_parser::BinaryOperator::*;
use lycheedev_emmy_adapter::session::*;
use std::sync::Arc;
use support::{Backend, Marker, Stop, Value, load};

fn value(ty: LuaType) -> Value {
    Value {
        ty,
        marker: None,
        slots: vec![],
    }
}

#[test]
fn contextual_unknown_any_gate_precedes_every_nonlogical_operator() {
    let (analysis, query, provider) = load("return 1", "1");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::new(provider.events.clone());
    for op in [
        OpAdd,
        OpSub,
        OpMul,
        OpDiv,
        OpIDiv,
        OpMod,
        OpPow,
        OpBAnd,
        OpBOr,
        OpBXor,
        OpShl,
        OpShr,
        OpShrAthrimetic,
        OpConcat,
        OpLt,
        OpLe,
        OpGt,
        OpGe,
        OpEq,
        OpNe,
        OpNilCoalescing,
        OpNop,
    ] {
        for unknown in [LuaType::Unknown, LuaType::Any] {
            for left_unknown in [false, true] {
                let (left, right) = if left_unknown {
                    (unknown.clone(), LuaType::IntegerConst(4))
                } else {
                    (LuaType::IntegerConst(4), unknown.clone())
                };
                let PrimitiveBinaryProjection::Complete(result) = projector
                    .project_primitive_binary(
                        query.file_id,
                        op,
                        value(left),
                        value(right),
                        &mut backend,
                    )
                    .unwrap()
                else {
                    panic!("nonlogical operation deferred")
                };
                assert_eq!(
                    result.ty,
                    LuaType::Unknown,
                    "{op:?} {unknown:?} left={left_unknown}"
                );
            }
        }
    }
    assert!(
        session
            .coverage_checked(|| Ok::<_, Stop>(()))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn integer_document_constants_comparisons_and_overflow_match_fixed_rules() {
    let (analysis, query, provider) = load("return 1", "1");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::new(provider.events.clone());
    for (op, left, right, expected) in [
        (
            OpAdd,
            LuaType::IntegerConst(4),
            LuaType::DocIntegerConst(5),
            LuaType::IntegerConst(9),
        ),
        (
            OpSub,
            LuaType::DocIntegerConst(4),
            LuaType::IntegerConst(5),
            LuaType::IntegerConst(-1),
        ),
        (
            OpMul,
            LuaType::DocIntegerConst(-4),
            LuaType::DocIntegerConst(5),
            LuaType::IntegerConst(-20),
        ),
        (
            OpAdd,
            LuaType::IntegerConst(i64::MAX),
            LuaType::IntegerConst(1),
            LuaType::Number,
        ),
        (
            OpSub,
            LuaType::IntegerConst(i64::MIN),
            LuaType::IntegerConst(1),
            LuaType::Number,
        ),
        (
            OpMul,
            LuaType::IntegerConst(i64::MIN),
            LuaType::IntegerConst(-1),
            LuaType::Number,
        ),
        (
            OpAdd,
            LuaType::Integer,
            LuaType::DocIntegerConst(1),
            LuaType::Integer,
        ),
        (OpSub, LuaType::Integer, LuaType::Integer, LuaType::Integer),
        (
            OpAdd,
            LuaType::FloatConst(1.5),
            LuaType::IntegerConst(2),
            LuaType::Number,
        ),
        (OpAdd, LuaType::Nil, LuaType::Nil, LuaType::Number),
        (
            OpConcat,
            LuaType::IntegerConst(1),
            LuaType::Nil,
            LuaType::String,
        ),
        (
            OpLt,
            LuaType::DocIntegerConst(4),
            LuaType::IntegerConst(5),
            LuaType::BooleanConst(true),
        ),
        (
            OpLe,
            LuaType::IntegerConst(5),
            LuaType::DocIntegerConst(5),
            LuaType::BooleanConst(true),
        ),
        (
            OpGt,
            LuaType::IntegerConst(4),
            LuaType::IntegerConst(5),
            LuaType::BooleanConst(false),
        ),
        (
            OpGe,
            LuaType::IntegerConst(4),
            LuaType::IntegerConst(5),
            LuaType::BooleanConst(false),
        ),
        (
            OpEq,
            LuaType::IntegerConst(4),
            LuaType::DocIntegerConst(4),
            LuaType::BooleanConst(true),
        ),
        (
            OpNe,
            LuaType::IntegerConst(4),
            LuaType::IntegerConst(4),
            LuaType::BooleanConst(false),
        ),
        (
            OpEq,
            LuaType::FloatConst(4.0),
            LuaType::IntegerConst(4),
            LuaType::Boolean,
        ),
        (
            OpBAnd,
            LuaType::IntegerConst(4),
            LuaType::IntegerConst(5),
            LuaType::Unknown,
        ),
    ] {
        let PrimitiveBinaryProjection::Complete(result) = projector
            .project_primitive_binary(
                query.file_id,
                op,
                value(left.clone()),
                value(right.clone()),
                &mut backend,
            )
            .unwrap()
        else {
            panic!("nonlogical operation deferred")
        };
        assert_eq!(result.ty, expected, "{left:?} {op:?} {right:?}");
    }
    for op in [OpDiv, OpIDiv, OpMod, OpPow] {
        let PrimitiveBinaryProjection::Complete(result) = projector
            .project_primitive_binary(
                query.file_id,
                op,
                value(LuaType::IntegerConst(4)),
                value(LuaType::IntegerConst(2)),
                &mut backend,
            )
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(result.ty, LuaType::Number);
    }
}

#[test]
fn logical_fast_paths_move_exact_carrier_before_unknown_gate_and_defer_other_work() {
    let (analysis, query, provider) = load("return {}", "{}");
    let table = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::new(provider.events.clone());
    let marker = Arc::new(Marker {
        identity: "object".into(),
        epoch: "epoch".into(),
        token: Arc::new(()),
    });
    for (op, left) in [(OpAnd, LuaType::Nil), (OpOr, LuaType::Table)] {
        let left = Value {
            ty: left,
            marker: Some(marker.clone()),
            slots: vec![],
        };
        let PrimitiveBinaryProjection::Complete(result) = projector
            .project_primitive_binary(
                query.file_id,
                op,
                left,
                value(LuaType::Unknown),
                &mut backend,
            )
            .unwrap()
        else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(result.marker.as_ref().unwrap(), &marker));
    }
    for (op, left, right) in [
        (OpAnd, LuaType::BooleanConst(true), table),
        (OpOr, LuaType::Nil, LuaType::IntegerConst(7)),
        (OpAnd, LuaType::Boolean, LuaType::String),
    ] {
        let right = Value {
            ty: right,
            marker: Some(marker.clone()),
            slots: vec![],
        };
        let PrimitiveBinaryProjection::DeferredLogical {
            op: actual_op,
            left: actual_left,
            right: actual_right,
        } = projector
            .project_primitive_binary(query.file_id, op, value(left.clone()), right, &mut backend)
            .unwrap()
        else {
            panic!("table/union logical work must not disappear")
        };
        assert_eq!(actual_op, op);
        assert_eq!(actual_left.ty, left);
        assert!(Arc::ptr_eq(actual_right.marker.as_ref().unwrap(), &marker));
    }
}

#[test]
fn actual_operator_overloads_choose_left_before_right_and_skip_on_unknown() {
    let source = "---@class Left\n---@operator add: string\nlocal Left={}\n---@class Right\n---@operator add: boolean\nlocal Right={}\n---@type Left\nlocal a\n---@type Right\nlocal b\nreturn a+b";
    let (analysis, query, provider) = load(source, "a+b");
    let model = analysis.semantic_model(query.file_id);
    let a = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "a")
        .unwrap()
        .0;
    let b = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "b")
        .unwrap()
        .0;
    let left = model.type_of_expr(a.syntax);
    let right = model.type_of_expr(b.syntax);
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::new(provider.events.clone());
    for (left, right, expected) in [
        (left.clone(), right.clone(), LuaType::String),
        (LuaType::IntegerConst(1), right, LuaType::Boolean),
        (left, LuaType::Unknown, LuaType::Unknown),
    ] {
        let PrimitiveBinaryProjection::Complete(result) = projector
            .project_primitive_binary(
                query.file_id,
                OpAdd,
                value(left),
                value(right),
                &mut backend,
            )
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(result.ty, expected);
    }
}

#[test]
fn each_binary_checkpoint_returns_the_original_typed_failure_without_coverage() {
    let (analysis, query, provider) = load("return 1+2", "1+2");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut full = Backend::new(provider.events.clone());
    projector
        .project_primitive_binary(
            query.file_id,
            OpAdd,
            value(LuaType::IntegerConst(1)),
            value(LuaType::IntegerConst(2)),
            &mut full,
        )
        .unwrap();
    for ordinal in 1..=full.checkpoints {
        let mut backend = Backend::new(provider.events.clone());
        backend.stop_at = ordinal;
        let token = backend.token.clone();
        let error = projector
            .project_primitive_binary(
                query.file_id,
                OpAdd,
                value(LuaType::IntegerConst(1)),
                value(LuaType::IntegerConst(2)),
                &mut backend,
            )
            .unwrap_err();
        assert_eq!(error.ordinal, ordinal);
        assert!(Arc::ptr_eq(&error.token, &token));
        assert!(
            session
                .coverage_checked(|| Ok::<_, Stop>(()))
                .unwrap()
                .is_empty()
        );
    }
}
