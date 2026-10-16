//! Original-scope compiler controls. All failures are typed, never inferred
//! from sqlparser's diagnostic text or a raw cancellation token.
use super::*;
use crate::resource::{Budget, Cost};
use lycheedev_core::control::{OperationControl, StopCause};
use std::{cell::Cell, sync::Arc};

fn live() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn budget_for(control: &OperationControl) -> Budget {
    Budget::controlled(Cost::defaults(), control.deadline(), control.clone())
}
#[test]
fn compiler_invocation_stop_precedes_bytes_nul_syntax_and_independent_budget() {
    let control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    let cancelled = Arc::new(AtomicBool::new(true));
    let budget = Budget::new(Cost::defaults(), Instant::now(), Some(cancelled));
    for sql in [
        "SELECT FROM t".to_owned(),
        "\0".into(),
        "x".repeat((256 << 10) + 1),
    ] {
        assert_eq!(
            Program::compile_controlled(&sql, &control, &budget)
                .unwrap_err()
                .code,
            "command.deadline"
        );
    }
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
}
#[test]
fn compiler_independent_budget_cancel_and_deadline_keep_their_actual_error() {
    let control = live();
    let cancelled = Budget::new(
        Cost::defaults(),
        control.deadline(),
        Some(Arc::new(AtomicBool::new(true))),
    );
    let expired = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    assert_eq!(
        Program::compile_controlled("SELECT FROM t", &control, &cancelled)
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
    assert_eq!(
        Program::compile_controlled("SELECT FROM t", &control, &expired)
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(control.stop_cause().unwrap(), None);
}
#[test]
fn compiler_success_keeps_both_original_controls_ordered_and_usage_unchanged() {
    let control = live();
    let separate = live();
    let budget = budget_for(&separate);
    budget.charge(Cost::metadata(29)).unwrap();
    let before = budget.snapshot().unwrap();
    let sql = "WITH c AS (SELECT ID FROM t) SELECT x.ID,(SELECT COUNT(*) FROM c) FROM c x WHERE EXISTS(SELECT ID FROM t WHERE ID=x.ID) UNION ALL SELECT ID,1 FROM t ORDER BY ID";
    let actual = Program::compile_controlled(sql, &control, &budget).unwrap();
    assert_eq!(actual, Program::compile(sql).unwrap());
    assert_eq!(budget.snapshot().unwrap(), before);
    assert_eq!(budget.logical_snapshot().unwrap(), before);
    for original in [&control, &separate] {
        assert_eq!(original.stop_cause().unwrap(), None);
        original
            .ordered_child(original.deadline())
            .unwrap()
            .check()
            .unwrap();
    }
}
#[test]
fn compiler_lexical_checkpoint_precedes_the_same_iteration_token_limit() {
    let checks = Cell::new(0usize);
    // Every punctuation occupies one byte and exactly one lexer iteration.
    let sql = ",".repeat(32769);
    let error = lexical::tokens_checked(&sql, &|| {
        let next = checks.get() + 1;
        checks.set(next);
        if next == 32770 {
            Err(fault("fixture.lexical_stop", 7, "original checkpoint"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.lexical_stop");
    assert_eq!(checks.get(), 32770);
}
#[test]
fn compiler_lexical_mid_scan_stop_keeps_typed_details_and_does_not_replay() {
    let checks = Cell::new(0usize);
    let error = lexical::tokens_checked("SELECT ID,Name FROM t", &|| {
        checks.set(checks.get() + 1);
        if checks.get() == 5 {
            let mut error = fault("fixture.scan_stopped", 7, "source stop");
            error.details = serde_json::json!({"checkpoint":5});
            Err(error)
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(checks.get(), 5);
    assert_eq!(error.details, serde_json::json!({"checkpoint":5}));
}
#[test]
fn compiler_dialect_stop_is_retained_across_parser_error_and_alias_fast_path() {
    for sql in [
        "SELECT ID FROM t",
        "SELECT * FROM t",
        "SELECT EXISTS(SELECT ID FROM t) FROM t",
    ] {
        let control = live();
        let budget = budget_for(&live());
        let dialect = syntax::QueryDialect::controlled(&control, &budget);
        let tokens = lexical::tokens(sql).unwrap();
        control.tracked_cancel_handle().unwrap().cancel();
        let mut parser = Parser::new(&dialect).with_tokens_with_locations(tokens);
        let _parsed = parser.parse_query();
        assert_eq!(
            dialect.take_stop().unwrap_err().code,
            "command.cancelled",
            "{sql}"
        );
        assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
    }
}
#[test]
fn compiler_lowering_checks_nested_queries_and_terms_before_progress() {
    let dialect = syntax::QueryDialect::default();
    let sql = "WITH c AS (SELECT ID FROM t) SELECT (SELECT ID FROM c),COALESCE(ID,2),CASE WHEN ID=1 THEN 2 ELSE 3 END FROM t";
    let mut query = Parser::new(&dialect)
        .with_tokens_with_locations(lexical::tokens(sql).unwrap())
        .parse_query()
        .unwrap();
    let checks = Cell::new(0usize);
    let error = Program::compile_query(&mut query, 0, &|| {
        checks.set(checks.get() + 1);
        if checks.get() == 11 {
            Err(fault("fixture.lower_stop", 7, "original lowering stop"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.lower_stop");
    assert_eq!(checks.get(), 11);
}
#[test]
fn compiler_logical_inspection_preserves_stop_before_node_depth_budget() {
    let program = Program::compile("SELECT ID+1 FROM t").unwrap();
    let calls = Cell::new(0);
    let error = subquery::validate_nodes(&program, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            Err(fault("fixture.inspect_stop", 7, "inspection stopped"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.inspect_stop");
    assert_eq!(calls.get(), 3);
}
#[test]
fn compiler_admission_checks_each_retained_parser_frame() {
    let dialect = syntax::QueryDialect::default();
    let tokens = lexical::tokens("SELECT (((ID))),ID+2 FROM t").unwrap();
    let query = Parser::new(&dialect)
        .with_tokens_with_locations(tokens.clone())
        .parse_query()
        .unwrap();
    let calls = Cell::new(0);
    let error = parser_admission::validate(&query, &tokens, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 4 {
            Err(fault("fixture.admission_stop", 7, "admission stopped"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.admission_stop");
    assert_eq!(calls.get(), 4);
}

#[test]
fn lowering_moves_each_query_expression_and_preserves_nested_parameters() {
    for (sql, expected) in [
        (
            "SELECT (SELECT :inner FROM t WHERE ID=:id) FROM t",
            vec!["id", "inner"],
        ),
        (
            "SELECT EXISTS(SELECT :inner FROM t WHERE ID=:id) FROM t",
            vec!["id", "inner"],
        ),
        (
            "SELECT :probe IN (SELECT :inner FROM t WHERE ID=:id) FROM t",
            vec!["id", "inner", "probe"],
        ),
    ] {
        let dialect = syntax::QueryDialect::default();
        let mut parser =
            Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(sql).unwrap());
        let mut query = retrieval::parse(&dialect, &mut parser).unwrap();
        let SetExpr::Select(select) = query.body.as_mut() else {
            panic!("SELECT fixture")
        };
        let SelectItem::UnnamedExpr(expr) = &mut select.projection[0] else {
            panic!("expression fixture")
        };
        let mut parameters = BTreeSet::new();
        let lowered = term(expr, &mut parameters, 0, &|| Ok(())).unwrap();
        assert_eq!(
            parameters.iter().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        assert!(matches!(expr, Expr::Value(value) if value.value == Value::Null));
        let child = match &lowered {
            Term::ScalarSubquery(query) | Term::Exists(query) | Term::InSubquery(_, query, _) => {
                query
            }
            other => panic!("query child lost: {other:?}"),
        };
        assert_eq!(child.source_tables(), ["t"]);
        assert_eq!(
            child
                .parameters
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["id", "inner"]
        );
    }
}

#[test]
fn iterative_union_lowering_keeps_branch_scope_tail_and_original_stop() {
    let sql = "SELECT ID FROM t UNION ALL WITH b AS (SELECT ID FROM u) SELECT ID FROM b UNION ALL SELECT ID FROM v ORDER BY ID LIMIT 1";
    let dialect = syntax::QueryDialect::default();
    let parse = || {
        let mut parser =
            Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(sql).unwrap());
        retrieval::parse(&dialect, &mut parser).unwrap()
    };
    let branches = compound::branches(parse().body, &|| Ok(())).unwrap();
    assert_eq!(branches.len(), 3);
    assert!(branches[0].with.is_none());
    assert!(branches[0].order_by.is_none());
    assert!(branches[0].limit_clause.is_none());
    assert_eq!(
        branches[1].with.as_ref().unwrap().cte_tables[0]
            .alias
            .name
            .value,
        "b"
    );
    assert!(branches[1].order_by.is_none());
    assert!(branches[1].limit_clause.is_none());
    assert!(branches[2].with.is_none());
    assert!(branches[2].order_by.is_some());
    assert!(branches[2].limit_clause.is_some());
    let calls = Cell::new(0);
    let error = compound::branches(parse().body, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 7 {
            Err(fault(
                "fixture.union_stop",
                7,
                "original union traversal stop",
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.union_stop");
    assert_eq!(calls.get(), 7);
}
