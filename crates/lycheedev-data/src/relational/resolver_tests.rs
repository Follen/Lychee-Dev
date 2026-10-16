//! Real preflight resolution order and query-owned schema/provider reuse.
//! Fixed Go 3.2.3 execute.go caches the resolver, not a replayed planning pass.
use super::*;
use std::{cell::RefCell, rc::Rc};
type Events = Rc<RefCell<Vec<String>>>;
struct Fixture {
    schema: Source,
    rows: Vec<Vec<Scalar>>,
    label: String,
    events: Events,
}
impl RowProvider for Fixture {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        self.events
            .borrow_mut()
            .push(format!("scan:{}", self.label));
        for row in &self.rows {
            emit(row)?;
        }
        Ok(())
    }
}
struct Resolver {
    roots: BTreeMap<(String, String), Fixture>,
    events: Events,
}
impl Resolver {
    fn new(names: &[&str]) -> Self {
        let events = Events::default();
        let roots = names
            .iter()
            .map(|name| {
                (
                    ("static".into(), (*name).into()),
                    Fixture {
                        schema: Source {
                            columns: vec!["ID".into()],
                            rows: vec![],
                            complete: true,
                        },
                        rows: vec![vec![Scalar::Int(1)]],
                        label: (*name).into(),
                        events: events.clone(),
                    },
                )
            })
            .collect();
        Self { roots, events }
    }
    fn execute(&self, sql: &str) -> Result<QueryResult> {
        Program::compile(sql)?.execute_resolved_budgeted(
            self,
            &BTreeMap::new(),
            Limits::default(),
            None,
            None,
        )
    }
}
impl RootResolver for Resolver {
    fn resolve(&self, table: SourceTableUse<'_>) -> Result<&Source> {
        self.events
            .borrow_mut()
            .push(format!("resolve:{}:{}", table.catalog, table.name));
        self.roots
            .get(&(
                identifier_lower(table.catalog),
                identifier_lower(table.name),
            ))
            .map(|root| &root.schema)
            .ok_or_else(|| fault("fixture.missing_root", 5, "missing physical source"))
    }
    fn provider(&self, schema: &Source) -> Option<&dyn RowProvider> {
        self.roots
            .values()
            .find(|root| std::ptr::eq(&root.schema, schema))
            .map(|root| root as &dyn RowProvider)
    }
}
fn resolved(events: &Events) -> Vec<String> {
    events
        .borrow()
        .iter()
        .filter(|event| event.starts_with("resolve:"))
        .cloned()
        .collect()
}

#[test]
fn resolver_uses_real_input_order_then_scans_without_second_resolution() {
    let resolver = Resolver::new(&["a", "z"]);
    let program = Program::compile("SELECT z.ID FROM z CROSS JOIN a").unwrap();
    assert_eq!(program.source_tables(), ["a", "z"]);
    let result = program
        .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert_eq!(
        *resolver.events.borrow(),
        ["resolve:static:z", "resolve:static:a", "scan:a", "scan:z"]
    );
}

#[test]
fn first_schema_binding_failure_precedes_later_physical_resolution() {
    let resolver = Resolver::new(&["z"]);
    let error = resolver
        .execute("SELECT Missing FROM z UNION ALL SELECT ID FROM absent")
        .unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}

