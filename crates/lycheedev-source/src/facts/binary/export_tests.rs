//! Opt-in, test-only evidence exporter. Production has no exporter API/CLI.
//! The independent owner authenticates this test executable and input manifest;
//! Node compares the complete exported public arrays to its independent oracle.
use super::*;
use lycheedev_core::storage;
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

const MANIFEST_LIMIT: usize = 8 << 20;
const TOTAL_LIMIT: usize = 128 << 20;
const INPUT_SCHEMA: &str = "lycheedev.source-facts-export-input.v1";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: String,
    records: Vec<Record>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    path: String,
    file: String,
    file_bytes: u64,
    #[serde(rename = "fileSHA256")]
    file_sha256: String,
    // Offset points to the LYCFCT13 envelope itself, after any pack header.
    record_offset: u64,
    record_bytes: u64,
    #[serde(rename = "recordSHA256")]
    record_sha256: String,
    #[serde(rename = "inputSHA256")]
    input_sha256: String,
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::invalid(message)
}

fn read_authenticated(
    path: &Path,
    bytes: u64,
    sha: &str,
    maximum: usize,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    if bytes > maximum as u64 || digest(sha).is_none() {
        return Err(invalid("export input identity is outside admission"));
    }
    let (mut file, observed) = storage::locked_sha256(path, maximum as u64, control)?;
    let result = (|| {
        if observed != sha || file.metadata()?.len() != bytes {
            return Err(invalid("export input identity differs"));
        }
        file.seek(SeekFrom::Start(0))?;
        let length =
            usize::try_from(bytes).map_err(|_| invalid("export input length overflows"))?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(length)
            .map_err(|_| invalid("export input allocation failed"))?;
        raw.resize(length, 0);
        for chunk in raw.chunks_mut(64 << 10) {
            control.check()?;
            file.read_exact(chunk)?;
        }
        let mut tail = [0];
        if file.read(&mut tail)? != 0 || storage::sha256(&raw) != sha {
            return Err(invalid("export input changed during read"));
        }
        Ok(raw)
    })();
    storage::finish_file(file, result, control)
}

struct Sink<'a> {
    file: BufWriter<File>,
    control: &'a OperationControl,
    maximum: usize,
    bytes: usize,
    hash: Sha256,
    stopped: Option<ToolkitError>,
}
impl Write for Sink<'_> {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = self.control.check() {
            self.stopped = Some(error);
            return Err(std::io::Error::other("export stopped"));
        }
        let length = self
            .bytes
            .checked_add(raw.len())
            .filter(|n| *n <= self.maximum)
            .ok_or_else(|| std::io::Error::other("export output exceeds total limit"))?;
        self.file.write_all(raw)?;
        self.bytes = length;
        self.hash.update(raw);
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
    path: &'a str,
    #[serde(rename = "inputSHA256")]
    input_sha256: &'a str,
    #[serde(rename = "recordSHA256")]
    record_sha256: &'a str,
    record_bytes: u64,
    facts: &'a Facts,
}

