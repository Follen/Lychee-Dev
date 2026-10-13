//! Runtime instruction-pattern/RIP root binding. The public entrypoint accepts
//! only the OS-issued RuntimeModule capability, never a caller-supplied RVA,
//! disk executable, archived scan or untrusted finalized-method flag.
//!
//! A binding proves a bounded observation of a root slot, not a Lua ABI, record
//! authenticity, atomic process snapshot or permission to send input. The Lua
//! reader must independently validate its profile and every current object path.
use lycheedev_windows::process::{
    RuntimeBudget, RuntimeCost, RuntimeModule, RuntimeModuleIdentity, RuntimeReadKind,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const HEADER_LIMIT: u64 = 1 << 20;
const TEXT_LIMIT: u64 = 128 << 20;
const CHUNK: usize = 1 << 20;
const READ_LIMIT: u64 = 2048;
const TOTAL_BYTES: u64 = 2 * HEADER_LIMIT + TEXT_LIMIT + 4096;
const READ: u32 = 0x40000000;
const WRITE: u32 = 0x80000000;
const EXECUTE: u32 = 0x20000000;

// Go v3.1.0 (9edc06b4591264eaac68e34575bc02f750ffe758), runtime-code recipes.
// Historical exact-hash/known-anchor shortcuts deliberately do not enter here.
const RETAIL_PATTERN: &str = "48 8b 3d ?? ?? ?? ?? 48 8b 1d ?? ?? ?? ?? 48 8b cf 44 8b 05 ?? ?? ?? ?? 48 8b d3 e8 ?? ?? ?? ?? 8b 05 ?? ?? ?? ?? 83 c0 01 89 05 ?? ?? ?? ?? 74 ?? 83 3d ?? ?? ?? ?? 00 75 ??";
const FOREVER_PATTERN: &str = "48 8b 1d ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ?? 48 8b cb 4c 8b 38 e8 ?? ?? ?? ?? 49 8b d7 48 8b cb 44 8b 40 08 e8 ?? ?? ?? ?? 48 8b cb e8 ?? ?? ?? ??";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum RecipeId {
    #[serde(rename = "retail-lua-state-root-rip-v1")]
    Retail,
    #[serde(rename = "forever-lua-state-root-runscript-rip-v1")]
    Forever,
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
        }
    }
    fn operands(self) -> &'static [Operand] {
        match self {
            Self::Retail => RETAIL_OPERANDS,
            Self::Forever => FOREVER_OPERANDS,
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
    pub anchor_rva: Option<u64>,
    pub root_rva: Option<u64>,
    pub anchor_sha256: Option<String>,
    pub header_bytes: u64,
    /// Successfully acquired code bytes; only full_text_pass establishes that
    /// comparisons exhausted the entire domain. A failed block is not atomic.
    pub scanned_bytes: u64,
    pub module_read_calls: u64,
    pub module_read_bytes: u64,
    pub full_text_pass: bool,
    pub stable_guards: bool,
    /// Always false: neither code nor the heap is an atomic OS snapshot.
    pub atomic: bool,
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
}
#[derive(Clone, Copy, Debug)]
struct Section {
    start: u64,
    end: u64,
    flags: u32,
    text: bool,
}
struct HeaderGuard {
    rva: u64,
    bytes: Vec<u8>,
}
struct Image {
    sections: Vec<Section>,
    text: Section,
    headers: Vec<HeaderGuard>,
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

/// Issued only after a complete first runtime text pass and stable guards. It
/// retains no Lua-state/object pointer or root-slot contents. A failed recheck
/// permanently invalidates this binding; a fresh resolution must use the same
/// remaining request budget. No implicit re-scan or private-heap fallback exists.
pub struct RootBinding {
    proof: Proof<RuntimeModuleIdentity>,
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
    /// Checks the same capability, current anchor/RIP, root stability and locating
    /// metadata. This is an anchor recheck, not a new proof of whole-code-domain
    /// uniqueness. The Mailbox reader must also guard its root/path across lookup.
    pub fn revalidate(
        &mut self,
        module: &RuntimeModule,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RootEvidence, RootFailure> {
        revalidate(&mut self.proof, module, budget)
    }
}
pub struct RuntimeRootRecipe;
impl RuntimeRootRecipe {
    pub fn resolve(
        module: &RuntimeModule,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RootBinding, RootFailure> {
        resolve(module, budget).map(|proof| RootBinding { proof })
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
            || into.len() as u64 > TOTAL_BYTES - self.evidence.module_read_bytes
        {
            return Err("root.read_budget".into());
        }
        self.evidence.module_read_calls += 1;
        self.evidence.module_read_bytes += into.len() as u64;
        self.source.read(rva, into, kind, self.budget)?;
        self.budget.check()
    }
    fn read(&mut self, rva: u64, size: usize, kind: RuntimeReadKind) -> Result<Vec<u8>> {
        if size > CHUNK {
            return Err("root.read_range".into());
        }
        self.budget.charge(RuntimeCost::allocation(size))?;
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
    fn scan(&mut self, image: &Image) -> Result<(RecipeId, u64, Vec<u8>)> {
        let recipes = [
            (RecipeId::Retail, self.pattern(RecipeId::Retail)?),
            (RecipeId::Forever, self.pattern(RecipeId::Forever)?),
        ];
        let longest = recipes.iter().map(|(_, p)| p.len()).max().unwrap();
        self.budget
            .charge(RuntimeCost::allocation(CHUNK + longest - 1))?;
        let mut buffer = vec![0; CHUNK + longest - 1];
        let mut selected = None;
        let mut tail = 0;
        let mut rva = image.text.start;
        let mut comparisons = 0u32;
        while rva < image.text.end {
            let size = (image.text.end - rva).min(CHUNK as u64) as usize;
            self.read_into(rva, &mut buffer[tail..tail + size], RuntimeReadKind::Code)?;
            self.evidence.scanned_bytes += size as u64;
            let combined = tail + size;
            for at in 0..combined {
                for (recipe, pattern) in &recipes {
                    if at + pattern.len() > combined
                        || rva > image.text.start && at + pattern.len() <= tail
                    {
                        continue;
                    }
                    let mut matches = true;
                    for (expected, value) in pattern.iter().zip(&buffer[at..]) {
                        // Reserve a finite batch before comparisons. Unused work
                        // reservations remain charged, and control is checked at
                        // most 4096 byte comparisons apart.
                        if comparisons == 0 {
                            self.budget.charge(RuntimeCost::work(4096))?;
                            comparisons = 4096;
                        }
                        comparisons -= 1;
                        if expected.is_some_and(|v| v != *value) {
                            matches = false;
                            break;
                        }
                    }
                    if !matches {
                        continue;
                    }
                    self.evidence.pattern_hits += 1;
                    // Raw hits compete before any RIP filtering, across both
                    // templates. An invalid second hit still proves ambiguity.
                    if self.evidence.pattern_hits > 1 {
                        return Err("root.ambiguous_anchor".into());
                    }
                    let anchor = rva - tail as u64 + at as u64;
                    self.evidence.recipe = Some(*recipe);
                    self.evidence.anchor_rva = Some(anchor);
                    self.budget.charge(RuntimeCost::allocation(pattern.len()))?;
                    selected = Some((*recipe, anchor, buffer[at..at + pattern.len()].to_vec()));
                }
            }
            tail = (longest - 1).min(combined);
            buffer.copy_within(combined - tail..combined, 0);
            rva += size as u64;
        }
        self.budget.check()?;
        self.evidence.full_text_pass = true;
        selected.ok_or_else(|| "root.anchor_missing".into())
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
        let image = access.image()?;
        let (recipe, anchor, code) = access.scan(&image)?;
        access.budget.charge(RuntimeCost::work(
            (recipe.operands().len() * image.sections.len()) as u64,
        ))?;
        let root = target(anchor, &code, access.geometry.size, &image.sections, recipe)?;
        access.evidence.root_rva = Some(root);
        access.stable(&image, anchor, &code, root)?;
        if identity != source.identity() || access.geometry != source.geometry() {
            return Err("root.module_changed".into());
        }
        access.budget.charge(RuntimeCost::allocation(256))?;
        access.budget.charge(RuntimeCost::work(code.len() as u64))?;
        access.evidence.anchor_sha256 = Some(hex::encode(Sha256::digest(&code)));
        Ok(Proof {
            identity,
            request: access.budget.request_id(),
            geometry: access.geometry,
            image,
            recipe,
            anchor,
            anchor_bytes: code,
            root,
            evidence: access.evidence.clone(),
            poisoned: None,
        })
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
            proof.poisoned.get_or_insert_with(|| cause.clone());
            Err(RootFailure {
                cause,
                evidence: Box::new(access.evidence),
            })
        }
    }
}

#[cfg(test)]
mod tests;
