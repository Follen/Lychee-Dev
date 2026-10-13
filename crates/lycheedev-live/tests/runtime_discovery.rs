//! Deterministic discovery faults using an immutable-record test format.
//! The fixture MAC below is test-only; no production protocol is defined here.
use lycheedev_live::runtime_discovery::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const HEADER: usize = 32;
const MAGIC: &[u8] = b"DISCOV01";
fn scope() -> Scope {
    Scope {
        pid: 123,
        created: 456,
        window: 789,
        installation: [4; 32],
        client_channel: [5; 32],
        protocol: 1,
        runtime: Some([6; 16]),
    }
}
fn format() -> Format {
    Format {
        marker: MAGIC.to_vec(),
        marker_offset: 0,
        header_bytes: HEADER,
    }
}
fn tag(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update([0xa9; 32]);
    digest.update(bytes);
    digest.finalize().into()
}
fn record(task: u8, payload: &[u8], body: bool) -> Vec<u8> {
    let mut out = vec![0; HEADER];
    out[..8].copy_from_slice(MAGIC);
    out[8] = 7; // Current observation nonce.
    out[9] = 6; // Runtime.
    out[10] = task;
    out[11] = u8::from(body);
    out[12..16].copy_from_slice(&((HEADER + payload.len() + 32) as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&tag(&out));
    out
}
#[derive(Default)]
struct Validator {
    bodies: BTreeMap<u8, ([u8; 32], usize)>,
    reject_payload: Option<Vec<u8>>,
    complete_calls: usize,
    deadlines: Vec<Instant>,
}
impl Verifier for Validator {
    fn admit_header(
        &mut self,
        _: &Scope,
        head: &[u8],
        deadline: Instant,
    ) -> std::result::Result<Option<Admission>, String> {
        self.deadlines.push(deadline);
        if head[..8] != *MAGIC || head[8] != 7 || head[9] != 6 {
            return Ok(None);
        }
        let total_bytes = u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
        if total_bytes < HEADER + 32 {
            return Ok(None);
        }
        Ok(if head[11] == 0 {
            Some(Admission::Small { total_bytes })
        } else if self
            .bodies
            .get(&head[10])
            .is_some_and(|(_, length)| *length == total_bytes)
        {
            Some(Admission::AuthorizedBody { total_bytes })
        } else {
            None
        })
    }
    fn verify_record(
        &mut self,
        _: &Scope,
        _: Admission,
        bytes: &[u8],
        deadline: Instant,
    ) -> std::result::Result<Option<Verified>, String> {
        self.deadlines.push(deadline);
        self.complete_calls += 1;
        let end = bytes.len() - 32;
        if tag(&bytes[..end]) != bytes[end..] {
            return Ok(None);
        }
        if bytes[11] == 1
            && self.bodies.get(&bytes[10]) != Some(&(Sha256::digest(bytes).into(), bytes.len()))
        {
            return Ok(None);
        }
        let matches_query = self.reject_payload.as_deref() != Some(&bytes[HEADER..end]);
        Ok(Some(Verified {
            identity: [bytes[10]; 32],
            matches_query,
        }))
    }
}

struct Memory {
    bytes: Vec<u8>,
    regions: Vec<Region>,
    reads: Vec<(u64, usize)>,
    checks: usize,
    replace_on_full: Option<(usize, Vec<u8>)>,
    fail_identity_after: Option<usize>,
    short_once: bool,
    zero_once: bool,
    fault_read: Option<(usize, SourceError)>,
    expire_deadline_once: bool,
    hole: Option<Span>,
    invalid_query: bool,
    cancelled: Option<std::sync::Arc<AtomicBool>>,
    deadlines: Vec<Instant>,
}
impl Memory {
    fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
            regions: vec![Region {
                span: Span {
                    start: 0,
                    end: size as u64,
                },
                private: true,
                committed: true,
                readable: true,
            }],
            reads: Vec::new(),
            checks: 0,
            replace_on_full: None,
            fail_identity_after: None,
            short_once: false,
            zero_once: false,
            fault_read: None,
            expire_deadline_once: false,
            hole: None,
            invalid_query: false,
            cancelled: None,
            deadlines: Vec::new(),
        }
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.bytes[at..at + bytes.len()].copy_from_slice(bytes);
    }
}
impl Source for Memory {
    fn verify_identity(
        &mut self,
        _: &Scope,
        deadline: Instant,
    ) -> std::result::Result<(), SourceError> {
        self.deadlines.push(deadline);
        self.checks += 1;
        if self.fail_identity_after.is_some_and(|at| self.checks >= at) {
            return Err(SourceError::Fatal("identity_reused".into()));
        }
        Ok(())
    }
    fn query_region(
        &mut self,
        at: u64,
        deadline: Instant,
    ) -> std::result::Result<Option<Region>, SourceError> {
        self.deadlines.push(deadline);
        if self.invalid_query {
            return Ok(Some(Region {
                span: Span { start: 0, end: 0 },
                private: true,
                committed: true,
                readable: true,
            }));
        }
        Ok(self
            .regions
            .iter()
            .find(|r| r.span.start <= at && at < r.span.end)
            .copied())
    }
    fn read_private(
        &mut self,
        at: u64,
        into: &mut [u8],
        deadline: Instant,
    ) -> std::result::Result<usize, SourceError> {
        self.deadlines.push(deadline);
        self.reads.push((at, into.len()));
        let expired = std::mem::take(&mut self.expire_deadline_once);
        if expired {
            let delay = deadline.saturating_duration_since(Instant::now());
            // A wrong global deadline must fail promptly instead of sleeping 10s.
            if delay > Duration::from_millis(500) {
                return Err(SourceError::Fatal("fixture_expected_short_deadline".into()));
            }
            std::thread::sleep(delay + Duration::from_millis(1));
        }
        if let Some((call, cause)) = &self.fault_read {
            if self.reads.len() == *call {
                return Err(cause.clone());
            }
        }
        if let Some(cancel) = &self.cancelled {
            cancel.store(true, Ordering::Release);
        }
        if expired {
            return Err(SourceError::Deadline);
        }
        if self.zero_once {
            self.zero_once = false;
            return Ok(0);
        }
        let end = at + into.len() as u64;
        let mut cursor = at;
        while cursor < end {
            let Some(r) = self
                .regions
                .iter()
                .find(|r| r.span.start <= cursor && cursor < r.span.end)
            else {
                return Ok(0);
            };
            if !(r.private && r.committed && r.readable) {
                return Ok(0);
            }
            cursor = r.span.end.min(end);
        }
        if let Some((address, replacement)) = self.replace_on_full.as_ref() {
            if at as usize == *address && into.len() > HEADER {
                let (address, replacement) = (*address, replacement.clone());
                self.put(address, &replacement);
                self.replace_on_full = None;
            }
        }
        let mut n = into.len();
        if self.short_once {
            n /= 2;
            self.short_once = false;
        }
        if let Some(hole) = self.hole {
            if at < hole.end && end > hole.start {
                n = n.min(hole.start.saturating_sub(at) as usize);
            }
        }
        into[..n].copy_from_slice(&self.bytes[at as usize..at as usize + n]);
        Ok(n)
    }
}
fn run(
    memory: &mut Memory,
    validator: &mut Validator,
    mode: Mode,
    hints: Option<&Hints>,
    limits: Limits,
) -> Discovery {
    let cancel = AtomicBool::new(false);
    Request::new(
        memory,
        validator,
        scope(),
        format(),
        limits,
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap()
    .discover(mode, hints)
}

#[test]
fn cold_scan_crosses_chunks_and_adjacent_private_regions() {
    let mut memory = Memory::new(1024);
    memory.regions = vec![
        Region {
            span: Span { start: 0, end: 128 },
            private: true,
            committed: true,
            readable: true,
        },
        Region {
            span: Span {
                start: 128,
                end: 1024,
            },
            private: true,
            committed: true,
            readable: true,
        },
    ];
    let bytes = record(1, b"current", false);
    memory.put(125, &bytes);
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 128,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, None);
    assert_eq!(out.records[0].bytes, bytes);
    assert!(out.coverage.complete);
    assert_eq!(out.coverage.searched_bytes, 1024);
    assert_eq!(
        out.usage.requested_bytes,
        memory.reads.iter().map(|(_, n)| *n as u64).sum::<u64>()
    );
}

