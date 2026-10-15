//! Fixed Go 3.2 SQL CSV exporter. The manifest is the completion record;
//! data and sidecar publication share one canonical, cross-workspace lease.
use crate::{
    assets::output,
    capture, fault,
    relational::{QueryResult, Scalar},
    resource::{Budget, Cost, RetainedReservation},
};
use lycheedev_core::{
    Invocation, Result, ToolkitError, evidence,
    storage::{Destination, ExclusiveLock},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, path::Path, thread, time::Duration};

const MAX_BYTES: usize = 32 << 20;
#[cfg(test)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod csv_fixture;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest<'a> {
    schema: &'static str,
    snapshot: &'a str,
    output: String,
    #[serde(rename = "sha256")]
    sha256: String,
    bytes: usize,
    columns: Option<&'a [String]>,
    rows: usize,
    query_capture: &'a str,
    csv_capture: &'a str,
    complete: bool,
}

pub(super) fn export(
    inv: &Invocation,
    snapshot: &str,
    query: &Value,
    result: &QueryResult,
    budget: &Budget,
) -> Result<(Value, Value)> {
    export_after_data(inv, snapshot, query, result, budget, || Ok(()))
}
fn export_after_data(
    inv: &Invocation,
    snapshot: &str,
    query: &Value,
    result: &QueryResult,
    budget: &Budget,
    after_data: impl FnOnce() -> Result<()>,
) -> Result<(Value, Value)> {
    if snapshot.is_empty() {
        return Err(fault("records.export_path", 2, "Missing snapshot"));
    }
    let query_id = query["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| fault("records.export_path", 2, "Missing query capture"))?;
    let path = inv.flag("--output").unwrap_or_default();
    let overwrite = inv.flag_bool("--overwrite");
    // Match Go export ordering: both preflights precede encoding and capture.
    let sidecar_path = format!("{path}.manifest.json");
    let (data, sidecar) = output::destination_pair(
        &inv.home,
        Path::new(path),
        Path::new(&sidecar_path),
        overwrite,
    )?;
    let raw = encode(&result.columns, &result.rows, budget, MAX_BYTES)?;
    let csv = evidence::commit_capture_budgeted_with_dependencies(
        &inv.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: MAX_BYTES,
            media_type: "text/csv; charset=utf-8",
            provenance: json!({"kind":"data-query-csv","locator":query_id,"snapshot":snapshot}),
            complete: result.complete,
            truncated: false,
        },
        &[query_id],
        &inv.control,
        budget,
    )?;
    let csv_id = csv["id"]
        .as_str()
        .ok_or_else(|| fault("records.internal", 8, "Missing CSV capture"))?;
    let manifest = Manifest {
        schema: "lycheedev.query-csv.v1",
        snapshot,
        output: output::ordinary(data.path()).to_string_lossy().into_owned(),
        sha256: digest(&raw, budget)?,
        bytes: raw.len(),
        columns: result.plan.is_none().then_some(result.columns.as_slice()),
        rows: result.rows.len(),
        query_capture: query_id,
        csv_capture: csv_id,
        complete: result.complete,
    };
    let mut sidecar_raw = capture::json_bytes_with_formatter(
        &manifest,
        budget,
        MAX_BYTES,
        limit,
        serde_json::ser::PrettyFormatter::with_indent(b"  "),
    )?;
    // Manifest strings use Go MarshalIndent's HTML/JS escapes. Structural
    // pretty formatting alone is insufficient; escape the serialized bytes.
    let sidecar_raw = escape_manifest(&mut sidecar_raw, budget)?;
    let public_bytes = capture::json_size(&manifest, budget)?;
    budget.charge(Cost::metadata(
        public_bytes.saturating_mul(4).saturating_add(4096),
    ))?;
    let public = serde_json::to_value(&manifest)?;
    publish_pair_after_data(
        inv,
        &data,
        &sidecar,
        overwrite,
        &raw,
        &sidecar_raw,
        budget,
        after_data,
    )?;
    Ok((public, csv))
}

