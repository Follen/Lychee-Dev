use super::*;
use crate::mailbox::lookup;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct FixtureClearControl;
impl lycheedev_windows::process::RuntimeControl for FixtureClearControl {
    fn check(&self) -> std::result::Result<(), String> {
        Ok(())
    }
}
struct FixtureFlagControl(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl lycheedev_windows::process::RuntimeControl for FixtureFlagControl {
    fn check(&self) -> std::result::Result<(), String> {
        if self.0.load(std::sync::atomic::Ordering::Acquire) {
            Err("memory.runtime_cancelled".into())
        } else {
            Ok(())
        }
    }
}
fn client() -> ClientInstallation {
    ClientInstallation {
        directory: "synthetic".into(),
        product: "forever".into(),
        product_code: "wow_forever".into(),
        catalog_product: "wow_classic_beta".into(),
        display_name: "Forever Asia Beta".into(),
        channel: "asia-beta".into(),
        metadata_branch: "us".into(),
        full_build: "1.60.1.70338".into(),
        interface: 16001,
        interface_known: true,
        toc: "Lychee Dev.toc".into(),
        identity_source: "synthetic".into(),
    }
}
fn admission() -> RuntimeLayoutAdmission {
    RuntimeLayoutAdmission::select_formal(&client(), RecipeId::Forever).unwrap()
}
fn budget() -> RuntimeBudget {
    RuntimeBudget::with_control_factory(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        0,
        || FixtureClearControl,
    )
    .unwrap()
}

struct Fixture {
    evidence: RootEvidence,
    bytes: BTreeMap<u64, Vec<u8>>,
    next: u64,
    validations: usize,
    fail_validation: usize,
    read_calls: Cell<usize>,
    fail_read: Cell<bool>,
    cancel_on_read: Option<Arc<AtomicBool>>,
    close_calls: Option<Arc<AtomicUsize>>,
    close_error: Option<&'static str>,
    tear_root: Cell<bool>,
    scope_checks: Cell<usize>,
    change_scope_at: usize,
    dropped: Option<Arc<AtomicUsize>>,
}
impl Fixture {
    fn new() -> Self {
        let mut this = Self {
            evidence: RootEvidence {
                recipe: Some(RecipeId::Forever),
                root_rva: Some(0x1000),
                full_text_pass: true,
                stable_guards: true,
                ..RootEvidence::default()
            },
            bytes: BTreeMap::new(),
            next: 0x2000,
            validations: 0,
            fail_validation: usize::MAX,
            read_calls: Cell::new(0),
            fail_read: Cell::new(false),
            cancel_on_read: None,
            close_calls: None,
            close_error: None,
            tear_root: Cell::new(false),
            scope_checks: Cell::new(0),
            change_scope_at: usize::MAX,
            dropped: None,
        };
        let globals = this.table(&[]);
        this.state(globals);
        this
    }
    fn alloc(&mut self, bytes: Vec<u8>) -> u64 {
        let address = self.next;
        self.next += (bytes.len() as u64 + 15) & !15;
        self.bytes.insert(address, bytes);
        address
    }
    fn string(&mut self, value: &[u8]) -> u64 {
        let mut bytes = vec![0; 32];
        bytes[16] = 4;
        bytes[24..32].copy_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value);
        self.alloc(bytes)
    }
    fn table(&mut self, fields: &[(&str, u64, u8)]) -> u64 {
        let mut nodes = vec![0u8; 256 * 56];
        let hash = |key: &str| {
            let mut hash = key.len() as u32;
            let step = (key.len() >> 5) + 1;
            let mut n = key.len();
            while n >= step {
                hash ^= hash
                    .wrapping_shl(5)
                    .wrapping_add(hash >> 2)
                    .wrapping_add(key.as_bytes()[n - 1] as u32);
                n -= step;
            }
            (hash & 255) as usize
        };
        let primary: BTreeSet<_> = fields.iter().map(|(key, _, _)| hash(key)).collect();
        let mut used = BTreeSet::new();
        let mut tails = BTreeMap::new();
        let node_address = self.next;
        self.next += nodes.len() as u64;
        for &(key, value, tag) in fields {
            let bucket = hash(key);
            let index = if used.contains(&bucket) {
                (0..256)
                    .find(|i| !primary.contains(i) && !used.contains(i))
                    .unwrap()
            } else {
                bucket
            };
            used.insert(index);
            let at = index * 56;
            if let Some(previous) = tails.insert(bucket, index) {
                let link = previous * 56 + 48;
                nodes[link..link + 8].copy_from_slice(&(node_address + at as u64).to_le_bytes());
            }
            let name = self.string(key.as_bytes());
            nodes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            nodes[at + 8] = tag;
            nodes[at + 24..at + 32].copy_from_slice(&name.to_le_bytes());
            nodes[at + 32] = 4;
        }
        self.bytes.insert(node_address, nodes);
        let mut table = vec![0; 72];
        table[16] = 5;
        table[19] = 8;
        table[40..48].copy_from_slice(&node_address.to_le_bytes());
        self.alloc(table)
    }
    fn state(&mut self, globals: u64) {
        let mut state = vec![0; 0x90 + 24];
        state[16] = 8;
        state[0x90..0x98].copy_from_slice(&globals.to_le_bytes());
        state[0x98] = 5;
        let address = self.alloc(state);
        self.bytes.insert(0x1000, address.to_le_bytes().to_vec());
    }
    fn publication(&mut self) {
        self.publication_payload(b"identity", "3.1.2");
    }
    fn publication_payload(&mut self, payload: &[u8], expected_release: &str) {
        let r = Record {
            address: 0,
            nonce: "0".repeat(32),
            runtime: "2".repeat(32),
            ticket: "0".repeat(32),
            kind: 1,
            state: 1,
            sequence: 1,
            checksum: 0,
            payload: payload.to_vec(),
        };
        let record = self.string(&lookup::encode(&r).unwrap());
        let schema = self.string(crate::mailbox::SCHEMA.as_bytes());
        let release = self.string(expected_release.as_bytes());
        let runtime = self.string(r.runtime.as_bytes());
        let mailbox = self.table(&[
            ("schema", schema, 4),
            ("release", release, 4),
            ("runtime", runtime, 4),
            ("identity", record, 4),
        ]);
        let namespace = self.table(&[("Mailbox", mailbox, 5)]);
        let globals = self.table(&[("LycheeDevInternal", namespace, 5)]);
        self.state(globals);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(dropped) = &self.dropped {
            dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl Source for Fixture {
    fn close(self, budget: &RuntimeBudget) -> Result<()> {
        if let Some(calls) = &self.close_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        match self.close_error {
            Some(cause) => Err(cause.into()),
            None => budget.check(),
        }
    }
    fn root(&self) -> u64 {
        0x1000
    }
    fn evidence(&self) -> &RootEvidence {
        &self.evidence
    }
    fn verify_scope(&self, _: &RuntimeLayoutAdmission, budget: &mut RuntimeBudget) -> Result<()> {
        self.scope_checks.set(self.scope_checks.get() + 1);
        let issued = client();
        let mut current = client();
        if self.scope_checks.get() == self.change_scope_at {
            current.full_build.push('1');
        }
        check_client_identity(&current, &issued, budget)
    }
    fn revalidate(&mut self, budget: &mut RuntimeBudget) -> Result<()> {
        budget.charge(RuntimeCost::work(10))?;
        self.validations += 1;
        if self.validations == self.fail_validation {
            self.evidence.stable_guards = false;
            return Err("fixture.anchor_changed".into());
        }
        Ok(())
    }
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        budget.charge(RuntimeCost::work(1))
    }
    fn read(
        &self,
        address: u64,
        out: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        budget.charge(RuntimeCost {
            read_calls: 1,
            requested_bytes: out.len() as u64,
            ..RuntimeCost::default()
        })?;
        self.read_calls.set(self.read_calls.get() + 1);
        if let Some(cancel) = &self.cancel_on_read {
            cancel.store(true, Ordering::Release);
        }
        if self.fail_read.get() {
            return Err("fixture.os_partial_copy:299".into());
        }
        if module && (address != 0x1000 || out.len() != 8) {
            return Err("fixture.module_range".into());
        }
        let (base, bytes) = self
            .bytes
            .range(..=address)
            .next_back()
            .ok_or("fixture.unmapped")?;
        let at = (address - base) as usize;
        out.copy_from_slice(bytes.get(at..at + out.len()).ok_or("fixture.short")?);
        if module && self.tear_root.get() && self.read_calls.get() > 1 {
            out[0] ^= 16;
        }
        Ok(())
    }
}
fn bound(source: Fixture, budget: &mut RuntimeBudget) -> Bound<Fixture> {
    Bound::bind(source, admission(), "3.1.2", budget).unwrap()
}


#[test]
fn formal_lookup_decode_preserves_identity_fields_and_optional_inventory() {
    use serde_json::json;
    for inventory in [None, Some(0), Some(200), Some(500)] {
        let mut payload = json!({"schema":crate::mailbox::SCHEMA,"runtime":"2".repeat(32),"nextSlot":1,
            "inputState":crate::mailbox::SCHEMA,"guid":"Player-1-ABC","character":"测试角色",
            "realm":"测试 realm","product":"forever","build":"1.60.1.70338",
            "release":lycheedev_core::VERSION,"slots":500});
        if let Some(inventory) = inventory { payload["inventory"] = json!(inventory); }
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut fixture = Fixture::new();
        fixture.dropped = Some(dropped.clone());
        fixture.publication_payload(&serde_json::to_vec(&payload).unwrap(), lycheedev_core::VERSION);
        let mut b = budget();
        let request = b.request_id();
        let mut reader = Bound::bind(fixture, admission(), lycheedev_core::VERSION, &mut b).unwrap();
        let record = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap().unwrap();
        let identity = crate::mailbox::decode_identity(&record.contract().unwrap(), lycheedev_core::VERSION).unwrap();
        assert_eq!(identity.runtime, "2".repeat(32));
        assert_eq!(identity.guid, "Player-1-ABC");
        assert_eq!(identity.character, "测试角色");
        assert_eq!(identity.realm, "测试 realm");
        assert_eq!(identity.product, "forever");
        assert_eq!(identity.build, "1.60.1.70338");
        assert_eq!(identity.release, lycheedev_core::VERSION);
        assert_eq!(identity.slots, 500);
        assert_eq!(identity.inventory, inventory);
        assert_eq!(b.request_id(), request);
        assert!(b.usage().read_calls > 10);
        assert!(b.usage().allocation_bytes > 0);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        reader.source.close(&b).unwrap();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn formal_lookup_read_limit_retains_coverage_and_releases_its_owned_source() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut fixture = Fixture::new();
    fixture.dropped = Some(dropped.clone());
    fixture.publication();
    let limits = RuntimeLimits { read_calls: 1, ..RuntimeLimits::defaults() };
    let mut b = runtime_budget(&lycheedev_core::control::OperationControl::default(), limits).unwrap();
    let mut reader = bound(fixture, &mut b);
    let failure = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(failure.cause, "memory.runtime_budget:reads");
    assert_eq!(failure.usage.read_calls, 1);
    assert_eq!(failure.root.recipe, Some(RecipeId::Forever));
    assert!(failure.root.full_text_pass);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    reader.source.close(&b).unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn layout_admission_rejects_every_altered_field() {
    let a = admission();
    for field in 0..8 {
        let mut l = a.layout();
        match field {
            0 => l.id.push('x'),
            1 => l.state_globals += 1,
            2 => l.tag_offset += 1,
            3 => l.string_length += 1,
            4 => l.string_data += 1,
            5 => l.table_nodes += 1,
            6 => l.table_log2 += 1,
            _ => l.node_bytes += 1,
        }
        assert!(a.admits_layout(&l).is_err());
    }
    assert!(a.admits_layout(&a.layout()).is_ok());
}
#[test]
fn bind_recipe_mismatch_never_reads_or_revalidates() {
    let mut f = Fixture::new();
    f.evidence.recipe = Some(RecipeId::Retail);
    let error = Bound::bind(f, admission(), "3.1.2", &mut budget())
        .err()
        .unwrap();
    assert_eq!(error.cause, "reader.runtime_recipe_mismatch");
    assert_eq!(error.usage, RuntimeCost::default());
}
#[test]
fn stable_absence_keeps_request_usage_and_rechecks_each_lookup() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_none()
    );
    let first = b.usage();
    assert!(first.read_calls > 0 && first.allocation_bytes > 0);
    assert_eq!(reader.source.validations, 3);
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_none()
    );
    assert_eq!(reader.source.validations, 5);
    assert!(b.usage().read_calls > first.read_calls);
    assert!(b.usage().allocation_bytes > first.allocation_bytes);
}
#[test]
fn bounded_publication_uses_named_path_and_returns_record() {
    let mut f = Fixture::new();
    f.publication();
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let r = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap().unwrap();
    assert_eq!(r.payload, b"identity");
    assert!(r.address > 0 && b.usage().read_calls > 10);
    assert_eq!(reader.source.validations, 3);
}
#[test]
fn replacement_budget_poison_cannot_reset_ledger() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    let original = b.usage();
    let e = reader
        .lookup_formal(&formal_identity_selector(), None, &mut budget())
        .unwrap_err();
    assert_eq!(e.cause, "reader.runtime_budget_identity");
    assert_eq!(e.usage, original);
    assert_eq!(reader.source.read_calls.get(), 0);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
    assert_eq!(b.usage(), original);
}
#[test]
fn cancellation_before_lookup_reads_nothing() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::with_control_factory(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        0,
        || FixtureFlagControl(cancel.clone()),
    )
    .unwrap();
    let mut reader = bound(Fixture::new(), &mut b);
    cancel.store(true, Ordering::Release);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_cancelled"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn root_before_lookup_failure_retains_evidence() {
    let mut f = Fixture::new();
    f.fail_validation = 2;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "before_lookup");
    assert!(!e.root.stable_guards);
    assert_eq!(e.usage, b.usage());
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn post_lookup_root_change_discards_successful_record() {
    let mut f = Fixture::new();
    f.publication();
    f.fail_validation = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "after_lookup");
    assert!(e.usage.read_calls > 0);
    let calls = reader.source.read_calls.get();
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
    assert_eq!(reader.source.read_calls.get(), calls);
}
#[test]
fn os_read_failure_keeps_exact_cause_and_never_retries() {
    let f = Fixture::new();
    f.fail_read.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.os_partial_copy:299");
    assert_eq!(e.stage, "lookup");
    assert_eq!(e.usage.read_calls, 1);
    assert_eq!(reader.source.validations, 3);
    reader.source.fail_read.set(false);
    assert!(reader.lookup_formal(&formal_identity_selector(), None, &mut b).is_err());
    assert_eq!(reader.source.read_calls.get(), 1);
}
#[test]
fn reader_root_path_tearing_discards_observation() {
    let f = Fixture::new();
    f.tear_root.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.path_changed"
    );
}
#[test]
fn exhausted_shared_allocation_budget_fails_before_path_read() {
    let mut limits = RuntimeCost::defaults();
    limits.allocation_bytes = 5;
    let mut b = RuntimeBudget::with_control_factory(
        limits,
        Instant::now() + Duration::from_secs(30),
        0,
        || FixtureClearControl,
    )
    .unwrap();
    let mut reader = bound(Fixture::new(), &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_budget:allocations"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn invalid_selector_cannot_reach_heap() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    assert_eq!(
        reader
            .lookup_formal(
                &crate::mailbox::Selector { kind: crate::mailbox::Kind::Receipt, ..formal_identity_selector() },
                None, &mut b
            )
            .unwrap_err()
            .cause,
        "mailbox.receipt_nonce"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}

#[test]
fn expired_deadline_and_invalid_release_refuse_binding() {
    let expired =
        RuntimeBudget::with_control_factory(RuntimeCost::defaults(), Instant::now(), 0, || {
            FixtureClearControl
        });
    assert_eq!(expired.err().unwrap(), "memory.runtime_deadline");
    for release in ["", "版本"] {
        let e = Bound::bind(Fixture::new(), admission(), release, &mut budget())
            .err()
            .unwrap();
        assert_eq!(e.cause, "reader.runtime_release");
        assert_eq!(e.usage, RuntimeCost::default());
    }
}

#[test]
fn bind_guard_failure_preserves_partial_root_evidence_and_cause() {
    let mut f = Fixture::new();
    f.fail_validation = 1;
    let mut b = budget();
    let e = Bound::bind(f, admission(), "3.1.2", &mut b).err().unwrap();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "bind");
    assert!(!e.root.stable_guards);
    assert_eq!(e.usage, b.usage());
}