#[test]
fn hints_reread_and_full_audit_ignores_hint_shortcut() {
    let mut memory = Memory::new(512);
    memory.put(100, &record(1, b"valid", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert_eq!(out.records.len(), 1);
    assert!(out.coverage.hint_hit && out.coverage.stopped_early && !out.coverage.complete);
    assert_eq!(memory.reads.len(), 2);
    assert_eq!(memory.checks, 2);
    let audit = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        Some(&hints),
        Limits::default(),
    );
    assert!(audit.coverage.complete && !audit.coverage.hint_hit);
}

#[test]
fn old_nonce_and_tamper_with_well_formed_header_are_rejected() {
    let mut memory = Memory::new(1024);
    let mut stale = record(1, b"old", false);
    stale[8] = 2;
    let mut tamper = record(2, b"tamper", false);
    tamper[HEADER] ^= 1;
    memory.put(40, &stale);
    memory.put(200, &tamper);
    memory.put(400, &record(3, b"good", false));
    let mut validator = Validator::default();
    let out = run(
        &mut memory,
        &mut validator,
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(out.records.len(), 1);
    assert_eq!(out.records[0].identity, [3; 32]);
    assert_eq!(validator.complete_calls, 2);
    assert!(out.coverage.complete);
}

#[test]
fn predicate_does_not_hide_observed_authenticated_conflict() {
    let mut memory = Memory::new(1024);
    memory.put(40, &record(1, b"prepared", false));
    memory.put(200, &record(1, b"reported", false));
    let mut validator = Validator {
        reject_payload: Some(b"prepared".to_vec()),
        ..Validator::default()
    };
    let out = run(
        &mut memory,
        &mut validator,
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(out.stop, Some(Stop::Conflict([1; 32])));
    assert!(out.records.is_empty() && !out.coverage.complete);
}

#[test]
fn copies_deduplicate_but_early_hit_never_claims_audit() {
    let mut memory = Memory::new(1024);
    let bytes = record(1, b"same", false);
    memory.put(40, &bytes);
    memory.put(200, &bytes);
    let audit = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(audit.records.len(), 1);
    assert!(audit.coverage.complete);
    let first = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        None,
        Limits::default(),
    );
    assert_eq!(first.records.len(), 1);
    assert!(first.coverage.stopped_early && !first.coverage.complete);
}

#[test]
fn body_requires_terminal_authorization_and_is_excluded_from_nearby() {
    let mut memory = Memory::new(2048);
    let bytes = record(1, &[42; 1024], true);
    memory.put(100, &bytes);
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let untrusted = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert!(untrusted.records.is_empty());
    assert!(!memory.reads.contains(&(100, bytes.len())));
    let mut validator = Validator::default();
    validator
        .bodies
        .insert(1, (Sha256::digest(&bytes).into(), bytes.len()));
    let near = run(
        &mut memory,
        &mut validator,
        Mode::Nearby,
        Some(&hints),
        Limits::default(),
    );
    assert!(near.records.is_empty() && near.coverage.local_only && !near.coverage.complete);
    let accepted = run(
        &mut memory,
        &mut validator,
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert_eq!(accepted.records[0].bytes, bytes);
}

#[test]
fn torn_header_and_changed_process_fail_closed() {
    let mut memory = Memory::new(512);
    memory.put(100, &record(1, b"old", false));
    memory.replace_on_full = Some((100, record(2, b"new", false)));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    // Only two reads available: the torn hint cannot release a record or reset budget.
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits {
            read_calls: 2,
            ..Limits::default()
        },
    );
    assert!(out.records.is_empty());
    assert_eq!(out.stop, Some(Stop::Budget("read_calls")));
    memory.fail_identity_after = Some(memory.checks + 2);
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert_eq!(out.stop, Some(Stop::Source("identity_reused".into())));
    assert!(out.records.is_empty());
}

#[test]
fn one_page_salvage_never_stitches_across_a_hole() {
    let mut memory = Memory::new(8192);
    memory.short_once = true;
    memory.put(4093, &record(1, b"boundary", false));
    let recovered = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 8192,
            ..Limits::default()
        },
    );
    assert!(recovered.coverage.complete);
    assert_eq!(recovered.records.len(), 1);
    memory.hole = Some(Span {
        start: 4096,
        end: 4097,
    });
    let failed = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 8192,
            ..Limits::default()
        },
    );
    assert!(failed.records.is_empty() && !failed.coverage.complete);
    assert!(!failed.coverage.gaps.is_empty());
    assert!(failed.coverage.unsearched_bytes > 0);
}

