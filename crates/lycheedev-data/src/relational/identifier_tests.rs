//! Fixed Go 3.2.3 uses independent qualifier/name fields. These regressions
//! exercise the real binder/evaluator/provider paths, not rendering helpers.
use super::*;

fn source(columns: &[&str], rows: &[&[i64]]) -> Source {
    Source {
        columns: columns.iter().map(|name| (*name).to_owned()).collect(),
        rows: rows
            .iter()
            .map(|row| row.iter().map(|n| Scalar::Int(*n)).collect())
            .collect(),
        complete: true,
    }
}
fn run(sql: &str, inputs: &BTreeMap<String, Source>) -> Result<QueryResult> {
    Program::compile(sql)?.execute(inputs, &BTreeMap::new(), Limits::default(), None)
}
fn ints(values: &[&[i64]]) -> Vec<Vec<Scalar>> {
    values
        .iter()
        .map(|row| row.iter().map(|n| Scalar::Int(*n)).collect())
        .collect()
}

#[test]
fn raw_dotted_column_and_qualified_column_are_distinct() {
    let inputs = BTreeMap::from([("t".into(), source(&["t.ID", "ID"], &[&[7, 2]]))]);
    let result = run(r#"SELECT "t.ID", t.ID, t."t.ID" FROM t"#, &inputs).unwrap();
    assert_eq!(result.columns, ["t.ID", "ID", "t.ID"]);
    assert_eq!(result.rows, ints(&[&[7, 2, 7]]));
    assert_eq!(
        run(r#"SELECT "t.ID" FROM t WHERE "t.ID" IN (7)"#, &inputs)
            .unwrap()
            .rows,
        ints(&[&[7]])
    );
}

#[test]
fn quoted_join_owners_and_columns_do_not_collide_at_dots() {
    let inputs = BTreeMap::from([
        (
            "lefts".into(),
            source(&["b.c", "q\"uote"], &[&[1, 8], &[2, 9]]),
        ),
        ("rights".into(), source(&["c"], &[&[2]])),
    ]);
    let result = run(r#"SELECT a."b.c", "a.b".c, a."q""uote" FROM lefts a LEFT JOIN rights "a.b" ON a."b.c"="a.b".c ORDER BY a."b.c""#, &inputs).unwrap();
    assert_eq!(result.columns, ["b.c", "c", "q\"uote"]);
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::Int(1), Scalar::Null, Scalar::Int(8)],
            vec![Scalar::Int(2), Scalar::Int(2), Scalar::Int(9)]
        ]
    );
    let stars = run(
        r#"SELECT "a.b".*, a.* FROM lefts a CROSS JOIN rights "a.b" ORDER BY a."b.c""#,
        &inputs,
    )
    .unwrap();
    assert_eq!(stars.columns, ["c", "b.c", "q\"uote"]);
    assert_eq!(stars.rows, ints(&[&[2, 1, 8], &[2, 2, 9]]));
}

