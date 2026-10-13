//! Synthetic module bytes behind the private seam. These tests cannot create a
//! public RootBinding or grant real process/module read authority.
use super::*;
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const TEXT: u64 = 0x2000;
const TABLE: usize = 0x188;
#[derive(Clone, Debug)]
struct Read {
    rva: u64,
    bytes: usize,
    kind: RuntimeReadKind,
}
enum Change {
    Bytes(u64, Vec<u8>),
    Identity,
    Cancel(Arc<AtomicBool>),
    Error(String),
    CancelAndError(Arc<AtomicBool>, String),
    Expire,
}
struct Trigger {
    rva: u64,
    occurrence: usize,
    change: Change,
}
struct Fixture {
    bytes: RefCell<Vec<u8>>,
    geometry: Cell<Geometry>,
    identity: Cell<u64>,
    text_bytes: usize,
    data: u64,
    reads: RefCell<Vec<Read>>,
    checks: Cell<usize>,
    failure_check: Cell<Option<usize>>,
    triggers: RefCell<Vec<Trigger>>,
}
impl Fixture {
    fn new(text_bytes: usize) -> Self {
        Self::with_data(text_bytes, 0x1000)
    }
    fn with_data(text_bytes: usize, data: u64) -> Self {
        let size = (TEXT + text_bytes as u64).max(data + 0x1000) + 0x1000;
        let mut bytes = vec![0; size as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        // Captured protected headers may change these non-locating fields. The
        // geometry/architecture come from the OS capability instead.
        bytes[0x84..0x86].copy_from_slice(&0x200u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[0xd0..0xd4].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        for (index, name, rva, length, flags) in [
            (0, b".text\0\0\0", TEXT, text_bytes as u64, READ | EXECUTE),
            (1, b".data\0\0\0", data, 0x1000, READ | WRITE),
        ] {
            let start = TABLE + index * 40;
            bytes[start..start + 8].copy_from_slice(name);
            bytes[start + 8..start + 12].copy_from_slice(&(length as u32).to_le_bytes());
            bytes[start + 12..start + 16].copy_from_slice(&(rva as u32).to_le_bytes());
            // Runtime VirtualAddress is unrelated to disk raw-data metadata.
            bytes[start + 16..start + 24].fill(0xfe);
            bytes[start + 36..start + 40].copy_from_slice(&flags.to_le_bytes());
        }
        Self {
            bytes: RefCell::new(bytes),
            geometry: Cell::new(Geometry {
                base: 0x140000000,
                size,
                architecture: 0x8664,
            }),
            identity: Cell::new(1),
            text_bytes,
            data,
            reads: RefCell::new(Vec::new()),
            checks: Cell::new(0),
            failure_check: Cell::new(None),
            triggers: RefCell::new(Vec::new()),
        }
    }
    fn write(&self, at: u64, bytes: &[u8]) {
        self.bytes.borrow_mut()[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
    }
    fn word(&self, at: usize, value: u32) {
        self.write(at as u64, &value.to_le_bytes());
    }
    fn anchor(&self, recipe: RecipeId, anchor: u64, root: u64) {
        let code: Vec<_> = recipe
            .text()
            .split_whitespace()
            .map(|s| {
                if s == "??" {
                    0
                } else {
                    u8::from_str_radix(s, 16).unwrap()
                }
            })
            .collect();
        self.write(anchor, &code);
        for (index, operand) in recipe.operands().iter().enumerate() {
            let target = if index == 0 {
                root
            } else if operand.call {
                TEXT + 0x20
            } else {
                self.data + 0x40
            };
            self.operand(recipe, anchor, index, target as i64);
        }
        self.write(root, &0x700000010000u64.to_le_bytes());
    }
    fn operand(&self, recipe: RecipeId, anchor: u64, index: usize, target: i64) {
        let operand = recipe.operands()[index];
        let displacement =
            i32::try_from(target - (anchor + (operand.offset + operand.length) as u64) as i64)
                .unwrap();
        self.write(
            anchor + (operand.offset + operand.displacement) as u64,
            &displacement.to_le_bytes(),
        );
    }
    fn trigger(&self, rva: u64, occurrence: usize, change: Change) {
        self.triggers.borrow_mut().push(Trigger {
            rva,
            occurrence,
            change,
        });
    }
}
impl Source for Fixture {
    type Identity = u64;
    fn identity(&self) -> u64 {
        self.identity.get()
    }
    fn geometry(&self) -> Geometry {
        self.geometry.get()
    }
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        budget.charge(RuntimeCost {
            os_call_units: 2,
            ..Default::default()
        })?;
        let check = self.checks.get() + 1;
        self.checks.set(check);
        if self.failure_check.get() == Some(check) {
            return Err("fixture.process_reused".into());
        }
        Ok(())
    }
    fn read(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        budget.charge(RuntimeCost {
            os_call_units: 2,
            read_calls: 1,
            requested_bytes: into.len() as u64,
            ..Default::default()
        })?;
        let end = rva.checked_add(into.len() as u64).ok_or("fixture.range")?;
        if end > self.geometry.get().size {
            return Err("fixture.range".into());
        }
        match kind {
            RuntimeReadKind::Code if rva < TEXT || end > TEXT + self.text_bytes as u64 => {
                return Err("fixture.code_mapping".into());
            }
            RuntimeReadKind::WritableData if rva < self.data || end > self.data + 0x1000 => {
                return Err("fixture.data_mapping".into());
            }
            _ => {}
        }
        self.reads.borrow_mut().push(Read {
            rva,
            bytes: into.len(),
            kind,
        });
        let occurrence = self
            .reads
            .borrow()
            .iter()
            .filter(|read| read.rva == rva)
            .count();
        into.copy_from_slice(&self.bytes.borrow()[rva as usize..end as usize]);
        let trigger = self
            .triggers
            .borrow()
            .iter()
            .position(|t| t.rva == rva && t.occurrence == occurrence);
        if let Some(index) = trigger {
            let change = self.triggers.borrow_mut().remove(index).change;
            match change {
                Change::Bytes(at, bytes) => self.write(at, &bytes),
                Change::Identity => self.identity.set(self.identity.get() + 1),
                Change::Cancel(cancel) => cancel.store(true, Ordering::Release),
                Change::Error(cause) => return Err(cause),
                Change::CancelAndError(cancel, cause) => {
                    cancel.store(true, Ordering::Release);
                    return Err(cause);
                }
                Change::Expire => {
                    let delay = budget.deadline().saturating_duration_since(Instant::now());
                    if delay > Duration::from_millis(500) {
                        return Err("fixture.expected_short_deadline".into());
                    }
                    std::thread::sleep(delay + Duration::from_millis(1));
                }
            }
        }
        Ok(())
    }
}
fn budget(limits: lycheedev_windows::process::RuntimeLimits) -> RuntimeBudget {
    RuntimeBudget::new(
        limits,
        Instant::now() + Duration::from_secs(10),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}
fn defaults() -> RuntimeBudget {
    budget(RuntimeCost::defaults())
}
fn ready(recipe: RecipeId) -> Fixture {
    let source = Fixture::new(8192);
    source.anchor(recipe, TEXT + 0x100, source.data + 0x10);
    source
}

#[test]
fn both_templates_derive_negative_rip_roots_under_aslr_and_protected_headers() {
    for recipe in [RecipeId::Retail, RecipeId::Forever] {
        for base in [0x140000000, 0x7ff600000000] {
            let source = ready(recipe);
            source.geometry.set(Geometry {
                base,
                ..source.geometry.get()
            });
            let original = source.bytes.borrow().clone();
            let mut budget = defaults();
            let proof = resolve(&source, &mut budget).unwrap();
            assert_eq!(
                (proof.root, proof.anchor, proof.recipe),
                (0x1010, TEXT + 0x100, recipe)
            );
            assert_eq!(proof.geometry.base, base);
            assert_eq!(proof.evidence.pattern_hits, 1);
            assert_eq!(proof.evidence.scanned_bytes, source.text_bytes as u64);
            assert!(proof.evidence.full_text_pass && proof.evidence.stable_guards);
            assert!(!proof.evidence.atomic);
            assert_eq!(source.bytes.borrow().as_slice(), original);
            assert_eq!(source.checks.get(), 2);
            assert_eq!(budget.usage().read_calls, proof.evidence.module_read_calls);
            assert!(
                !source
                    .reads
                    .borrow()
                    .iter()
                    .any(|r| r.rva == 0x84 || r.rva == 0xd0)
            );
            assert!(
                source
                    .reads
                    .borrow()
                    .iter()
                    .all(|r| r.rva + r.bytes as u64 <= source.geometry.get().size)
            );
        }
    }
}
#[test]
fn current_positive_displacements_move_the_root_without_any_known_rva() {
    for root in [0x5010, 0x5080] {
        let source = Fixture::with_data(8192, 0x5000);
        source.anchor(RecipeId::Retail, TEXT + 0x100, root);
        let proof = resolve(&source, &mut defaults()).unwrap();
        assert_eq!(proof.root, root);
        assert_eq!(proof.evidence.scanned_bytes, 8192);
    }
}
#[test]
fn instruction_crossing_a_one_mib_chunk_boundary_is_counted_once() {
    for recipe in [RecipeId::Retail, RecipeId::Forever] {
        let source = Fixture::new(CHUNK + 4096);
        let anchor = TEXT + CHUNK as u64 - 20;
        source.anchor(recipe, anchor, 0x1010);
        let proof = resolve(&source, &mut defaults()).unwrap();
        assert_eq!(proof.anchor, anchor);
        assert_eq!(proof.evidence.pattern_hits, 1);
        assert_eq!(proof.evidence.scanned_bytes, (CHUNK + 4096) as u64);
        let reads = source.reads.borrow();
        assert!(reads.iter().all(|r| r.bytes <= CHUNK));
        assert!(
            reads
                .iter()
                .any(|r| r.rva == TEXT + CHUNK as u64 && r.bytes == 4096)
        );
    }
}
#[test]
fn zero_raw_hits_retains_complete_search_coverage_without_a_binding() {
    let source = Fixture::new(8192);
    let failure = resolve(&source, &mut defaults()).err().unwrap();
    assert_eq!(failure.cause, "root.anchor_missing");
    assert_eq!(failure.evidence.pattern_hits, 0);
    assert_eq!(failure.evidence.scanned_bytes, 8192);
    assert!(failure.evidence.full_text_pass);
    assert!(!failure.evidence.stable_guards);
}
#[test]
fn all_raw_hits_compete_including_invalid_second_and_other_template() {
    for (second, invalid) in [
        (RecipeId::Retail, false),
        (RecipeId::Retail, true),
        (RecipeId::Forever, false),
        (RecipeId::Forever, true),
    ] {
        let source = ready(RecipeId::Retail);
        source.anchor(second, TEXT + 0x300, 0x1020);
        if invalid {
            source.operand(second, TEXT + 0x300, 0, -1);
        }
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert_eq!(failure.cause, "root.ambiguous_anchor");
        assert_eq!(failure.evidence.pattern_hits, 2);
        assert!(!failure.evidence.stable_guards);
        assert!(
            source
                .reads
                .borrow()
                .iter()
                .all(|r| r.kind != RuntimeReadKind::WritableData)
        );
    }
}
#[test]
fn a_late_invalid_hit_prevents_known_first_anchor_shortcuts() {
    let source = Fixture::new(CHUNK + 4096);
    source.anchor(RecipeId::Retail, TEXT + 0x100, 0x1010);
    let second = TEXT + CHUNK as u64 + 300;
    source.anchor(RecipeId::Forever, second, 0x1020);
    source.operand(RecipeId::Forever, second, 0, -1);
    let failure = resolve(&source, &mut defaults()).err().unwrap();
    assert_eq!(failure.cause, "root.ambiguous_anchor");
    assert_eq!(failure.evidence.scanned_bytes, (CHUNK + 4096) as u64);
}
#[test]
fn each_rip_operand_uses_its_actual_one_four_or_eight_byte_width() {
    for recipe in [RecipeId::Retail, RecipeId::Forever] {
        let source = ready(recipe);
        let anchor = TEXT + 0x100;
        for (index, operand) in recipe.operands().iter().enumerate() {
            let target = if operand.call {
                TEXT + source.text_bytes as u64 - 1
            } else {
                source.data + 0x1000 - operand.width
            };
            source.operand(recipe, anchor, index, target as i64);
        }
        assert_eq!(
            resolve(&source, &mut defaults()).unwrap().root,
            source.data + 0x1000 - 8
        );
    }
    for (operand, target) in [(1, 0x1ffc), (2, 0x1ffd)] {
        let source = ready(RecipeId::Retail);
        source.operand(RecipeId::Retail, TEXT + 0x100, operand, target);
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert_eq!(failure.cause, "root.rip_target_section");
        assert_eq!(failure.evidence.pattern_hits, 1);
    }
}
#[test]
fn signed_rip_overflow_module_range_alignment_and_target_permissions_are_independent() {
    for (index, target, cause) in [
        (0, -1, "root.rip_overflow"),
        (0, 0x6000, "root.rip_target_range"),
        (0, 0x1011, "root.alignment"),
        (0, TEXT as i64, "root.rip_target_section"),
        (3, 0x1010, "root.rip_target_section"),
    ] {
        let source = ready(RecipeId::Retail);
        source.operand(RecipeId::Retail, TEXT + 0x100, index, target);
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert_eq!(failure.cause, cause);
        assert!(failure.evidence.full_text_pass);
    }
}
#[test]
fn forever_repeated_getter_calls_must_have_one_target() {
    let source = ready(RecipeId::Forever);
    source.operand(RecipeId::Forever, TEXT + 0x100, 4, (TEXT + 0x30) as i64);
    let failure = resolve(&source, &mut defaults()).err().unwrap();
    assert_eq!(failure.cause, "root.call_target_mismatch");
    assert_eq!(failure.evidence.pattern_hits, 1);
}
#[test]
fn os_geometry_is_required_even_when_runtime_headers_look_plausible() {
    for geometry in [
        Geometry {
            base: u64::MAX - 10,
            size: 100,
            architecture: 0x8664,
        },
        Geometry {
            base: 0x140000000,
            size: 0,
            architecture: 0x8664,
        },
        Geometry {
            base: 0x140000000,
            size: 0x5000,
            architecture: 0x14c,
        },
    ] {
        let source = ready(RecipeId::Retail);
        source.geometry.set(geometry);
        assert_eq!(
            resolve(&source, &mut defaults()).err().unwrap().cause,
            "root.module_identity"
        );
        assert!(source.reads.borrow().is_empty());
    }
}
#[test]
fn malformed_locating_headers_and_sections_never_begin_a_text_scan() {
    for mode in [
        "nt_far",
        "count",
        "magic",
        "overlap",
        "text_missing",
        "text_duplicate",
        "text_permissions",
        "section_range",
    ] {
        let source = ready(RecipeId::Retail);
        match mode {
            "nt_far" => source.word(0x3c, 0x10000000),
            "count" => source.write(0x86, &97u16.to_le_bytes()),
            "magic" => source.write(0x98, &0x10bu16.to_le_bytes()),
            "overlap" => source.word(TABLE + 40 + 12, TEXT as u32),
            "text_missing" => source.write(TABLE as u64, b".other\0\0"),
            "text_duplicate" => source.write((TABLE + 40) as u64, b".text\0\0\0"),
            "text_permissions" => source.word(TABLE + 36, READ | WRITE),
            _ => source.word(TABLE + 8, u32::MAX),
        }
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert!(failure.cause.starts_with("root."), "{mode}: {failure:?}");
        assert_eq!(failure.evidence.scanned_bytes, 0);
        assert!(
            source
                .reads
                .borrow()
                .iter()
                .all(|r| r.kind == RuntimeReadKind::Metadata)
        );
    }
}
#[test]
fn anchor_root_header_and_process_races_do_not_issue_a_binding() {
    for mode in ["anchor", "root", "header", "identity", "process"] {
        let source = ready(RecipeId::Retail);
        let cause = match mode {
            "anchor" => {
                source.trigger(TEXT, 1, Change::Bytes(TEXT + 0x100, vec![0x49]));
                "root.anchor_changed"
            }
            "root" => {
                source.trigger(
                    0x1010,
                    1,
                    Change::Bytes(0x1010, 0x700000020000u64.to_le_bytes().to_vec()),
                );
                "root.slot_changed"
            }
            "header" => {
                source.trigger(TEXT, 1, Change::Bytes((TABLE + 20) as u64, vec![0x77]));
                "root.header_changed"
            }
            "identity" => {
                source.trigger(TEXT, 1, Change::Identity);
                "root.module_changed"
            }
            _ => {
                source.failure_check.set(Some(2));
                "fixture.process_reused"
            }
        };
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert_eq!(failure.cause, cause, "{mode}");
        assert_eq!(failure.evidence.pattern_hits, 1);
    }
}
#[test]
fn source_failures_and_incomplete_code_reads_remain_fatal_without_salvage() {
    for cause in [
        "fixture.short_read",
        "fixture.guard_page",
        "fixture.remapped",
        "fixture.os_denied:5",
    ] {
        let source = ready(RecipeId::Retail);
        source.trigger(TEXT, 1, Change::Error(cause.into()));
        let failure = resolve(&source, &mut defaults()).err().unwrap();
        assert_eq!(failure.cause, cause);
        assert_eq!(failure.evidence.pattern_hits, 0);
        assert!(!failure.evidence.full_text_pass);
        assert_eq!(
            source
                .reads
                .borrow()
                .iter()
                .filter(|r| r.rva == TEXT)
                .count(),
            1
        );
        assert_eq!(source.checks.get(), 1);
    }
}
#[test]
fn revalidation_does_not_rescan_or_cache_heap_addresses() {
    let source = ready(RecipeId::Retail);
    let mut budget = defaults();
    let mut proof = resolve(&source, &mut budget).unwrap();
    let before = source.reads.borrow().len();
    // The root slot may hold a different state by a later observation. This
    // binding holds only its RVA; the future Lua reader must validate that state
    // and runtime afresh. No prior heap pointer is followed here.
    source.write(0x1010, &0x700000030000u64.to_le_bytes());
    let checked = revalidate(&mut proof, &source, &mut budget).unwrap();
    assert!(checked.stable_guards && !checked.full_text_pass);
    assert_eq!(checked.scanned_bytes, 0);
    assert!(
        source.reads.borrow()[before..]
            .iter()
            .all(|r| r.bytes <= 80)
    );
    assert_eq!(
        source.reads.borrow()[before..]
            .iter()
            .filter(|r| r.rva == 0x1010)
            .count(),
        2
    );
}
#[test]
fn failed_revalidation_is_poisoned_and_fresh_resolution_keeps_original_budget() {
    let source = ready(RecipeId::Retail);
    let mut budget = defaults();
    let mut proof = resolve(&source, &mut budget).unwrap();
    source.write(TEXT + 0x100, &[0x49]);
    assert_eq!(
        revalidate(&mut proof, &source, &mut budget)
            .unwrap_err()
            .cause,
        "root.anchor_changed"
    );
    let used = budget.usage();
    let reads = source.reads.borrow().len();
    source.write(TEXT + 0x100, &[0x48]);
    assert_eq!(
        revalidate(&mut proof, &source, &mut budget)
            .unwrap_err()
            .cause,
        "root.anchor_changed"
    );
    assert_eq!(source.reads.borrow().len(), reads);
    assert_eq!(budget.usage(), used);
    let fresh = resolve(&source, &mut budget).unwrap();
    assert!(fresh.evidence.full_text_pass);
    assert!(budget.usage().read_calls > used.read_calls);
    assert_eq!(fresh.request, proof.request);
}
#[test]
fn a_different_module_instance_or_new_budget_cannot_reuse_a_binding() {
    for changed_budget in [false, true] {
        let source = ready(RecipeId::Retail);
        let mut budget = defaults();
        let mut proof = resolve(&source, &mut budget).unwrap();
        let reads = source.reads.borrow().len();
        if changed_budget {
            let mut replacement = defaults();
            assert_eq!(
                revalidate(&mut proof, &source, &mut replacement)
                    .unwrap_err()
                    .cause,
                "root.budget_replaced"
            );
            assert_eq!(replacement.usage(), RuntimeCost::default());
        } else {
            source.identity.set(2);
            assert_eq!(
                revalidate(&mut proof, &source, &mut budget)
                    .unwrap_err()
                    .cause,
                "root.module_changed"
            );
        }
        assert_eq!(source.reads.borrow().len(), reads);
    }
}
#[test]
fn global_read_allocation_and_cpu_budgets_preserve_partial_evidence() {
    for (limits, cause) in [
        (
            RuntimeCost {
                read_calls: 7,
                ..RuntimeCost::defaults()
            },
            "memory.runtime_budget:reads",
        ),
        (
            RuntimeCost {
                requested_bytes: 15,
                ..RuntimeCost::defaults()
            },
            "memory.runtime_budget:bytes",
        ),
        (
            RuntimeCost {
                allocation_bytes: 1,
                ..RuntimeCost::defaults()
            },
            "memory.runtime_budget:allocations",
        ),
        (
            RuntimeCost {
                work_units: 0,
                ..RuntimeCost::defaults()
            },
            "memory.runtime_budget:work",
        ),
    ] {
        let source = ready(RecipeId::Retail);
        let mut budget = budget(limits);
        let failure = resolve(&source, &mut budget).err().unwrap();
        assert_eq!(failure.cause, cause);
        assert!(!failure.evidence.full_text_pass);
        assert!(budget.usage().read_calls <= limits.read_calls);
        assert!(budget.usage().requested_bytes <= limits.requested_bytes);
        assert!(budget.usage().allocation_bytes <= limits.allocation_bytes);
        assert!(budget.usage().work_units <= limits.work_units);
    }
}
#[test]
fn cancelled_expired_and_during_read_control_never_produces_a_binding() {
    for (cancelled, expired) in [(true, false), (false, true)] {
        let source = ready(RecipeId::Retail);
        let deadline = if expired {
            Instant::now()
        } else {
            Instant::now() + Duration::from_secs(10)
        };
        let mut budget = RuntimeBudget::new(
            RuntimeCost::defaults(),
            deadline,
            Arc::new(AtomicBool::new(cancelled)),
        )
        .unwrap();
        let failure = resolve(&source, &mut budget).err().unwrap();
        assert_eq!(
            failure.cause,
            if cancelled {
                "memory.runtime_cancelled"
            } else {
                "memory.runtime_deadline"
            }
        );
        assert!(source.reads.borrow().is_empty());
        assert_eq!(budget.usage(), RuntimeCost::default());
    }
    let source = ready(RecipeId::Retail);
    let cancel = Arc::new(AtomicBool::new(false));
    source.trigger(TEXT, 1, Change::Cancel(cancel.clone()));
    let mut budget = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(10),
        cancel,
    )
    .unwrap();
    assert_eq!(
        resolve(&source, &mut budget).err().unwrap().cause,
        "memory.runtime_cancelled"
    );
    assert_eq!(
        source
            .reads
            .borrow()
            .iter()
            .filter(|r| r.rva == TEXT)
            .count(),
        1
    );
}
#[test]
fn a_real_source_fault_cannot_be_reclassified_by_simultaneous_cancellation() {
    let source = ready(RecipeId::Retail);
    let cancel = Arc::new(AtomicBool::new(false));
    source.trigger(
        TEXT,
        1,
        Change::CancelAndError(cancel.clone(), "fixture.os_denied:5".into()),
    );
    let mut budget = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(10),
        cancel,
    )
    .unwrap();
    let failure = resolve(&source, &mut budget).err().unwrap();
    assert_eq!(failure.cause, "fixture.os_denied:5");
    assert!(!failure.evidence.stable_guards);
}
#[test]
fn deadline_overrun_on_successful_read_is_preserved_without_retry() {
    let source = ready(RecipeId::Retail);
    source.trigger(TEXT, 1, Change::Expire);
    let mut budget = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_millis(100),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let failure = resolve(&source, &mut budget).err().unwrap();
    assert_eq!(failure.cause, "memory.runtime_deadline");
    assert_eq!(
        source
            .reads
            .borrow()
            .iter()
            .filter(|r| r.rva == TEXT)
            .count(),
        1
    );
    assert!(!failure.evidence.full_text_pass);
}
