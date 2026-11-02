use super::*;
use std::time::{Duration, Instant};

#[test]
fn selected_extent_uses_caller_limit_beyond_old_materialization_ceiling() {
    let limits = casc::Limits {
        encoded_bytes: 256 << 20,
        ..Default::default()
    };
    for bytes in [256 << 20, (256 << 20) + 1, 512 << 20] {
        assert_eq!(
            materialized_extent(bytes, limits, Some(512 << 20)).unwrap(),
            (bytes as usize, 512 << 20)
        );
    }
    assert_eq!(
        materialized_extent((512 << 20) + 1, limits, Some(512 << 20))
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
}
#[test]
fn metadata_extent_keeps_its_separate_admitted_bound() {
    let limits = casc::Limits {
        encoded_bytes: 32 << 20,
        ..Default::default()
    };
    assert_eq!(
        materialized_extent(32 << 20, limits, None).unwrap(),
        (32 << 20, 32 << 20)
    );
    assert_eq!(
        materialized_extent((32 << 20) + 1, limits, None)
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
    assert!(materialized_extent(64 << 20, limits, Some(128 << 20)).is_ok());
}
#[test]
fn admitted_size_is_not_permission_to_bypass_shared_query_budget() {
    let limits = casc::Limits::default();
    let (decoded, _) = materialized_extent((256 << 20) + 1, limits, Some(512 << 20)).unwrap();
    let budget = Budget::new(
        Cost {
            retained_bytes: 256 << 20,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    assert_eq!(
        budget
            .charge_split(Cost::retained(decoded), Cost::default())
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(budget.logical_snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}