#[test]
fn malformed_build_does_not_enter_catalog_matching() {
    for build in ["1.60.1", "1.60.1.70338.0", "1.60..70338", "1.60.1.70338x"] {
        let mut c = client();
        c.full_build = build.into();
        assert_eq!(
            RuntimeLayoutAdmission::select_formal(&c, RecipeId::Forever)
                .err()
                .unwrap(),
            "reader.runtime_client_identity"
        );
    }
}

#[test]
fn scope_identity_rejects_missing_or_foreign_module() {
    assert_eq!(
        check_scope_identity::<u64>(None, &1).unwrap_err(),
        "reader.runtime_scope_missing"
    );
    assert_eq!(
        check_scope_identity(Some(&1), &2).unwrap_err(),
        "reader.runtime_scope_module_changed"
    );
    assert!(check_scope_identity(Some(&1), &1).is_ok());
}

#[test]
fn metadata_change_before_lookup_prevents_heap_read() {
    let mut f = Fixture::new();
    f.change_scope_at = 2;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(e.cause, "reader.runtime_metadata_changed");
    assert_eq!(e.stage, "before_lookup");
    assert_eq!(reader.source.read_calls.get(), 0);
}

#[test]
fn metadata_change_after_lookup_discards_record_and_poisons() {
    let mut f = Fixture::new();
    f.publication();
    f.change_scope_at = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup_formal(&formal_identity_selector(), None, &mut b).unwrap_err();
    assert_eq!(e.cause, "reader.runtime_metadata_changed");
    assert_eq!(e.stage, "after_lookup");
    assert!(e.usage.read_calls > 0);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
}