#[test]
fn ineligible_mappings_are_not_read_or_crossed() {
    let mut memory = Memory::new(512);
    memory.regions = vec![
        Region {
            span: Span { start: 0, end: 256 },
            private: false,
            committed: true,
            readable: true,
        },
        Region {
            span: Span {
                start: 256,
                end: 512,
            },
            private: true,
            committed: true,
            readable: true,
        },
    ];
    memory.put(100, &record(1, b"image", false));
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert!(out.records.is_empty() && out.coverage.complete);
    assert_eq!(out.coverage.skipped_regions, 1);
    assert!(memory.reads.iter().all(|(at, _)| *at >= 256));
}

#[test]
fn byte_calls_candidates_and_retention_are_hard_caps() {
    let cases = [
        (
            Limits {
                read_bytes: 1024,
                ..Limits::default()
            },
            "read_bytes",
        ),
        (
            Limits {
                read_calls: 1,
                ..Limits::default()
            },
            "read_calls",
        ),
        (
            Limits {
                candidates: 1,
                ..Limits::default()
            },
            "candidates",
        ),
        (
            Limits {
                records: 1,
                ..Limits::default()
            },
            "records",
        ),
        (
            Limits {
                retained_bytes: 1,
                ..Limits::default()
            },
            "retained_bytes",
        ),
    ];
    for (limits, reason) in cases {
        let mut memory = Memory::new(1024);
        memory.put(40, &record(1, b"one", false));
        memory.put(200, &record(2, b"two", false));
        let out = run(
            &mut memory,
            &mut Validator::default(),
            Mode::Audit,
            None,
            limits.clone(),
        );
        assert_eq!(out.stop, Some(Stop::Budget(reason)), "{reason}");
        assert!(out.records.is_empty());
        assert!(
            out.usage.requested_bytes <= limits.read_bytes
                && out.usage.read_calls <= limits.read_calls
        );
        assert!(out.usage.candidates <= limits.candidates);
    }
}