#[test]
fn normalized_physical_root_is_shared_across_ctes_branches_and_both_passes() {
    let resolver = Resolver::new(&["z"]);
    let result = resolver.execute(r#"WITH "a.b" AS (SELECT ID FROM z) SELECT ID FROM "a.b" UNION ALL SELECT ID FROM static.Z UNION ALL SELECT ID FROM z"#).unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]; 3]);
    assert_eq!(resolved(&resolver.events), ["resolve:static:z"]);
    assert_eq!(
        resolver
            .events
            .borrow()
            .iter()
            .filter(|event| event.starts_with("scan:"))
            .count(),
        3
    );
}

#[test]
fn missing_root_is_returned_once_without_retry_or_later_input() {
    let resolver = Resolver::new(&["z"]);
    let error = resolver
        .execute("SELECT ID FROM absent UNION ALL SELECT ID FROM z")
        .unwrap_err();
    assert_eq!(error.code, "fixture.missing_root");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:absent"]);
}

#[test]
fn explain_callback_can_resolve_physical_root_absent_from_syntactic_dependencies() {
    let resolver = Resolver::new(&["a", "b", "hidden"]);
    let sql = "EXPLAIN SELECT ID IN (WITH hidden AS (SELECT ID FROM b) SELECT ID FROM a UNION ALL SELECT ID FROM a ORDER BY ID IN (SELECT ID FROM hidden)) FROM a";
    let program = Program::compile(sql).unwrap();
    assert_eq!(program.source_tables(), ["a", "b"]);
    assert!(
        program
            .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
            .unwrap()
            .plan
            .is_some()
    );
    assert_eq!(
        *resolver.events.borrow(),
        [
            "resolve:static:a",
            "resolve:static:b",
            "resolve:static:hidden"
        ]
    );
}

#[test]
fn reserved_forward_cte_never_falls_through_to_root_resolver() {
    let resolver = Resolver::new(&["base", "forward"]);
    let error = resolver.execute("WITH first AS (SELECT ID FROM forward), forward AS (SELECT ID FROM base) SELECT ID FROM first").unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    assert!(resolver.events.borrow().is_empty());
}

#[test]
fn schema_memory_is_charged_once_to_original_executor_allowance() {
    let resolver = Resolver::new(&["z"]);
    let mut work = Work {
        left: 100,
        bytes: 4096,
        deadline: Instant::now() + Duration::from_secs(10),
        cancel: None,
        shared: None,
        planning: true,
        temporaries: Vec::new(),
        selection_sources: vec![],
        providers: &[],
        resolver: Some(&resolver),
        roots: BTreeMap::new(),
        measure: false,
        scans: Vec::new(),
    };
    let first = work
        .resolve_root(&RelationName::from_key("z").unwrap())
        .unwrap();
    let remaining = work.bytes;
    assert_eq!(remaining, 4096 - 128 - 32 - 2);
    work.planning = false;
    let second = work
        .resolve_root(&RelationName::from_key("static.Z").unwrap())
        .unwrap();
    assert!(std::ptr::eq(first, second));
    assert_eq!(work.bytes, remaining);
    assert_eq!(work.left, 100);
    assert_eq!(resolved(&resolver.events), ["resolve:static:z"]);
}

#[test]
fn original_stop_and_small_work_limit_prevent_any_resolver_callback() {
    let resolver = Resolver::new(&["z"]);
    let program = Program::compile("SELECT ID FROM z").unwrap();
    let cancel = AtomicBool::new(true);
    assert_eq!(
        program
            .execute_resolved_budgeted(
                &resolver,
                &BTreeMap::new(),
                Limits::default(),
                Some(&cancel),
                None
            )
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
    assert_eq!(
        program
            .execute_resolved_budgeted(
                &resolver,
                &BTreeMap::new(),
                Limits {
                    work: 1,
                    ..Limits::default()
                },
                None,
                None
            )
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
    assert!(resolver.events.borrow().is_empty());
}

#[test]
fn root_schema_budget_failure_never_replays_preflight_or_scans() {
    let resolver = Resolver::new(&["z"]);
    let program = Program::compile("SELECT ID FROM z").unwrap();
    // This root requires 128 + 32 + len("ID") bytes before binding.
    let error = program
        .execute_resolved_budgeted(
            &resolver,
            &BTreeMap::new(),
            Limits {
                memory_bytes: 161,
                ..Limits::default()
            },
            None,
            None,
        )
        .unwrap_err();
    assert_eq!(error.code, "relational.resource_limit");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}

#[test]
fn physical_input_failure_precedes_a_later_derived_input_resolution() {
    let resolver = Resolver::new(&["later"]);
    let error = resolver
        .execute("SELECT d.ID FROM absent CROSS JOIN (SELECT ID FROM later) d")
        .unwrap_err();
    assert_eq!(error.code, "fixture.missing_root");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:absent"]);
}
#[test]
fn invalid_physical_schema_precedes_later_derived_input() {
    let mut resolver = Resolver::new(&["bad", "later"]);
    resolver
        .roots
        .get_mut(&("static".into(), "bad".into()))
        .unwrap()
        .schema
        .columns
        .clear();
    assert_eq!(
        resolver
            .execute("SELECT d.ID FROM bad CROSS JOIN (SELECT ID FROM later) d")
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert_eq!(*resolver.events.borrow(), ["resolve:static:bad"]);
}
#[test]
fn duplicate_alias_is_rejected_before_that_derived_body_resolves() {
    let resolver = Resolver::new(&["a", "later"]);
    assert_eq!(
        resolver
            .execute("SELECT a.ID FROM a CROSS JOIN (SELECT ID FROM later) a")
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert_eq!(*resolver.events.borrow(), ["resolve:static:a"]);
}
#[test]
fn derived_execution_in_relation_order_finishes_before_right_materialization() {
    let resolver = Resolver::new(&["a", "b", "c"]);
    resolver
        .execute("SELECT d.ID FROM a CROSS JOIN (SELECT ID FROM b) d CROSS JOIN c")
        .unwrap();
    assert_eq!(
        *resolver.events.borrow(),
        [
            "resolve:static:a",
            "resolve:static:b",
            "resolve:static:c",
            "scan:b",
            "scan:c",
            "scan:a"
        ]
    );
}

#[test]
fn unknown_function_binding_waits_for_the_real_source_resolver() {
    let resolver = Resolver::new(&["a"]);
    let program = Program::compile("SELECT UNKNOWN_CALL(ID) FROM absent").unwrap();
    assert_eq!(
        program
            .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
            .unwrap_err()
            .code,
        "fixture.missing_root"
    );
    assert_eq!(*resolver.events.borrow(), ["resolve:static:absent"]);
    resolver.events.borrow_mut().clear();
    assert_eq!(
        resolver
            .execute("SELECT UNKNOWN_CALL(ID) FROM a")
            .unwrap_err()
            .code,
        "relational.unsupported"
    );
    assert_eq!(*resolver.events.borrow(), ["resolve:static:a"]);
}

#[test]
fn malformed_functions_compile_then_preserve_first_physical_error() {
    for call in [
        "COUNT()",
        "SUM()",
        "COUNT(ID, Missing)",
        "SUM(*)",
        "COUNT(t.*)",
        "COUNT(DISTINCT *)",
        "COUNT(DISTINCT)",
        "LOWER(DISTINCT ID)",
        "UNKNOWN(DISTINCT ID)",
        "LOWER(*)",
        "SUM(COUNT(*))",
        "SUM(*+Missing)",
    ] {
        let sql = format!("SELECT {call} FROM absent t");
        let program = Program::compile(&sql).unwrap_or_else(|e| panic!("{sql}: compile {e:?}"));
        let resolver = Resolver::new(&["t"]);
        let error = program
            .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
            .unwrap_err();
        assert_eq!(error.code, "fixture.missing_root", "{sql}: {error:?}");
        assert_eq!(
            *resolver.events.borrow(),
            ["resolve:static:absent"],
            "{sql}"
        );
    }
}

#[test]
fn malformed_function_binding_does_not_scan_or_enter_later_arguments() {
    for (call, code) in [
        ("COUNT()", "relational.type_mismatch"),
        ("COUNT(DISTINCT)", "relational.type_mismatch"),
        (
            "SUM(ID, (SELECT ID FROM absent))",
            "relational.type_mismatch",
        ),
        ("SUM(*)", "relational.type_mismatch"),
        ("COUNT(z.*)", "relational.type_mismatch"),
        ("COUNT(DISTINCT *)", "relational.type_mismatch"),
        ("LOWER(DISTINCT Missing)", "relational.type_mismatch"),
        (
            "UNKNOWN(DISTINCT (SELECT ID FROM absent))",
            "relational.type_mismatch",
        ),
        ("UNKNOWN((SELECT ID FROM absent))", "relational.unsupported"),
        ("COALESCE(1, *)", "relational.type_mismatch"),
        ("SUM(COUNT(*))", "relational.type_mismatch"),
        ("SUM(*+Missing)", "relational.type_mismatch"),
        ("SUM(Missing+*)", "relational.invalid_binding"),
    ] {
        let resolver = Resolver::new(&["z"]);
        let sql = format!("SELECT {call} FROM z");
        let error = resolver.execute(&sql).unwrap_err();
        assert_eq!(error.code, code, "{sql}: {error:?}");
        assert_eq!(*resolver.events.borrow(), ["resolve:static:z"], "{sql}");
    }
}

#[test]
fn malformed_aggregate_arguments_still_collect_required_parameters() {
    let resolver = Resolver::new(&["z"]);
    let program = Program::compile("SELECT SUM(:first, :second) FROM absent").unwrap();
    assert_eq!(
        program.parameters,
        BTreeSet::from(["first".into(), "second".into()])
    );
    let error = program
        .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
        .unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    assert!(resolver.events.borrow().is_empty());
}

#[test]
fn malformed_join_call_waits_for_each_input_schema() {
    let resolver = Resolver::new(&["z"]);
    let error = resolver
        .execute("SELECT z.ID FROM z JOIN absent a ON LOWER(DISTINCT z.ID)=a.ID")
        .unwrap_err();
    assert_eq!(error.code, "fixture.missing_root");
    assert_eq!(
        *resolver.events.borrow(),
        ["resolve:static:z", "resolve:static:absent"]
    );
}

#[test]
fn valid_nested_query_aggregates_keep_separate_binding_scopes() {
    let resolver = Resolver::new(&["z"]);
    let result = resolver
        .execute("SELECT SUM((SELECT COUNT(*) FROM z)) FROM z")
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert_eq!(resolved(&resolver.events), ["resolve:static:z"]);
}

#[test]
fn invalid_pagination_is_syntax_before_any_source_resolution() {
    for suffix in [
        "LIMIT -1",
        "LIMIT +1",
        "LIMIT 1.0",
        "LIMIT 1e0",
        "LIMIT 1L",
        "LIMIT (1)",
        "LIMIT LOWER('1')",
        "LIMIT COUNT(*)",
        "LIMIT NULL",
        "LIMIT ID",
        "OFFSET -1",
        "OFFSET (0)",
        "OFFSET COALESCE(0)",
        "OFFSET 1 ROWS",
        "OFFSET 1 LIMIT 2",
        "LIMIT 1,2",
        "LIMIT 1 BY ID",
        "LIMIT ALL",
    ] {
        let resolver = Resolver::new(&[]);
        let sql = format!("SELECT ID FROM absent {suffix}");
        let error = resolver.execute(&sql).unwrap_err();
        assert_eq!(error.code, "relational.invalid_syntax", "{sql}: {error:?}");
        assert!(resolver.events.borrow().is_empty(), "{sql}");
    }
}

#[test]
fn negative_pagination_parameter_is_runtime_type_error_after_resolve() {
    let resolver = Resolver::new(&["z"]);
    let program = Program::compile("SELECT ID FROM z LIMIT :n").unwrap();
    let error = program
        .execute_resolved_budgeted(
            &resolver,
            &BTreeMap::from([("n".into(), Scalar::Int(-1))]),
            Limits::default(),
            None,
            None,
        )
        .unwrap_err();
    assert_eq!(error.code, "relational.type_mismatch");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}

#[test]
fn grouped_root_star_is_binding_error_before_any_scan() {
    for sql in [
        "SELECT COUNT(*) FROM z HAVING *",
        "SELECT COUNT(*) FROM z ORDER BY *",
    ] {
        let resolver = Resolver::new(&["z"]);
        assert_eq!(
            resolver.execute(sql).unwrap_err().code,
            "relational.invalid_binding",
            "{sql}"
        );
        assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
    }
}

#[test]
fn nonfinal_union_suffix_runs_outer_cte_before_rejecting_branch() {
    let resolver = Resolver::new(&["z"]);
    let sql = "WITH c AS (SELECT ID FROM z) SELECT ID FROM absent LIMIT 1 UNION ALL SELECT ID FROM also_absent";
    let program = Program::compile(sql).unwrap();
    let error = program
        .execute_resolved_budgeted(&resolver, &BTreeMap::new(), Limits::default(), None, None)
        .unwrap_err();
    assert_eq!(error.code, "relational.invalid_syntax");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}
#[test]
fn failing_outer_cte_precedes_nonfinal_union_suffix_syntax() {
    let resolver = Resolver::new(&[]);
    let error=resolver.execute("WITH c AS (SELECT ID FROM missing) SELECT ID FROM absent LIMIT 1 UNION ALL SELECT ID FROM later").unwrap_err();
    assert_eq!(error.code, "fixture.missing_root");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:missing"]);
}
#[test]
fn middle_union_suffix_follows_earlier_binding_failure() {
    let resolver = Resolver::new(&["z"]);
    let error=resolver.execute("SELECT Missing FROM z UNION ALL SELECT ID FROM absent LIMIT 1 UNION ALL SELECT ID FROM later").unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}
#[test]
fn middle_union_suffix_precedes_its_own_cte_or_source_resolution() {
    let resolver = Resolver::new(&["z"]);
    let error=resolver.execute("SELECT ID FROM z UNION ALL WITH c AS (SELECT ID FROM absent) SELECT ID FROM c OFFSET 1 UNION ALL SELECT ID FROM later").unwrap_err();
    assert_eq!(error.code, "relational.invalid_syntax");
    assert_eq!(*resolver.events.borrow(), ["resolve:static:z"]);
}
#[test]
fn right_branch_cte_is_resolved_once_in_real_preflight_then_scanned() {
    let resolver = Resolver::new(&["a", "z"]);
    let result=resolver.execute("SELECT ID FROM z UNION ALL WITH c AS (SELECT ID FROM a) SELECT ID FROM c UNION ALL SELECT ID FROM z").unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]; 3]);
    assert_eq!(
        *resolver.events.borrow(),
        [
            "resolve:static:z",
            "resolve:static:a",
            "scan:z",
            "scan:a",
            "scan:z"
        ]
    );
}