#[test]
fn formerly_reserved_join_marker_is_an_ordinary_quoted_alias() {
    let inputs = BTreeMap::from([("t".into(), source(&["x.y"], &[&[4]]))]);
    assert_eq!(
        run(r#"SELECT "<joined>"."x.y" FROM t "<joined>""#, &inputs)
            .unwrap()
            .rows,
        ints(&[&[4]])
    );
}

#[test]
fn table_catalog_and_default_alias_preserve_raw_components() {
    let sql = r#"SELECT "a.b"."x.y", q."x.y" FROM "a.b" CROSS JOIN a.b q"#;
    let inputs = BTreeMap::from([
        (r#""a.b""#.into(), source(&["x.y"], &[&[1]])),
        ("a.b".into(), source(&["x.y"], &[&[2]])),
    ]);
    assert_eq!(run(sql, &inputs).unwrap().rows, ints(&[&[1, 2]]));
    let query = Program::compile(sql).unwrap();
    let uses = query.source_table_uses();
    assert_eq!(
        uses.iter()
            .map(|t| (t.catalog, t.name, t.key))
            .collect::<Vec<_>>(),
        vec![("a", "b", "a.b"), ("static", "a.b", r#""a.b""#)]
    );
    let quoted = BTreeMap::from([(r#""ca.t"."ta""ble""#.into(), source(&["x"], &[&[3]]))]);
    assert_eq!(
        run(r#"SELECT "ta""ble".x FROM "ca.t"."ta""ble""#, &quoted)
            .unwrap()
            .rows,
        ints(&[&[3]])
    );
}

#[test]
fn dotted_ctes_shadow_only_the_matching_unqualified_identifier() {
    let inputs = BTreeMap::from([
        ("base".into(), source(&["ID"], &[&[1]])),
        ("a.b".into(), source(&["ID"], &[&[2]])),
        (r#"static."a.b""#.into(), source(&["ID"], &[&[3]])),
    ]);
    let sql = r#"WITH "a.b" AS (SELECT ID FROM base) SELECT ID FROM "a.b" UNION ALL SELECT ID FROM a.b UNION ALL SELECT ID FROM static."a.b""#;
    assert_eq!(run(sql, &inputs).unwrap().rows, ints(&[&[1], &[2], &[3]]));
    let dependencies = Program::compile(sql).unwrap();
    assert_eq!(
        dependencies.source_tables(),
        ["a.b", r#"static."a.b""#, "base"]
    );
    assert_eq!(
        run(
            r#"WITH "a.b" AS (SELECT ID FROM "a.b") SELECT ID FROM "a.b""#,
            &inputs
        )
        .unwrap_err()
        .code,
        "relational.invalid_binding"
    );
}

#[test]
fn recursive_dotted_cte_uses_only_its_delta() {
    let inputs = BTreeMap::from([("base".into(), source(&["ID"], &[&[1]]))]);
    let sql = r#"WITH RECURSIVE "r.s" AS (SELECT ID FROM base UNION ALL SELECT ID+1 AS ID FROM "r.s" WHERE ID<3) SELECT ID FROM "r.s""#;
    assert_eq!(run(sql, &inputs).unwrap().rows, ints(&[&[1], &[2], &[3]]));
}

#[test]
fn dotted_correlations_groups_and_derived_outputs_keep_field_identity() {
    let inputs = BTreeMap::from([
        ("a".into(), source(&["x.y"], &[&[1], &[2], &[2]])),
        ("b".into(), source(&["x.y"], &[&[2]])),
    ]);
    let sql = r#"SELECT "o.p"."x.y", COUNT(*) FROM a "o.p" WHERE "o.p"."x.y" IN (SELECT "i.j"."x.y" FROM b "i.j" WHERE "i.j"."x.y"="o.p"."x.y") GROUP BY "x.y" ORDER BY "o.p"."x.y""#;
    assert_eq!(run(sql, &inputs).unwrap().rows, ints(&[&[2, 2]]));
    let grouped = r#"SELECT "o.p"."x.y", "o.p"."x.y" IN (SELECT "x.y" FROM b WHERE "x.y"="o.p"."x.y"), COUNT(*) FROM a "o.p" GROUP BY "x.y" ORDER BY "x.y""#;
    assert_eq!(
        run(grouped, &inputs).unwrap().rows,
        vec![
            vec![Scalar::Int(1), Scalar::Bool(false), Scalar::Int(1)],
            vec![Scalar::Int(2), Scalar::Bool(true), Scalar::Int(2)]
        ]
    );
    assert_eq!(
        run(
            r#"SELECT "d.e"."z.z" FROM (SELECT "x.y" AS "z.z" FROM b) "d.e""#,
            &inputs
        )
        .unwrap()
        .rows,
        ints(&[&[2]])
    );
}

#[test]
fn quoted_output_aliases_are_resolved_before_source_names() {
    let inputs = BTreeMap::from([("t".into(), source(&["x.y", "other"], &[&[1, 8], &[2, 7]]))]);
    let result = run(r#"SELECT other AS "x.y" FROM t ORDER BY "x.y""#, &inputs).unwrap();
    assert_eq!(result.rows, ints(&[&[7], &[8]]));
    assert_eq!(
        run(
            r#"SELECT other AS "x.y", "x.y" FROM t ORDER BY "x.y""#,
            &inputs
        )
        .unwrap_err()
        .code,
        "relational.invalid_binding"
    );
}

#[test]
fn union_keeps_dotted_and_quoted_names_and_ordinal_owners() {
    let inputs = BTreeMap::from([("t".into(), source(&["x.y", "q\"uote"], &[&[2, 8], &[1, 9]]))]);
    let result = run(r#"SELECT "x.y" AS "out.dot", "q""uote" FROM t WHERE "x.y"=1 UNION ALL SELECT "x.y", "q""uote" FROM t WHERE "x.y"=2 ORDER BY "out.dot" DESC"#, &inputs).unwrap();
    assert_eq!(result.columns, ["out.dot", "q\"uote"]);
    assert_eq!(result.rows, ints(&[&[2, 8], &[1, 9]]));
    assert_eq!(run(r#"SELECT "x.y", "q""uote" FROM t UNION ALL SELECT "x.y", "q""uote" FROM t ORDER BY "0"."x.y" DESC LIMIT 1"#, &inputs).unwrap().rows, ints(&[&[2, 8]]));
}

#[test]
fn diagnostics_keep_go_raw_labels_without_changing_lookup_boundaries() {
    let inputs = BTreeMap::from([("t".into(), source(&["x.y"], &[]))]);
    let error = run(r#"SELECT "a.b"."x.z" FROM t "a.b""#, &inputs).unwrap_err();
    assert_eq!(
        error.details,
        serde_json::json!({"column":"a.b.x.z","candidates":["a.b.x.y"]})
    );
    let explained = run(
        r#"EXPLAIN SELECT "a.b"."x.y" FROM "a.b""#,
        &BTreeMap::from([(r#""a.b""#.into(), source(&["x.y"], &[]))]),
    )
    .unwrap();
    assert!(
        explained
            .plan
            .unwrap()
            .steps
            .iter()
            .any(|s| s.operation == "input" && s.detail == "a.b")
    );
}

#[test]
fn unicode_equal_fold_and_lowercase_keys_keep_their_distinct_contracts() {
    let inputs = BTreeMap::from([("t".into(), source(&["Σ.x", "S.y"], &[&[1, 2]]))]);
    assert_eq!(
        run(
            r#"SELECT "ς.x", "ſ.y" FROM t "Σ.a" WHERE "ς.a"."σ.x"=1"#,
            &inputs
        )
        .unwrap()
        .rows,
        ints(&[&[1, 2]])
    );
    // Duplicate-name admission uses Go ToLower, while resolution is EqualFold.
    let ambiguous = BTreeMap::from([("t".into(), source(&["Σ.x", "ς.x"], &[]))]);
    assert_eq!(
        run(r#"SELECT "σ.x" FROM t"#, &ambiguous).unwrap_err().code,
        "relational.invalid_binding"
    );
    let distinct =
        Program::compile(r#"SELECT ID FROM "Σ.t" UNION ALL SELECT ID FROM "ς.t""#).unwrap();
    assert_eq!(distinct.source_tables().len(), 2);
}

#[test]
fn source_key_roundtrips_do_not_conflate_components() {
    for sql in [
        r#"SELECT x FROM "a.b""#,
        r#"SELECT x FROM "a"."b""#,
        r#"SELECT x FROM "a.b"."c""#,
        r#"SELECT x FROM "a"."b.c""#,
        r#"SELECT x FROM "quo""te"."123""#,
        r#"SELECT x FROM "中文"."表.一""#,
    ] {
        let query = Program::compile(sql).unwrap();
        let table = &query.table;
        let decoded = RelationName::from_key(table.key()).unwrap();
        assert_eq!(
            (&decoded.catalog, &decoded.name),
            (&table.catalog, &table.name)
        );
    }
    let inputs = BTreeMap::from([
        (r#""a.b".c"#.into(), source(&["x"], &[&[1]])),
        (r#"a."b.c""#.into(), source(&["x"], &[&[2]])),
    ]);
    assert_eq!(
        run(
            r#"SELECT x FROM "a.b".c UNION ALL SELECT x FROM a."b.c""#,
            &inputs
        )
        .unwrap()
        .rows,
        ints(&[&[1], &[2]])
    );
}

struct IdentifierProvider {
    rows: Vec<Vec<Scalar>>,
    events: std::cell::RefCell<Vec<String>>,
}
struct IdentifierCursor<'a> {
    rows: &'a [Vec<Scalar>],
    selected: Option<Vec<usize>>,
    equal: Option<Scalar>,
    next: usize,
    scratch: Vec<Scalar>,
}
impl RowCursor for IdentifierCursor<'_> {
    fn advance(&mut self) -> Result<bool> {
        self.scratch.fill(Scalar::Null);
        self.scratch.clear();
        while let Some(row) = self.rows.get(self.next) {
            self.next += 1;
            if self.equal.as_ref().is_some_and(|value| row[0] != *value) {
                continue;
            }
            self.scratch.clone_from(row);
            if let Some(selected) = &self.selected {
                for (index, value) in self.scratch.iter_mut().enumerate() {
                    if !selected.contains(&index) {
                        *value = Scalar::Null;
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn row(&self) -> &[Scalar] {
        &self.scratch
    }
}
impl RowProvider for IdentifierProvider {
    fn cursor(&self, selected: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        self.events.borrow_mut().push(match selected {
            Some(selected) => format!("selected:{selected:?}"),
            None => "scan".into(),
        });
        Ok(Box::new(IdentifierCursor {
            rows: &self.rows,
            selected: selected.map(<[usize]>::to_vec),
            equal: None,
            next: 0,
            scratch: vec![],
        }))
    }
    fn equal_cursor(&self, name: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
        self.events
            .borrow_mut()
            .push(format!("equal:{name}:{value:?}"));
        assert_eq!(
            name, "ID.dot",
            "provider receives a raw field, never a qualified path"
        );
        Ok(EqualityCursor::Rows(Box::new(IdentifierCursor {
            rows: &self.rows,
            selected: None,
            equal: Some(value.clone()),
            next: 0,
            scratch: vec![],
        })))
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        let mut cursor = self.cursor(None)?;
        while cursor.advance()? {
            emit(cursor.row())?;
        }
        Ok(())
    }
    fn scan_selected(&self, selected: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        let mut cursor = self.cursor(Some(selected))?;
        while cursor.advance()? {
            emit(cursor.row())?;
        }
        Ok(())
    }
    fn has_equal(&self) -> bool {
        true
    }
    fn equal(
        &self,
        name: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        match self.equal_cursor(name, value)? {
            EqualityCursor::Declined => Ok(EqualityOutcome::Declined),
            EqualityCursor::Empty => Ok(EqualityOutcome::Handled),
            EqualityCursor::Rows(mut cursor) => {
                while cursor.advance()? {
                    emit(cursor.row())?;
                }
                Ok(EqualityOutcome::Handled)
            }
        }
    }
}

fn lazy(
    sql: &str,
    schema: &Source,
    provider: &IdentifierProvider,
    limits: Limits,
    cancel: Option<&AtomicBool>,
) -> Result<QueryResult> {
    let roots = BTreeMap::from([(r#""t.dot""#.into(), schema)]);
    let providers = [PreparedSource { schema, provider }];
    Program::compile(sql)?.execute_providers_budgeted(
        &roots,
        &providers,
        &BTreeMap::new(),
        limits,
        cancel,
        None,
    )
}
#[test]
fn quoted_equality_in_and_selected_callbacks_receive_raw_columns_once() {
    let schema = source(&["ID.dot", "unused", "value.dot"], &[]);
    let provider = IdentifierProvider {
        rows: ints(&[&[1, 99, 8], &[2, 98, 7]]),
        events: Default::default(),
    };
    let result = lazy(
        r#"SELECT "a.b"."value.dot" FROM "t.dot" "a.b" WHERE "a.b"."ID.dot" IN (2,2,1)"#,
        &schema,
        &provider,
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.rows, ints(&[&[7], &[8]]));
    assert_eq!(
        *provider.events.borrow(),
        ["equal:ID.dot:Int(2)", "equal:ID.dot:Int(1)"]
    );
    provider.events.borrow_mut().clear();
    assert_eq!(
        lazy(
            r#"SELECT "a.b"."value.dot" FROM "t.dot" "a.b" WHERE "a.b"."ID.dot"=2"#,
            &schema,
            &provider,
            Limits::default(),
            None
        )
        .unwrap()
        .rows,
        ints(&[&[7]])
    );
    assert_eq!(*provider.events.borrow(), ["equal:ID.dot:Int(2)"]);
    provider.events.borrow_mut().clear();
    assert_eq!(
        lazy(
            r#"SELECT "a.b"."value.dot" FROM "t.dot" "a.b" WHERE "a.b"."ID.dot">1"#,
            &schema,
            &provider,
            Limits::default(),
            None
        )
        .unwrap()
        .rows,
        ints(&[&[7]])
    );
    assert_eq!(*provider.events.borrow(), ["selected:[0, 2]"]);
}

#[test]
fn quoted_preflight_cancel_and_budget_never_add_provider_scans() {
    let schema = source(&["ID.dot", "unused", "value.dot"], &[]);
    let provider = IdentifierProvider {
        rows: ints(&[&[1, 99, 8]]),
        events: Default::default(),
    };
    let sql = r#"SELECT "a.b"."value.dot" FROM "t.dot" "a.b" WHERE "a.b"."ID.dot" IN (1)"#;
    assert!(
        lazy(
            &format!("EXPLAIN {sql}"),
            &schema,
            &provider,
            Limits::default(),
            None
        )
        .unwrap()
        .plan
        .is_some()
    );
    let error = lazy(
        r#"SELECT "a.b"."missing.dot" FROM "t.dot" "a.b""#,
        &schema,
        &provider,
        Limits::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    let cancel = AtomicBool::new(true);
    assert_eq!(
        lazy(sql, &schema, &provider, Limits::default(), Some(&cancel))
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
    assert_eq!(
        lazy(
            sql,
            &schema,
            &provider,
            Limits {
                work: 1,
                ..Limits::default()
            },
            None
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
    assert!(provider.events.borrow().is_empty());
}

#[test]
fn nested_compound_callback_keeps_dotted_inherited_and_current_fields() {
    let inputs = BTreeMap::from([
        ("a".into(), source(&["x.y"], &[&[1], &[2]])),
        ("c".into(), source(&["Other"], &[&[9]])),
    ]);
    let sql = r#"SELECT "x.y" FROM (SELECT "x.y" FROM a) WHERE "x.y" IN (SELECT "x.y" FROM a WHERE "x.y"=1 UNION ALL SELECT "x.y" FROM a WHERE "x.y"=2 ORDER BY "x.y" IN (SELECT "x.y" FROM c) DESC LIMIT 1)"#;
    assert_eq!(run(sql, &inputs).unwrap().rows, ints(&[&[1]]));
}
