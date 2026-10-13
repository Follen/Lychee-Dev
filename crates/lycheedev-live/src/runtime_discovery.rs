//! Bounded discovery of immutable runtime records, independent of EXE and Lua layout.
//!
//! This is a read-only engine, not an input capability. `Verifier` is a mandatory
//! protocol boundary: a checksum, marker, hint or completed scan grants no authority.
//! A request owns one deadline and cumulative limits across hint, nearby and full
//! passes. Adapters must bound each synchronous call by the supplied deadline;
//! this engine cannot interrupt an adapter blocked inside an OS/library call.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Stop>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub pid: u32,
    pub created: u64,
    pub window: u64,
    pub installation: [u8; 32],
    /// Explicit product/channel identity, independent of build and installation path.
    pub client_channel: [u8; 32],
    pub protocol: u32,
    pub runtime: Option<[u8; 16]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub span: Span,
    pub private: bool,
    pub committed: bool,
    pub readable: bool,
}

/// Hold the selected process object throughout a request. Every read must recheck
/// *all* spanned current mappings: committed readable MEM_PRIVATE, no guard pages.
/// Return only the contiguous valid prefix on a partial read; never fabricate zeros.
/// Recoverable unreadable memory is Ok(0) or a short prefix. Fatal faults retain
/// their cause even when a deadline also expires. Deadline is only valid after
/// the supplied deadline; the request validates it before allowing local recovery.
/// `query_region` returns a region containing `at`, or None only at address-space
/// end. Enumeration and reads must not open/hash an executable or inspect code.
/// Each read invocation performs at most one physical memory read. Its mapping
/// checks must be finite and deadline-aware, bounded by the pages in `into`;
/// the engine's `source_calls` counts adapter calls, not hidden OS calls.
/// A production adapter's mapping checks and OS-call bounds need their own audit.
pub trait Source {
    fn verify_identity(
        &mut self,
        scope: &Scope,
        deadline: Instant,
    ) -> std::result::Result<(), SourceError>;
    fn query_region(
        &mut self,
        at: u64,
        deadline: Instant,
    ) -> std::result::Result<Option<Region>, SourceError>;
    fn read_private(
        &mut self,
        at: u64,
        into: &mut [u8],
        deadline: Instant,
    ) -> std::result::Result<usize, SourceError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    Fatal(String),
    /// The supplied deadline expired. This is not an unreadable-memory signal.
    Deadline,
}

