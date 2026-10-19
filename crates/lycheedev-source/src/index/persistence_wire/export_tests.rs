//! Opt-in evidence export through the private production decoder. No product
//! command/API and no second-language Postcard implementation. The external
//! owner authenticates the executable/toolchain/source and this input manifest.
use super::*;
use lycheedev_core::storage;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};
const MANIFEST_LIMIT: usize = 8 << 20;
const TOTAL_LIMIT: usize = 128 << 20;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: String,
    records: Vec<Record>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    file: String,
    #[serde(rename = "fileSHA256")]
    file_sha256: String,
    file_bytes: u64,
    identity: String,
}
fn read_authenticated(
    path: &Path,
    bytes: u64,
    sha: &str,
    maximum: usize,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    if bytes > maximum as u64 || !valid_digest(sha, 64) {
        return Err(invalid("Index export input admission differs"));
    }
    let (mut file, observed) = storage::locked_sha256(path, maximum as u64, control)?;
    let result: Result<Vec<u8>> = (|| {
        if observed != sha || file.metadata()?.len() != bytes {
            return Err(invalid("Index export input identity differs"));
        }
        file.seek(SeekFrom::Start(0))?;
        let length = usize::try_from(bytes).map_err(|_| budget())?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(length).map_err(|_| budget())?;
        if raw.capacity() > maximum {
            return Err(budget());
        }
        raw.resize(length, 0);
        for chunk in raw.chunks_mut(65536) {
            control.check()?;
            file.read_exact(chunk)?;
        }
        let mut tail = [0];
        if file.read(&mut tail)? != 0 || storage::sha256(&raw) != sha {
            return Err(invalid("Index export input changed"));
        }
        Ok(raw)
    })();
    storage::finish_file(file, result, control)
}
struct Sink<'a> {
    file: BufWriter<File>,
    control: &'a OperationControl,
    limit: usize,
    bytes: usize,
    hash: Sha256,
    stopped: Option<ToolkitError>,
}
impl Write for Sink<'_> {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = self.control.check() {
            self.stopped = Some(error);
            return Err(std::io::Error::other("Index export stopped"));
        }
        let total = self
            .bytes
            .checked_add(raw.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("Index export output limit"))?;
        for chunk in raw.chunks(65536) {
            if let Err(error) = self.control.check() {
                self.stopped = Some(error);
                return Err(std::io::Error::other("Index export stopped"));
            }
            self.file.write_all(chunk)?;
            self.hash.update(chunk);
        }
        self.bytes = total;
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportRow<'a> {
    schema: &'static str,
    ordinal: usize,
    file: &'a str,
    #[serde(rename = "fileSHA256")]
    file_sha256: &'a str,
    file_bytes: u64,
    identity: &'a str,
    index: &'a Index,
}
fn export(
    input: &Path,
    bytes: u64,
    sha: &str,
    output: &Path,
    control: &OperationControl,
    limit: usize,
) -> Result<Value> {
    control.check()?;
    if limit > TOTAL_LIMIT {
        return Err(budget());
    }
    let root = input
        .parent()
        .ok_or_else(|| invalid("Index export manifest has no parent"))?;
    storage::guard_path(root, input)?;
    let raw = read_authenticated(input, bytes, sha, MANIFEST_LIMIT, control)?;
    let manifest: Manifest = serde_json::from_slice(&raw)?;
    if manifest.schema != "lycheedev.source-index-export-input.v1" || manifest.records.len() > 1024
    {
        return Err(invalid("Unsupported Index export manifest"));
    }
    let mut total = 0usize;
    for record in &manifest.records {
        control.check()?;
        if record.file.is_empty()
            || !Path::new(&record.file)
                .components()
                .all(|p| matches!(p, Component::Normal(_)))
            || record.file_bytes < HEADER as u64
            || record.file_bytes > MAXIMUM as u64
            || !valid_digest(&record.file_sha256, 64)
            || !valid_digest(&record.identity, 64)
        {
            return Err(invalid("Invalid Index export record"));
        }
        total = total
            .checked_add(usize::try_from(record.file_bytes).map_err(|_| budget())?)
            .filter(|n| *n <= TOTAL_LIMIT)
            .ok_or_else(budget)?;
    }
    let parent = output
        .parent()
        .ok_or_else(|| invalid("Index export output has no parent"))?;
    storage::guard_path(parent, output)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut sink = Sink {
        file: BufWriter::with_capacity(65536, file),
        control,
        limit,
        bytes: 0,
        hash: Sha256::new(),
        stopped: None,
    };
    let result: Result<Value> = (|| {
        for (ordinal, record) in manifest.records.iter().enumerate() {
            control.check()?;
            let path = root.join(&record.file);
            storage::guard_path(root, &path)?;
            let raw = read_authenticated(
                &path,
                record.file_bytes,
                &record.file_sha256,
                MAXIMUM,
                control,
            )?;
            let index: Index = decode(&raw, &record.identity, control)?;
            serde_json::to_writer(
                &mut sink,
                &ExportRow {
                    schema: "lycheedev.source-index-export-record.v1",
                    ordinal,
                    file: &record.file,
                    file_sha256: &record.file_sha256,
                    file_bytes: record.file_bytes,
                    identity: &record.identity,
                    index: &index,
                },
            )?;
            sink.write_all(b"\n")?;
        }
        sink.flush()?;
        sink.file.get_ref().sync_all()?;
        control.check()?;
        Ok(
            json!({"schema":"lycheedev.source-index-export-summary.v1","codec":"postcard-1.1.3","layout":"LYCIDX01","manifestSHA256":sha,"manifestBytes":bytes,"records":manifest.records.len(),"recordBytes":total,"outputBytes":sink.bytes,"outputSHA256":format!("{:x}",sink.hash.clone().finalize())}),
        )
    })();
    let result = match (result, sink.stopped.take()) {
        (_, Some(error)) => Err(error),
        (result, None) => result,
    };
    // Retain partial owned evidence on failure; never replace an earlier output,
    // emit a success receipt, or let BufWriter::drop perform an implicit flush.
    let (file, _) = sink.file.into_parts();
    storage::finish_file(file, result, control)
}
#[test]
#[ignore = "owner-authenticated Index executable, manifest and fresh output required"]
fn export_binary_index_manifest() {
    let input = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_INDEX_EXPORT_INPUT").expect("manifest path"),
    );
    let bytes = std::env::var("LYCHEEDEV_SOURCE_INDEX_EXPORT_BYTES")
        .expect("manifest bytes")
        .parse::<u64>()
        .expect("unsigned manifest size");
    let sha = std::env::var("LYCHEEDEV_SOURCE_INDEX_EXPORT_SHA256").expect("manifest SHA256");
    let output = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_INDEX_EXPORT_OUTPUT").expect("fresh output path"),
    );
    let summary = export(
        &input,
        bytes,
        &sha,
        &output,
        &OperationControl::default(),
        TOTAL_LIMIT,
    )
    .expect("authenticated Index export");
    println!("{}", serde_json::to_string(&summary).unwrap());
}
#[test]
fn index_exporter_full_rows_duplicates_manifest_custody_and_output_bounds() {
    let fixture = crate::index::facts_cache_tests::Fixture::new();
    let (index, pin) = super::tests::fixture();
    let identity = crate::index::fingerprint(&pin);
    let raw = encode(&index, &identity, &OperationControl::default()).unwrap();
    let file = fixture.home.join("index.bin");
    std::fs::write(&file, &raw).unwrap();
    let record = || Record {
        file: "index.bin".into(),
        file_sha256: storage::sha256(&raw),
        file_bytes: raw.len() as u64,
        identity: identity.clone(),
    };
    let manifest = Manifest {
        schema: "lycheedev.source-index-export-input.v1".into(),
        records: vec![record(), record()],
    };
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let input = fixture.home.join("input.json");
    std::fs::write(&input, &bytes).unwrap();
    let sha = storage::sha256(&bytes);
    let output = fixture.home.join("output.jsonl");
    let summary = export(
        &input,
        bytes.len() as u64,
        &sha,
        &output,
        &OperationControl::default(),
        TOTAL_LIMIT,
    )
    .unwrap();
    let result = std::fs::read(&output).unwrap();
    assert_eq!(summary["outputSHA256"], storage::sha256(&result));
    assert_eq!(summary["outputBytes"], result.len());
    assert_eq!(summary["records"], 2);
    let lines: Vec<Value> = result
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["index"], lines[1]["index"]);
    assert_eq!(lines[1]["ordinal"], 1);
    assert_eq!(lines[0]["index"]["symbols"].as_array().unwrap().len(), 3);
    assert_eq!(
        lines[0]["index"]["summary"]["diagnosticSample"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        export(
            &input,
            bytes.len() as u64,
            &sha,
            &output,
            &OperationControl::default(),
            TOTAL_LIMIT
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&output).unwrap(), result);
    let partial = fixture.home.join("partial.jsonl");
    assert!(
        export(
            &input,
            bytes.len() as u64,
            &sha,
            &partial,
            &OperationControl::default(),
            8
        )
        .is_err()
    );
    assert!(partial.exists());
    assert!(std::fs::metadata(&partial).unwrap().len() <= 8);
    let stopped = OperationControl::default();
    stopped.tracked_cancel_handle().unwrap().cancel();
    let absent = fixture.home.join("cancelled.jsonl");
    assert_eq!(
        export(
            &input,
            bytes.len() as u64,
            &sha,
            &absent,
            &stopped,
            TOTAL_LIMIT
        )
        .unwrap_err()
        .code,
        "command.cancelled"
    );
    assert!(!absent.exists());
    let bad = fixture.home.join("bad.jsonl");
    assert!(
        export(
            &input,
            bytes.len() as u64,
            &"a".repeat(64),
            &bad,
            &OperationControl::default(),
            TOTAL_LIMIT
        )
        .is_err()
    );
    assert!(!bad.exists());
}
