use super::*;
use crate::resource::{Budget, Cost};
use std::{cell::Cell, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
struct Fixture(PathBuf);
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn context(offline: bool, limits: Cost) -> (Fixture, Context, Arc<Budget>) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!("lycheedev-public-keys-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    lycheedev_core::workspace::initialize(&home).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let budget = Arc::new(Budget::new(limits, deadline, None));
    let inv = Invocation { command: "records file".into(), positional: None,
        flags: BTreeMap::from([("--offline".into(), offline.to_string())]),
        home: home.clone(), cwd: home.clone(), control: OperationControl::uncancelable(deadline) };
    let ctx = Context::prepare_budgeted(&inv, deadline, Some(budget.clone())).unwrap();
    (Fixture(home), ctx, budget)
}
struct Failed(Cell<usize>);
impl http::Fetcher for Failed {
    fn fetch(&self, _: &OperationControl, _: Option<&Budget>, _: Instant) -> Result<Vec<u8>> {
        self.0.set(self.0.get() + 1);
        Err(fault("records.invalid_key_document", 4, "scripted failure"))
    }
}
#[test]
fn public_offline_failure_is_memoized_with_exact_go_reservation() {
    let (_home, mut ctx, budget) = context(true, Cost::defaults());
    let fetcher = Failed(Cell::new(0));
    for _ in 0..2 { assert_eq!(ctx.ensure_public_with(&fetcher).unwrap_err().code, "container.key_unavailable"); }
    let expected = Cost { decode_work: PUBLIC_BYTES.div_ceil(65536) as u64,
        ..Cost::metadata(PUBLIC_BYTES * 8 + 65536) };
    assert_eq!(budget.logical_snapshot().unwrap(), expected);
    assert_eq!(budget.snapshot().unwrap(), expected);
    assert_eq!(ctx.source.state, "unavailable_offline");
    assert_eq!(fetcher.0.get(), 0);
}
#[test]
fn public_network_failure_is_memoized_without_a_second_fetch() {
    let (_home, mut ctx, budget) = context(false, Cost::defaults());
    let fetcher = Failed(Cell::new(0));
    let first = ctx.ensure_public_with(&fetcher).unwrap_err();
    let before = budget.snapshot().unwrap();
    assert_eq!(ctx.ensure_public_with(&fetcher).unwrap_err().message, first.message);
    assert_eq!(fetcher.0.get(), 1);
    assert_eq!(budget.snapshot().unwrap(), before);
    assert_eq!(ctx.source.state, "unavailable");
}
#[test]
fn public_reservation_failure_is_memoized_before_io() {
    let (_home, mut ctx, budget) = context(false, Cost { metadata_bytes: 1, ..Cost::defaults() });
    let fetcher = Failed(Cell::new(0));
    for _ in 0..2 { assert_eq!(ctx.ensure_public_with(&fetcher).unwrap_err().code, "records.query_resource_budget"); }
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
    assert_eq!(fetcher.0.get(), 0);
    assert_eq!(ctx.source.state, "unavailable");
}
#[test]
fn corrupt_public_cache_uses_core_validation_without_fetch_or_network_charge() {
    let (home, mut ctx, budget) = context(false, Cost::defaults());
    let path = evidence::blob_path(&home.0, PUBLIC_SHA256).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"corrupt").unwrap();
    let fetcher = Failed(Cell::new(0));
    assert!(ctx.ensure_public_with(&fetcher).is_err());
    assert_eq!(ctx.source.state, "unavailable");
    assert_eq!(fetcher.0.get(), 0);
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}
#[test]
fn independent_budget_stop_and_original_invocation_stop_prevent_materialization() {
    for independent in [false, true] {
        let (_home, mut ctx, _) = context(false, Cost::defaults());
        if independent {
            ctx.budget = Some(Arc::new(Budget::new(Cost::defaults(), ctx.deadline,
                Some(Arc::new(AtomicBool::new(true))))));
        } else { ctx.control.tracked_cancel_handle().unwrap().cancel(); }
        let fetcher = Failed(Cell::new(0));
        let code = ctx.ensure_public_with(&fetcher).unwrap_err().code;
        assert_eq!(code, if independent { "operation.cancelled" } else { "command.cancelled" });
        assert_eq!(fetcher.0.get(), 0);
    }
}
#[test]
fn public_fetch_digest_failure_never_publishes_or_loads_keys() {
    struct Wrong;
    impl http::Fetcher for Wrong {
        fn fetch(&self, _: &OperationControl, _: Option<&Budget>, _: Instant) -> Result<Vec<u8>> { Ok(vec![b'0'; PUBLIC_BYTES]) }
    }
    let (home, mut ctx, _) = context(false, Cost::defaults());
    assert_eq!(ctx.ensure_public_with(&Wrong).unwrap_err().code, "container.integrity");
    assert!(ctx.keys().is_empty());
    assert!(!evidence::blob_path(&home.0, PUBLIC_SHA256).unwrap().exists());
}
#[test]
fn public_text_parser_observes_independent_stop_inside_actual_line_walk() {
    let raw = b"# first\n000000000000002A 000102030405060708090A0B0C0D0E0F\n";
    let mut checks = 0;
    let result = KeySet::parse_checked(raw, "text", &storage::sha256(raw), || {
        checks += 1;
        if checks == 4 { Err(fault("operation.cancelled", 7, "scripted line stop")) } else { Ok(()) }
    });
    assert_eq!(result.err().unwrap().code, "operation.cancelled");
    assert_eq!(checks, 4);
}
#[test]
fn scoped_constructor_uses_only_admitted_home_and_original_control() {
    let (home, ctx, budget) = context(true, Cost::defaults());
    let mut scoped = Context::prepare_scoped(&home.0, None, true, &ctx.control,
        ctx.deadline, Some(budget)).unwrap();
    assert_eq!(scoped.home, home.0);
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(scoped.ensure_public_with(&Failed(Cell::new(0))).unwrap_err().code, "command.cancelled");
}
