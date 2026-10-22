//! Runtime instruction-pattern/RIP root binding. The public entrypoint accepts
//! only the OS-issued RuntimeModule capability, never a caller-supplied RVA,
//! disk executable, archived scan or untrusted finalized-method flag.
//!
//! A binding proves a bounded observation of a root slot, not a Lua ABI, record
//! authenticity, atomic process snapshot or permission to send input. The Lua
//! reader must independently validate its profile and every current object path.
use lycheedev_windows::process::{
    RuntimeBudget, RuntimeCost, RuntimeImageLayout, RuntimeModule, RuntimeModuleIdentity,
    RuntimeReadKind,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{ops::Deref, sync::Arc};

const HEADER_LIMIT: u64 = 1 << 20;
const TEXT_LIMIT: u64 = 128 << 20;
const CHUNK: usize = 1 << 20;
const READ_LIMIT: u64 = 8192;
const MAX_GAPS: usize = READ_LIMIT as usize + 2;
const TOTAL_BYTES: u64 = 2 * HEADER_LIMIT + 2 * TEXT_LIMIT + 4096;
const READ: u32 = 0x40000000;
const WRITE: u32 = 0x80000000;
const EXECUTE: u32 = 0x20000000;

// Fixed Go v3.2.0 (3057a9666ab078270b63c7f06de748069b0d1f4b), runtime-code recipes.
// Historical exact-hash/known-anchor shortcuts deliberately do not enter here.
const RETAIL_PATTERN: &str = "48 8b 3d ?? ?? ?? ?? 48 8b 1d ?? ?? ?? ?? 48 8b cf 44 8b 05 ?? ?? ?? ?? 48 8b d3 e8 ?? ?? ?? ?? 8b 05 ?? ?? ?? ?? 83 c0 01 89 05 ?? ?? ?? ?? 74 ?? 83 3d ?? ?? ?? ?? 00 75 ??";
const FOREVER_PATTERN: &str = "48 8b 1d ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ?? 48 8b cb 4c 8b 38 e8 ?? ?? ?? ?? 49 8b d7 48 8b cb 44 8b 40 08 e8 ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ??";

const FOREVER_CONTEXT_PATTERN: &str = "48 8b 1d ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ?? 48 8b cb 48 8b 28 e8 ?? ?? ?? ?? 48 8b d5 48 8b cb 44 8b 40 08 e8 ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ??";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum RecipeId {
    #[serde(rename = "retail-lua-state-root-rip-v1")]
    Retail,
    #[serde(rename = "forever-lua-state-root-runscript-rip-v1")]
    Forever,
    #[serde(rename = "forever-lua-state-context-rip-v1")]
    ForeverContext,
}
#[derive(Clone, Copy)]
struct Operand {
    offset: usize,
    displacement: usize,
    length: usize,
    width: u64,
    call: bool,
    same_target: u8,
}
const RETAIL_OPERANDS: &[Operand] = &[
    Operand {
        offset: 0,
        displacement: 3,
        length: 7,
        width: 8,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 7,
        displacement: 3,
        length: 7,
        width: 8,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 17,
        displacement: 3,
        length: 7,
        width: 4,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 27,
        displacement: 1,
        length: 5,
        width: 1,
        call: true,
        same_target: 0,
    },
    Operand {
        offset: 32,
        displacement: 2,
        length: 6,
        width: 4,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 41,
        displacement: 2,
        length: 6,
        width: 4,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 49,
        displacement: 2,
        length: 7,
        width: 4,
        call: false,
        same_target: 0,
    },
];
const FOREVER_OPERANDS: &[Operand] = &[
    Operand {
        offset: 0,
        displacement: 3,
        length: 7,
        width: 8,
        call: false,
        same_target: 0,
    },
    Operand {
        offset: 10,
        displacement: 1,
        length: 5,
        width: 1,
        call: true,
        same_target: 1,
    },
    Operand {
        offset: 21,
        displacement: 1,
        length: 5,
        width: 1,
        call: true,
        same_target: 1,
    },
    Operand {
        offset: 36,
        displacement: 1,
        length: 5,
        width: 1,
        call: true,
        same_target: 0,
    },
    Operand {
        offset: 44,
        displacement: 1,
        length: 5,
        width: 1,
        call: true,
        same_target: 1,
    },
];
impl RecipeId {
    fn text(self) -> &'static str {
        match self {
            Self::Retail => RETAIL_PATTERN,
            Self::Forever => FOREVER_PATTERN,
            Self::ForeverContext => FOREVER_CONTEXT_PATTERN,
        }
    }
    fn operands(self) -> &'static [Operand] {
        match self {
            Self::Retail => RETAIL_OPERANDS,
            Self::Forever | Self::ForeverContext => FOREVER_OPERANDS,
        }
    }
}