#[test]
fn invalid_regions_and_region_budget_do_not_claim_absence() {
    let mut memory = Memory::new(512);
    memory.invalid_query = true;
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(out.stop, Some(Stop::InvalidRegion));
    memory.invalid_query = false;
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            regions: 1,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, Some(Stop::Budget("regions")));
    assert!(!out.coverage.enumeration_complete && !out.coverage.complete);
}

#[test]
fn nearby_miss_full_fallback_share_budget_and_pass_limit() {
    let mut memory = Memory::new(2 << 20);
    memory.put((1 << 20) + 100, &record(1, b"moved_far", false));
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits {
            passes: 2,
            ..Limits::default()
        },
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert!(near.records.is_empty() && !near.coverage.complete);
    let full = request.discover(Mode::First, None);
    assert_eq!(full.records.len(), 1);
    assert!(full.usage.requested_bytes > near.usage.requested_bytes);
    assert_eq!(full.usage.passes, 2);
    assert_eq!(
        request.discover(Mode::Audit, None).stop,
        Some(Stop::Budget("passes"))
    );
}

#[test]
fn client_channel_runtime_and_process_scopes_never_inherit_hint_shortcut() {
    for variant in 0..4 {
        let mut memory = Memory::new(512);
        memory.put(100, &record(1, b"same_build_other_channel", false));
        let mut other = scope();
        match variant {
            0 => other.client_channel = [9; 32],
            1 => other.created += 1,
            2 => other.runtime = Some([9; 16]),
            _ => other.installation = [9; 32],
        }
        let hints = Hints {
            scope: other,
            addresses: vec![100],
        };
        let out = run(
            &mut memory,
            &mut Validator::default(),
            Mode::First,
            Some(&hints),
            Limits::default(),
        );
        assert_eq!(out.records.len(), 1);
        assert!(!out.coverage.hint_hit);
        assert_eq!(memory.reads[0].0, 0);
    }
}

