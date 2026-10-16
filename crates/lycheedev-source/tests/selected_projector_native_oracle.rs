//! Owned atomic projections consume independently captured fixed-engine values.
//! The controlled runner must first complete the historical A/B reference target.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/selected_projector_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::*;

#[test]
fn every_atomic_selected_type_and_all_coverage_fields_match_real_patched_model() {
    let _trace = trace::begin_candidate();
    let (analysis, query, statement) = load();
    let ordinary = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    for case in cases(query, statement) {
        let session =
            owned::OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(()))
                .unwrap();
        let value = owned::SelectedProjector::new(&analysis, &session)
            .project_decision(
                query,
                case.decision.clone(),
                "resolve",
                &mut AtomicBackend::default(),
            )
            .unwrap();
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_case(&case, &value, &rows, query);
        trace::check(
            &format!("{}:full_type", case.name),
            &(value.as_ref().unwrap_or(&ordinary)),
        );
        trace::check(
            &format!("{}:all_debug_fields", case.name),
            &(format!("{:?}", value.as_ref().unwrap_or(&ordinary))),
        );
        trace::check(&format!("{}:ordered_ledger", case.name), &(rows));
        println!(
            "SELECTED_ATOMIC_NATIVE_ORACLE {}",
            serde_json::json!({"case":case.name,"type":format!("{:?}", value.as_ref().unwrap_or(&ordinary)),"coverage":format!("{rows:?}")})
        );
    }
}