fn export(
    manifest_path: &Path,
    manifest_bytes: u64,
    manifest_sha: &str,
    output: &Path,
    control: &OperationControl,
    output_limit: usize,
) -> Result<serde_json::Value> {
    control.check()?;
    if output_limit > TOTAL_LIMIT {
        return Err(invalid("export output allowance exceeds fixed limit"));
    }
    let root = manifest_path
        .parent()
        .ok_or_else(|| invalid("export manifest has no parent"))?;
    storage::guard_path(root, manifest_path)?;
    let manifest_raw = read_authenticated(
        manifest_path,
        manifest_bytes,
        manifest_sha,
        MANIFEST_LIMIT,
        control,
    )?;
    let manifest: Manifest = serde_json::from_slice(&manifest_raw)?;
    if manifest.schema != INPUT_SCHEMA {
        return Err(invalid("unsupported export manifest"));
    }
    let mut total = 0usize;
    for record in &manifest.records {
        control.check()?;
        let relative = Path::new(&record.file);
        if record.file.is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || record.file_bytes > MAXIMUM as u64
            || record.record_bytes < HEADER as u64
            || record.record_bytes > MAXIMUM as u64
            || record
                .record_offset
                .checked_add(record.record_bytes)
                .is_none_or(|end| end > record.file_bytes)
            || [
                record.file_sha256.as_str(),
                record.record_sha256.as_str(),
                record.input_sha256.as_str(),
            ]
            .into_iter()
            .any(|sha| digest(sha).is_none())
        {
            return Err(invalid("invalid export record identity"));
        }
        total = total
            .checked_add(record.record_bytes as usize)
            .filter(|n| *n <= TOTAL_LIMIT)
            .ok_or_else(|| invalid("export records exceed total input limit"))?;
    }
    let parent = output
        .parent()
        .ok_or_else(|| invalid("export output has no parent"))?;
    storage::guard_path(parent, output)?;
    // Never replace a prior artifact, even on a failed or cancelled exporter.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut sink = Sink {
        file: BufWriter::with_capacity(64 << 10, file),
        control,
        maximum: output_limit,
        bytes: 0,
        hash: Sha256::new(),
        stopped: None,
    };
    let result = (|| {
        // One authenticated container at a time; adjacent records may share a
        // pack. No corpus-sized binary/JSON owner or repeated per-record process.
        let mut container: Option<(&str, u64, &str, Vec<u8>)> = None;
        for (ordinal, record) in manifest.records.iter().enumerate() {
            control.check()?;
            if !container.as_ref().is_some_and(|(file, bytes, sha, _)| {
                *file == record.file && *bytes == record.file_bytes && *sha == record.file_sha256
            }) {
                container = None;
                let path = root.join(&record.file);
                storage::guard_path(root, &path)?;
                let raw = read_authenticated(
                    &path,
                    record.file_bytes,
                    &record.file_sha256,
                    MAXIMUM,
                    control,
                )?;
                container = Some((&record.file, record.file_bytes, &record.file_sha256, raw));
            }
            let raw = &container.as_ref().unwrap().3;
            let start = usize::try_from(record.record_offset)
                .map_err(|_| invalid("export offset overflows"))?;
            let end = start
                .checked_add(record.record_bytes as usize)
                .ok_or_else(|| invalid("export range overflows"))?;
            let record_raw = raw
                .get(start..end)
                .ok_or_else(|| invalid("export record range differs"))?;
            if storage::sha256(record_raw) != record.record_sha256 {
                return Err(invalid("export record hash differs"));
            }
            let facts = decode(record_raw, &record.input_sha256, control)?
                .ok_or_else(|| invalid("export record is not valid v13 Facts"))?;
            serde_json::to_writer(
                &mut sink,
                &ExportRow {
                    schema: "lycheedev.source-facts-export-record.v1",
                    ordinal,
                    path: &record.path,
                    input_sha256: &record.input_sha256,
                    record_sha256: &record.record_sha256,
                    record_bytes: record.record_bytes,
                    facts: &facts,
                },
            )?;
            sink.write_all(b"\n")?;
        }
        sink.flush()?;
        sink.file.get_ref().sync_all()?;
        control.check()?;
        Ok(
            json!({"schema":"lycheedev.source-facts-export-summary.v1", "codec":"postcard-1.1.3", "layout":"LYCFCT13", "manifestSHA256":manifest_sha, "manifestBytes":manifest_bytes, "records":manifest.records.len(), "recordBytes":total, "outputBytes":sink.bytes, "outputSHA256":format!("{:x}", sink.hash.clone().finalize())}),
        )
    })();
    let result = match (result, sink.stopped.take()) {
        (_, Some(error)) => Err(error),
        (result, None) => result,
    };
    // On failure keep the partial owned output for evidence, without a success
    // summary. into_parts avoids a second implicit flush during Drop.
    let (file, _) = sink.file.into_parts();
    storage::finish_file(file, result, control)
}