/// Coverage records attempted logical module reads, independently of the shared
/// ledger's actual OS calls. Failed/partial scans retain their counters and cause.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootEvidence {
    pub recipe: Option<RecipeId>,
    pub pattern_hits: u64,
    pub rejected_matches: u64,
    pub root_candidates: u64,
    pub text_bytes: u64,
    pub gaps: RootGaps,
    pub read_failures: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_read_failure: Option<crate::mailbox::lookup::ReadFailure>,
    pub anchor_rva: Option<u64>,
    pub root_rva: Option<u64>,
    pub anchor_sha256: Option<String>,
    pub header_bytes: u64,
    /// Successfully acquired code bytes; only full_text_pass establishes that
    /// comparisons exhausted the entire domain. A failed block is not atomic.
    pub scanned_bytes: u64,
    pub module_read_calls: u64,
    /// Successfully received bytes. Requested bytes remain independently charged.
    pub module_read_bytes: u64,
    pub module_read_requested_bytes: u64,
    pub full_text_pass: bool,
    pub stable_guards: bool,
    /// Always false: neither code nor the heap is an atomic OS snapshot.
    pub atomic: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootGap {
    pub start: u64,
    pub end: u64,
    pub reason: &'static str,
}
/// Immutable shared scan diagnostics once a candidate is issued. Cloning an
/// error/coverage DTO never re-allocates a potentially large gap ledger after a
/// request has exhausted its budget. Only the unshared scanner can mutate it.
#[derive(Clone, Debug, Default)]
pub struct RootGaps(Option<Arc<Vec<RootGap>>>);
impl Deref for RootGaps {
    type Target = [RootGap];
    fn deref(&self) -> &[RootGap] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}
impl Serialize for RootGaps {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.deref().serialize(serializer)
    }
}
impl PartialEq<Vec<RootGap>> for RootGaps {
    fn eq(&self, other: &Vec<RootGap>) -> bool {
        self.deref() == other.as_slice()
    }
}
impl RootGaps {
    fn scanner(&mut self) -> &mut Vec<RootGap> {
        Arc::get_mut(self.0.as_mut().expect("scan gap ledger reserved"))
            .expect("only unissued scanner mutates gaps")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RootFailure {
    pub cause: String,
    pub evidence: Box<RootEvidence>,
}
impl std::fmt::Display for RootFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cause)
    }
}
impl std::error::Error for RootFailure {}
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Geometry {
    base: u64,
    size: u64,
    architecture: u16,
}
// Private fixture seam. External implementations cannot mint a RootBinding;
// production resolution accepts RuntimeModule only.
trait Source {
    type Identity: Clone + Eq;
    fn identity(&self) -> Self::Identity;
    fn geometry(&self) -> Geometry;
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()>;
    fn read(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> Result<()>;
    fn read_diagnostic(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        self.read(rva, into, kind, budget)
            .map_err(crate::mailbox::lookup::ReadError::unknown)
    }
    fn readable_spans(
        &self,
        start: u64,
        end: u64,
        budget: &mut RuntimeBudget,
    ) -> Result<Vec<(u64, u64)>> {
        budget.charge(RuntimeCost::allocation(std::mem::size_of::<(u64, u64)>()))?;
        Ok(vec![(start, end)])
    }
    /// A short prefix is usable only after the source's identity/mapping guards.
    /// The boolean records an ordinary read failure even if bytes were returned.
    fn read_some(
        &self,
        rva: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
    ) -> Result<(usize, bool)> {
        self.read(rva, into, RuntimeReadKind::Code, budget)?;
        Ok((into.len(), false))
    }
}
impl Source for RuntimeModule {
    type Identity = RuntimeModuleIdentity;
    fn identity(&self) -> Self::Identity {
        RuntimeModule::identity(self).clone()
    }
    fn geometry(&self) -> Geometry {
        Geometry {
            base: self.identity().base(),
            size: self.identity().size(),
            architecture: self.identity().architecture(),
        }
    }
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        RuntimeModule::verify(self, budget)
    }
    fn read(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        self.read_exact(rva, into, kind, budget)
    }
    fn read_diagnostic(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        self.read_exact_observed(rva, into, kind, budget)
            .map_err(Into::into)
    }
    fn readable_spans(
        &self,
        start: u64,
        end: u64,
        budget: &mut RuntimeBudget,
    ) -> Result<Vec<(u64, u64)>> {
        self.code_readable_spans(start, end, budget)
    }
    fn read_some(
        &self,
        rva: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
    ) -> Result<(usize, bool)> {
        self.read_code_prefix(rva, into, budget)
    }
}
#[derive(Clone, Copy, Debug)]
struct Section {
    start: u64,
    end: u64,
    flags: u32,
    text: bool,
}
#[derive(Clone)]
struct HeaderGuard {
    rva: u64,
    bytes: Vec<u8>,
}
#[derive(Clone)]
struct Image {
    sections: Vec<Section>,
    text: Section,
    headers: Vec<HeaderGuard>,
}
struct Candidate {
    recipe: RecipeId,
    anchor: u64,
    root: u64,
    code: Vec<u8>,
}
struct Proof<I> {
    identity: I,
    request: u64,
    geometry: Geometry,
    image: Image,
    recipe: RecipeId,
    anchor: u64,
    anchor_bytes: Vec<u8>,
    root: u64,
    evidence: RootEvidence,
    poisoned: Option<String>,
}

