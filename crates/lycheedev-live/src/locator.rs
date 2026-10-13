//! Finite disk-only native location rules. No runtime scan and no numeric RVA fast path.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub type Result<T> = std::result::Result<T, String>;
pub const ENGINE: &str = "lycheedev.static-locator.v1";
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum OutputType {
    ModuleRva,
    PointerSlot,
    FieldDisplacement,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub build: String,
    pub sha256: String,
    pub value: u64,
    pub evidence_sha256: String,
    pub evidence_kind: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operand {
    pub offset: usize,
    pub instruction_end: usize,
    pub target: TargetSection,
    pub output: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetSection {
    Executable,
    WritableData,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Method {
    pub id: String,
    pub version: u32,
    pub output: OutputType,
    pub section: String,
    pub pattern: String,
    pub operands: Vec<Operand>,
    pub source: Source,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    pub engine: String,
    pub method_digest: String,
    pub input_sha256: String,
    pub source_provenance: Source,
    pub source_replay_required: bool,
    pub semantic_proof: bool,
    pub complete: bool,
    pub scanned_bytes: u64,
    pub candidates: Vec<Candidate>,
    pub result: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub anchor: u64,
    pub value: u64,
}
/// Constructed only by an actual complete disk finalization; imported flags cannot issue this.
#[derive(Debug, Clone)]
pub struct FinalizedMethod {
    method: Method,
    digest: String,
}
impl FinalizedMethod {
    pub(crate) fn pattern_length(&self) -> usize {
        self.method.pattern.split_whitespace().count()
    }
}
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub rva: u64,
    pub virtual_size: u64,
    pub file_offset: u64,
    pub raw_size: u64,
    pub flags: u32,
}
pub struct Pe<'a> {
    pub bytes: &'a [u8],
    pub sha256: String,
    pub image_size: u64,
    pub sections: Vec<Section>,
}
fn u16at(b: &[u8], i: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(i..i + 2).ok_or("pe.truncated")?.try_into().unwrap(),
    ))
}
fn u32at(b: &[u8], i: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(i..i + 4).ok_or("pe.truncated")?.try_into().unwrap(),
    ))
}
impl<'a> Pe<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 512 << 20 || bytes.get(..2) != Some(b"MZ") {
            return Err("pe.invalid_dos".into());
        }
        let nt = u32at(bytes, 0x3c)? as usize;
        if bytes.get(nt..nt + 4) != Some(b"PE\0\0") || u16at(bytes, nt + 4)? != 0x8664 {
            return Err("pe.not_amd64".into());
        }
        let count = u16at(bytes, nt + 6)? as usize;
        let opt = u16at(bytes, nt + 20)? as usize;
        if count == 0 || count > 96 || opt < 112 || u16at(bytes, nt + 24)? != 0x20b {
            return Err("pe.header".into());
        }
        let image_size = u32at(bytes, nt + 24 + 56)? as u64;
        if image_size == 0 || image_size > 512 << 20 {
            return Err("pe.image_size".into());
        }
        let mut sections = Vec::new();
        for i in 0..count {
            let at = nt + 24 + opt + i * 40;
            let h = bytes.get(at..at + 40).ok_or("pe.section_table")?;
            let name = String::from_utf8_lossy(&h[..8])
                .trim_end_matches('\0')
                .to_owned();
            let s = Section {
                name,
                rva: u32at(h, 12)? as u64,
                virtual_size: u32at(h, 8)? as u64,
                file_offset: u32at(h, 20)? as u64,
                raw_size: u32at(h, 16)? as u64,
                flags: u32at(h, 36)?,
            };
            if sections.iter().any(|p: &Section| p.name == s.name) {
                return Err("pe.section_name_ambiguous".into());
            }
            if s.rva
                .checked_add(s.virtual_size)
                .is_none_or(|v| v > image_size)
                || s.file_offset
                    .checked_add(s.raw_size)
                    .is_none_or(|v| v > bytes.len() as u64)
            {
                return Err("pe.section_bounds".into());
            }
            if sections
                .iter()
                .any(|p: &Section| s.rva < p.rva + p.virtual_size && p.rva < s.rva + s.virtual_size)
            {
                return Err("pe.section_overlap".into());
            }
            sections.push(s);
        }
        Ok(Self {
            bytes,
            sha256: hex::encode(Sha256::digest(bytes)),
            image_size,
            sections,
        })
    }
    fn target(&self, value: u64, kind: &TargetSection) -> bool {
        self.sections.iter().any(|s| {
            value >= s.rva
                && value
                    .checked_add(8)
                    .is_some_and(|end| end <= s.rva + s.virtual_size)
                && match kind {
                    TargetSection::Executable => s.flags & 0x20000000 != 0,
                    TargetSection::WritableData => {
                        s.flags & 0x80000000 != 0 && s.flags & 0x20000000 == 0
                    }
                }
        })
    }
}
fn pattern(text: &str) -> Result<Vec<Option<u8>>> {
    let p: Vec<_> = text
        .split_whitespace()
        .map(|s| {
            if s == "??" {
                Ok(None)
            } else {
                u8::from_str_radix(s, 16)
                    .map(Some)
                    .map_err(|_| "locator.pattern".to_string())
            }
        })
        .collect::<Result<_>>()?;
    if p.len() < 8 || p.len() > 256 || p.iter().filter(|v| v.is_some()).count() < 4 {
        return Err("locator.pattern_bounds".into());
    }
    Ok(p)
}
fn scan(pe: &Pe, method: &Method) -> Result<History> {
    let section = pe
        .sections
        .iter()
        .find(|s| s.name == method.section)
        .ok_or("locator.section_missing")?;
    if section.virtual_size > 128 << 20 || section.raw_size < section.virtual_size {
        return Err("TARGET_COVERAGE_INCOMPLETE".into());
    }
    let data = &pe.bytes
        [section.file_offset as usize..(section.file_offset + section.virtual_size) as usize];
    scan_domain(pe, method, section.rva, data, &pe.sha256)
}
/// The runtime archive seam supplies its independently authenticated, complete domain.
/// This scanner issues history only, never a disk finalization or live capability.
pub(crate) fn scan_domain(
    pe: &Pe,
    method: &Method,
    rva: u64,
    data: &[u8],
    input_digest: &str,
) -> Result<History> {
    if method.output == OutputType::FieldDisplacement {
        return Err("locator.unsupported_field_extraction".into());
    }
    let section = pe
        .sections
        .iter()
        .find(|s| s.name == method.section)
        .ok_or("locator.section_missing")?;
    if section.rva != rva || section.virtual_size != data.len() as u64 || data.len() > 128 << 20 {
        return Err("TARGET_COVERAGE_INCOMPLETE".into());
    }
    let digest = hex::encode(Sha256::digest(
        serde_json::to_vec(method).map_err(|e| e.to_string())?,
    ));
    let p = pattern(&method.pattern)?;
    if data.len() < p.len() {
        return Err("TARGET_METHOD_NO_MATCH".into());
    }
    let mut candidates = Vec::new();
    for at in 0..=data.len() - p.len() {
        let code = &data[at..at + p.len()];
        if !p.iter().zip(code).all(|(p, b)| p.is_none_or(|v| v == *b)) {
            continue;
        }
        let anchor = rva + at as u64;
        let mut value = None;
        let mut valid = true;
        for o in &method.operands {
            if o.offset + 4 > code.len() || o.instruction_end > code.len() {
                return Err("locator.operand_bounds".into());
            }
            let disp = i32::from_le_bytes(code[o.offset..o.offset + 4].try_into().unwrap()) as i64;
            let Some(target) = (anchor + o.instruction_end as u64).checked_add_signed(disp) else {
                valid = false;
                break;
            };
            if !pe.target(target, &o.target) {
                valid = false;
                break;
            }
            if o.output && value.replace(target).is_some() {
                return Err("locator.multiple_outputs".into());
            }
        }
        if valid {
            let value = value.ok_or("locator.output_missing")?;
            if method.output == OutputType::PointerSlot && value % 8 != 0 {
                continue;
            }
            candidates.push(Candidate { anchor, value });
            if candidates.len() > 65536 {
                return Err("TARGET_SEARCH_TRUNCATED".into());
            }
        }
    }
    let result = if candidates.len() == 1 {
        Some(candidates[0].value)
    } else {
        None
    };
    Ok(History {
        engine: ENGINE.into(),
        method_digest: digest,
        input_sha256: input_digest.to_owned(),
        source_provenance: method.source.clone(),
        source_replay_required: false,
        semantic_proof: false,
        complete: true,
        scanned_bytes: data.len() as u64,
        candidates,
        result,
    })
}
/// Discovery retains complete competition even when no unique candidate exists.
/// It does not issue finalization or process admission.
pub fn inspect_disk(pe: &Pe, method: &Method) -> Result<History> {
    scan(pe, method)
}
pub fn finalize(pe: &Pe, method: Method, evidence: &[u8]) -> Result<(FinalizedMethod, History)> {
    if method.source.sha256 != pe.sha256
        || method.source.build.is_empty()
        || method.source.evidence_kind != "independent-confirmation"
        || hex::encode(Sha256::digest(evidence)) != method.source.evidence_sha256
    {
        return Err("SOURCE_PROOF_INCOMPLETE".into());
    }
    let history = scan(pe, &method)?;
    let actual = unique(&history).map_err(|issue| issue.replacen("TARGET_", "SOURCE_", 1))?;
    if actual != method.source.value {
        return Err("SOURCE_LOCATION_CHANGED".into());
    }
    Ok((
        FinalizedMethod {
            method,
            digest: history.method_digest.clone(),
        },
        history,
    ))
}
pub fn locate(pe: &Pe, method: &FinalizedMethod) -> Result<History> {
    let h = scan(pe, &method.method)?;
    if h.method_digest != method.digest {
        return Err("locator.method_changed".into());
    }
    unique(&h)?;
    Ok(h)
}
fn unique(h: &History) -> Result<u64> {
    match h.candidates.len() {
        0 => Err("TARGET_METHOD_NO_MATCH".into()),
        1 => Ok(h.candidates[0].value),
        _ => Err("TARGET_METHOD_AMBIGUOUS".into()),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub method_id: String,
    pub expected_type: OutputType,
    pub reader_pointer: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub id: String,
    pub applicable_hashes: Vec<String>,
    pub state_globals: u64,
    pub tag_offset: u64,
    pub string_length: u64,
    pub string_data: u64,
    pub table_nodes: u64,
    pub table_log2: u64,
    pub node_bytes: u64,
}
impl Layout {
    pub fn admit(&self, hash: &str) -> Result<()> {
        if !self.applicable_hashes.iter().any(|v| v == hash) {
            return Err("reader.layout_unsupported".into());
        }
        if self.state_globals != 0x90
            || self.node_bytes != 56
            || self.tag_offset != 16
            || self.string_length != 24
            || self.string_data != 32
            || self.table_nodes != 40
            || self.table_log2 != 19
        {
            return Err("reader.layout_invalid".into());
        }
        Ok(())
    }
}
pub fn bind(parameter: &Parameter, method: &FinalizedMethod, history: &History) -> Result<u64> {
    if parameter.name.is_empty()
        || parameter.reader_pointer != "/root"
        || parameter.method_id != method.method.id
        || parameter.expected_type != method.method.output
        || history.method_digest != method.digest
    {
        return Err("reader.binding_missing".into());
    }
    unique(history)
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture(second: bool) -> (Vec<u8>, Method, Vec<u8>) {
        let mut b = vec![0u8; 0x600];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        b[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        b[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        b[0xd0..0xd4].copy_from_slice(&0x3000u32.to_le_bytes());
        for (n, name, rva, off, flags) in [
            (0, b".text\0\0\0", 0x1000u32, 0x200u32, 0x60000020u32),
            (1, b".data\0\0\0", 0x2000, 0x400, 0xc0000040),
        ] {
            let at = 0x188 + n * 40;
            b[at..at + 8].copy_from_slice(name);
            for (o, v) in [(8, 0x100), (12, rva), (16, 0x100), (20, off), (36, flags)] {
                b[at + o..at + o + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        for at in [0x200, if second { 0x220 } else { 0x200 }] {
            b[at..at + 3].copy_from_slice(&[0x48, 0x8b, 0x05]);
            let rva = 0x1000 + (at - 0x200);
            b[at + 3..at + 7].copy_from_slice(&(0x2000i32 - rva as i32 - 7).to_le_bytes());
            b[at + 7] = 0xc3;
        }
        let evidence = b"independent static callgraph output=0x2000".to_vec();
        let method = Method {
            id: "fixture-rip".into(),
            version: 1,
            output: OutputType::PointerSlot,
            section: ".text".into(),
            pattern: "48 8b 05 ?? ?? ?? ?? c3".into(),
            operands: vec![Operand {
                offset: 3,
                instruction_end: 7,
                target: TargetSection::WritableData,
                output: true,
            }],
            source: Source {
                build: "fixture".into(),
                sha256: hex::encode(Sha256::digest(&b)),
                value: 0x2000,
                evidence_sha256: hex::encode(Sha256::digest(&evidence)),
                evidence_kind: "independent-confirmation".into(),
            },
        };
        (b, method, evidence)
    }
    #[test]
    fn finalizes_and_target_needs_no_source_replay() {
        let (b, m, e) = fixture(false);
        let (f, h) = finalize(&Pe::parse(&b).unwrap(), m, &e).unwrap();
        assert_eq!(h.result, Some(0x2000));
        let mut target = b.clone();
        target[0x500] = 1;
        let h = locate(&Pe::parse(&target).unwrap(), &f).unwrap();
        assert!(!h.source_replay_required);
        assert_ne!(h.input_sha256, h.source_provenance.sha256);
    }
    #[test]
    fn complete_competition_not_old_rva_filter() {
        let (b, m, e) = fixture(true);
        assert_eq!(
            finalize(&Pe::parse(&b).unwrap(), m, &e).unwrap_err(),
            "SOURCE_METHOD_AMBIGUOUS"
        );
    }
    #[test]
    fn independent_output_not_backfilled() {
        let (b, mut m, e) = fixture(false);
        m.source.value = 0x2008;
        assert_eq!(
            finalize(&Pe::parse(&b).unwrap(), m, &e).unwrap_err(),
            "SOURCE_LOCATION_CHANGED"
        );
    }
    #[test]
    fn missing_disk_bytes_fail() {
        let (mut b, m, e) = fixture(false);
        b[0x188 + 16..0x188 + 20].copy_from_slice(&0x80u32.to_le_bytes());
        assert_eq!(
            finalize(&Pe::parse(&b).unwrap(), m, &e).unwrap_err(),
            "SOURCE_PROOF_INCOMPLETE"
        );
    }
}