#[test]
fn cancellation_and_expired_deadline_release_no_observations() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let mut memory = Memory::new(512);
    memory.put(100, &record(1, b"current", false));
    memory.cancelled = Some(cancel.clone());
    let mut validator = Validator::default();
    let out = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap()
    .discover(Mode::First, None);
    assert_eq!(out.stop, Some(Stop::Cancelled));
    assert!(out.records.is_empty() && !out.coverage.complete);
    cancel.store(false, Ordering::Release);
    let out = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        Instant::now(),
        &cancel,
    )
    .unwrap()
    .discover(Mode::First, None);
    assert_eq!(out.stop, Some(Stop::Deadline));
    assert_eq!(out.usage.read_calls, 0);
}

#[test]
fn stale_hint_relocates_nearby_but_unknown_runtime_only_scans() {
    let mut memory = Memory::new(1024);
    memory.put(500, &record(1, b"moved", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Nearby,
        Some(&hints),
        Limits::default(),
    );
    assert_eq!(out.records[0].address, 500);
    assert!(out.coverage.local_only && !out.coverage.hint_hit);
    assert_eq!(out.coverage.hint_misses, 1);
    let mut unknown = scope();
    unknown.runtime = None;
    let hints = Hints {
        scope: unknown.clone(),
        addresses: vec![500],
    };
    let mut validator = Validator::default();
    let cancel = AtomicBool::new(false);
    memory.reads.clear();
    let out = Request::new(
        &mut memory,
        &mut validator,
        unknown,
        format(),
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap()
    .discover(Mode::First, Some(&hints));
    assert_eq!(out.records.len(), 1);
    assert!(!out.coverage.hint_hit);
    assert_eq!(memory.reads[0].0, 0);
}

#[test]
fn salvage_and_hint_validation_are_charged_before_each_attempt() {
    let mut memory = Memory::new(8192);
    memory.short_once = true;
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 8192,
            read_bytes: 8192,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, Some(Stop::Budget("read_bytes")));
    assert_eq!(out.usage.requested_bytes, 8192);
    assert_eq!(memory.reads.len(), 1);
    let mut memory = Memory::new(1024);
    memory.put(100, &record(1, b"hint", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits {
            read_bytes: HEADER as u64,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, Some(Stop::Budget("read_bytes")));
    assert!(out.records.is_empty());
    assert_eq!(memory.reads, vec![(100, HEADER)]);
}

#[test]
fn oversized_admission_never_allocates_or_reads_claimed_body() {
    let mut memory = Memory::new(1024);
    let mut bytes = record(1, b"length", false);
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    memory.put(100, &bytes);
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert!(out.records.is_empty());
    assert_eq!(out.stop, Some(Stop::Budget("record_bytes")));
    assert!(memory.reads.iter().all(|(_, len)| *len <= 1024));
}

#[test]
fn observed_conflicts_survive_multiple_passes() {
    let mut memory = Memory::new(2048);
    memory.put(100, &record(1, b"first", false));
    memory.put(500, &record(1, b"second", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let mut validator = Validator::default();
    let cancel = AtomicBool::new(false);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap();
    assert_eq!(request.discover(Mode::First, Some(&hints)).records.len(), 1);
    let out = request.discover(Mode::Audit, None);
    assert_eq!(out.stop, Some(Stop::Conflict([1; 32])));
    assert!(out.records.is_empty());
}

#[test]
fn final_identity_check_itself_is_budgeted_and_required() {
    let mut memory = Memory::new(512);
    memory.put(100, &record(1, b"hint", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::First,
        Some(&hints),
        Limits {
            source_calls: 3,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, Some(Stop::Budget("source_calls")));
    assert!(out.records.is_empty());
    assert_eq!(out.usage.source_calls, 3);
    assert_eq!(memory.checks, 1);
}

#[test]
fn fatal_read_causes_survive_hint_scan_and_full_candidate_paths() {
    for (hint, call, cause) in [
        (true, 1, "hint_process_identity_changed"),
        (false, 1, "scan_access_revoked"),
        (false, 3, "candidate_mapping_raced"),
    ] {
        let mut memory = Memory::new(1024);
        memory.put(100, &record(1, b"valid_after_fault", false));
        memory.fault_read = Some((call, SourceError::Fatal(cause.into())));
        let hints = Hints {
            scope: scope(),
            addresses: vec![100],
        };
        let mut validator = Validator::default();
        let cancel = AtomicBool::new(false);
        let mut request = Request::new(
            &mut memory,
            &mut validator,
            scope(),
            format(),
            Limits::default(),
            Instant::now() + Duration::from_secs(10),
            &cancel,
        )
        .unwrap();
        let out = request.discover(Mode::First, hint.then_some(&hints));
        assert_eq!(out.stop, Some(Stop::Source(cause.into())));
        assert!(out.records.is_empty() && !out.coverage.complete);
        let retried = request.discover(Mode::Audit, None);
        assert_eq!(retried.stop, Some(Stop::Source(cause.into())));
        assert_eq!(retried.usage.read_calls, out.usage.read_calls);
        assert_eq!(memory.reads.len(), call, "fault must not trigger salvage");
    }
}

#[test]
fn first_fatal_read_cause_is_preserved_if_final_identity_check_also_fails() {
    let mut memory = Memory::new(512);
    memory.fault_read = Some((1, SourceError::Fatal("original_read_fault".into())));
    memory.fail_identity_after = Some(2);
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(out.stop, Some(Stop::Source("original_read_fault".into())));
    assert_eq!(memory.checks, 2);
    assert!(out.records.is_empty());
}

#[test]
fn fatal_candidate_read_discards_previously_validated_records() {
    let mut memory = Memory::new(1024);
    memory.put(40, &record(1, b"accepted_first", false));
    memory.put(200, &record(2, b"fault_second", false));
    memory.fault_read = Some((5, SourceError::Fatal("second_record_fault".into())));
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits::default(),
    );
    assert_eq!(out.stop, Some(Stop::Source("second_record_fault".into())));
    assert!(out.records.is_empty());
    assert_eq!(memory.reads.len(), 5);
}

#[test]
fn recoverable_zero_read_salvages_once_and_can_complete_coverage() {
    let mut memory = Memory::new(8192);
    memory.zero_once = true;
    memory.put(4093, &record(1, b"page_crossing", false));
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 8192,
            ..Limits::default()
        },
    );
    assert_eq!(out.stop, None);
    assert_eq!(out.records.len(), 1);
    assert!(out.coverage.complete && out.coverage.gaps.is_empty());
    assert_eq!(out.coverage.searched_bytes, 8192);
    assert_eq!(&memory.reads[..3], &[(0, 8192), (0, 4096), (4096, 4096)]);
}

#[test]
fn early_hit_accounts_only_inspected_prefix_without_overlap_double_counting() {
    for at in [40usize, 125, 300] {
        let mut memory = Memory::new(1024);
        memory.put(at, &record(1, b"target", false));
        let out = run(
            &mut memory,
            &mut Validator::default(),
            Mode::First,
            None,
            Limits {
                chunk_bytes: 128,
                ..Limits::default()
            },
        );
        let inspected = (at + MAGIC.len()).min((at / 128 + 1) * 128) as u64;
        assert_eq!(out.stop, None);
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.coverage.searched_bytes, inspected);
        assert_eq!(out.coverage.unsearched_bytes, 1024 - inspected);
        assert!(out.coverage.stopped_early && !out.coverage.complete);
    }
    let mut memory = Memory::new(1024);
    let out = run(
        &mut memory,
        &mut Validator::default(),
        Mode::Audit,
        None,
        Limits {
            chunk_bytes: 128,
            ..Limits::default()
        },
    );
    assert!(out.coverage.complete);
    assert_eq!(out.coverage.searched_bytes, 1024);
    assert_eq!(out.coverage.unsearched_bytes, 0);
}

#[test]
fn nearby_passes_effective_deadline_to_every_source_and_verifier_call() {
    let mut memory = Memory::new(1024);
    memory.put(500, &record(1, b"nearby", false));
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let mut validator = Validator::default();
    let cancel = AtomicBool::new(false);
    let before = Instant::now();
    let global = before + Duration::from_secs(10);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        global,
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, None);
    assert_eq!(near.records.len(), 1);
    let source_count = near.usage.source_calls as usize;
    let full = request.discover(Mode::Audit, None);
    assert_eq!(full.stop, None);
    assert!(full.coverage.complete);
    drop(request);
    let local = memory.deadlines[0];
    assert!(local >= before && local <= before + Duration::from_millis(300));
    assert!(local < global);
    assert!(
        memory.deadlines[..source_count]
            .iter()
            .all(|deadline| *deadline == local)
    );
    assert!(
        memory.deadlines[source_count..]
            .iter()
            .all(|deadline| *deadline == global)
    );
    // The stale hint fails the marker check before entering the verifier.
    assert_eq!(validator.deadlines.len(), 4);
    assert!(
        validator.deadlines[..2]
            .iter()
            .all(|deadline| *deadline == local)
    );
    assert!(
        validator.deadlines[2..]
            .iter()
            .all(|deadline| *deadline == global)
    );
}

#[test]
fn same_task_body_length_tamper_is_rejected_before_large_read() {
    let mut memory = Memory::new(2048);
    let bytes = record(1, &[42; 1024], true);
    let mut validator = Validator::default();
    validator
        .bodies
        .insert(1, (Sha256::digest(&bytes).into(), bytes.len()));
    let mut tampered = bytes.clone();
    tampered[12..16].copy_from_slice(&120_000u32.to_le_bytes());
    memory.put(100, &tampered);
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let out = run(
        &mut memory,
        &mut validator,
        Mode::First,
        Some(&hints),
        Limits::default(),
    );
    assert_eq!(out.stop, None);
    assert!(out.records.is_empty());
    assert_eq!(validator.complete_calls, 0);
    assert!(
        memory
            .reads
            .iter()
            .all(|(_, size)| *size == HEADER || *size == 2048)
    );
    assert!(!memory.reads.contains(&(100, 120_000)));
}

#[test]
fn expired_nearby_recovers_in_same_request_with_original_global_deadline() {
    let mut memory = Memory::new(1024);
    memory.put(100, &record(1, b"current", false));
    memory.expire_deadline_once = true;
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    let global = Instant::now() + Duration::from_secs(10);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        global,
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, Some(Stop::NearbyBudget));
    assert!(near.records.is_empty() && !near.coverage.complete);
    // Initial identity + one timed-out hint read. Final identity is not called.
    assert_eq!(near.usage.source_calls, 2);
    assert_eq!(near.usage.read_calls, 1);
    let full = request.discover(Mode::Audit, None);
    assert_eq!(full.stop, None);
    assert_eq!(full.records.len(), 1);
    assert!(full.coverage.complete);
    assert_eq!(full.usage.passes, 2);
    assert!(full.usage.requested_bytes > near.usage.requested_bytes);
    drop(request);
    assert_eq!(memory.checks, 3); // Nearby entry, full entry and full exit.
    assert!(
        memory.deadlines[..2]
            .iter()
            .all(|deadline| *deadline < global)
    );
    assert!(
        memory.deadlines[2..]
            .iter()
            .all(|deadline| *deadline == global)
    );
    assert!(
        validator
            .deadlines
            .iter()
            .all(|deadline| *deadline == global)
    );
}

#[test]
fn expired_nearby_cannot_bypass_failed_identity_on_full_fallback() {
    let mut memory = Memory::new(1024);
    memory.put(100, &record(1, b"stale_instance", false));
    memory.expire_deadline_once = true;
    memory.fail_identity_after = Some(2);
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    let global = Instant::now() + Duration::from_secs(10);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        global,
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, Some(Stop::NearbyBudget));
    assert!(near.records.is_empty());
    let full = request.discover(Mode::Audit, None);
    assert_eq!(full.stop, Some(Stop::Source("identity_reused".into())));
    assert!(full.records.is_empty() && !full.coverage.complete);
    assert_eq!(full.usage.read_calls, near.usage.read_calls);
    assert_eq!(full.usage.requested_bytes, near.usage.requested_bytes);
    let retried = request.discover(Mode::First, None);
    assert_eq!(retried.stop, Some(Stop::Source("identity_reused".into())));
    assert!(retried.records.is_empty());
    assert_eq!(retried.usage.source_calls, full.usage.source_calls);
    drop(request);
    assert!(
        memory.deadlines[2..]
            .iter()
            .all(|deadline| *deadline == global)
    );
    assert_eq!(validator.complete_calls, 0);
}

