//! Runtime reader capabilities can only be issued by an actually finalized finite method.
use crate::{
    locator::{self, FinalizedMethod, Layout, Parameter, Pe},
    mailbox::{Memory, Reader, Record, Selector},
};
use lycheedev_windows::{Process, process::ModulePlan};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
pub struct RootRule {
    method: FinalizedMethod,
    parameter: Parameter,
    layout: Layout,
}
impl RootRule {
    pub fn new(method: FinalizedMethod, parameter: Parameter, layout: Layout) -> Self {
        Self {
            method,
            parameter,
            layout,
        }
    }
    pub fn accepts_hash(&self, hash: &str) -> bool {
        self.layout.admit(hash).is_ok()
    }
}
pub struct BoundMemory {
    process: Process,
    module: ModulePlan,
    layout: Layout,
    root: u64,
    guards: Vec<(u64, Vec<u8>)>,
}
impl Memory for BoundMemory {
    fn verify(&self) -> Result<()> {
        self.process.verify()
    }
    fn read(&self, address: u64, bytes: &mut [u8], module: bool) -> Result<()> {
        if module {
            self.process.read_module(&self.module, address, bytes)
        } else {
            self.process.read_private(address, bytes)
        }
    }
}
impl BoundMemory {
    pub fn open(pid: u32, rule: &RootRule) -> Result<Self> {
        let process = Process::open(pid)?;
        let module = process.main_module()?;
        let pe = Pe::parse(module.disk())?;
        rule.layout.admit(module.hash())?;
        if pe.image_size != module.size() {
            return Err("memory.image_size_mismatch".into());
        }
        let history = locator::locate(&pe, &rule.method)?;
        let root = locator::bind(&rule.parameter, &rule.method, &history)?;
        let bytes = module.disk();
        let nt = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        let opt = u16::from_le_bytes(bytes[nt + 20..nt + 22].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(bytes[nt + 6..nt + 8].try_into().unwrap()) as usize;
        let table = nt + 24 + opt;
        let mut guards = Vec::new();
        for (offset, size) in [
            (0, 2),
            (60, 4),
            (nt, 4),
            (nt + 6, 2),
            (nt + 20, 2),
            (nt + 24, 2),
            (nt + 24 + 56, 4),
            (table, count * 40),
        ] {
            guards.push((offset as u64, bytes[offset..offset + size].to_vec()));
        }
        // The disk-located code anchor itself must match current runtime bytes.
        let anchor = history.candidates[0].anchor;
        let section = pe
            .sections
            .iter()
            .find(|s| anchor >= s.rva && anchor < s.rva + s.virtual_size)
            .ok_or("memory.anchor_section")?;
        let length = rule.method_pattern_length();
        let offset = section.file_offset + anchor - section.rva;
        guards.push((
            anchor,
            bytes[offset as usize..offset as usize + length].to_vec(),
        ));
        let memory = Self {
            root: module.base() + root,
            process,
            module,
            layout: rule.layout.clone(),
            guards,
        };
        memory.check()?;
        Ok(memory)
    }
    pub fn hash(&self) -> &str {
        self.module.hash()
    }
    pub fn check(&self) -> Result<()> {
        self.process.verify()?;
        for (rva, want) in &self.guards {
            let mut got = vec![0; want.len()];
            self.process
                .read_module(&self.module, self.module.base() + rva, &mut got)?;
            if &got != want {
                return Err("memory.image_or_anchor_changed".into());
            }
        }
        self.process.verify()
    }
    pub fn lookup(&self, selector: &Selector) -> Result<Option<Record>> {
        self.check()?;
        let reader = Reader {
            memory: self,
            root: self.root,
            layout: self.layout.clone(),
            module_hash: self.module.hash().into(),
            release: lycheedev_core::VERSION.into(),
        };
        let result = reader.lookup(selector)?;
        self.check()?;
        Ok(result)
    }
}
impl RootRule {
    fn method_pattern_length(&self) -> usize {
        self.method.pattern_length()
    }
}
