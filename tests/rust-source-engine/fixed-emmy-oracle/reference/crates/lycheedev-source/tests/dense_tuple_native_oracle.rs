//! Dense RHS tuple projection through the real fixed contextual model and the
//! owned compiler/executor/binary/tuple path. No general logical union backend.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/expression_execution_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::{LuaType, binding_context as native};
use emmylua_parser::{LuaAstNode, LuaExpr};
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::{Backend, Provider, Stop, absent, load, select};

struct NativeProvider(Provider);
fn to_owned(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        "expression-execution-oracle"
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        match self.0.values.get(&to_owned(site)) {
            Some(owned::BindingDecision::KnownAbsent { proof }) => {
                native::BindingDecision::KnownAbsent {
                    proof: proof.clone(),
                }
            }
            Some(owned::BindingDecision::Unavailable { proof, reason }) => {
                native::BindingDecision::Unavailable {
                    proof: proof.clone(),
                    reason: reason.clone(),
                }
            }
            Some(owned::BindingDecision::Bound(value)) => {
                let owned::SelectedValue::Expression(origin) = &value.value else {
                    panic!("this oracle only projects real selected expressions")
                };
                native::BindingDecision::Bound(native::BoundValue {
                    identity: value.identity.clone(),
                    proof: value.proof.clone(),
                    value: native::SelectedValue::Expression(native::BindingSite {
                        file_id: origin.file_id,
                        syntax: origin.syntax,
                    }),
                })
            }
            _ => native::BindingDecision::NotManaged,
        }
    }
}
fn row(row: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: to_owned(row.site),
        operation: row.operation,
        state: row.state,
        proof: row.proof,
        reason: row.reason,
        parameter: row.parameter,
        parameter_index: row.parameter_index,
        argument: row.argument.map(to_owned),
    }
}
fn compare(
    analysis: &emmylua_code_analysis::EmmyLuaAnalysis,
    query: owned::BindingSite,
    provider: Provider,
) -> (LuaType, Vec<owned::BindingCoverage>) {
    let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
    let old = analysis
        .semantic_model_with_context(query.file_id, native.clone())
        .type_of_expr(query.syntax);
    let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
            .unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend.primitive_binary = true;
    backend.dense_table_logical = true;
    let new = owned::SelectedProjector::new(analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    trace::record(&format!("{}:full_type", provider.names[&query]), &(old));
    assert_eq!(new.ty, old, "complete type and tuple status");
    trace::record(
        &format!("{}:ordered_ledger", provider.names[&query]),
        &(old_rows),
    );
    assert_eq!(
        rows, old_rows,
        "complete coverage fields and producer order, including failed tuple attempts"
    );
    println!(
        "DENSE_TUPLE_NATIVE {}",
        serde_json::json!({"expression":provider.names[&query],"oldType":format!("{old:?}"),"ownedType":format!("{:?}",new.ty),"coverage":format!("{rows:?}")})
    );
    (old, rows)
}

#[test]
fn actual_dense_hole_named_duplicate_and_nested_rhs_tables_keep_full_type_and_producer_order() {
    let _trace = trace::begin_reference();
    let mut count = 0;
    for table in [
        "{}",
        "{first,second}",
        "{[2]=second,[1]=first}",
        "{[1]=first,[3]=third}",
        "{[1]=first,named=second,[2]=third}",
        "{named=first,[1]=second}",
        "{[0]=first,[1]=second}",
        "{[1]=first,[1]=second}",
        "{{first},second}",
        "{[2]=second+1,[1]=first}",
    ] {
        for prefix in ["true and", "nil or"] {
            for unknown in [false, true] {
                let expression = format!("{prefix} {table}");
                let (analysis, query, mut provider) =
                    load(&format!("return {expression}"), &expression);
                for name in ["first", "second", "third"] {
                    if provider.names.values().any(|text| text == name) {
                        select(
                            &mut provider,
                            name,
                            if unknown {
                                owned::BindingDecision::Unavailable {
                                    proof: format!("{name}-proof"),
                                    reason: format!("{name}-unknown"),
                                }
                            } else {
                                absent(&format!("{name}-proof"))
                            },
                        );
                    }
                }
                compare(&analysis, query, provider);
                count += 1;
            }
        }
    }
    assert_eq!(count, 40);
}

#[test]
fn original_contextual_fast_paths_do_not_query_rhs_table_fields() {
    let _trace = trace::begin_reference();
    for prefix in ["false and", "true or"] {
        let expression = format!("{prefix} {{first,second}}");
        let (analysis, query, mut provider) = load(&format!("return {expression}"), &expression);
        select(&mut provider, "first", absent("first"));
        select(&mut provider, "second", absent("second"));
        let (ty, rows) = compare(&analysis, query, provider);
        assert_eq!(ty, LuaType::BooleanConst(prefix == "true or"));
        assert!(rows.is_empty());
    }
}

#[test]
fn foreign_table_members_keep_current_model_file_query_behavior_in_the_fixed_engine() {
    let _trace = trace::begin_reference();
    let (mut analysis, query, mut provider) = load("return true and foreign", "true and foreign");
    let path = std::env::current_dir()
        .unwrap()
        .join("expression-execution-logical/foreign.lua");
    let source = format!(
        "{}return {{first,second}}",
        "-- foreign padding\n".repeat(100)
    );
    let file = analysis.update_files_by_path(vec![(path, Some(source))])[0];
    assert_ne!(file, query.file_id);
    let model = analysis.semantic_model(file);
    let root = model.syntax_tree().unwrap().get_red_root();
    let expressions = root
        .descendants()
        .filter_map(LuaExpr::cast)
        .collect::<Vec<_>>();
    let origin = expressions
        .iter()
        .find(|expr| expr.syntax().text().to_string() == "{first,second}")
        .unwrap();
    let origin = owned::BindingSite {
        file_id: file,
        syntax: origin.get_syntax_id(),
    };
    for expr in expressions {
        provider.names.insert(
            owned::BindingSite {
                file_id: file,
                syntax: expr.get_syntax_id(),
            },
            expr.syntax().text().to_string(),
        );
    }
    select(
        &mut provider,
        "foreign",
        owned::BindingDecision::Bound(owned::BoundValue {
            identity: "foreign-table".into(),
            proof: "foreign-table-proof".into(),
            value: owned::SelectedValue::Expression(origin),
        }),
    );
    select(
        &mut provider,
        "first",
        absent("must-not-query-foreign-first"),
    );
    select(
        &mut provider,
        "second",
        absent("must-not-query-foreign-second"),
    );
    let (ty, rows) = compare(&analysis, query, provider);
    let LuaType::Tuple(tuple) = ty else {
        panic!("fixed cross-file behavior changed")
    };
    assert_eq!(tuple.get_types(), [LuaType::Unknown, LuaType::Unknown]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].proof, "foreign-table-proof");
}