fn escape_manifest(raw: &mut Vec<u8>, budget: &Budget) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| fault("records.internal", 8, "Invalid manifest UTF-8"))?;
    let mut out = Buffer::new(budget, MAX_BYTES);
    let mut start = 0;
    let mut checked = 0;
    for (at, ch) in text.char_indices() {
        if at - checked >= 4096 {
            budget.charge(Cost::work(at - checked))?;
            checked = at;
        }
        let replacement = match ch {
            '<' => b"\\u003c".as_slice(),
            '>' => b"\\u003e",
            '&' => b"\\u0026",
            '\u{2028}' => b"\\u2028",
            '\u{2029}' => b"\\u2029",
            _ => continue,
        };
        out.append(&raw[start..at])?;
        out.append(replacement)?;
        start = at + ch.len_utf8();
    }
    out.append(&raw[start..])?;
    out.append(b"\n")?;
    out.finish()
}

pub(crate) fn publish_pair_after_data(
    inv: &Invocation,
    data: &Destination,
    sidecar: &Destination,
    overwrite: bool,
    raw: &[u8],
    manifest: &[u8],
    budget: &Budget,
    after_data: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = data
        .path()
        .parent()
        .ok_or_else(|| fault("records.export_path", 2, "Missing output parent"))?;
    let name = data
        .path()
        .file_name()
        .ok_or_else(|| fault("records.export_path", 2, "Missing output name"))?;
    let resource = format!("sql-csv:{}", name.to_string_lossy().to_lowercase());
    let scope = parent.join(".lycheedev-locks");
    let lease = loop {
        budget.check()?;
        match ExclusiveLock::resource_controlled(&scope, &resource, &inv.control) {
            Ok(lease) => break lease,
            Err(error) if error.code == "workspace.busy" => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        budget.check()?;
        // Original parent handles remain live, so this recheck cannot silently
        // follow a renamed/replaced parent. Both checks precede the first write.
        let _data_check = Destination::open(data.path(), overwrite)
            .map_err(|e| output::publication_error(e, data.path()))?;
        let _sidecar_check = Destination::open(sidecar.path(), overwrite)
            .map_err(|e| output::publication_error(e, sidecar.path()))?;
        publish(data, raw, budget)?;
        after_data()?;
        budget.check()?;
        publish(sidecar, manifest, budget)
    })();
    let close = lease.close();
    result.and(close)
}