#[test]
fn runtime_budget_observes_parent_cancel_after_issuance() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    let parent_token = TrackedCancellation::new();
    let parent = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        parent_token.clone(),
    );
    let child = parent.ordered_child(parent.deadline()).unwrap();
    let mut b = runtime_budget(&child, RuntimeLimits::defaults()).unwrap();
    parent_token.cancel();
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    let before = b.usage();
    assert_eq!(
        b.charge(RuntimeCost::work(1)).unwrap_err(),
        "memory.runtime_cancelled"
    );
    assert_eq!(b.usage(), before);
}

#[test]
fn runtime_budget_keeps_nested_ancestry_and_independent_child_cancellation() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    let top_token = TrackedCancellation::new();
    let top = OperationControl::tracked(Instant::now() + Duration::from_secs(30), top_token.clone());
    let middle = top.ordered_child(top.deadline()).unwrap();
    let leaf = middle.ordered_child(middle.deadline()).unwrap();
    let b = runtime_budget(&leaf, RuntimeLimits::defaults()).unwrap();
    assert!(b.usage().allocation_bytes >= leaf.clone_allocation_bytes() as u64);
    top_token.cancel();
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    let independent_top = OperationControl::default();
    let independent_child = independent_top
        .ordered_child(independent_top.deadline())
        .unwrap();
    let b = runtime_budget(&independent_child, RuntimeLimits::defaults()).unwrap();
    independent_child
        .tracked_cancel_handle()
        .unwrap()
        .cancel();
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    assert!(independent_top.check().is_ok());
}