#[derive(Clone, Debug)]
pub struct Format {
    pub marker: Vec<u8>,
    pub marker_offset: usize,
    pub header_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Admission {
    Small {
        total_bytes: usize,
    },
    /// May ONLY be returned after a verified current terminal record binds the
    /// exact task, digest and length. A selector boolean or public header is
    /// insufficient. The verifier retains that proof and checks it again below.
    AuthorizedBody {
        total_bytes: usize,
    },
}
impl Admission {
    fn len(self) -> usize {
        match self {
            Self::Small { total_bytes } | Self::AuthorizedBody { total_bytes } => total_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Verified {
    /// Canonical authenticated record identity, including message role/revision.
    /// Two different byte strings with this identity are a conflict, even when
    /// the business predicate would reject one of them.
    pub identity: [u8; 32],
    pub matches_query: bool,
}

/// The protocol owner, not the discovery engine, implements full validation.
/// `admit_header` filters schema/kind/runtime/current nonce/task/length before
/// allocating. `verify_record` checks complete framing, trailer, digest, private
/// authentication, exact instance/client/channel/task, current observation nonce,
/// immutable freshness deadline and BODY binding. None means rejected bytes;
/// Err means a fatal verification failure. No checksum-only adapter is supplied.
/// Methods must obey the supplied deadline and perform no unbudgeted memory I/O.
pub trait Verifier {
    fn admit_header(
        &mut self,
        scope: &Scope,
        header: &[u8],
        deadline: Instant,
    ) -> std::result::Result<Option<Admission>, String>;
    fn verify_record(
        &mut self,
        scope: &Scope,
        admission: Admission,
        bytes: &[u8],
        deadline: Instant,
    ) -> std::result::Result<Option<Verified>, String>;
}

#[derive(Clone, Debug)]
pub struct Hints {
    pub scope: Scope,
    pub addresses: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub read_bytes: u64,
    pub read_calls: u64,
    /// All source calls, including identity and mapping queries.
    pub source_calls: u64,
    pub regions: u64,
    pub planned_bytes: u64,
    pub candidates: u64,
    pub records: usize,
    pub retained_bytes: usize,
    pub record_bytes: usize,
    pub chunk_bytes: usize,
    pub passes: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            read_bytes: 64 << 30,
            read_calls: 200_000,
            source_calls: 400_000,
            regions: 100_000,
            planned_bytes: 64 << 30,
            candidates: 4096,
            records: 4096,
            retained_bytes: 16 << 20,
            record_bytes: 1 << 20,
            chunk_bytes: 1 << 20,
            passes: 3,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub requested_bytes: u64,
    pub read_calls: u64,
    pub source_calls: u64,
    pub region_queries: u64,
    pub candidates: u64,
    pub passes: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    InvalidOptions,
    Cancelled,
    Deadline,
    NearbyBudget,
    Budget(&'static str),
    Source(String),
    Verifier(String),
    InvalidRegion,
    InvalidRead,
    Conflict([u8; 32]),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    First,
    Audit,
    Nearby,
}
#[derive(Clone, Debug)]
pub struct Accepted {
    pub address: u64,
    pub identity: [u8; 32],
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    /// Complete traversal of the requested map scope, not an atomic heap snapshot.
    pub enumeration_complete: bool,
    /// True only for a full pass with every planned byte searched and no gaps.
    pub complete: bool,
    pub stopped_early: bool,
    pub truncated: bool,
    pub planned_bytes: u64,
    /// Bytes in the inspected marker-search prefix, excluding forward overlap
    /// owned by a later chunk. Point/full-record validation adds no scan coverage.
    pub searched_bytes: u64,
    pub skipped_regions: u64,
    pub gaps: Vec<Span>,
    pub omitted_gaps: u64,
    /// Known planned scan bytes not searched; point rereads never count as
    /// coverage. When enumeration is incomplete, additional scope is unknown.
    pub unsearched_bytes: u64,
    pub local_only: bool,
    pub hint_hit: bool,
    pub hint_misses: u64,
}
#[derive(Clone, Debug)]
pub struct Discovery {
    /// Always empty on fatal identity/verification/conflict errors. No records
    /// are released when final identity validation could not complete.
    pub records: Vec<Accepted>,
    pub coverage: Coverage,
    pub usage: Usage,
    pub stop: Option<Stop>,
}

pub struct Request<'a, S, V> {
    source: &'a mut S,
    verifier: &'a mut V,
    scope: Scope,
    format: Format,
    limits: Limits,
    deadline: Instant,
    cancel: &'a AtomicBool,
    usage: Usage,
    // Retain conflicts across passes without retaining bodies or authority.
    observed: BTreeMap<[u8; 32], [u8; 32]>,
    poisoned: Option<Stop>,
    local: Option<(u64, u64, Instant)>,
}
impl<'a, S: Source, V: Verifier> Request<'a, S, V> {
    pub fn new(
        source: &'a mut S,
        verifier: &'a mut V,
        scope: Scope,
        format: Format,
        limits: Limits,
        deadline: Instant,
        cancel: &'a AtomicBool,
    ) -> Result<Self> {
        if scope.pid == 0
            || scope.created == 0
            || scope.window == 0
            || scope.protocol == 0
            || scope.installation == [0; 32]
            || scope.client_channel == [0; 32]
            || scope.runtime == Some([0; 16])
            || format.marker.is_empty()
            || format.marker.len() > 256
            || format.header_bytes == 0
            || format.header_bytes > 4096
            || format
                .marker_offset
                .checked_add(format.marker.len())
                .is_none_or(|n| n > format.header_bytes)
            || limits.chunk_bytes < format.marker.len()
            || limits.chunk_bytes > 8 << 20
            || limits.record_bytes < format.header_bytes
            || limits.record_bytes > 16 << 20
            || limits.records == 0
            || limits.records > 4096
            || limits.candidates == 0
            || limits.candidates > 65536
            || limits.regions == 0
            || limits.regions > 100_000
            || limits.passes == 0
            || limits.passes > 8
            || limits.read_calls == 0
            || limits.read_calls > 1_000_000
            || limits.source_calls < 2
            || limits.source_calls > 2_000_000
            || limits.read_bytes == 0
            || limits.read_bytes > 128 << 30
            || limits.planned_bytes == 0
            || limits.planned_bytes > 64 << 30
            || limits.retained_bytes == 0
            || limits.retained_bytes > 64 << 20
        {
            return Err(Stop::InvalidOptions);
        }
        Ok(Self {
            source,
            verifier,
            scope,
            format,
            limits,
            deadline,
            cancel,
            usage: Usage::default(),
            observed: BTreeMap::new(),
            poisoned: None,
            local: None,
        })
    }
    fn check(&self) -> Result<()> {
        if let Some(stop) = &self.poisoned {
            return Err(stop.clone());
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(Stop::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Stop::Deadline);
        }
        if self
            .local
            .is_some_and(|(_, _, deadline)| Instant::now() >= deadline)
        {
            return Err(Stop::NearbyBudget);
        }
        Ok(())
    }
    fn call(&mut self) -> Result<()> {
        self.check()?;
        if self.usage.source_calls >= self.limits.source_calls {
            return Err(Stop::Budget("source_calls"));
        }
        self.usage.source_calls += 1;
        Ok(())
    }
    fn call_deadline(&self) -> Instant {
        self.local
            .map_or(self.deadline, |(_, _, local)| local.min(self.deadline))
    }
    fn source_result<T>(
        &self,
        result: std::result::Result<T, SourceError>,
        deadline: Instant,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(SourceError::Fatal(cause)) => Err(Stop::Source(cause)),
            Err(SourceError::Deadline) => {
                // An adapter cannot manufacture a recoverable local stop, nor
                // hide a fault behind cancellation before the supplied deadline.
                if Instant::now() < deadline {
                    return Err(Stop::Source("source.deadline_not_reached".into()));
                }
                match self.check() {
                    Err(stop) => Err(stop),
                    Ok(()) => Err(Stop::Source("source.deadline_not_reached".into())),
                }
            }
        }
    }
    fn verify(&mut self) -> Result<()> {
        self.call()?;
        let deadline = self.call_deadline();
        let result = self.source.verify_identity(&self.scope, deadline);
        self.source_result(result, deadline)?;
        self.check()
    }
    fn read(&mut self, at: u64, into: &mut [u8]) -> Result<usize> {
        self.check()?;
        if at.checked_add(into.len() as u64).is_none() {
            return Err(Stop::InvalidRead);
        }
        if self.usage.read_calls >= self.limits.read_calls {
            return Err(Stop::Budget("read_calls"));
        }
        if into.len() as u64 > self.limits.read_bytes - self.usage.requested_bytes {
            return Err(Stop::Budget("read_bytes"));
        }
        if let Some((bytes, calls, _)) = self.local
            && (self.usage.read_calls - calls >= 4096
                || into.len() as u64 > (8 << 20) - (self.usage.requested_bytes - bytes))
        {
            return Err(Stop::NearbyBudget);
        }
        self.call()?;
        self.usage.read_calls += 1;
        self.usage.requested_bytes += into.len() as u64;
        // Only a successful short prefix permits salvage. Fatal source faults
        // retain their cause and never expose bytes from this buffer.
        let deadline = self.call_deadline();
        let result = self.source.read_private(at, into, deadline);
        let n = self.source_result(result, deadline)?;
        self.check()?;
        if n > into.len() {
            return Err(Stop::InvalidRead);
        }
        Ok(n)
    }
    fn candidate(&mut self, address: u64, coverage: &mut Coverage) -> Result<Option<Accepted>> {
        self.check()?;
        if self.usage.candidates >= self.limits.candidates {
            return Err(Stop::Budget("candidates"));
        }
        self.usage.candidates += 1;
        let mut head = vec![0; self.format.header_bytes];
        if self.read(address, &mut head)? != head.len() {
            Self::gap(coverage, address, address.saturating_add(head.len() as u64));
            return Ok(None);
        }
        let offset = self.format.marker_offset;
        if head[offset..offset + self.format.marker.len()] != self.format.marker {
            return Ok(None);
        }
        let deadline = self.call_deadline();
        let admission = self
            .verifier
            .admit_header(&self.scope, &head, deadline)
            .map_err(Stop::Verifier)?;
        self.check()?;
        let Some(admission) = admission else {
            return Ok(None);
        };
        if self.local.is_some() && matches!(admission, Admission::AuthorizedBody { .. }) {
            return Ok(None);
        }
        let len = admission.len();
        if len > self.limits.record_bytes {
            return Err(Stop::Budget("record_bytes"));
        }
        if len < head.len()
            || matches!(admission, Admission::Small { .. }) && len > 16 * 1024 + 4096
        {
            return Err(Stop::Verifier("invalid_admission_length".into()));
        }
        let mut bytes = vec![0; len];
        if self.read(address, &mut bytes)? != len || bytes[..head.len()] != head {
            Self::gap(coverage, address, address.saturating_add(len as u64));
            return Ok(None);
        }
        let deadline = self.call_deadline();
        let verified = self
            .verifier
            .verify_record(&self.scope, admission, &bytes, deadline)
            .map_err(Stop::Verifier)?;
        self.check()?;
        let Some(verified) = verified else {
            return Ok(None);
        };
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if let Some(previous) = self.observed.get(&verified.identity) {
            if previous != &digest {
                return Err(Stop::Conflict(verified.identity));
            }
        } else {
            self.observed.insert(verified.identity, digest);
        }
        Ok(verified.matches_query.then_some(Accepted {
            address,
            identity: verified.identity,
            bytes,
        }))
    }
    fn query(&mut self, at: u64) -> Result<Option<Region>> {
        self.check()?;
        if self.usage.region_queries >= self.limits.regions {
            return Err(Stop::Budget("regions"));
        }
        self.call()?;
        self.usage.region_queries += 1;
        let deadline = self.call_deadline();
        let result = self.source.query_region(at, deadline);
        let region = self.source_result(result, deadline)?;
        self.check()?;
        if region.is_some_and(|r| r.span.start > at || r.span.end <= at) {
            return Err(Stop::InvalidRegion);
        }
        Ok(region)
    }
    fn plan(&mut self, windows: &[Span], coverage: &mut Coverage) -> Result<Vec<Span>> {
        let mut ranges: Vec<Span> = Vec::new();
        for window in windows {
            let mut at = window.start;
            while at < window.end {
                let Some(region) = self.query(at)? else {
                    break;
                };
                let end = region.span.end.min(window.end);
                if region.private && region.committed && region.readable {
                    if end - at > self.limits.planned_bytes - coverage.planned_bytes {
                        return Err(Stop::Budget("planned_bytes"));
                    }
                    coverage.planned_bytes += end - at;
                    if let Some(last) = ranges.last_mut().filter(|r| r.end == at) {
                        last.end = end;
                    } else {
                        ranges.push(Span { start: at, end });
                    }
                } else {
                    coverage.skipped_regions += 1;
                }
                at = end;
            }
        }
        coverage.enumeration_complete = true;
        Ok(ranges)
    }
    fn gap(coverage: &mut Coverage, start: u64, end: u64) {
        if start >= end {
            return;
        }
        if coverage.gaps.len() < 4096 {
            coverage.gaps.push(Span { start, end });
        } else {
            coverage.omitted_gaps += 1;
        }
    }
    /// One original read plus at most one retry for each remaining page portion.
    /// Every retry rechecks current eligibility in Source and uses the same caps.
    fn spans(&mut self, at: u64, bytes: &mut [u8], coverage: &mut Coverage) -> Result<Vec<Span>> {
        let n = self.read(at, bytes)?;
        let mut spans = Vec::<Span>::new();
        if n != 0 {
            spans.push(Span {
                start: at,
                end: at + n as u64,
            });
        }
        let mut offset = n;
        while offset < bytes.len() {
            let address = at + offset as u64;
            let end = (offset + (4096 - address % 4096) as usize).min(bytes.len());
            let n = self.read(address, &mut bytes[offset..end])?;
            if n != 0 {
                if let Some(last) = spans.last_mut().filter(|r| r.end == address) {
                    last.end += n as u64;
                } else {
                    spans.push(Span {
                        start: address,
                        end: address + n as u64,
                    });
                }
            }
            Self::gap(coverage, address + n as u64, at + end as u64);
            offset = end;
        }
        Ok(spans)
    }
    fn keep(&self, record: Accepted, out: &mut Discovery) -> Result<()> {
        if out.records.iter().any(|r| r.identity == record.identity) {
            return Ok(());
        }
        if out.records.len() >= self.limits.records {
            return Err(Stop::Budget("records"));
        }
        let retained: usize = out.records.iter().map(|r| r.bytes.len()).sum();
        if record.bytes.len() > self.limits.retained_bytes - retained {
            return Err(Stop::Budget("retained_bytes"));
        }
        out.records.push(record);
        Ok(())
    }
    fn run(&mut self, mode: Mode, hints: Option<&Hints>, out: &mut Discovery) -> Result<()> {
        self.check()?;
        if self.usage.passes >= self.limits.passes {
            return Err(Stop::Budget("passes"));
        }
        self.usage.passes += 1;
        if mode == Mode::Nearby {
            self.local = Some((
                self.usage.requested_bytes,
                self.usage.read_calls,
                Instant::now()
                    .checked_add(Duration::from_millis(250))
                    .unwrap_or(self.deadline)
                    .min(self.deadline),
            ));
        }
        self.verify()?;
        let hints = hints.filter(|h| h.scope == self.scope && h.addresses.len() <= 64);
        let known = self.scope.runtime.is_some();
        if mode != Mode::Audit
            && known
            && let Some(hints) = hints
        {
            for &at in &hints.addresses {
                // Point hints are not part of the scan plan. A stale hint
                // cannot introduce a permanent gap into a later full pass.
                let mut point = Coverage::default();
                if let Some(record) = self.candidate(at, &mut point)? {
                    self.keep(record, out)?;
                    out.coverage.hint_hit = true;
                    out.coverage.stopped_early = true;
                    return Ok(());
                }
                out.coverage.hint_misses += 1;
            }
        }
        let windows = if mode == Mode::Nearby {
            if !known {
                return Ok(());
            }
            let mut windows = hints
                .into_iter()
                .flat_map(|h| h.addresses.iter())
                .take(8)
                .filter_map(|at| {
                    let start = at.saturating_sub(1 << 19) & !4095;
                    start.checked_add(1 << 20).map(|end| Span { start, end })
                })
                .collect::<Vec<_>>();
            windows.sort_by_key(|r| r.start);
            let mut merged = Vec::<Span>::new();
            for r in windows {
                if let Some(last) = merged.last_mut().filter(|last| last.end >= r.start) {
                    last.end = last.end.max(r.end);
                } else {
                    merged.push(r);
                }
            }
            merged
        } else {
            vec![Span {
                start: 0,
                end: u64::MAX,
            }]
        };
        let ranges = self.plan(&windows, &mut out.coverage)?;
        // Stream chunks instead of materializing an unbounded queue. Concurrency
        // is exactly one. Unknown-runtime hints never issue point reads.
        for range in ranges {
            let mut at = range.start;
            while at < range.end {
                self.check()?;
                let end = at
                    .saturating_add(self.limits.chunk_bytes as u64)
                    .min(range.end);
                let read_end = end
                    .saturating_add((self.format.marker.len() - 1) as u64)
                    .min(range.end);
                let mut bytes = vec![0; (read_end - at) as usize];
                let spans = self.spans(at, &mut bytes, &mut out.coverage)?;
                for span in spans {
                    let view = &bytes[(span.start - at) as usize..(span.end - at) as usize];
                    let mut accounted_end = span.start;
                    for (offset, candidate) in view.windows(self.format.marker.len()).enumerate() {
                        // Periodic checks bound cancellation latency
                        // when a large block contains no marker.
                        if offset % 1024 == 0 {
                            self.check()?;
                        }
                        let marker_at = span.start + offset as u64;
                        if marker_at >= end {
                            break;
                        }
                        let matches = candidate == self.format.marker;
                        // Charge only the inspected prefix before any candidate
                        // reread can stop or fail. Forward overlap belongs to the
                        // next chunk and is never counted a second time.
                        let inspected_end = (marker_at + self.format.marker.len() as u64).min(end);
                        out.coverage.searched_bytes += inspected_end.saturating_sub(accounted_end);
                        accounted_end = inspected_end;
                        if matches
                            && let Some(address) =
                                marker_at.checked_sub(self.format.marker_offset as u64)
                            && let Some(record) = self.candidate(address, &mut out.coverage)?
                        {
                            self.keep(record, out)?;
                            if mode != Mode::Audit {
                                out.coverage.stopped_early = true;
                                return Ok(());
                            }
                        }
                    }
                    // A fully exhausted span also examines a trailing fragment
                    // too short to contain a marker. Early returns never count it.
                    out.coverage.searched_bytes += span.end.min(end).saturating_sub(accounted_end);
                }
                at = end;
            }
        }
        Ok(())
    }
    /// No retries are hidden here. A caller may perform Nearby then First on
    /// this same request; pass/read/candidate/deadline budgets never reset.
    /// Audit ignores point hints and never terminates merely because of a hit.
    pub fn discover(&mut self, mode: Mode, hints: Option<&Hints>) -> Discovery {
        let mut out = Discovery {
            records: Vec::new(),
            coverage: Coverage {
                local_only: mode == Mode::Nearby,
                ..Coverage::default()
            },
            usage: self.usage.clone(),
            stop: None,
        };
        let result = self.run(mode, hints, &mut out);
        // Even after a read/verification failure, check identity if control and
        // budget allow. Failure to perform this check never releases records.
        let identity = self.verify();
        self.local = None;
        out.stop = match (result, identity) {
            // Local expiry can prevent the final identity call itself. No
            // records escape; a later full pass must verify identity afresh.
            // An adapter Fatal remains fatal even during local expiry.
            (Err(Stop::NearbyBudget), Err(error)) => Some(error),
            // Preserve the first fatal cause even if the final check also fails.
            (Err(error), _) | (Ok(()), Err(error)) => Some(error),
            (Ok(()), Ok(())) => None,
        };
        if let Some(stop) = &out.stop {
            out.records.clear();
            // A failed request cannot regain authority via a subsequent pass.
            if *stop != Stop::NearbyBudget {
                self.poisoned = Some(stop.clone());
            }
        }
        out.coverage.unsearched_bytes = out
            .coverage
            .planned_bytes
            .saturating_sub(out.coverage.searched_bytes);
        out.coverage.complete = out.stop.is_none()
            && !out.coverage.local_only
            && !out.coverage.stopped_early
            && out.coverage.enumeration_complete
            && out.coverage.unsearched_bytes == 0
            && out.coverage.gaps.is_empty()
            && out.coverage.omitted_gaps == 0;
        out.coverage.truncated = !out.coverage.complete;
        out.usage = self.usage.clone();
        out
    }
}