fn publish(destination: &Destination, raw: &[u8], budget: &Budget) -> Result<()> {
    destination
        .publish(raw, raw.len().max(MAX_BYTES) as u64, || {
            budget
                .check()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Interrupted, error))
        })
        .map_err(|error| output::publication_error(error, destination.path()))
}
fn digest(raw: &[u8], budget: &Budget) -> Result<String> {
    let mut hash = Sha256::new();
    for chunk in raw.chunks(65536) {
        budget.charge(Cost::work(chunk.len()))?;
        hash.update(chunk);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn limit() -> ToolkitError {
    fault("records.query_csv_limit", 3, "records.query_csv_limit")
}
struct Buffer<'a> {
    raw: Vec<u8>,
    retained: Option<RetainedReservation<'a>>,
    budget: &'a Budget,
    maximum: usize,
}
impl<'a> Buffer<'a> {
    fn new(budget: &'a Budget, maximum: usize) -> Self {
        Self {
            raw: Vec::new(),
            retained: None,
            budget,
            maximum,
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        for chunk in bytes.chunks(4096) {
            self.budget.check()?;
            let end = self
                .raw
                .len()
                .checked_add(chunk.len())
                .filter(|n| *n <= self.maximum)
                .ok_or_else(limit)?;
            self.budget.charge(Cost::work(chunk.len() + 1))?;
            if end > self.raw.capacity() {
                let capacity = end
                    .max(self.raw.capacity().saturating_mul(2))
                    .max(4096.min(self.maximum))
                    .min(self.maximum);
                let retained = self.budget.reserve_retained(capacity)?;
                self.raw
                    .try_reserve_exact(capacity - self.raw.len())
                    .map_err(|_| {
                        fault("records.query_resource_budget", 3, "CSV allocation failed")
                    })?;
                self.retained = Some(retained);
            }
            self.raw.extend_from_slice(chunk);
        }
        Ok(())
    }
    fn finish(mut self) -> Result<Vec<u8>> {
        self.budget.check()?;
        if let Some(retained) = self.retained.take() {
            retained.commit();
        }
        Ok(self.raw)
    }
    fn cell(&mut self, value: &str) -> Result<()> {
        self.budget.check()?;
        let mut quote = value == "\\." || value.chars().next().is_some_and(char::is_whitespace);
        for chunk in value.as_bytes().chunks(4096) {
            self.budget.charge(Cost::work(chunk.len() + 1))?;
            quote |= chunk
                .iter()
                .any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n'));
        }
        if !quote {
            return self.append(value.as_bytes());
        }
        self.append(b"\"")?;
        let mut start = 0;
        for (index, chunk) in value.as_bytes().chunks(4096).enumerate() {
            self.budget.check()?;
            for (offset, byte) in chunk.iter().enumerate() {
                if *byte == b'"' {
                    let at = index * 4096 + offset;
                    self.append(&value.as_bytes()[start..at])?;
                    self.append(b"\"\"")?;
                    start = at + 1;
                }
            }
        }
        self.append(&value.as_bytes()[start..])?;
        self.append(b"\"")
    }
}
fn scalar(value: &Scalar) -> Result<Cow<'_, str>> {
    crate::relational::scalar_text(value)
}

fn encode(
    columns: &[String],
    rows: &[Vec<Scalar>],
    budget: &Budget,
    maximum: usize,
) -> Result<Vec<u8>> {
    let mut out = Buffer::new(budget, maximum);
    for (index, column) in columns.iter().enumerate() {
        if index != 0 {
            out.append(b",")?;
        }
        out.cell(column)?;
    }
    out.append(b"\n")?;
    for row in rows {
        budget.check()?;
        if row.len() != columns.len() {
            return Err(fault(
                "records.sql_csv_row_width",
                8,
                "records.sql_csv_row_width",
            ));
        }
        for (index, value) in row.iter().enumerate() {
            if index != 0 {
                out.append(b",")?;
            }
            out.cell(&scalar(value)?)?;
        }
        out.append(b"\n")?;
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    fn budget() -> Budget {
        Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            None,
        )
    }
    fn encoded(columns: &[&str], rows: Vec<Vec<Scalar>>) -> String {
        String::from_utf8(
            encode(
                &columns.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &rows,
                &budget(),
                MAX_BYTES,
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn sql_csv_lf_escaping_and_go_whitespace() {
        let cells = [
            "", "\\.", " a", "\u{85}a", "a,b", "a\"b", "a\rb", "a\nb", "a\r\nb", "尾 ", "=1+1",
        ];
        assert_eq!(
            encoded(
                &["value"],
                cells.map(|s| vec![Scalar::String(s.into())]).to_vec()
            ),
            "value\n\n\"\\.\"\n\" a\"\n\"\u{85}a\"\n\"a,b\"\n\"a\"\"b\"\n\"a\rb\"\n\"a\nb\"\n\"a\r\nb\"\n尾 \n=1+1\n"
        );
    }
    #[test]
    fn sql_csv_scalars_keep_integer_precision_and_go_general_floats() {
        let values = vec![
            Scalar::Null,
            Scalar::Bool(true),
            Scalar::Bool(false),
            Scalar::Int(i64::MIN),
            Scalar::UInt(u64::MAX),
            Scalar::Float(-0.0),
            Scalar::Float(1e-4),
            Scalar::Float(1e-5),
            Scalar::Float(1e6),
            Scalar::Float(1e20),
        ];
        assert_eq!(
            encoded(&["n"], values.into_iter().map(|v| vec![v]).collect()),
            "n\n\ntrue\nfalse\n-9223372036854775808\n18446744073709551615\n-0\n0.0001\n1e-05\n1e+06\n1e+20\n"
        );
    }
    #[test]
    fn sql_csv_headers_duplicates_and_zero_columns_match_go_writer() {
        assert_eq!(encoded(&[" a", "a", "a"], vec![]), "\" a\",a,a\n");
        assert_eq!(encoded(&[], vec![vec![]]), "\n\n");
    }
    #[test]
    fn sql_csv_width_limit_and_nonfinite_are_errors() {
        let columns = vec!["n".into()];
        assert_eq!(
            encode(&columns, &[vec![]], &budget(), MAX_BYTES)
                .unwrap_err()
                .code,
            "records.sql_csv_row_width"
        );
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                encode(
                    &columns,
                    &[vec![Scalar::Float(value)]],
                    &budget(),
                    MAX_BYTES
                )
                .unwrap_err()
                .code,
                "relational.numeric_range"
            );
        }
        assert_eq!(encode(&columns, &[], &budget(), 2).unwrap(), b"n\n");
        assert_eq!(
            encode(&columns, &[], &budget(), 1).unwrap_err().code,
            "records.query_csv_limit"
        );
    }
    #[test]
    fn sql_csv_cancel_preserves_typed_cause() {
        let control = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            Some(Arc::new(AtomicBool::new(true))),
        );
        let expected = control.check().unwrap_err().code;
        assert_eq!(
            encode(&[], &[], &control, MAX_BYTES).unwrap_err().code,
            expected
        );
    }
    #[test]
    fn sql_csv_manifest_go_html_and_js_escapes() {
        let mut raw = b"{\n  \"x\": \"<&>".to_vec();
        raw.extend_from_slice("\u{2028}\u{2029}\"\n}".as_bytes());
        assert_eq!(
            escape_manifest(&mut raw, &budget()).unwrap(),
            b"{\n  \"x\": \"\\u003c\\u0026\\u003e\\u2028\\u2029\"\n}\n"
        );
    }

    fn fixture() -> csv_fixture::Fixture {
        csv_fixture::Fixture::new(&[csv_fixture::table(
            "Sample",
            201,
            &[("ID", "uint")],
            &[vec![csv_fixture::Cell::U(1)]],
        )])
    }
    #[test]
    fn sql_csv_missing_query_never_publishes_existing_or_new_pair() {
        for existing in [false, true] {
            let fx = fixture();
            let path = fx.root.join("missing.csv");
            let sidecar = fx.root.join("missing.csv.manifest.json");
            if existing {
                std::fs::write(&path, b"old-data").unwrap();
                std::fs::write(&sidecar, b"old-manifest").unwrap();
            }
            let mut inv = fx.invocation("data sql", &[("--output", path.to_str().unwrap())]);
            if existing {
                inv.flags.insert("--overwrite".into(), "true".into());
            }
            let query = evidence::commit_capture(
                &fx.home,
                b"{}",
                "application/json",
                json!({"kind":"data-query","locator":"fixture"}),
                true,
                false,
            )
            .unwrap();
            evidence::remove_capture(&fx.home, query["id"].as_str().unwrap()).unwrap();
            let result = QueryResult {
                plan: None,
                columns: vec!["value".into()],
                rows: vec![vec![Scalar::Int(1)]],
                complete: true,
            };
            assert!(export(&inv, &fx.pin, &query, &result, &budget()).is_err());
            if existing {
                assert_eq!(std::fs::read(path).unwrap(), b"old-data");
                assert_eq!(std::fs::read(sidecar).unwrap(), b"old-manifest");
            } else {
                assert!(!path.exists());
                assert!(!sidecar.exists());
            }
        }
    }
    #[test]
    fn sql_csv_manifest_failure_keeps_published_data_and_releases_lease() {
        let fx = fixture();
        let path = fx.root.join("failed.csv");
        let manifest = fx.root.join("failed.csv.manifest.json");
        let inv = fx.invocation("data sql", &[]);
        let (data, sidecar) = output::destination_pair(&fx.home, &path, &manifest, true).unwrap();
        let error = publish_pair_after_data(
            &inv,
            &data,
            &sidecar,
            true,
            b"n\n1\n",
            b"{}\n",
            &budget(),
            || {
                assert_eq!(std::fs::read(&path).unwrap(), b"n\n1\n");
                // Deterministic substitution after CSV publication, before manifest.
                std::fs::create_dir(&manifest)?;
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "records.export_conflict");
        assert_eq!(std::fs::read(&path).unwrap(), b"n\n1\n");
        assert!(manifest.is_dir());
        let lease =
            ExclusiveLock::resource(&fx.root.join(".lycheedev-locks"), "sql-csv:failed.csv")
                .unwrap();
        lease.close().unwrap();
    }
    #[test]
    fn sql_csv_pair_holds_lease_through_manifest_and_preserves_cancel() {
        let fx = fixture();
        let path = fx.root.join("cancel.csv");
        let manifest = fx.root.join("cancel.csv.manifest.json");
        let inv = fx.invocation("data sql", &[]);
        let cancel = inv.control.tracked_cancel_handle().unwrap();
        let (data, sidecar) = output::destination_pair(&fx.home, &path, &manifest, false).unwrap();
        let shared_budget = Budget::controlled(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            inv.control.clone(),
        );
        let error = publish_pair_after_data(
            &inv,
            &data,
            &sidecar,
            false,
            b"n\n1\n",
            b"{}\n",
            &shared_budget,
            || {
                match ExclusiveLock::resource(
                    &fx.root.join(".lycheedev-locks"),
                    "sql-csv:cancel.csv",
                ) {
                    Ok(lease) => {
                        lease.close().unwrap();
                        panic!("pair lease released before manifest");
                    }
                    Err(error) => assert_eq!(error.code, "workspace.busy"),
                }
                cancel.cancel();
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, inv.control.check().unwrap_err().code);
        assert_eq!(std::fs::read(&path).unwrap(), b"n\n1\n");
        assert!(!manifest.exists());
    }

    fn query_capture(fx: &csv_fixture::Fixture, value: i64) -> (Value, QueryResult) {
        let query = evidence::commit_capture(
            &fx.home,
            format!("{{\"value\":{value}}}").as_bytes(),
            "application/json",
            json!({"kind":"data-query","locator":format!("fixture:{value}")}),
            true,
            false,
        )
        .unwrap();
        (
            query,
            QueryResult {
                plan: None,
                columns: vec!["value".into()],
                rows: vec![vec![Scalar::Int(value)]],
                complete: true,
            },
        )
    }
    fn verify_pair(fx: &csv_fixture::Fixture, path: &Path, manifest: &Value) {
        let raw = std::fs::read(path).unwrap();
        let sidecar: Value = serde_json::from_slice(
            &std::fs::read(format!("{}.manifest.json", path.display())).unwrap(),
        )
        .unwrap();
        assert_eq!(&sidecar, manifest);
        assert_eq!(manifest["sha256"], lycheedev_core::storage::sha256(&raw));
        assert_eq!(manifest["bytes"], raw.len());
        let csv =
            evidence::read_capture(&fx.home, manifest["csvCapture"].as_str().unwrap()).unwrap();
        assert_eq!(
            evidence::read_blob(&fx.home, &csv["blob"], MAX_BYTES).unwrap(),
            raw
        );
        assert_eq!(csv["dependencies"][0], manifest["queryCapture"]);
    }
    #[test]
    fn sql_csv_cross_workspace_process_helper() {
        let Some(root) = std::env::var_os("LYCHEEDEV_SQL_CSV_PROCESS_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let fx = csv_fixture::Fixture {
            root: root.clone(),
            home: root.join("home"),
            client: root.join("client"),
            pin: std::env::var("LYCHEEDEV_SQL_CSV_PROCESS_PIN").unwrap(),
        };
        // Parent passes exact fixture paths; these are test inputs, not product discovery.
        let mut fx = fx;
        fx.home = std::env::var_os("LYCHEEDEV_SQL_CSV_PROCESS_HOME")
            .unwrap()
            .into();
        let output: std::path::PathBuf = std::env::var_os("LYCHEEDEV_SQL_CSV_PROCESS_OUTPUT")
            .unwrap()
            .into();
        let parent = output::ordinary(&std::fs::canonicalize(output.parent().unwrap()).unwrap());
        let resource = format!(
            "sql-csv:{}",
            output.file_name().unwrap().to_string_lossy().to_lowercase()
        );
        match ExclusiveLock::resource(&parent.join(".lycheedev-locks"), &resource) {
            Ok(lease) => {
                lease.close().unwrap();
                panic!("other process did not hold canonical pair lease");
            }
            Err(error) => assert_eq!(error.code, "workspace.busy"),
        }
        std::fs::write(root.join("csv-helper-blocked"), b"blocked").unwrap();
        let (query, result) = query_capture(&fx, 2);
        let inv = fx.invocation(
            "data sql",
            &[
                ("--output", output.to_str().unwrap()),
                ("--overwrite", "true"),
            ],
        );
        let (manifest, _) = export(&inv, &fx.pin, &query, &result, &budget()).unwrap();
        verify_pair(&fx, &output, &manifest);
        std::fs::write(
            root.join("csv-helper-result.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn sql_csv_serializes_complete_pair_across_workspaces_and_processes() {
        let first = fixture();
        let second = fixture();
        let path = first.root.join("query.csv");
        // Distinct textual parent path and case still address the same Windows pair.
        let alias_dir = first.root.join("alias");
        std::fs::create_dir(&alias_dir).unwrap();
        let alias = alias_dir.join("..").join("QUERY.csv");
        let (query, result) = query_capture(&first, 1);
        let inv = first.invocation(
            "data sql",
            &[
                ("--output", path.to_str().unwrap()),
                ("--overwrite", "true"),
            ],
        );
        let until = Instant::now() + Duration::from_secs(20);
        let child_control = lycheedev_core::control::OperationControl::uncancelable(until);
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "local_query::query_csv::tests::sql_csv_cross_workspace_process_helper",
                "--nocapture",
            ])
            .env("LYCHEEDEV_SQL_CSV_PROCESS_ROOT", &second.root)
            .env("LYCHEEDEV_SQL_CSV_PROCESS_HOME", &second.home)
            .env("LYCHEEDEV_SQL_CSV_PROCESS_PIN", &second.pin)
            .env("LYCHEEDEV_SQL_CSV_PROCESS_OUTPUT", &alias);
        std::thread::scope(|scope| {
            let mut child = None;
            let first_manifest =
                export_after_data(&inv, &first.pin, &query, &result, &budget(), || {
                    // Existing Core runner owns JobGuard, bounded stdout/stderr and
                    // deadline cleanup/reaping. No raw Child or new process framework.
                    child = Some(scope.spawn(|| {
                        lycheedev_core::process::run(command, None, 65536, 65536, &child_control)
                    }));
                    while std::fs::read(second.root.join("csv-helper-blocked"))
                        .ok()
                        .as_deref()
                        != Some(b"blocked")
                    {
                        assert!(
                            Instant::now() < until,
                            "owned helper did not reach held lease"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    // Wait until the competing exporter has committed its CSV
                    // capture. It is now in the actual publication path, not
                    // parked behind a test-only retry gate before export.
                    loop {
                        let captures =
                            lycheedev_core::metadata::Metadata::open(&second.home, false)
                                .unwrap()
                                .list::<Value>("capture/", 100)
                                .unwrap();
                        if captures
                            .iter()
                            .any(|capture| capture["provenance"]["kind"] == "data-query-csv")
                        {
                            break;
                        }
                        assert!(
                            Instant::now() < until,
                            "competing exporter did not prepare CSV"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    assert!(!second.root.join("csv-helper-result.json").exists());
                    assert_eq!(std::fs::read(&path).unwrap(), b"value\n1\n");
                    assert!(!first.root.join("query.csv.manifest.json").exists());
                    Ok(())
                })
                .unwrap()
                .0;
            // The second process may replace the pair as soon as the first
            // lease closes. Verify the first immutable capture, not racing disk.
            let first_capture =
                evidence::read_capture(&first.home, first_manifest["csvCapture"].as_str().unwrap())
                    .unwrap();
            assert_eq!(
                evidence::read_blob(&first.home, &first_capture["blob"], MAX_BYTES).unwrap(),
                b"value\n1\n"
            );
            let output = child.take().unwrap().join().unwrap().unwrap();
            std::fs::write(second.root.join("csv-helper-stdout.log"), &output.stdout).unwrap();
            std::fs::write(second.root.join("csv-helper-stderr.log"), &output.stderr).unwrap();
            assert_eq!(
                output.status,
                0,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let final_manifest: Value = serde_json::from_slice(
                &std::fs::read(second.root.join("csv-helper-result.json")).unwrap(),
            )
            .unwrap();
            verify_pair(&second, &path, &final_manifest);
            assert_ne!(
                first_manifest["queryCapture"],
                final_manifest["queryCapture"]
            );
            assert_ne!(first_manifest["sha256"], final_manifest["sha256"]);
            assert_eq!(std::fs::read(&path).unwrap(), b"value\n2\n");
        });
    }
}