#[test]
fn runtime_budget_inherits_expired_parent_deadline() {
    use lycheedev_core::control::OperationControl;
    let parent = OperationControl::uncancelable(Instant::now());
    let child = parent
        .ordered_child(Instant::now() + Duration::from_secs(30))
        .unwrap();
    assert_eq!(
        runtime_budget(&child, RuntimeLimits::defaults())
            .err()
            .unwrap(),
        "memory.runtime_deadline"
    );
}

fn formal_identity_selector() -> crate::mailbox::Selector {
    use crate::mailbox::{Kind, Selector, ZERO_TOKEN};
    Selector {
        kind: Kind::Identity,
        nonce: ZERO_TOKEN,
        runtime: ZERO_TOKEN,
        ticket: ZERO_TOKEN,
        body: None,
    }
}
#[test]
fn go320_formal_abi_follows_all_seven_exact_product_slots_without_old_build_allowlist() {
    for product in &client::PRODUCTS {
        let mut c = client();
        c.product = product.family.into();
        c.product_code = if product.family == "forever" {
            "wow_forever"
        } else {
            product.catalog_product
        }
        .into();
        c.catalog_product = if product.id != product.family {
            product.catalog_product
        } else {
            ""
        }
        .into();
        c.channel = product.channel.into();
        c.full_build = format!("{}.99999", product.series);
        for recipe in [
            RecipeId::Retail,
            RecipeId::Forever,
            RecipeId::ForeverContext,
        ] {
            let admitted = RuntimeLayoutAdmission::select_formal(&c, recipe).unwrap();
            assert_eq!(admitted.evidence_sha256(), FORMAL_LAYOUT_SOURCE);
            admitted.admits_layout(&admitted.layout()).unwrap();
        }
        c.product_code = "unrelated".into();
        assert!(RuntimeLayoutAdmission::select_formal(&c, RecipeId::Retail).is_err());
    }
}
#[test]
fn go320_root_selection_prefers_actual_or_upgrade_mailbox_and_rejects_ties() {
    assert_eq!(choose_observed_root(&[Ok(false), Ok(true)]).unwrap(), 1);
    assert_eq!(
        choose_observed_root(&[Err("mailbox.thread_tag".into()), Ok(false)]).unwrap(),
        1
    );
    assert_eq!(
        choose_observed_root(&[
            Ok(false),
            Err("memory.mailbox_incompatible: mailbox_schema_mismatch".into())
        ])
        .unwrap(),
        1
    );
    assert!(choose_observed_root(&[Ok(true), Ok(true)]).is_err());
    assert!(choose_observed_root(&[Ok(false), Ok(false)]).is_err());
    assert!(
        choose_observed_root(&[
            Err("mailbox.thread_tag".into()),
            Err("mailbox.globals".into())
        ])
        .unwrap_err()
        .contains("mailbox.globals")
    );
    assert!(fatal_lookup_cause("mailbox.path_changed\nwindow.reused"));
    assert!(fatal_lookup_cause(
        "memory.mailbox_incompatible: mailbox_schema_mismatch\nmemory.runtime_cancelled"
    ));
}
#[test]
fn go320_formal_accept_runs_after_outer_root_guard_and_cannot_hide_cancellation() {
    let mut f = Fixture::new();
    f.publication();
    f.fail_validation = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let calls = Cell::new(0);
    let mut accept = |_: &Record| {
        calls.set(calls.get() + 1);
        true
    };
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), Some(&mut accept), &mut b)
            .unwrap_err()
            .cause,
        "fixture.anchor_changed"
    );
    assert_eq!(calls.get(), 0);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::with_control_factory(
        RuntimeLimits::defaults(),
        Instant::now() + Duration::from_secs(10),
        0,
        || FixtureFlagControl(cancel.clone()),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.publication();
    let mut reader = bound(f, &mut b);
    let mut accept = |_: &Record| {
        cancel.store(true, Ordering::Release);
        false
    };
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), Some(&mut accept), &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_cancelled"
    );
}
#[test]
fn go320_transient_named_path_can_recover_without_new_binding_or_budget() {
    let f = Fixture::new();
    f.tear_root.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.path_changed"
    );
    let used = b.usage();
    reader.source.tear_root.set(false);
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_none()
    );
    assert!(b.usage().read_calls > used.read_calls);
    assert!(reader.poisoned.is_none());
}
#[test]
fn go320_state_null_is_observation_and_does_not_decide_driver_retry_policy() {
    let mut f = Fixture::new();
    f.bytes.insert(0x1000, 0u64.to_le_bytes().to_vec());
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.lua_state_null"
    );
    reader.source.publication();
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_some()
    );
}

