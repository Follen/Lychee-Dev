//! Test-only physical transport for complete, unchanged oracle JSONL frames.
//! Each evidence file retains the existing 16 MiB cap. Large cases use bounded,
//! consecutive parts; missing, surplus, reordered or incomplete rows still fail.
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const MAX_PART_BYTES: u64 = 16 << 20;
pub const MAX_TOTAL_BYTES: u64 = 64 << 20;
pub const MAX_PARTS: usize = 8;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn part_name(name: &str, part: usize) -> String {
    if part == 0 {
        name.to_owned()
    } else {
        format!("{name}.part{part}")
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        && !name.starts_with('.')
}

pub struct TraceOutput {
    directory: PathBuf,
    name: String,
    part: usize,
    part_bytes: u64,
    total_bytes: u64,
    output: fs::File,
    failed: bool,
}
impl TraceOutput {
    pub fn new(directory: &Path, name: &str) -> io::Result<Self> {
        if !valid_name(name) {
            return Err(invalid("oracle.trace_name"));
        }
        let output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))?;
        Ok(Self {
            directory: directory.to_owned(),
            name: name.to_owned(),
            part: 0,
            part_bytes: 0,
            total_bytes: 0,
            output,
            failed: false,
        })
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn emit(&mut self, value: &Value) -> io::Result<()> {
        let result = self.emit_inner(value);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn emit_inner(&mut self, value: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        bytes.push(b'\n');
        let length = bytes.len() as u64;
        // Check before any write. A rejected oversized row can never grow an
        // evidence file past the custody limit or masquerade as a completed row.
        if length > MAX_PART_BYTES {
            return Err(invalid("oracle.trace_row_budget"));
        }
        if self.total_bytes + length > MAX_TOTAL_BYTES {
            return Err(invalid("oracle.trace_total_budget"));
        }
        if self.part_bytes + length > MAX_PART_BYTES {
            if self.part + 1 >= MAX_PARTS {
                return Err(invalid("oracle.trace_parts_budget"));
            }
            self.output.flush()?;
            let next = self.part + 1;
            let output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.directory.join(part_name(&self.name, next)))?;
            self.output = output;
            self.part = next;
            self.part_bytes = 0;
        }
        self.output.write_all(&bytes)?;
        self.output.flush()?;
        self.part_bytes += length;
        self.total_bytes += length;
        Ok(())
    }
}

/// Join physical parts without changing any logical header, row or terminal.
/// Semantic identity/ordinal/completion validation remains in the trace helper.
pub fn read_frames(directory: &Path, name: &str) -> io::Result<Vec<Value>> {
    if !valid_name(name) {
        return Err(invalid("oracle.trace_name"));
    }
    let prefix = format!("{name}.part");
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file = entry.file_name();
        let Some(file) = file.to_str() else {
            return Err(invalid("oracle.trace_name"));
        };
        if file == name {
            names.push((0, file.to_owned()));
        } else if let Some(suffix) = file.strip_prefix(&prefix) {
            let part: usize = suffix
                .parse()
                .map_err(|_| invalid("oracle.trace_part_name"))?;
            if part == 0 || part >= MAX_PARTS || suffix != part.to_string() {
                return Err(invalid("oracle.trace_part_name"));
            }
            names.push((part, file.to_owned()));
        }
    }
    names.sort_by_key(|(part, _)| *part);
    if names.is_empty() || names.len() > MAX_PARTS {
        return Err(invalid("oracle.trace_parts"));
    }
    let mut frames = Vec::new();
    let mut total = 0;
    for (expected, (part, file)) in names.into_iter().enumerate() {
        if part != expected {
            return Err(invalid("oracle.trace_part_gap"));
        }
        let path = directory.join(file);
        let info = fs::symlink_metadata(&path)?;
        if !info.is_file()
            || info.file_type().is_symlink()
            || info.len() == 0
            || info.len() > MAX_PART_BYTES
        {
            return Err(invalid("oracle.trace_budget"));
        }
        total += info.len();
        if total > MAX_TOTAL_BYTES {
            return Err(invalid("oracle.trace_total_budget"));
        }
        let bytes = fs::read(&path)?;
        if bytes.len() as u64 != info.len() {
            return Err(invalid("oracle.trace_changed"));
        }
        if bytes.last() != Some(&b'\n') {
            return Err(invalid("oracle.truncated_line"));
        }
        for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
            if line.is_empty() {
                return Err(invalid("oracle.empty_line"));
            }
            frames.push(serde_json::from_slice(line).map_err(io::Error::other)?);
        }
    }
    Ok(frames)
}

/// No destructor may panic a second time while preserving an earlier failure.
/// On a normal path, callers must propagate this Result as a failed Rust test.
pub fn finish(output: &mut TraceOutput, rows: usize, complete: bool) -> io::Result<()> {
    let complete = complete && !output.failed();
    output.emit(&serde_json::json!({"kind":"end","rows":rows,"complete":complete}))
}
