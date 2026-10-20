use lycheedev_core::{Invocation, control::OperationControl};
use lycheedev_data::resource::{Budget, Cost};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn rejected_combined_cost_does_not_consume_other_dimensions() {
    let budget = Budget::new(
        Cost {
            retained_bytes: 16,
            decode_work: 10,
            ..Default::default()
        },
        Instant::now() + Duration::from_secs(10),
        None,
    );
    budget
        .charge(Cost {
            retained_bytes: 8,
            decode_work: 6,
            ..Default::default()
        })
        .unwrap();
    let error = budget
        .charge(Cost {
            retained_bytes: 4,
            decode_work: 5,
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 8);
    budget
        .charge(Cost {
            retained_bytes: 8,
            decode_work: 4,
            ..Default::default()
        })
        .unwrap();
    assert!(budget.charge(Cost::work(1)).is_err());
}

#[test]
fn shared_identity_is_reserved_once_but_repeated_work_is_not_free() {
    let budget = Budget::new(
        Cost {
            metadata_bytes: 32,
            retained_bytes: 32,
            decode_work: 3,
            ..Default::default()
        },
        Instant::now() + Duration::from_secs(10),
        None,
    );
    budget
        .charge_once("same-physical-index", Cost::metadata(32))
        .unwrap();
    budget
        .charge_once("same-physical-index", Cost::metadata(32))
        .unwrap();
    assert!(
        budget
            .charge_once("other-index", Cost::metadata(1))
            .is_err()
    );
    for _ in 0..3 {
        budget.charge(Cost::work(1)).unwrap();
    }
    assert!(budget.charge(Cost::work(1)).is_err());
}

#[test]
fn public_handlers_observe_the_caller_token_before_any_input_access() {
    let cancel = Arc::new(AtomicBool::new(false));
    let control = OperationControl::new(Instant::now() + Duration::from_secs(10), cancel.clone());
    let budget = Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        control.clone(),
    );
    assert!(budget.deadline <= control.deadline());
    cancel.store(true, Ordering::Release);
    assert_eq!(budget.check().unwrap_err().exit_code, 7);
    for command in ["data sql", "asset inspect"] {
        let invocation = Invocation {
            command: command.into(),
            positional: None,
            flags: BTreeMap::new(),
            home: PathBuf::from("unopened-home"),
            cwd: PathBuf::from("unopened-cwd"),
            control: control.clone(),
        };
        let error = lycheedev_data::migration_oracle::dispatch(&invocation).unwrap_err();
        assert_eq!(error.exit_code, 7);
        assert_eq!(error.code, "command.cancelled");
    }
}