#[test]
fn go320_generic_forever_identity_cannot_replace_a_selected_regional_slot() {
    let mut legacy = client();
    legacy.catalog_product.clear();
    legacy.channel.clear();
    assert_eq!(
        RuntimeLayoutAdmission::select_formal(&legacy, RecipeId::ForeverContext)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
}
#[test]
fn go320_candidate_selection_uses_actual_named_lua_observations() {
    let mut b = budget();
    let absent = Fixture::new();
    let mut present = Fixture::new();
    present.publication();
    let mut malformed = Fixture::new();
    malformed.bytes.insert(0x1000, 0u64.to_le_bytes().to_vec());
    let observations: Vec<_> = [absent, present, malformed]
        .into_iter()
        .map(|fixture| {
            let mut reader = bound(fixture, &mut b);
            reader
                .lookup_formal(&formal_identity_selector(), None, &mut b)
                .map(|record| record.is_some())
                .map_err(|failure| failure.cause)
        })
        .collect();
    assert_eq!(choose_observed_root(&observations).unwrap(), 1);
    assert!(b.usage().read_calls > 10);
}

#[test]
fn failed_lookup_joins_independent_outer_recipe_and_metadata_changes_without_retry() {
    for fail_read in [false, true] {
        let mut f = Fixture::new();
        f.tear_root.set(!fail_read);
        f.fail_read.set(fail_read);
        f.fail_validation = 3; // bind, before lookup, mandatory error-return recheck
        f.change_scope_at = 3;
        let mut b = budget();
        let mut reader = bound(f, &mut b);
        let request = b.request_id();
        let failure = reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err();
        let original = if fail_read {
            "fixture.os_partial_copy:299"
        } else {
            "mailbox.path_changed"
        };
        assert_eq!(failure.cause.lines().next(), Some(original));
        assert!(failure.cause.contains("fixture.anchor_changed"));
        assert!(failure.cause.contains("reader.runtime_metadata_changed"));
        assert_eq!(failure.stage, "after_lookup");
        assert_eq!(reader.source.validations, 3);
        assert_eq!(reader.source.scope_checks.get(), 3);
        assert_eq!(b.request_id(), request);
        assert!(!transient_publication(&failure.cause));
        if fail_read {
            let first = failure.first_read_failure.unwrap();
            assert_eq!(first.path, "root.state");
            assert_eq!(first.source, "module");
        }
        let reads = reader.source.read_calls.get();
        assert_eq!(
            reader
                .lookup_formal(&formal_identity_selector(), None, &mut b)
                .unwrap_err()
                .stage,
            "poisoned"
        );
        assert_eq!(reader.source.read_calls.get(), reads);
    }
}

#[test]
fn failed_lookup_retains_original_read_detail_and_cancellation_at_outer_guard_boundary() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::with_control_factory(
        RuntimeLimits::defaults(),
        Instant::now() + Duration::from_secs(10),
        0,
        || FixtureFlagControl(cancel.clone()),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.fail_read.set(true);
    f.cancel_on_read = Some(cancel);
    let mut reader = bound(f, &mut b);
    let failure = reader
        .lookup_formal(&formal_identity_selector(), None, &mut b)
        .unwrap_err();
    assert_eq!(
        failure.cause.lines().next(),
        Some("fixture.os_partial_copy:299")
    );
    assert!(failure.cause.contains("memory.runtime_cancelled"));
    assert_eq!(failure.first_read_failure.unwrap().path, "root.state");
    assert!(reader.poisoned.is_some());
}

#[test]
fn binding_cleanup_retains_original_diagnostics_and_all_close_failures() {
    let detail = lookup::ReadFailure {
        path: "record.wire_header".into(),
        requested_bytes: 80,
        returned_bytes: Some(7),
        source: "private",
        reason: "native_read_failed",
        guard_recheck: "verified",
        io_attempted: Some(true),
        native_error_code: Some(299),
        region: None,
    };
    let failure = RuntimeBindingFailure {
        cause: "fixture.read_failed".into(),
        stage: "lookup",
        root: Box::default(),
        usage: RuntimeCost::default(),
        first_read_failure: Some(detail.clone()),
    };
    let failure = join_close_failure(
        failure,
        Err("memory.runtime_process_close: fixture\nmemory.runtime_cancelled".into()),
    );
    assert_eq!(
        failure.cause.lines().collect::<Vec<_>>(),
        vec![
            "fixture.read_failed",
            "memory.runtime_process_close: fixture",
            "memory.runtime_cancelled"
        ]
    );
    assert_eq!(failure.first_read_failure, Some(detail));
    assert_eq!(failure.stage, "lookup");
}

#[test]
fn failed_binding_closes_its_owned_source_once_and_preserves_both_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut source = Fixture::new();
    source.fail_validation = 1;
    source.close_calls = Some(calls.clone());
    source.close_error = Some("fixture.checked_close_failed");
    source.dropped = Some(drops.clone());
    let mut b = budget();
    let failure = Bound::bind(source, admission(), "3.1.2", &mut b)
        .err()
        .unwrap();
    assert_eq!(
        failure.cause,
        "fixture.anchor_changed\nfixture.checked_close_failed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