#[test]
fn typed_global_deadline_poison_prevents_fallback_and_further_io() {
    let mut memory = Memory::new(1024);
    memory.put(100, &record(1, b"current", false));
    memory.expire_deadline_once = true;
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    // Earlier than Nearby's 250 ms cap, so the supplied deadline is global.
    let global = Instant::now() + Duration::from_millis(100);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        global,
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, Some(Stop::Deadline));
    assert!(near.records.is_empty());
    assert_eq!(near.usage.read_calls, 1);
    let full = request.discover(Mode::Audit, None);
    assert_eq!(full.stop, Some(Stop::Deadline));
    assert!(full.records.is_empty());
    assert_eq!(full.usage.source_calls, near.usage.source_calls);
    assert_eq!(full.usage.requested_bytes, near.usage.requested_bytes);
    drop(request);
    assert!(memory.deadlines.iter().all(|value| *value == global));
    assert_eq!(memory.checks, 1);
    assert_eq!(validator.complete_calls, 0);
}

#[test]
fn premature_typed_deadline_is_fatal_and_later_cancellation_cannot_hide_it() {
    let mut memory = Memory::new(1024);
    memory.fault_read = Some((1, SourceError::Deadline));
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    let global = Instant::now() + Duration::from_secs(10);
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        global,
        &cancel,
    )
    .unwrap();
    // Full mode gives a generous deadline; an immediate Deadline is malformed.
    let first = request.discover(Mode::First, Some(&hints));
    let expected = Some(Stop::Source("source.deadline_not_reached".into()));
    assert_eq!(first.stop, expected);
    assert!(first.records.is_empty());
    cancel.store(true, Ordering::Release);
    let retry = request.discover(Mode::Audit, None);
    assert_eq!(retry.stop, expected);
    assert_eq!(retry.usage.source_calls, first.usage.source_calls);
    assert_eq!(retry.usage.read_calls, 1);
}