#[test]
#[ignore = "owner-authenticated binary Facts manifest and fresh output artifact required"]
fn export_binary_facts_manifest() {
    let input = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_FACTS_EXPORT_INPUT").expect("manifest input path"),
    );
    let bytes = std::env::var("LYCHEEDEV_SOURCE_FACTS_EXPORT_BYTES")
        .expect("manifest bytes")
        .parse::<u64>()
        .expect("manifest byte count");
    let sha = std::env::var("LYCHEEDEV_SOURCE_FACTS_EXPORT_SHA256").expect("manifest SHA256");
    let output = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_FACTS_EXPORT_OUTPUT").expect("fresh output path"),
    );
    let summary = export(
        &input,
        bytes,
        &sha,
        &output,
        &OperationControl::default(),
        TOTAL_LIMIT,
    )
    .unwrap();
    println!("{}", serde_json::to_string(&summary).unwrap());
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-binary-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn manifest(&self, manifest: &Manifest) -> (PathBuf, u64, String) {
        let raw = serde_json::to_vec(manifest).unwrap();
        let path = self.0.join("manifest.json");
        fs::write(&path, &raw).unwrap();
        (path, raw.len() as u64, storage::sha256(&raw))
    }
    fn record(&self) -> (Manifest, Facts) {
        let input = storage::sha256(b"fixture original");
        let facts = crate::facts::toc(b"## Title: snow\nA.lua\nA.lua\n");
        let raw = encode(&facts, &input).unwrap();
        let mut packed = b"fixture-prefix".to_vec();
        let offset = packed.len() as u64;
        packed.extend_from_slice(&raw);
        fs::write(self.0.join("container.bin"), &packed).unwrap();
        let mut records = Vec::new();
        for path in ["A.toc", "B.toc", "A.toc"] {
            records.push(Record {
                path: path.into(),
                file: "container.bin".into(),
                file_bytes: packed.len() as u64,
                file_sha256: storage::sha256(&packed),
                record_offset: offset,
                record_bytes: raw.len() as u64,
                record_sha256: storage::sha256(&raw),
                input_sha256: input.clone(),
            });
        }
        (
            Manifest {
                schema: INPUT_SCHEMA.into(),
                records,
            },
            facts,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exporter_retains_complete_arrays_duplicate_records_order_and_output_identity() {
    let fixture = Fixture::new();
    let (manifest, facts) = fixture.record();
    let (input, bytes, sha) = fixture.manifest(&manifest);
    let output = fixture.0.join("output.jsonl");
    let summary = export(
        &input,
        bytes,
        &sha,
        &output,
        &OperationControl::default(),
        TOTAL_LIMIT,
    )
    .unwrap();
    let raw = fs::read(&output).unwrap();
    assert_eq!(summary["outputBytes"], raw.len());
    assert_eq!(summary["outputSHA256"], storage::sha256(&raw));
    assert_eq!(summary["records"], 3);
    let rows: Vec<serde_json::Value> = std::str::from_utf8(&raw)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    for (ordinal, (row, record)) in rows.iter().zip(&manifest.records).enumerate() {
        assert_eq!(row["ordinal"], ordinal);
        assert_eq!(row["path"], record.path);
        assert_eq!(row["facts"], serde_json::to_value(&facts).unwrap());
        assert_eq!(row["facts"]["loads"].as_array().unwrap().len(), 2);
    }
    assert!(
        export(
            &input,
            bytes,
            &sha,
            &output,
            &OperationControl::default(),
            TOTAL_LIMIT
        )
        .is_err()
    );
    assert_eq!(fs::read(&output).unwrap(), raw);
}

#[test]
fn exporter_rejects_manifest_and_record_corruption_without_a_success_artifact() {
    let fixture = Fixture::new();
    let (mut manifest, _) = fixture.record();
    let (input, bytes, sha) = fixture.manifest(&manifest);
    let output = fixture.0.join("output.jsonl");
    let control = OperationControl::default();
    assert!(export(&input, bytes + 1, &sha, &output, &control, TOTAL_LIMIT).is_err());
    assert!(!output.exists());
    manifest.records[0].file = "../escape.bin".into();
    let (input, bytes, sha) = fixture.manifest(&manifest);
    assert!(export(&input, bytes, &sha, &output, &control, TOTAL_LIMIT).is_err());
    assert!(!output.exists());
    manifest.records[0].file = "container.bin".into();
    manifest.records[0].record_sha256 = "0".repeat(64);
    let (input, bytes, sha) = fixture.manifest(&manifest);
    assert!(export(&input, bytes, &sha, &output, &control, TOTAL_LIMIT).is_err());
    assert_eq!(fs::metadata(&output).unwrap().len(), 0);
}

#[test]
fn exporter_observes_cancellation_and_exact_output_budget_without_overwriting() {
    let fixture = Fixture::new();
    let (manifest, _) = fixture.record();
    let (input, bytes, sha) = fixture.manifest(&manifest);
    let stopped = OperationControl::default();
    stopped.tracked_cancel_handle().unwrap().cancel();
    let output = fixture.0.join("cancelled.jsonl");
    assert_eq!(
        export(&input, bytes, &sha, &output, &stopped, TOTAL_LIMIT)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!output.exists());
    let full = fixture.0.join("full.jsonl");
    let summary = export(
        &input,
        bytes,
        &sha,
        &full,
        &OperationControl::default(),
        TOTAL_LIMIT,
    )
    .unwrap();
    let length = summary["outputBytes"].as_u64().unwrap() as usize;
    for limit in [length - 1, length, length + 1] {
        let path = fixture.0.join(format!("limit-{limit}.jsonl"));
        let result = export(
            &input,
            bytes,
            &sha,
            &path,
            &OperationControl::default(),
            limit,
        );
        assert_eq!(result.is_ok(), limit >= length);
        assert!(fs::metadata(&path).unwrap().len() <= limit as u64);
        if result.is_ok() {
            assert_eq!(fs::read(path).unwrap(), fs::read(&full).unwrap());
        }
    }
}

#[test]
fn compact_escaped_facts_still_obey_the_independent_export_output_budget() {
    let fixture = Fixture::new();
    let input_sha = storage::sha256(b"escaped source");
    let facts = Facts {
        diagnostics: vec![crate::facts::SyntaxNote {
            message: "\0".repeat(MAXIMUM / 6 + 1),
            line: 0,
        }],
        ..Facts::default()
    };
    assert!(facts.diagnostics[0].message.len() * 6 > MAXIMUM);
    let raw = encode(&facts, &input_sha).unwrap();
    assert!(raw.len() < MAXIMUM);
    assert!(
        decode(&raw, &input_sha, &OperationControl::default())
            .unwrap()
            .is_some()
    );
    fs::write(fixture.0.join("compact.bin"), &raw).unwrap();
    let manifest = Manifest {
        schema: INPUT_SCHEMA.into(),
        records: vec![Record {
            path: "Escaped.lua".into(),
            file: "compact.bin".into(),
            file_bytes: raw.len() as u64,
            file_sha256: storage::sha256(&raw),
            record_offset: 0,
            record_bytes: raw.len() as u64,
            record_sha256: storage::sha256(&raw),
            input_sha256: input_sha,
        }],
    };
    let (input, bytes, sha) = fixture.manifest(&manifest);
    let output = fixture.0.join("limited.jsonl");
    let limit = 64 << 10;
    let error = export(
        &input,
        bytes,
        &sha,
        &output,
        &OperationControl::default(),
        limit,
    )
    .unwrap_err();
    assert!(error.message.contains("export output exceeds total limit"));
    assert!(fs::metadata(&output).unwrap().len() <= limit as u64);
    assert_eq!(TOTAL_LIMIT, 128 << 20);
    let denied = fixture.0.join("over-allowance.jsonl");
    assert!(
        export(
            &input,
            bytes,
            &sha,
            &denied,
            &OperationControl::default(),
            TOTAL_LIMIT + 1
        )
        .is_err()
    );
    assert!(!denied.exists());
}
