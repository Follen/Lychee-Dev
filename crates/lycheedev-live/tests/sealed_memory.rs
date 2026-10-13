//! Real self-process PE/module/heap admission. The deliberate test accessor is
//! finalized by the complete disk engine; no arbitrary opaque binding is made.
use lycheedev_core::storage;
use lycheedev_live::{
    binding::{BoundMemory, RootRule},
    locator::{self, Layout, Method, Operand, OutputType, Parameter, Pe, Source, TargetSection},
    mailbox::{self, Record, Selector},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

#[unsafe(no_mangle)]
static LD_FIXTURE_ROOT_SLOT: AtomicU64 = AtomicU64::new(0);
std::arch::global_asm!(
    ".text",
    ".globl ld_fixture_root_getter",
    "ld_fixture_root_getter:",
    "mov rax, qword ptr [rip + LD_FIXTURE_ROOT_SLOT]",
    "ret",
    ".ascii \"LycheeSealedSelfRoot20261008\"",
);
unsafe extern "C" {
    fn ld_fixture_root_getter() -> u64;
}
struct Heap {
    allocations: Vec<Box<[u8]>>,
    namespace_node: u64,
}
impl Heap {
    fn alloc(&mut self, b: Vec<u8>) -> u64 {
        let b = b.into_boxed_slice();
        let address = b.as_ptr() as u64;
        self.allocations.push(b);
        address
    }
    fn string(&mut self, bytes: &[u8]) -> u64 {
        let mut b = vec![0; 32];
        b[16] = 4;
        b[24..32].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        b.extend(bytes);
        self.alloc(b)
    }
    fn write(&mut self, address: u64, bytes: &[u8]) {
        let b = self
            .allocations
            .iter_mut()
            .find(|b| {
                address >= b.as_ptr() as u64
                    && address + bytes.len() as u64 <= b.as_ptr() as u64 + b.len() as u64
            })
            .unwrap();
        let offset = (address - b.as_ptr() as u64) as usize;
        b[offset..offset + bytes.len()].copy_from_slice(bytes)
    }
    fn table(&mut self, fields: Vec<(&str, u64, u8)>) -> (u64, BTreeMap<String, u64>) {
        let primary: BTreeSet<_> = fields
            .iter()
            .map(|(k, _, _)| (hash(k) & 255) as usize)
            .collect();
        let mut used = BTreeSet::new();
        let mut tail = BTreeMap::new();
        let mut locations = BTreeMap::new();
        let node_base = self.alloc(vec![0; 56 * 256]);
        for (key, value, tag) in fields {
            let p = (hash(key) & 255) as usize;
            let index = if used.contains(&p) {
                (0..256)
                    .find(|i| !primary.contains(i) && !used.contains(i))
                    .unwrap()
            } else {
                p
            };
            used.insert(index);
            let address = node_base + index as u64 * 56;
            let string = self.string(key.as_bytes());
            self.write(address, &value.to_le_bytes());
            self.write(address + 8, &[tag]);
            self.write(address + 24, &string.to_le_bytes());
            self.write(address + 32, &[4]);
            if let Some(previous) = tail.insert(p, index) {
                self.write(
                    node_base + previous as u64 * 56 + 48,
                    &address.to_le_bytes(),
                )
            }
            locations.insert(key.into(), address);
        }
        let mut b = vec![0; 72];
        b[16] = 5;
        b[19] = 8;
        b[40..48].copy_from_slice(&node_base.to_le_bytes());
        (self.alloc(b), locations)
    }
    fn publication(record: &Record) -> Self {
        let mut h = Self {
            allocations: vec![],
            namespace_node: 0,
        };
        let wire = h.string(&mailbox::encode(record).unwrap());
        let schema = h.string(b"lycheedev.mailbox.v2");
        let release = h.string(b"3.1.2");
        let runtime = h.string(record.runtime.as_bytes());
        let receipts = h.table(vec![(&record.nonce, wire, 4)]).0;
        let bodies = h.table(vec![]).0;
        let mb = h
            .table(vec![
                ("schema", schema, 4),
                ("release", release, 4),
                ("runtime", runtime, 4),
                ("input", wire, 4),
                ("identity", wire, 4),
                ("receipts", receipts, 5),
                ("bodies", bodies, 5),
            ])
            .0;
        let ns = h.table(vec![("Mailbox", mb, 5)]).0;
        let (globals, locations) = h.table(vec![("LycheeDevInternal", ns, 5)]);
        h.namespace_node = locations["LycheeDevInternal"];
        let mut state = vec![0; 0x90 + 24];
        state[16] = 8;
        state[0x90..0x98].copy_from_slice(&globals.to_le_bytes());
        state[0x98] = 5;
        let state = h.alloc(state);
        LD_FIXTURE_ROOT_SLOT.store(state, Ordering::Release);
        h
    }
}
impl Drop for Heap {
    fn drop(&mut self) {
        LD_FIXTURE_ROOT_SLOT.store(0, Ordering::Release)
    }
}
fn hash(key: &str) -> u32 {
    let b = key.as_bytes();
    let mut h = b.len() as u32;
    let step = (b.len() >> 5) + 1;
    let mut n = b.len();
    while n >= step {
        h ^= h
            .wrapping_shl(5)
            .wrapping_add(h >> 2)
            .wrapping_add(b[n - 1] as u32);
        n -= step;
    }
    h
}

#[test]
fn complete_disk_finalization_issues_actual_os_module_heap_reader() {
    let record = Record {
        address: 0,
        nonce: "1".repeat(32),
        runtime: "2".repeat(32),
        ticket: "3".repeat(32),
        kind: 5,
        state: 1,
        sequence: 7,
        checksum: 0,
        payload: br#"{"schema":"sealed.self.fixture"}"#.to_vec(),
    };
    let mut heap = Heap::publication(&record);
    let process = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let module = process.main_module().unwrap();
    // Independent compiled-symbol/accessor witness precedes method output.
    let root_value = (&LD_FIXTURE_ROOT_SLOT as *const AtomicU64 as u64) - module.base();
    assert_eq!(
        unsafe { ld_fixture_root_getter() },
        LD_FIXTURE_ROOT_SLOT.load(Ordering::Acquire)
    );
    let evidence = format!(
        "self-owned test PE accessor and symbol; sha={}; pid={}; creation={}; independentlyObservedRootRva={root_value}; admissionScope=self-fixture",
        module.hash(),
        process.identity().pid,
        process.identity().created
    );
    let marker = b"LycheeSealedSelfRoot20261008"
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let method = Method {
        id: "sealed-self-accessor-v1".into(),
        version: 1,
        output: OutputType::PointerSlot,
        section: ".text".into(),
        pattern: format!("48 8b 05 ?? ?? ?? ?? c3 {marker}"),
        operands: vec![Operand {
            offset: 3,
            instruction_end: 7,
            target: TargetSection::WritableData,
            output: true,
        }],
        source: Source {
            build: "self-test-executable".into(),
            sha256: module.hash().into(),
            value: root_value,
            evidence_sha256: storage::sha256(evidence.as_bytes()),
            evidence_kind: "independent-confirmation".into(),
        },
    };
    let (finalized, history) = locator::finalize(
        &Pe::parse(module.disk()).unwrap(),
        method,
        evidence.as_bytes(),
    )
    .unwrap();
    assert!(history.complete);
    assert_eq!(history.candidates.len(), 1);
    assert_eq!(history.result, Some(root_value));
    let proof_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/live-static");
    std::fs::create_dir_all(&proof_root).unwrap();
    storage::atomic_write(
        &proof_root.join("sealed-self-test.original.exe"),
        module.disk(),
    )
    .unwrap();
    storage::atomic_json(&proof_root.join("sealed-self-test-proof.json"),&serde_json::json!({
        "schema":"lycheedev.sealed-self-proof.v1","scope":"actual OS process/module/readlocked original; synthetic Lua heap and ABI; no game or WGC admission",
        "process":process.identity(),"moduleSha256":module.hash(),"moduleBase":module.base(),"moduleSize":module.size(),
        "independentEvidence":evidence,"independentRootRva":root_value,"history":history,"actualGameAbi":"not_run","wgc":"not_run"
    })).unwrap();
    let parameter = Parameter {
        name: "self-lua-root".into(),
        method_id: "sealed-self-accessor-v1".into(),
        expected_type: OutputType::PointerSlot,
        reader_pointer: "/root".into(),
    };
    let layout = Layout {
        id: "self-owned-lua-fixture-v1".into(),
        applicable_hashes: vec![module.hash().into()],
        state_globals: 0x90,
        tag_offset: 16,
        string_length: 24,
        string_data: 32,
        table_nodes: 40,
        table_log2: 19,
        node_bytes: 56,
    };
    let mut wrong_parameter = parameter.clone();
    wrong_parameter.expected_type = OutputType::ModuleRva;
    let wrong = RootRule::new(finalized.clone(), wrong_parameter, layout.clone());
    assert!(BoundMemory::open(std::process::id(), &wrong).is_err());
    let mut wrong_layout = layout.clone();
    wrong_layout.applicable_hashes = vec!["0".repeat(64)];
    let wrong = RootRule::new(finalized.clone(), parameter.clone(), wrong_layout);
    assert!(BoundMemory::open(std::process::id(), &wrong).is_err());
    let rule = RootRule::new(finalized, parameter, layout);
    let memory = BoundMemory::open(std::process::id(), &rule).unwrap();
    let observed = memory.lookup(&Selector::Input).unwrap().unwrap();
    assert_eq!(observed.payload, record.payload);
    assert_eq!(observed.sequence, 7);
    assert_ne!(observed.address, 0);
    // Same reader re-derives the current named path; a secret namespace is not
    // replaced by cached record bytes or a heap-wide scanner.
    heap.write(heap.namespace_node + 9, &[1]);
    assert!(memory.lookup(&Selector::Input).is_err());
    heap.write(heap.namespace_node + 9, &[0]);
    assert_eq!(
        memory.lookup(&Selector::Input).unwrap().unwrap().payload,
        record.payload
    );
}