#[test]
fn fatal_window_and_os_faults_at_local_expiry_preserve_cause_and_poison_request() {
    for cause in ["window.closed", "memory.read:fixture_os_failure"] {
        let mut memory = Memory::new(1024);
        memory.expire_deadline_once = true;
        memory.fault_read = Some((1, SourceError::Fatal(cause.into())));
        let mut validator = Validator::default();
        let hints = Hints {
            scope: scope(),
            addresses: vec![100],
        };
        let cancel = AtomicBool::new(false);
        let mut request = Request::new(
            &mut memory,
            &mut validator,
            scope(),
            format(),
            Limits::default(),
            Instant::now() + Duration::from_secs(10),
            &cancel,
        )
        .unwrap();
        let near = request.discover(Mode::Nearby, Some(&hints));
        assert_eq!(near.stop, Some(Stop::Source(cause.into())));
        assert!(near.records.is_empty());
        assert_eq!(near.usage.source_calls, 2);
        let full = request.discover(Mode::Audit, None);
        assert_eq!(full.stop, near.stop);
        assert!(full.records.is_empty());
        assert_eq!(full.usage.source_calls, near.usage.source_calls);
        assert_eq!(full.usage.requested_bytes, near.usage.requested_bytes);
    }
}

#[test]
fn typed_local_deadline_recovery_keeps_consumed_read_budget() {
    let mut memory = Memory::new(1024);
    memory.expire_deadline_once = true;
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let cancel = AtomicBool::new(false);
    let limits = Limits {
        read_calls: 1,
        ..Limits::default()
    };
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        limits,
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, Some(Stop::NearbyBudget));
    let full = request.discover(Mode::First, Some(&hints));
    assert_eq!(full.stop, Some(Stop::Budget("read_calls")));
    assert!(full.records.is_empty());
    assert_eq!(full.usage.read_calls, 1);
    assert_eq!(full.usage.requested_bytes, near.usage.requested_bytes);
    drop(request);
    assert_eq!(memory.checks, 3); // Local entry, full entry and full exit.
}

#[test]
fn cancellation_during_typed_local_expiry_prevents_fallback() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let mut memory = Memory::new(1024);
    memory.expire_deadline_once = true;
    memory.cancelled = Some(cancel.clone());
    let mut validator = Validator::default();
    let hints = Hints {
        scope: scope(),
        addresses: vec![100],
    };
    let mut request = Request::new(
        &mut memory,
        &mut validator,
        scope(),
        format(),
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        &cancel,
    )
    .unwrap();
    let near = request.discover(Mode::Nearby, Some(&hints));
    assert_eq!(near.stop, Some(Stop::Cancelled));
    assert!(near.records.is_empty());
    let full = request.discover(Mode::Audit, None);
    assert_eq!(full.stop, Some(Stop::Cancelled));
    assert_eq!(full.usage.source_calls, near.usage.source_calls);
    assert_eq!(full.usage.read_calls, 1);
}
