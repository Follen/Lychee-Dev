use super::*;
use lycheedev_core::control::OperationControl;
use std::time::{Duration, Instant};
struct Body(Content);
impl RootContent for Body {
    fn content(&self) -> &Content {
        &self.0
    }
}
fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(120),
        None,
    ))
}
fn groups(count: usize) -> Content {
    let mut bytes = Vec::new();
    for group in 0..count {
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&(if group % 2 == 0 { 0u32 } else { 0x80 }).to_le_bytes());
        bytes.extend_from_slice(&(if group % 2 == 0 { 2u32 } else { 0x40 }).to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for id in [7u64, 8] {
            bytes.extend_from_slice(&[id as u8; 16]);
            bytes.extend_from_slice(&(group as u64 * 10 + id).to_le_bytes());
        }
    }
    Content {
        bytes,
        missing: vec![],
    }
}
fn limits() -> RootLimits {
    RootLimits {
        matches: 4096,
        groups: 65536,
        ..Default::default()
    }
}
#[test]
fn compact_index_reuses_shared_root_parser_and_preserves_all_variants() {
    let content = groups(5);
    let expected7 = lookup_root(&content, &[7], limits(), None).unwrap();
    let expected8 = lookup_root(&content, &[8], limits(), None).unwrap();
    let budget = budget();
    let index = RootIndex::open(Body(content), limits(), budget.clone(), || Ok(())).unwrap();
    let prepared = budget.logical_snapshot().unwrap();
    assert_eq!(prepared.metadata_bytes, 5 * (2 * 4 + 256));
    assert_eq!(prepared.decode_work, 5 * 3);
    for (id, expected) in [(7, &expected7), (8, &expected8), (7, &expected7)] {
        let before = budget.logical_snapshot().unwrap();
        let actual = index.lookup(id, || Ok(())).unwrap();
        assert_eq!(&*actual, expected.as_slice());
        let after = budget.logical_snapshot().unwrap();
        assert_eq!(after.metadata_bytes, before.metadata_bytes);
        assert_eq!(after.retained_bytes, before.retained_bytes);
        assert_eq!(after.decode_work, before.decode_work + 5);
    }
    let matches = index.lookup(7, || Ok(())).unwrap();
    drop(index);
    assert_eq!(&*matches, expected7.as_slice());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 4096 * 256);
    drop(matches);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(
        budget.logical_snapshot().unwrap().retained_bytes,
        prepared.retained_bytes
    );
}
#[test]
fn root_match_limit_applies_to_each_id_not_the_sum_of_distinct_lookups() {
    let budget = budget();
    let index = RootIndex::open(Body(groups(4096)), limits(), budget, || Ok(())).unwrap();
    assert_eq!(index.lookup(7, || Ok(())).unwrap().len(), 4096);
    assert_eq!(index.lookup(8, || Ok(())).unwrap().len(), 4096);
    let budget = self::budget();
    let too_many = RootIndex::open(Body(groups(4097)), limits(), budget, || Ok(())).unwrap();
    assert_eq!(
        too_many.lookup(7, || Ok(())).err().unwrap().code,
        "records.metadata_limit"
    );
}
#[test]
fn failed_root_preparation_refunds_backing_but_preserves_admitted_logical_work() {
    let mut content = groups(1);
    content.bytes.extend_from_slice(&100u32.to_le_bytes());
    content.bytes.extend_from_slice(&[0; 8]);
    let budget = budget();
    assert_eq!(
        RootIndex::open(Body(content), limits(), budget.clone(), || Ok(()))
            .err()
            .unwrap()
            .code,
        "records.metadata_invalid"
    );
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    let logical = budget.logical_snapshot().unwrap();
    assert_eq!(logical.metadata_bytes, 264);
    assert_eq!(logical.retained_bytes, 264);
    assert_eq!(logical.decode_work, 104);
}
#[test]
fn prepared_root_lookup_checks_original_stop_before_admission_or_work() {
    let budget = budget();
    let control = OperationControl::default();
    let index = RootIndex::open(Body(groups(1)), limits(), budget.clone(), || {
        control.check()
    })
    .unwrap();
    let logical = budget.logical_snapshot().unwrap();
    let safety = budget.snapshot().unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        index.lookup(7, || control.check()).err().unwrap().code,
        "command.cancelled"
    );
    assert_eq!(budget.logical_snapshot().unwrap(), logical);
    assert_eq!(budget.snapshot().unwrap(), safety);
}
