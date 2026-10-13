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
    fn publication(&mut self, record: &Record, body: Option<&Record>) {
        let wire = mailbox::encode(record).unwrap();
        let str = self.string(&wire);
        self.record_data = str + 32;
        let schema = self.string(b"lycheedev.mailbox.v2");
        let release = self.string(b"3.1.2");
        let rt = self.string(record.runtime.as_bytes());
        let receipts = self.table(vec![(&record.nonce, str, 4)]).0;
        let bodies = if let Some(body) = body {
            let wire = mailbox::encode(body).unwrap();
            let str = self.string(&wire);
            self.table(vec![(&body.ticket, str, 4)]).0
        } else {
            self.table(vec![]).0
        };
        let mb = self
            .table(vec![
                ("schema", schema, 4),
                ("release", release, 4),
                ("runtime", rt, 4),
                ("input", str, 4),
                ("identity", str, 4),
                ("receipts", receipts, 5),
                ("bodies", bodies, 5),
            ])
            .0;
        let ns = self.table(vec![("Mailbox", mb, 5)]).0;
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
        state: 1,
        sequence: 1,
        checksum: mailbox::checksum(&payload),
        payload,
    }
}
#[test]
fn complete_named_lookup_no_heap_scan() {
    let r = record(4, b"input".to_vec());
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
    let r = record(4, b"input".to_vec());
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
    let r = record(4, b"input".to_vec());
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
    let head=record(2,serde_json::to_vec(&serde_json::json!({"state":"reported","ticket":body.ticket,"nonce":body.nonce,"reportBytes":body.payload.len(),"reportChecksum":body.checksum})).unwrap());
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
    let head=record(2,serde_json::to_vec(&serde_json::json!({"state":"reported","ticket":body.ticket,"nonce":body.nonce,"reportBytes":5,"reportChecksum":body.checksum})).unwrap());
    let mut f = Fixture::new();
    f.publication(&head, Some(&body));
    assert!(f.reader().lookup(&Selector::Body { head }).is_err());
}
#[test]
fn corrupt_state_pointer_fails_without_overflow() {
    let r = record(4, b"input".to_vec());
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
    let r = record(4, b"input".to_vec());
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
        fixture.publication(&record(4, b"input".to_vec()), None);
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
    limits.allocation_bytes = 8;
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
    assert_eq!(
        f.reader().lookup(&Selector::Input).unwrap_err(),
        "mailbox.record_kind"
    );
}
