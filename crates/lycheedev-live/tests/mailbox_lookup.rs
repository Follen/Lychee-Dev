use lycheedev_live::{
    locator::Layout,
    mailbox::{self, Memory, Reader, Record, Selector},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
};
struct Fixture {
    bytes: RefCell<BTreeMap<u64, Vec<u8>>>,
    next: u64,
    root: u64,
    namespace_value: u64,
    record_data: u64,
    mailbox_nodes: BTreeMap<String, u64>,
    namespace_nodes: BTreeMap<String, u64>,
    retire_record: Cell<bool>,
    mutate: Cell<bool>,
    calls: Cell<u32>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            bytes: RefCell::new(BTreeMap::new()),
            next: 0x2000,
            root: 0x1000,
            namespace_value: 0,
            record_data: 0,
            mailbox_nodes: BTreeMap::new(),
            namespace_nodes: BTreeMap::new(),
            retire_record: Cell::new(false),
            mutate: Cell::new(false),
            calls: Cell::new(0),
        }
    }
    fn alloc(&mut self, b: Vec<u8>) -> u64 {
        let a = self.next;
        self.next += (b.len() as u64 + 15) & !15;
        self.bytes.get_mut().insert(a, b);
        a
    }
    fn string(&mut self, s: &[u8]) -> u64 {
        let mut b = vec![0; 32];
        b[16] = 4;
        b[24..32].copy_from_slice(&(s.len() as u64).to_le_bytes());
        b.extend(s);
        self.alloc(b)
    }
    fn table(&mut self, fields: Vec<(&str, u64, u8)>) -> (u64, BTreeMap<String, u64>) {
        let primary: BTreeSet<usize> = fields
            .iter()
            .map(|(k, _, _)| (lua_hash(k) & 255) as usize)
            .collect();
        let mut used = BTreeSet::new();
        let mut nodes = vec![0; 56 * 256];
        let mut locations = BTreeMap::new();
        let node_base = self.next;
        // Reserve nodes before allocating key strings so node pointers remain exact.
        self.next += nodes.len() as u64;
        let mut tail = BTreeMap::new();
        for (key, value, tag) in fields {
            let primary_index = (lua_hash(key) & 255) as usize;
            let index = if used.contains(&primary_index) {
                (0..256)
                    .find(|i| !primary.contains(i) && !used.contains(i))
                    .unwrap()
            } else {
                primary_index
            };
            used.insert(index);
            let at = index * 56;
            let str = self.string(key.as_bytes());
            nodes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            nodes[at + 8] = tag;
            nodes[at + 24..at + 32].copy_from_slice(&str.to_le_bytes());
            nodes[at + 32] = 4;
            if let Some(previous) = tail.insert(primary_index, index) {
                let link = previous * 56 + 48;
                nodes[link..link + 8].copy_from_slice(&(node_base + at as u64).to_le_bytes());
            }
            locations.insert(key.into(), node_base + at as u64);
        }
        self.bytes.get_mut().insert(node_base, nodes);
        let mut header = vec![0; 72];
        header[16] = 5;
        header[19] = 8;
        header[40..48].copy_from_slice(&node_base.to_le_bytes());
        (self.alloc(header), locations)
    }
    fn schema_only_map(&mut self) -> u64 {
        let value = self.string(lycheedev_live::go_mailbox_v1::SCHEMA.as_bytes());
        let key = self.string(b"schema");
        let mut node = vec![0; 56];
        node[..8].copy_from_slice(&value.to_le_bytes());
        node[8] = 4;
        node[24..32].copy_from_slice(&key.to_le_bytes());
        node[32] = 4;
        let address = self.alloc(node);
        let mut table = vec![0; 72];
        table[16] = 5;
        table[19] = 0;
        table[40..48].copy_from_slice(&address.to_le_bytes());
        table[48..56].copy_from_slice(&(address + 56).to_le_bytes());
        self.alloc(table)
    }
    fn replace_pointer(&mut self, at: u64, value: u64) {
        let (base, bytes) = self.bytes.get_mut().range_mut(..=at).next_back().unwrap();
        let offset = (at - base) as usize;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn publication(&mut self, record: &Record, body: Option<&Record>) {
        let wire = mailbox::encode(record).unwrap();
        let str = self.string(&wire);
        self.record_data = str + 32;
        let schema = self.string(lycheedev_live::go_mailbox_v1::SCHEMA.as_bytes());
        let release = self.string(b"3.1.2");
        let rt = self.string(record.runtime.as_bytes());
        let receipts = self
            .table(vec![("schema", schema, 4), (&record.nonce, str, 4)])
            .0;
        let bodies = if let Some(body) = body {
            let wire = mailbox::encode(body).unwrap();
            let str = self.string(&wire);
            self.table(vec![("schema", schema, 4), (&body.ticket, str, 4)])
                .0
        } else {
            self.schema_only_map()
        };
        let (mb, mailbox_nodes) = self.table(vec![
            ("schema", schema, 4),
            ("release", release, 4),
            ("runtime", rt, 4),
            ("input", str, 4),
            ("identity", str, 4),
            ("bootstrap", str, 4),
            ("receipts", receipts, 5),
            ("bodies", bodies, 5),
        ]);
        self.mailbox_nodes = mailbox_nodes;
        let (ns, namespace_nodes) =
            self.table(vec![("Mailbox", mb, 5), ("BootstrapMailbox", mb, 5)]);
        self.namespace_nodes = namespace_nodes;
        let (globals, keys) = self.table(vec![("LycheeDevInternal", ns, 5)]);
        self.namespace_value = keys["LycheeDevInternal"];
        let mut state = vec![0; 0x90 + 24];
        state[16] = 8;
        state[0x90..0x98].copy_from_slice(&globals.to_le_bytes());
        state[0x98] = 5;
        let state = self.alloc(state);
        self.bytes
            .get_mut()
            .insert(self.root, state.to_le_bytes().to_vec());
    }
    fn reader(&self) -> Reader<'_, Self> {
        Reader {
            memory: self,
            root: self.root,
            module_hash: "fixture".into(),
            release: "3.1.2".into(),
            layout: Layout {
                id: "synthetic-lua".into(),
                applicable_hashes: vec!["fixture".into()],
                state_globals: 0x90,
                tag_offset: 16,
                string_length: 24,
                string_data: 32,
                table_nodes: 40,
                table_log2: 19,
                node_bytes: 56,
            },
        }
    }
}
impl Memory for Fixture {
    fn verify(&self) -> mailbox::Result<()> {
        Ok(())
    }
    fn read(&self, address: u64, out: &mut [u8], _: bool) -> mailbox::Result<()> {
        self.calls.set(self.calls.get() + 1);
        if address == self.record_data && self.retire_record.replace(false) {
            let mut bytes = self.bytes.borrow_mut();
            let (base, b) = bytes
                .range_mut(..=self.namespace_value)
                .next_back()
                .unwrap();
            b[(self.namespace_value - *base) as usize + 9] = 1;
            return Err("fixture.short".into());
        }
        let bytes = self.bytes.borrow();
        let (base, b) = bytes
            .range(..=address)
            .next_back()
            .ok_or("fixture.unmapped")?;
        let start = (address - base) as usize;
        let slice = b.get(start..start + out.len()).ok_or("fixture.short")?;
        out.copy_from_slice(slice);
        drop(bytes);
        if self.mutate.get() && address == self.record_data {
            self.mutate.set(false);
            let mut bytes = self.bytes.borrow_mut();
            let (base, b) = bytes
                .range_mut(..=self.namespace_value)
                .next_back()
                .unwrap();
            b[(self.namespace_value - *base) as usize + 9] = 1;
        }
        Ok(())
    }
}
fn lua_hash(key: &str) -> u32 {
    let bytes = key.as_bytes();
    let mut hash = bytes.len() as u32;
    let step = (bytes.len() >> 5) + 1;
    let mut n = bytes.len();
    while n >= step {
        hash ^= hash
            .wrapping_shl(5)
            .wrapping_add(hash >> 2)
            .wrapping_add(bytes[n - 1] as u32);
        n -= step;
    }
    hash
}
fn record(kind: u8, payload: Vec<u8>) -> Record {
    Record {
        address: 0,
        nonce: "1".repeat(32),
        runtime: "2".repeat(32),
        ticket: "3".repeat(32),
        kind,
        state: if kind == 3 { 3 } else { 1 },
        sequence: 1,
        checksum: mailbox::checksum(&payload),
        payload,
    }
}
fn head(body: &Record, report_bytes: u32) -> Record {
    record(
        2,
        serde_json::to_vec(&serde_json::json!({
            "schema":lycheedev_live::go_mailbox_v1::SCHEMA,"runtime":body.runtime,
            "slots":500,"nextSlot":2,"guid":"g","character":"c","realm":"r",
            "build":"b","product":"retail","release":"3.1.2",
            "state":"reported","action":"commit","ticket":body.ticket,"nonce":body.nonce,
            "reportBytes":report_bytes,"reportChecksum":body.checksum
        }))
        .unwrap(),
    )
}
#[test]
fn complete_named_lookup_no_heap_scan() {
    let r = record(5, b"input".to_vec());
    let mut f = Fixture::new();
    f.publication(&r, None);
    assert_eq!(
        f.reader()
            .lookup(&Selector::Input)
            .unwrap()
            .unwrap()
            .payload,
        r.payload
    );
    assert!(f.calls.get() < 512);
}
#[test]
fn path_mutation_discards_sample() {
    let r = record(5, b"input".to_vec());
    let mut f = Fixture::new();
    f.publication(&r, None);
    f.mutate.set(true);
    assert_eq!(
        f.reader().lookup(&Selector::Input).unwrap_err(),
        "mailbox.path_changed"
    );
}
#[test]
fn secret_value_rejected_before_pointer_use() {
    let r = record(5, b"input".to_vec());
    let mut f = Fixture::new();
    f.publication(&r, None);
    let (base, b) = f
        .bytes
        .get_mut()
        .range_mut(..=f.namespace_value)
        .next_back()
        .unwrap();
    b[(f.namespace_value - *base) as usize + 9] = 1;
    assert_eq!(
        f.reader().lookup(&Selector::Input).unwrap_err(),
        "mailbox.secret_value"
    );
}
#[test]
fn exact_body_head_and_maximum_budget() {
    let body = record(3, vec![b'x'; 524288]);
    let head = head(&body, body.payload.len() as u32);
    let mut f = Fixture::new();
    f.publication(&head, Some(&body));
    let observed = f
        .reader()
        .lookup(&Selector::Receipt {
            nonce: head.nonce.clone(),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        f.reader()
            .lookup(&Selector::Body { head: observed })
            .unwrap()
            .unwrap()
            .payload
            .len(),
        524288
    );
}
#[test]
fn wrong_head_length_fails() {
    let body = record(3, b"result".to_vec());
    let head = head(&body, 5);
    let mut f = Fixture::new();
    f.publication(&head, Some(&body));
    assert!(f.reader().lookup(&Selector::Body { head }).is_err());
}
#[test]
fn corrupt_state_pointer_fails_without_overflow() {
    let r = record(5, b"input".to_vec());
    let mut f = Fixture::new();
    f.publication(&r, None);
    f.bytes
        .get_mut()
        .insert(f.root, u64::MAX.to_le_bytes().to_vec());
    assert_eq!(
        f.reader().lookup(&Selector::Input).unwrap_err(),
        "mailbox.state_address"
    );
}
#[test]
fn unsupported_globals_layout_is_not_inherited() {
    let r = record(5, b"input".to_vec());
    let mut f = Fixture::new();
    f.publication(&r, None);
    let mut reader = f.reader();
    reader.layout.state_globals = u64::MAX;
    assert_eq!(
        reader.lookup(&Selector::Input).unwrap_err(),
        "reader.layout_invalid"
    );
}

struct BudgetedFixture {
    fixture: Fixture,
    budget: RefCell<lycheedev_windows::process::RuntimeBudget>,
}
impl Memory for BudgetedFixture {
    fn verify(&self) -> mailbox::Result<()> {
        self.budget.borrow().check()?;
        self.fixture.verify()
    }
    fn read(&self, address: u64, bytes: &mut [u8], module: bool) -> mailbox::Result<()> {
        self.budget.borrow().check()?;
        self.fixture.read(address, bytes, module)
    }
    fn charge(&self, allocation_bytes: u64, work_units: u64) -> mailbox::Result<()> {
        self.budget
            .borrow_mut()
            .charge(lycheedev_windows::process::RuntimeCost {
                allocation_bytes,
                work_units,
                ..Default::default()
            })
    }
}
impl BudgetedFixture {
    fn new(limits: lycheedev_windows::process::RuntimeLimits) -> Self {
        let mut fixture = Fixture::new();
        fixture.publication(&record(5, b"input".to_vec()), None);
        Self {
            fixture,
            budget: RefCell::new(
                lycheedev_windows::process::RuntimeBudget::new(
                    limits,
                    std::time::Instant::now() + std::time::Duration::from_secs(30),
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                )
                .unwrap(),
            ),
        }
    }
    fn lookup(&self) -> mailbox::Result<Option<Record>> {
        let old = self.fixture.reader();
        Reader {
            memory: self,
            root: old.root,
            layout: old.layout,
            module_hash: old.module_hash,
            release: old.release,
        }
        .lookup(&Selector::Input)
    }
}
#[test]
fn global_allocation_budget_includes_path_guards_and_is_not_reset_by_lookup() {
    let mut limits = lycheedev_windows::process::RuntimeCost::defaults();
    limits.allocation_bytes = 65536;
    let f = BudgetedFixture::new(limits);
    assert!(f.lookup().unwrap().is_some());
    let first = f.budget.borrow().usage();
    assert!(first.allocation_bytes > 0 && first.work_units > 0);
    let mut exhausted = false;
    for _ in 0..32 {
        if let Err(error) = f.lookup() {
            assert!(error.contains("budget"), "{error}");
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "one caller ledger must bound repeated observations"
    );
    assert!(f.budget.borrow().usage().allocation_bytes >= first.allocation_bytes);
    assert!(f.budget.borrow().usage().allocation_bytes <= limits.allocation_bytes);
}
#[test]
fn guard_allocation_failure_discards_observation_before_following_root() {
    let mut limits = lycheedev_windows::process::RuntimeCost::defaults();
    // Eight root bytes plus the fixed logical-failure evidence reservation;
    // the following guard copy is deliberately not admitted.
    limits.allocation_bytes = 8 + 256;
    let f = BudgetedFixture::new(limits);
    assert!(f.lookup().is_err());
    assert_eq!(f.fixture.calls.get(), 1);
}
#[test]
fn malformed_selector_and_oversized_head_do_not_read_the_lua_path() {
    let f = Fixture::new();
    for nonce in ["1".repeat(33), "F".repeat(32), "0".repeat(32)] {
        assert_eq!(
            f.reader().lookup(&Selector::Receipt { nonce }).unwrap_err(),
            "mailbox.receipt_nonce"
        );
    }
    let head = record(2, vec![b' '; 16385]);
    assert_eq!(
        f.reader().lookup(&Selector::Body { head }).unwrap_err(),
        "mailbox.body_head"
    );
    assert_eq!(f.calls.get(), 0);
}
#[test]
fn a_valid_wire_record_of_another_kind_cannot_supply_input_state() {
    let mut f = Fixture::new();
    f.publication(&record(1, b"identity".to_vec()), None);
    // Place this valid identity wire in the named input field as an adversarial
    // publication; the fixture publishes the same string for both fields.
    assert!(f.reader().lookup(&Selector::Input).unwrap().is_none());
}

#[test]
fn schema_only_lua_maps_preserve_exact_missing_keys() {
    let body = record(3, b"".to_vec());
    let head = head(&body, 0);
    for field in ["receipts", "bodies"] {
        let mut fixture = Fixture::new();
        fixture.publication(&head, Some(&body));
        let map = fixture.schema_only_map();
        fixture.replace_pointer(fixture.mailbox_nodes[field], map);
        let selector = if field == "receipts" {
            Selector::Receipt {
                nonce: head.nonce.clone(),
            }
        } else {
            Selector::Body { head: head.clone() }
        };
        assert!(fixture.reader().lookup(&selector).unwrap().is_none());
        assert!(fixture.calls.get() < 512);
    }
}
#[test]
fn empty_body_is_a_record_but_missing_body_is_not() {
    let body = record(3, Vec::new());
    let head = head(&body, 0);
    let mut fixture = Fixture::new();
    fixture.publication(&head, Some(&body));
    let observed = fixture
        .reader()
        .lookup(&Selector::Body { head: head.clone() })
        .unwrap()
        .unwrap();
    assert!(observed.payload.is_empty());
    assert_eq!(observed.checksum, 1);
    let map = fixture.schema_only_map();
    fixture.replace_pointer(fixture.mailbox_nodes["bodies"], map);
    assert!(
        fixture
            .reader()
            .lookup(&Selector::Body { head })
            .unwrap()
            .is_none()
    );
}
#[test]
fn accept_is_never_called_before_path_guard_or_for_missing_token() {
    let r = record(5, b"input".to_vec());
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    fixture.mutate.set(true);
    let calls = Cell::new(0);
    let result = fixture.reader().lookup_with_accept(&Selector::Input, |_| {
        calls.set(calls.get() + 1);
        true
    });
    assert_eq!(result.unwrap_err(), "mailbox.path_changed");
    assert_eq!(calls.get(), 0);
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    assert!(
        fixture
            .reader()
            .lookup_with_accept(&Selector::Input, |_| {
                calls.set(calls.get() + 1);
                false
            })
            .unwrap()
            .is_none()
    );
    assert_eq!(calls.get(), 1);
    assert!(
        fixture
            .reader()
            .lookup_with_accept(
                &Selector::Receipt {
                    nonce: "9".repeat(32)
                },
                |_| {
                    calls.set(calls.get() + 1);
                    true
                }
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(calls.get(), 1);
}
#[test]
fn retired_record_short_read_rechecks_only_captured_path() {
    let r = record(5, b"input".to_vec());
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    fixture.retire_record.set(true);
    assert_eq!(
        fixture.reader().lookup(&Selector::Input).unwrap_err(),
        "mailbox.path_changed"
    );
    assert!(fixture.calls.get() < 512);
}
#[test]
fn bootstrap_uses_separate_namespace_and_confirmation_uses_kind_four() {
    let r = record(6, b"bootstrap diagnostic".to_vec());
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    fixture.replace_pointer(fixture.namespace_nodes["Mailbox"], u64::MAX);
    assert_eq!(
        fixture
            .reader()
            .lookup(&Selector::Bootstrap)
            .unwrap()
            .unwrap()
            .kind,
        6
    );
    assert!(fixture.reader().lookup(&Selector::Identity).is_err());
    let r = record(4, b"confirmation".to_vec());
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    assert_eq!(
        fixture
            .reader()
            .lookup(&Selector::Confirmation {
                nonce: r.nonce.clone()
            })
            .unwrap()
            .unwrap()
            .kind,
        4
    );
    assert!(
        fixture
            .reader()
            .lookup(&Selector::Receipt { nonce: r.nonce })
            .unwrap()
            .is_none()
    );
}
#[test]
fn previous_publication_schema_is_only_identity_or_input_evidence() {
    for kind in [1, 5] {
        let r = record(kind, b"old upgrade evidence".to_vec());
        let mut fixture = Fixture::new();
        fixture.publication(&r, None);
        let schema = fixture.string(lycheedev_live::go_mailbox_v1::PREVIOUS_SCHEMA.as_bytes());
        fixture.replace_pointer(fixture.mailbox_nodes["schema"], schema);
        let selector = if kind == 1 {
            Selector::Identity
        } else {
            Selector::Input
        };
        assert!(fixture.reader().lookup(&selector).unwrap().is_some());
        let error = fixture
            .reader()
            .lookup(&Selector::Receipt { nonce: r.nonce })
            .unwrap_err();
        assert_eq!(
            error,
            "memory.mailbox_incompatible: mailbox_schema_mismatch"
        );
    }
}

struct FailureDuringGuard {
    fixture: Fixture,
    failed: Cell<bool>,
    malformed: bool,
    cause: &'static str,
}
impl Memory for FailureDuringGuard {
    fn verify(&self) -> mailbox::Result<()> {
        if self.failed.get() {
            Err(self.cause.into())
        } else {
            Ok(())
        }
    }
    fn read(&self, address: u64, out: &mut [u8], module: bool) -> mailbox::Result<()> {
        if self.failed.get() {
            return Err(self.cause.into());
        }
        if !self.malformed && address == self.fixture.record_data {
            self.failed.set(true);
            return Err("fixture.short".into());
        }
        self.fixture.read(address, out, module)?;
        if self.malformed && address == self.fixture.record_data - 32 {
            out[16] = 0;
            self.failed.set(true);
        }
        Ok(())
    }
}
#[test]
fn original_failure_cannot_hide_guard_cancellation_deadline_or_identity_loss() {
    for cause in [
        "memory.runtime_cancelled",
        "memory.runtime_deadline",
        "memory.process_identity_changed",
    ] {
        for malformed in [false, true] {
            let mut fixture = Fixture::new();
            fixture.publication(&record(5, b"input".to_vec()), None);
            let failed = FailureDuringGuard {
                fixture,
                failed: Cell::new(false),
                malformed,
                cause,
            };
            let original = failed.fixture.reader();
            let reader = Reader {
                memory: &failed,
                root: original.root,
                layout: original.layout,
                module_hash: original.module_hash,
                release: original.release,
            };
            let error = reader.lookup(&Selector::Input).unwrap_err();
            assert!(error.contains(cause), "{error}");
            assert!(
                error.contains(if malformed {
                    "mailbox.record_length"
                } else {
                    "fixture.short"
                }),
                "{error}"
            );
            assert!(!error.contains("mailbox.path_changed"), "{error}");
        }
    }
}

struct CompleteThenGuardFailure {
    fixture: Fixture,
    complete_record_read: Cell<bool>,
    guard_changed: Cell<bool>,
    cause: &'static str,
}
impl Memory for CompleteThenGuardFailure {
    fn verify(&self) -> mailbox::Result<()> {
        if self.guard_changed.get() {
            Err(self.cause.into())
        } else {
            Ok(())
        }
    }
    fn read(&self, address: u64, out: &mut [u8], module: bool) -> mailbox::Result<()> {
        self.fixture.read(address, out, module)?;
        if address == self.fixture.record_data && out.len() > 80 {
            self.complete_record_read.set(true);
        }
        // The complete record succeeds. Only its subsequent path guard sees
        // both a changed parent publication and loss of cancel/identity validity.
        if self.complete_record_read.get()
            && address == self.fixture.namespace_value
            && out.len() == 10
        {
            out[9] ^= 1;
            self.guard_changed.set(true);
        }
        Ok(())
    }
}
#[test]
fn complete_record_guard_change_cannot_hide_cancellation_deadline_or_identity_loss() {
    for cause in [
        "memory.runtime_cancelled",
        "memory.runtime_deadline",
        "memory.process_identity_changed",
    ] {
        let mut fixture = Fixture::new();
        fixture.publication(&record(5, b"complete input record".to_vec()), None);
        let failure = CompleteThenGuardFailure {
            fixture,
            complete_record_read: Cell::new(false),
            guard_changed: Cell::new(false),
            cause,
        };
        let original = failure.fixture.reader();
        let reader = Reader {
            memory: &failure,
            root: original.root,
            layout: original.layout,
            module_hash: original.module_hash,
            release: original.release,
        };
        let accept_calls = Cell::new(0);
        let error = reader
            .lookup_with_accept(&Selector::Input, |_| {
                accept_calls.set(accept_calls.get() + 1);
                true
            })
            .unwrap_err();
        assert!(failure.complete_record_read.get());
        assert!(failure.guard_changed.get());
        assert_eq!(accept_calls.get(), 0);
        assert!(
            error.lines().any(|line| line == "mailbox.path_changed"),
            "{error}"
        );
        assert!(error.lines().any(|line| line == cause), "{error}");
    }
}

fn formal_selector(
    kind: lycheedev_live::go_mailbox_v1::Kind,
    record: &Record,
) -> lycheedev_live::go_mailbox_v1::Selector {
    use lycheedev_live::go_mailbox_v1::{Selector, parse_token};
    Selector {
        kind,
        nonce: parse_token(&record.nonce).unwrap(),
        runtime: parse_token(&record.runtime).unwrap(),
        ticket: parse_token(&record.ticket).unwrap(),
        body: None,
    }
}
#[test]
fn formal_body_selector_uses_exact_head_qualification_without_a_synthetic_receipt() {
    use lycheedev_live::go_mailbox_v1::{BodyQualification, Kind};
    let body = record(3, b"{\"ok\":true}".to_vec());
    let h = head(&body, body.payload.len() as u32);
    let mut fixture = Fixture::new();
    fixture.publication(&h, Some(&body));
    let mut selector = formal_selector(Kind::Body, &body);
    selector.body = Some(BodyQualification {
        length: body.payload.len() as u32,
        checksum: body.checksum,
    });
    assert_eq!(
        fixture
            .reader()
            .lookup_formal(&selector, None)
            .unwrap()
            .unwrap()
            .payload,
        body.payload
    );
    selector.body.as_mut().unwrap().checksum ^= 1;
    assert!(
        fixture
            .reader()
            .lookup_formal(&selector, None)
            .unwrap()
            .is_none()
    );
    selector.body = None;
    let before = fixture.calls.get();
    assert_eq!(
        fixture
            .reader()
            .lookup_formal(&selector, None)
            .unwrap_err()
            .cause,
        "mailbox.body_authorization"
    );
    assert_eq!(fixture.calls.get(), before);
}
#[test]
fn formal_selector_checks_runtime_before_record_bytes_and_accept_after_guards() {
    use lycheedev_live::go_mailbox_v1::Kind;
    let r = record(5, b"input".to_vec());
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    let mut selector = formal_selector(Kind::Input, &r);
    selector.runtime = [9; 16];
    let called = Cell::new(0);
    let mut accept = |_: &Record| {
        called.set(called.get() + 1);
        true
    };
    assert!(
        fixture
            .reader()
            .lookup_formal(&selector, Some(&mut accept))
            .unwrap()
            .is_none()
    );
    assert_eq!(called.get(), 0);
    selector.runtime = lycheedev_live::go_mailbox_v1::parse_token(&r.runtime).unwrap();
    fixture.mutate.set(true);
    assert_eq!(
        fixture
            .reader()
            .lookup_formal(&selector, Some(&mut accept))
            .unwrap_err()
            .cause,
        "mailbox.path_changed"
    );
    assert_eq!(called.get(), 0);
}
struct DiagnosticRead<'a> {
    fixture: &'a Fixture,
    failed: Cell<bool>,
    fail_guard: bool,
}
impl Memory for DiagnosticRead<'_> {
    fn verify(&self) -> mailbox::Result<()> {
        Ok(())
    }
    fn read(&self, at: u64, bytes: &mut [u8], module: bool) -> mailbox::Result<()> {
        self.fixture.read(at, bytes, module)
    }
    fn read_diagnostic(
        &self,
        at: u64,
        bytes: &mut [u8],
        module: bool,
    ) -> std::result::Result<(), mailbox::ReadError> {
        if at == self.fixture.record_data || self.failed.get() && self.fail_guard {
            let first = !self.failed.replace(true);
            return Err(mailbox::ReadError {
                cause: if first {
                    "fixture.partial_read"
                } else {
                    "fixture.guard_read"
                }
                .into(),
                returned_bytes: Some(if first { 7 } else { 0 }),
                reason: "short_read",
                io_attempted: Some(true),
                native_error_code: Some(299),
                region: None,
            });
        }
        self.fixture
            .read(at, bytes, module)
            .map_err(mailbox::ReadError::unknown)
    }
}
#[test]
fn first_read_failure_retains_original_logical_path_and_never_adds_a_diagnostic_scan() {
    use lycheedev_live::go_mailbox_v1::Kind;
    for fail_guard in [false, true] {
        let r = record(5, b"input".to_vec());
        let mut fixture = Fixture::new();
        fixture.publication(&r, None);
        let memory = DiagnosticRead {
            fixture: &fixture,
            failed: Cell::new(false),
            fail_guard,
        };
        let old = fixture.reader();
        let reader = Reader {
            memory: &memory,
            root: old.root,
            layout: old.layout,
            module_hash: old.module_hash,
            release: old.release,
        };
        let failure = reader
            .lookup_formal(&formal_selector(Kind::Input, &r), None)
            .unwrap_err();
        let detail = failure.first_read_failure.unwrap();
        assert_eq!(detail.path, "record.wire_header");
        assert_eq!(detail.requested_bytes, 80);
        assert_eq!(detail.returned_bytes, Some(7));
        assert_eq!(detail.native_error_code, Some(299));
        assert_eq!(
            detail.guard_recheck,
            if fail_guard { "read_failed" } else { "stable" }
        );
        assert!(failure.cause.contains("fixture.partial_read"));
        if fail_guard {
            assert!(failure.cause.contains("fixture.guard_read"));
        }
        let json = serde_json::to_string(&detail).unwrap();
        for forbidden in ["address", "nonce", "payload", &r.nonce, &r.runtime] {
            assert!(!json.contains(forbidden));
        }
        assert!(fixture.calls.get() < 512);
    }
}

#[test]
fn non_body_selector_ignores_unrelated_body_qualification_for_length_limit() {
    use lycheedev_live::go_mailbox_v1::{BodyQualification, Kind};
    for kind in [Kind::Identity, Kind::Input] {
        let r = record(kind as u8, b"nonempty publication".to_vec());
        let mut fixture = Fixture::new();
        fixture.publication(&r, None);
        let mut selector = formal_selector(kind, &r);
        selector.body = Some(BodyQualification {
            length: 0,
            checksum: 0,
        });
        assert_eq!(
            fixture
                .reader()
                .lookup_formal(&selector, None)
                .unwrap()
                .unwrap()
                .payload,
            r.payload
        );
    }
}

#[test]
fn non_body_qualification_cannot_expand_fixed_sixteen_kib_record_limit() {
    use lycheedev_live::go_mailbox_v1::{BodyQualification, Kind};
    let r = record(Kind::Input as u8, vec![b'x'; (16 << 10) + 1]);
    let mut fixture = Fixture::new();
    fixture.publication(&r, None);
    let mut selector = formal_selector(Kind::Input, &r);
    selector.body = Some(BodyQualification {
        length: 524288,
        checksum: 0,
    });
    assert_eq!(
        fixture
            .reader()
            .lookup_formal(&selector, None)
            .unwrap_err()
            .cause,
        "mailbox.record_length"
    );
}