/// Issued after one bounded runtime text pass and stable guards. Gaps remain
/// explicit: a readable-span match does not claim a complete scan. It
/// retains no Lua-state/object pointer or root-slot contents. A failed recheck
/// permanently invalidates this binding; a fresh resolution must use the same
/// remaining request budget. No implicit re-scan or private-heap fallback exists.
pub struct RootBinding {
    proof: Proof<RuntimeModuleIdentity>,
    trusted_image: Option<Arc<RuntimeImageLayout>>,
}
impl RootBinding {
    pub fn root_rva(&self) -> u64 {
        self.proof.root
    }
    pub fn root_address(&self) -> u64 {
        self.proof.geometry.base + self.proof.root
    }
    pub fn identity(&self) -> &RuntimeModuleIdentity {
        &self.proof.identity
    }
    pub fn evidence(&self) -> &RootEvidence {
        &self.proof.evidence
    }
    pub fn executable_sha256(&self) -> Option<&str> {
        self.trusted_image
            .as_ref()
            .map(|layout| layout.executable_sha256())
    }
    /// Checks the same capability, current anchor/RIP, root stability and locating
    /// metadata. This is an anchor recheck, not a new proof of whole-code-domain
    /// uniqueness. The Mailbox reader must also guard its root/path across lookup.
    pub fn revalidate(
        &mut self,
        module: &RuntimeModule,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RootEvidence, RootFailure> {
        if self.proof.poisoned.is_none() {
            if let Some(layout) = &self.trusted_image {
                if let Err(cause) = layout.verify(module, budget) {
                    self.proof.poisoned = Some(cause.clone());
                    return Err(RootFailure {
                        cause,
                        evidence: Box::default(),
                    });
                }
            }
        }
        revalidate(&mut self.proof, module, budget)
    }
}
pub struct RuntimeRootRecipe;
impl RuntimeRootRecipe {
    pub fn resolve(
        module: &RuntimeModule,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RootBinding, RootFailure> {
        resolve(module, budget).map(|proof| RootBinding {
            proof,
            trusted_image: None,
        })
    }
    /// Only the named Lua reader may choose among distinct guarded roots. No
    /// numeric address or caller-authored layout can mint these capabilities.
    pub(crate) fn candidates(
        module: &RuntimeModule,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Vec<RootBinding>, RootFailure> {
        resolve_candidates(module, budget).map(|proofs| {
            proofs
                .into_iter()
                .map(|proof| RootBinding {
                    proof,
                    trusted_image: None,
                })
                .collect()
        })
    }
    /// Formal native-reader issuance. The held opaque layout comes only from
    /// this module's controlled EXE hash/PE verification, and supplies trusted
    /// locating metadata rather than reparsing mutable runtime header bounds.
    pub(crate) fn candidates_with_layout(
        module: &RuntimeModule,
        layout: RuntimeImageLayout,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Vec<RootBinding>, RootFailure> {
        let prepare = (|| {
            layout.verify(module, budget)?;
            let bytes = layout.sections().len() * std::mem::size_of::<Section>()
                + layout.guards().len() * std::mem::size_of::<HeaderGuard>()
                + layout
                    .guards()
                    .iter()
                    .map(|guard| guard.bytes().len())
                    .sum::<usize>()
                + std::mem::size_of::<RuntimeImageLayout>()
                + 2 * std::mem::size_of::<usize>()
                + 16 * std::mem::size_of::<RootBinding>();
            budget.charge(RuntimeCost {
                allocation_bytes: bytes as u64,
                work_units: bytes as u64,
                ..RuntimeCost::default()
            })?;
            let section = |value: &lycheedev_windows::process::RuntimeImageSection| Section {
                start: value.start(),
                end: value.end(),
                flags: value.flags(),
                text: value.is_text(),
            };
            Ok(Image {
                sections: layout.sections().iter().map(section).collect(),
                text: section(layout.text()),
                headers: layout
                    .guards()
                    .iter()
                    .map(|guard| HeaderGuard {
                        rva: guard.rva(),
                        bytes: guard.bytes().to_vec(),
                    })
                    .collect(),
            })
        })();
        let image = prepare.map_err(|cause| RootFailure {
            cause,
            evidence: Box::default(),
        })?;
        let proofs = resolve_candidates_with_image(module, budget, Some(image))?;
        let trusted_image = Arc::new(layout);
        Ok(proofs
            .into_iter()
            .map(|proof| RootBinding {
                proof,
                trusted_image: Some(Arc::clone(&trusted_image)),
            })
            .collect())
    }
}

struct Access<'a, S: Source> {
    source: &'a S,
    budget: &'a mut RuntimeBudget,
    geometry: Geometry,
    evidence: RootEvidence,
}
impl<S: Source> Access<'_, S> {
    fn verify(&mut self) -> Result<()> {
        self.budget.check()?;
        self.source.verify(self.budget)?;
        self.budget.check()
    }
    fn read_into(&mut self, rva: u64, into: &mut [u8], kind: RuntimeReadKind) -> Result<()> {
        self.budget.check()?;
        if into.is_empty()
            || into.len() > CHUNK
            || rva > self.geometry.size
            || into.len() as u64 > self.geometry.size - rva
        {
            return Err("root.read_range".into());
        }
        if self.evidence.module_read_calls >= READ_LIMIT
            || into.len() as u64 > TOTAL_BYTES - self.evidence.module_read_requested_bytes
        {
            return Err("root.read_budget".into());
        }
        self.evidence.module_read_calls += 1;
        self.evidence.module_read_requested_bytes += into.len() as u64;
        if let Err(failure) = self.source.read_diagnostic(rva, into, kind, self.budget) {
            if self.evidence.first_read_failure.is_none() {
                self.evidence.first_read_failure = Some(crate::mailbox::lookup::ReadFailure {
                    path: match kind {
                        RuntimeReadKind::Code => "root.recipe",
                        RuntimeReadKind::WritableData => "root.state",
                        RuntimeReadKind::Metadata => "root.metadata",
                    }
                    .into(),
                    requested_bytes: into.len(),
                    returned_bytes: failure.returned_bytes,
                    source: "module",
                    reason: failure.reason,
                    guard_recheck: "not_run",
                    io_attempted: failure.io_attempted,
                    native_error_code: failure.native_error_code,
                    region: failure.region,
                });
            }
            return Err(failure.cause);
        }
        self.evidence.module_read_bytes += into.len() as u64;
        self.budget.check()
    }
    fn read(&mut self, rva: u64, size: usize, kind: RuntimeReadKind) -> Result<Vec<u8>> {
        if size > CHUNK {
            return Err("root.read_range".into());
        }
        self.budget.charge(RuntimeCost::allocation(size + 256))?;
        let mut out = vec![0; size];
        self.read_into(rva, &mut out, kind)?;
        Ok(out)
    }
    fn header(&mut self, rva: u64, size: usize) -> Result<Vec<u8>> {
        if rva > HEADER_LIMIT
            || size as u64 > HEADER_LIMIT - rva
            || size as u64 > HEADER_LIMIT - self.evidence.header_bytes
        {
            return Err("root.header_budget".into());
        }
        self.evidence.header_bytes += size as u64;
        self.read(rva, size, RuntimeReadKind::Metadata)
    }
    fn image(&mut self) -> Result<Image> {
        // Only locating fields are decoded. Architecture and image bounds come
        // from the OS capability, not runtime Machine/SizeOfImage/COFF/raw offsets.
        self.budget.charge(RuntimeCost::allocation(
            7 * std::mem::size_of::<HeaderGuard>(),
        ))?;
        let mut headers = Vec::with_capacity(7);
        let mut field = |this: &mut Self, rva, len| -> Result<Vec<u8>> {
            let bytes = this.header(rva, len)?;
            this.budget.charge(RuntimeCost::allocation(bytes.len()))?;
            headers.push(HeaderGuard {
                rva,
                bytes: bytes.clone(),
            });
            Ok(bytes)
        };
        if field(self, 0, 2)? != b"MZ" {
            return Err("root.dos_header".into());
        }
        let nt = u32::from_le_bytes(field(self, 0x3c, 4)?.try_into().unwrap()) as u64;
        if field(self, nt, 4)? != b"PE\0\0" {
            return Err("root.nt_signature".into());
        }
        let count = u16::from_le_bytes(field(self, nt + 6, 2)?.try_into().unwrap()) as usize;
        let optional = u16::from_le_bytes(field(self, nt + 20, 2)?.try_into().unwrap()) as u64;
        if count == 0 || count > 96 || optional < 112 {
            return Err("root.image_layout".into());
        }
        if u16::from_le_bytes(field(self, nt + 24, 2)?.try_into().unwrap()) != 0x20b {
            return Err("root.optional_magic".into());
        }
        let table = nt + 24 + optional;
        let bytes = field(self, table, count * 40)?;
        self.budget.charge(RuntimeCost::allocation(
            count * std::mem::size_of::<Section>(),
        ))?;
        let mut sections: Vec<Section> = Vec::with_capacity(count);
        let mut text = None;
        for raw in bytes.as_chunks::<40>().0 {
            self.budget
                .charge(RuntimeCost::work(sections.len() as u64 + 1))?;
            let length = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as u64;
            let start = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as u64;
            let flags = u32::from_le_bytes(raw[36..40].try_into().unwrap());
            if length == 0 || start > self.geometry.size || length > self.geometry.size - start {
                return Err("root.section_range".into());
            }
            let section = Section {
                start,
                end: start + length,
                flags,
                text: raw[..8] == *b".text\0\0\0",
            };
            if sections
                .iter()
                .any(|s| start < s.end && s.start < section.end)
            {
                return Err("root.section_overlap".into());
            }
            if section.text && text.replace(section).is_some() {
                return Err("root.text_ambiguous".into());
            }
            sections.push(section);
        }
        let text = text.ok_or("root.text_missing")?;
        if text.flags & (READ | EXECUTE) != READ | EXECUTE || text.end - text.start > TEXT_LIMIT {
            return Err("root.text_section".into());
        }
        Ok(Image {
            sections,
            text,
            headers,
        })
    }
    fn pattern(&mut self, id: RecipeId) -> Result<Vec<Option<u8>>> {
        self.budget
            .charge(RuntimeCost::work(2 * id.text().len() as u64))?;
        let length = id.text().split_whitespace().count();
        if !(8..=256).contains(&length) {
            return Err("root.recipe_pattern".into());
        }
        self.budget.charge(RuntimeCost::allocation(
            length * std::mem::size_of::<Option<u8>>(),
        ))?;
        id.text()
            .split_whitespace()
            .map(|s| {
                if s == "??" {
                    Ok(None)
                } else {
                    u8::from_str_radix(s, 16)
                        .map(Some)
                        .map_err(|_| "root.recipe_pattern".into())
                }
            })
            .collect()
    }
    fn gap(&mut self, start: u64, end: u64, reason: &'static str) -> Result<()> {
        if start >= end {
            return Ok(());
        }
        if let Some(last) = self.evidence.gaps.scanner().last_mut() {
            if last.end == start && last.reason == reason {
                last.end = end;
                return Ok(());
            }
        }
        // Every gap follows one source span or a bounded read attempt.
        if self.evidence.gaps.len() >= MAX_GAPS {
            return Err("root.gap_budget".into());
        }
        self.evidence
            .gaps
            .scanner()
            .push(RootGap { start, end, reason });
        Ok(())
    }
    fn scan(&mut self, image: &Image) -> Result<Vec<Candidate>> {
        self.evidence.text_bytes = image.text.end - image.text.start;
        // Reserve error-path evidence before reading. Recording a fatal gap
        // must never replace the original source fault with a late cancellation
        // or allocation-budget error.
        self.budget.charge(RuntimeCost::allocation(
            MAX_GAPS * std::mem::size_of::<RootGap>()
                + std::mem::size_of::<Vec<RootGap>>()
                + 2 * std::mem::size_of::<usize>(),
        ))?;
        self.evidence.gaps = RootGaps(Some(Arc::new(Vec::with_capacity(MAX_GAPS))));
        let recipes = [
            (RecipeId::Retail, self.pattern(RecipeId::Retail)?),
            (RecipeId::Forever, self.pattern(RecipeId::Forever)?),
            (
                RecipeId::ForeverContext,
                self.pattern(RecipeId::ForeverContext)?,
            ),
        ];
        let longest = recipes.iter().map(|(_, p)| p.len()).max().unwrap();
        self.budget.charge(RuntimeCost::allocation(
            CHUNK + longest - 1 + 16 * std::mem::size_of::<Candidate>(),
        ))?;
        let mut buffer = vec![0; CHUNK + longest - 1];
        let mut candidates = Vec::with_capacity(16);
        let spans = self
            .source
            .readable_spans(image.text.start, image.text.end, self.budget)?;
        let mut tail = 0;
        let mut tail_end = 0;
        let mut cursor = image.text.start;
        let mut comparisons = 0u32;
        for (start, end) in spans {
            if start < cursor || start >= end || start < image.text.start || end > image.text.end {
                return Err("root.readable_spans".into());
            }
            if cursor < start {
                self.gap(cursor, start, "unreadable_mapping")?;
                tail = 0;
            }
            let mut at = start;
            while at < end {
                let limit = (at + CHUNK as u64).min(end);
                let count = match self.scan_read(
                    image,
                    &recipes,
                    at,
                    limit,
                    &mut buffer,
                    &mut tail,
                    &mut tail_end,
                    &mut comparisons,
                    &mut candidates,
                ) {
                    Ok(n) => n,
                    Err(e) => {
                        self.gap(at, image.text.end, "scan_stopped")?;
                        return Err(e);
                    }
                };
                let mut retry = at + count as u64;
                while retry < limit {
                    let next = ((retry / 4096 + 1) * 4096).min(limit);
                    if count == 0 && retry == at && next == limit {
                        self.gap(retry, next, "module_read_failed")?;
                        tail = 0;
                        break;
                    }
                    let got = match self.scan_read(
                        image,
                        &recipes,
                        retry,
                        next,
                        &mut buffer,
                        &mut tail,
                        &mut tail_end,
                        &mut comparisons,
                        &mut candidates,
                    ) {
                        Ok(n) => n,
                        Err(e) => {
                            self.gap(retry, image.text.end, "scan_stopped")?;
                            return Err(e);
                        }
                    };
                    if retry + (got as u64) < next {
                        self.gap(retry + got as u64, next, "module_read_failed")?;
                        tail = 0;
                    }
                    retry = next;
                }
                at = limit;
            }
            cursor = end;
        }
        self.gap(cursor, image.text.end, "unreadable_mapping")?;
        self.budget.check()?;
        self.evidence.full_text_pass = self.evidence.gaps.is_empty()
            && self.evidence.scanned_bytes == self.evidence.text_bytes;
        Ok(candidates)
    }
    #[allow(clippy::too_many_arguments)]
    fn scan_read(
        &mut self,
        image: &Image,
        recipes: &[(RecipeId, Vec<Option<u8>>)],
        at: u64,
        end: u64,
        buffer: &mut [u8],
        tail: &mut usize,
        tail_end: &mut u64,
        comparisons: &mut u32,
        candidates: &mut Vec<Candidate>,
    ) -> Result<usize> {
        self.budget.check()?;
        let requested = usize::try_from(end - at).map_err(|_| "root.read_range")?;
        if requested == 0 || requested > CHUNK || end > self.geometry.size {
            return Err("root.read_range".into());
        }
        if self.evidence.module_read_calls >= READ_LIMIT
            || requested as u64 > TOTAL_BYTES - self.evidence.module_read_requested_bytes
        {
            return Err("root.read_budget".into());
        }
        if at != *tail_end {
            *tail = 0;
        }
        self.evidence.module_read_calls += 1;
        self.evidence.module_read_requested_bytes += requested as u64;
        let read = self
            .source
            .read_some(at, &mut buffer[*tail..*tail + requested], self.budget);
        let (received, failed) = match read {
            Ok(value) => value,
            Err(e) => {
                self.evidence.read_failures += 1;
                return Err(e);
            }
        };
        if received > requested {
            return Err("root.invalid_read_count".into());
        }
        self.evidence.module_read_bytes += received as u64;
        if failed || received != requested {
            self.evidence.read_failures += 1;
        }
        self.budget.check()?;
        if received == 0 {
            return Ok(0);
        }
        self.evidence.scanned_bytes += received as u64;
        let combined = *tail + received;
        let start = at - *tail as u64;
        for index in 0..combined {
            if *comparisons == 0 {
                self.budget.charge(RuntimeCost::work(4096))?;
                *comparisons = 4096;
            }
            *comparisons -= 1;
            if buffer[index] != 0x48 {
                continue;
            }
            for (recipe, pattern) in recipes {
                if index + pattern.len() > combined || start + (index + pattern.len()) as u64 <= at
                {
                    continue;
                }
                self.budget
                    .charge(RuntimeCost::work(pattern.len() as u64))?;
                if !pattern
                    .iter()
                    .zip(&buffer[index..])
                    .all(|(expected, byte)| expected.is_none_or(|value| value == *byte))
                {
                    continue;
                }
                self.evidence.pattern_hits += 1;
                if self.evidence.pattern_hits > 4096 {
                    return Err("root.pattern_match_budget".into());
                }
                let anchor = start + index as u64;
                self.budget.charge(RuntimeCost::work(
                    (recipe.operands().len() * image.sections.len()) as u64,
                ))?;
                let root = match target(
                    anchor,
                    &buffer[index..index + pattern.len()],
                    self.geometry.size,
                    &image.sections,
                    *recipe,
                ) {
                    Ok(root) => root,
                    Err(_) => {
                        self.evidence.rejected_matches += 1;
                        continue;
                    }
                };
                if candidates.iter().any(|candidate| candidate.root == root) {
                    continue;
                }
                if candidates.len() >= 16 {
                    return Err("root.root_candidate_budget".into());
                }
                self.budget.charge(RuntimeCost::allocation(pattern.len()))?;
                candidates.push(Candidate {
                    recipe: *recipe,
                    anchor,
                    root,
                    code: buffer[index..index + pattern.len()].to_vec(),
                });
                self.evidence.root_candidates = candidates.len() as u64;
            }
        }
        let keep = (recipes.iter().map(|(_, p)| p.len()).max().unwrap() - 1).min(combined);
        buffer.copy_within(combined - keep..combined, 0);
        *tail = keep;
        *tail_end = at + received as u64;
        Ok(received)
    }
    fn stable(&mut self, image: &Image, anchor: u64, code: &[u8], root: u64) -> Result<()> {
        let before = self.read(root, 8, RuntimeReadKind::WritableData)?;
        let current = self.read(anchor, code.len(), RuntimeReadKind::Code)?;
        self.budget.charge(RuntimeCost::work(code.len() as u64))?;
        if current != code {
            return Err("root.anchor_changed".into());
        }
        for guard in &image.headers {
            let current = self.header(guard.rva, guard.bytes.len())?;
            self.budget
                .charge(RuntimeCost::work(current.len() as u64))?;
            if current != guard.bytes {
                return Err("root.header_changed".into());
            }
        }
        let after = self.read(root, 8, RuntimeReadKind::WritableData)?;
        self.budget.charge(RuntimeCost::work(8))?;
        if before != after {
            return Err("root.slot_changed".into());
        }
        self.verify()?;
        self.evidence.stable_guards = true;
        Ok(())
    }
}
fn target(
    anchor: u64,
    code: &[u8],
    size: u64,
    sections: &[Section],
    recipe: RecipeId,
) -> Result<u64> {
    if anchor > size || code.len() as u64 > size - anchor {
        return Err("root.anchor_range".into());
    }
    let mut root = None;
    let mut repeated = None;
    for operand in recipe.operands() {
        let start = operand.offset + operand.displacement;
        let displacement = i32::from_le_bytes(
            code.get(start..start + 4)
                .ok_or("root.operand_range")?
                .try_into()
                .unwrap(),
        ) as i64;
        let end = anchor
            .checked_add((operand.offset + operand.length) as u64)
            .ok_or("root.rip_overflow")?;
        let target = end
            .checked_add_signed(displacement)
            .ok_or("root.rip_overflow")?;
        if target > size || operand.width > size - target {
            return Err("root.rip_target_range".into());
        }
        if operand.offset == 0 {
            if target % 8 != 0 {
                return Err("root.alignment".into());
            }
            root = Some(target);
        }
        if !sections.iter().any(|section| {
            target >= section.start
                && target + operand.width <= section.end
                && section.flags & READ != 0
                && if operand.call {
                    section.text && section.flags & EXECUTE != 0
                } else {
                    section.flags & WRITE != 0 && section.flags & EXECUTE == 0
                }
        }) {
            return Err("root.rip_target_section".into());
        }
        if operand.same_target != 0 {
            if repeated.is_some_and(|previous| previous != target) {
                return Err("root.call_target_mismatch".into());
            }
            repeated = Some(target);
        }
    }
    root.ok_or_else(|| "root.output_missing".into())
}
fn geometry(geometry: Geometry) -> Result<()> {
    if geometry.architecture != 0x8664
        || geometry.base == 0
        || geometry.size == 0
        || geometry.size > 512 << 20
        || geometry.base.checked_add(geometry.size).is_none()
    {
        return Err("root.module_identity".into());
    }
    Ok(())
}
fn resolve<S: Source>(
    source: &S,
    budget: &mut RuntimeBudget,
) -> std::result::Result<Proof<S::Identity>, RootFailure> {
    let mut candidates = resolve_candidates(source, budget)?;
    if candidates.len() != 1 {
        return Err(RootFailure {
            cause: "root.ambiguous_root".into(),
            evidence: Box::new(
                candidates
                    .first()
                    .map(|p| p.evidence.clone())
                    .unwrap_or_default(),
            ),
        });
    }
    Ok(candidates.remove(0))
}
fn resolve_candidates<S: Source>(
    source: &S,
    budget: &mut RuntimeBudget,
) -> std::result::Result<Vec<Proof<S::Identity>>, RootFailure> {
    resolve_candidates_with_image(source, budget, None)
}
fn resolve_candidates_with_image<S: Source>(
    source: &S,
    budget: &mut RuntimeBudget,
    image: Option<Image>,
) -> std::result::Result<Vec<Proof<S::Identity>>, RootFailure> {
    let mut access = Access {
        source,
        geometry: source.geometry(),
        budget,
        evidence: RootEvidence::default(),
    };
    let result = (|| {
        access.verify()?;
        geometry(access.geometry)?;
        let identity = source.identity();
        let image = match image {
            Some(image) => image,
            None => access.image()?,
        };
        let candidates = access.scan(&image)?;
        if candidates.is_empty() {
            return Err("root.anchor_missing".into());
        }
        access.budget.charge(RuntimeCost::allocation(
            candidates.len() * std::mem::size_of::<Proof<S::Identity>>(),
        ))?;
        let mut proofs = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let Candidate {
                recipe,
                anchor,
                root,
                code,
            } = candidate;
            access.evidence.recipe = Some(recipe);
            access.evidence.anchor_rva = Some(anchor);
            access.evidence.root_rva = Some(root);
            access.evidence.stable_guards = false;
            access.stable(&image, anchor, &code, root)?;
            if identity != source.identity() || access.geometry != source.geometry() {
                return Err("root.module_changed".into());
            }
            let clone_bytes = image.sections.len() * std::mem::size_of::<Section>()
                + image.headers.len() * std::mem::size_of::<HeaderGuard>()
                + image.headers.iter().map(|h| h.bytes.len()).sum::<usize>()
                + 256;
            access.budget.charge(RuntimeCost::allocation(clone_bytes))?;
            access
                .budget
                .charge(RuntimeCost::work(code.len() as u64 + clone_bytes as u64))?;
            access.evidence.anchor_sha256 = Some(hex::encode(Sha256::digest(&code)));
            proofs.push(Proof {
                identity: identity.clone(),
                request: access.budget.request_id(),
                geometry: access.geometry,
                image: image.clone(),
                recipe,
                anchor,
                anchor_bytes: code,
                root,
                evidence: access.evidence.clone(),
                poisoned: None,
            });
        }
        // Every candidate reports the whole scan and all candidate guard work.
        for proof in &mut proofs {
            proof.evidence.module_read_calls = access.evidence.module_read_calls;
            proof.evidence.module_read_bytes = access.evidence.module_read_bytes;
            proof.evidence.module_read_requested_bytes =
                access.evidence.module_read_requested_bytes;
            proof.evidence.header_bytes = access.evidence.header_bytes;
        }
        Ok(proofs)
    })();
    result.map_err(|cause| RootFailure {
        cause,
        evidence: Box::new(access.evidence),
    })
}
fn revalidate<S: Source>(
    proof: &mut Proof<S::Identity>,
    source: &S,
    budget: &mut RuntimeBudget,
) -> std::result::Result<RootEvidence, RootFailure> {
    let mut access = Access {
        source,
        geometry: source.geometry(),
        budget,
        evidence: RootEvidence::default(),
    };
    let result = (|| {
        if let Some(cause) = &proof.poisoned {
            return Err(cause.clone());
        }
        if proof.request != access.budget.request_id() {
            return Err("root.budget_replaced".into());
        }
        if proof.identity != source.identity() || proof.geometry != source.geometry() {
            return Err("root.module_changed".into());
        }
        access.verify()?;
        let current = access.read(
            proof.anchor,
            proof.anchor_bytes.len(),
            RuntimeReadKind::Code,
        )?;
        access.budget.charge(RuntimeCost::work(
            current.len() as u64
                + (proof.recipe.operands().len() * proof.image.sections.len()) as u64,
        ))?;
        if current != proof.anchor_bytes {
            return Err("root.anchor_changed".into());
        }
        let root = target(
            proof.anchor,
            &current,
            proof.geometry.size,
            &proof.image.sections,
            proof.recipe,
        )?;
        if root != proof.root {
            return Err("root.target_changed".into());
        }
        access.evidence.recipe = Some(proof.recipe);
        access.evidence.anchor_rva = Some(proof.anchor);
        access.evidence.root_rva = Some(root);
        access.stable(&proof.image, proof.anchor, &current, root)?;
        if proof.identity != source.identity() || proof.geometry != source.geometry() {
            return Err("root.module_changed".into());
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(access.evidence),
        Err(cause) => {
            let cause = if cause == "root.slot_changed" {
                "mailbox.path_changed".to_string()
            } else {
                cause
            };
            if cause != "mailbox.path_changed" {
                proof.poisoned.get_or_insert_with(|| cause.clone());
            }
            Err(RootFailure {
                cause,
                evidence: Box::new(access.evidence),
            })
        }
    }
}

#[cfg(test)]
mod tests;
