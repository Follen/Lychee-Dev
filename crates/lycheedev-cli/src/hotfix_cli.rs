//! Hotfix CLI admission and exact terminal presentation.
use crate::Invocation;
use lycheedev_core::{
    Outcome, Result, ToolkitError, control::OperationControl, selection,
};
use lycheedev_data::{
    hotfix::{
        self, CacheFilter, LocalInput, LocalRequest, Request, ScanRequest,
        remote::{self, Coverage, Query, Record, RemoteResult},
    },
    request::{DataContext, SourceOptions},
    resource::Budget,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
fn fault(code: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(code, message, exit)
}
fn error(kind: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    let code = format!("records.hotfix_{kind}");
    fault(&code, exit, format!("{code}: {}", message.into()))
}
// The CLI contract accepts exactly eight unprefixed hexadecimal digits.
// Keep this separate from decimal record/region/index parsing.
fn parse_table_hash(value: &str) -> lycheedev_core::Result<u32> {
    if value.len() != 8 {
        return Err(lycheedev_core::ToolkitError::arguments(
            "--table-hash requires eight hexadecimal digits",
        ));
    }
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(lycheedev_core::ToolkitError::arguments(
            "invalid --table-hash",
        ));
    }
    u32::from_str_radix(value, 16)
        .map_err(|_| lycheedev_core::ToolkitError::arguments("invalid --table-hash"))
}
fn arg_number<T: std::str::FromStr>(inv: &Invocation, flag: &str) -> Result<Option<T>> {
    inv.flag(flag)
        .map(|v| {
            v.parse()
                .map_err(|_| error("query", 2, format!("invalid {flag}")))
        })
        .transpose()
}
fn uint(inv: &Invocation, flag: &str) -> Result<Option<u32>> {
    inv.flag(flag)
        .map(|v| {
            validate_option(flag, v)?;
            if flag == "--table-hash" {
                u32::from_str_radix(v, 16)
            } else {
                v.parse()
            }
            .map_err(|_| ToolkitError::arguments(format!("invalid {flag}")))
        })
        .transpose()
}
/// Go entry.go validates each admitted option before provider/workspace I/O.
/// The CLI calls this in argv order; direct provider users validate the same bounds.
pub(crate) fn validate_option(key: &str, value: &str) -> Result<()> {
    let signed = || value.parse::<i64>().ok();
    let unsigned = || {
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && value.parse::<u32>().is_ok()
    };
    let message = match key {
        "--source" if !["wago", "dbcache", "raidbots"].contains(&value) => {
            Some("--source must be wago, dbcache, or raidbots".into())
        }
        "--encoding" if !["csv", "raw", "png", "webp"].contains(&value) => {
            Some("--encoding must be raw, png, or webp".into())
        }
        "--table-hash" if value.len() != 8 => {
            Some("--table-hash requires eight hexadecimal digits".into())
        }
        "--table-hash" if !value.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some("invalid --table-hash".into())
        }
        "--record" | "--region-id" | "--after-index" if !unsigned() => {
            Some(format!("invalid {key}"))
        }
        "--push" if value.parse::<i32>().is_err() => Some("invalid --push".into()),
        "--status" if signed().is_none_or(|n| !(0..=255).contains(&n)) => {
            Some("--status must be between 0 and 255".into())
        }
        "--page" if signed().is_none_or(|n| n < 0) => Some("invalid --page".into()),
        "--limit" if signed().is_none_or(|n| !(1..=200).contains(&n)) => {
            Some("invalid --limit".into())
        }
        "--max-pages" if signed().is_none_or(|n| !(1..=2048).contains(&n)) => {
            Some("--max-pages must be between 1 and 2048".into())
        }
        "--max-requests" if signed().is_none_or(|n| n < 1) => {
            Some("--max-requests must be at least 1".into())
        }
        "--max-bytes" if signed().is_none_or(|n| !(1..=536870912).contains(&n)) => {
            Some("--max-bytes must be between 1 and 536870912".into())
        }
        "--timeout-seconds" if signed().is_none_or(|n| !(1..=3600).contains(&n)) => {
            Some("--timeout-seconds must be between 1 and 3600".into())
        }
        _ => None,
    };
    message.map_or(Ok(()), |m| Err(ToolkitError::arguments(m)))
}
fn local_number(inv: &Invocation, flag: &str) -> Result<Option<u32>> {
    inv.flag(flag)
        .map(|value| {
            if let Some(hex) = value.strip_prefix("0x") {
                u32::from_str_radix(hex, 16)
            } else {
                value.parse()
            }
        })
        .transpose()
        .map_err(|_| fault("records.hotfix_query", 2, format!("Invalid {flag}")))
}
fn local_limit(inv: &Invocation, flag: &str, default: usize, maximum: usize) -> Result<usize> {
    let value = inv
        .flag(flag)
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| ToolkitError::arguments(format!("Invalid {flag}")))?
        .unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(fault(
            "records.hotfix_limit",
            2,
            "Hotfix request exceeds input/page limits",
        ));
    }
    Ok(value)
}
fn validate_source_flags(inv: &Invocation) -> Result<()> {
    for flag in [
        "--raidbots",
        "--product",
        "--build",
        "--region",
        "--locale",
        "--search",
        "--to",
        "--cursor",
    ] {
        if inv.flag(flag).is_some_and(|value| !value.is_empty())
            && !(flag == "--cursor" && inv.flag_bool("--scan"))
        {
            return Err(ToolkitError::arguments(format!(
                "{flag} is not supported by the dbcache hotfix source"
            )));
        }
    }
    for flag in ["--page", "--max-pages", "--max-requests"] {
        if flag == "--max-pages" && inv.flag_bool("--scan") {
            continue;
        }
        if local_number(inv, flag)?.is_some_and(|value| value != 0) {
            return Err(ToolkitError::arguments(format!(
                "{flag} is not supported by the dbcache hotfix source"
            )));
        }
    }
    if inv.flag("--encoding") == Some("csv") {
        return Err(ToolkitError::arguments(
            "--encoding csv is not supported by the dbcache hotfix source",
        ));
    }
    Ok(())
}
fn remote_query(inv: &Invocation) -> Result<Query> {
    for (key, value) in &inv.flags {
        validate_option(key, value)?;
    }
    let source = inv.required("--source")?;
    if source == "raidbots" && inv.flag("--raidbots").unwrap_or_default().is_empty() {
        return Err(ToolkitError::arguments(
            "data hotfix --source raidbots requires --raidbots",
        ));
    }
    if source == "wago" && inv.flag("--raidbots").is_some_and(|v| !v.is_empty()) {
        return Err(ToolkitError::arguments(
            "--raidbots is not supported by the wago hotfix source",
        ));
    }
    if inv.flag("--dbcache").is_some_and(|v| !v.is_empty()) {
        return Err(ToolkitError::arguments(format!(
            "--dbcache is not supported by the {source} hotfix source"
        )));
    }
    if ["--product", "--build", "--region", "--locale"]
        .iter()
        .any(|f| inv.flag(f).unwrap_or_default().is_empty())
    {
        return Err(ToolkitError::arguments(format!(
            "data hotfix --source {source} requires --product, --build, --region, and --locale"
        )));
    }
    if inv.flag("--after-index").is_some() {
        return Err(ToolkitError::arguments(format!(
            "data hotfix --source {source} pages with --cursor, not --after-index"
        )));
    }
    let time = |flag| {
        inv.flag(flag)
            .filter(|s| !s.is_empty())
            .map(remote::normalize_time)
            .transpose()
    };
    let query = Query {
        product: inv.required("--product")?.into(),
        full_build: inv.required("--build")?.into(),
        region: inv.required("--region")?.into(),
        locale: inv.required("--locale")?.into(),
        table: inv.flag("--table").unwrap_or_default().into(),
        table_hash: uint(inv, "--table-hash")?,
        record_id: uint(inv, "--record")?,
        push_id: arg_number(inv, "--push")?,
        region_id: uint(inv, "--region-id")?,
        status: arg_number(inv, "--status")?,
        from: time("--from")?,
        to: time("--to")?,
        search: inv.flag("--search").unwrap_or_default().into(),
        latest: inv.flag_bool("--latest"),
        page: arg_number(inv, "--page")?.unwrap_or(0),
        limit: arg_number(inv, "--limit")?.unwrap_or(50),
        cursor: inv.flag("--cursor").unwrap_or_default().into(),
        definition: None,
    };
    query.validate()?;
    Ok(query)
}

fn absolute(inv: &Invocation, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.into()
    } else {
        inv.cwd.join(path)
    }
}
fn local_request(inv: &Invocation) -> Result<LocalRequest> {
    let snapshot = match inv.flag("--snapshot") {
        Some(_) => inv.required("--snapshot")?.to_owned(),
        None => crate::core_cli::resolve_pinned(inv)?.id,
    };
    let file = inv.flag("--dbcache").filter(|v| !v.is_empty());
    let from = inv.flag("--from").filter(|v| !v.is_empty());
    if file.is_some() == from.is_some() {
        return Err(ToolkitError::arguments(
            "data hotfix --source dbcache requires exactly one of --dbcache or --from",
        ));
    }
    validate_source_flags(inv)?;
    let table = inv
        .flag("--table")
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if table.is_some() && inv.flag("--table-hash").is_some() {
        return Err(fault(
            "records.hotfix_identity",
            2,
            "Select --table or --table-hash, not both",
        ));
    }
    if inv.flag("--after-index").is_some() && from.is_none() {
        return Err(fault(
            "records.hotfix_identity",
            2,
            "Pagination requires an archived --from",
        ));
    }
    let max_bytes = local_limit(inv, "--max-bytes", 128 << 20, 512 << 20)?;
    let filter = CacheFilter {
        build: 0,
        table_hash: inv.flag("--table-hash").map(parse_table_hash).transpose()?,
        record_id: local_number(inv, "--record")?,
        push: inv
            .flag("--push")
            .map(str::parse)
            .transpose()
            .map_err(|_| fault("records.hotfix_query", 2, "Invalid signed push"))?,
        region: local_number(inv, "--region-id")?,
        status: local_number(inv, "--status")?
            .map(|v| {
                u8::try_from(v)
                    .map_err(|_| fault("records.hotfix_query", 2, "Status outside 0..255"))
            })
            .transpose()?,
        latest: inv.flag_bool("--latest"),
        after_index: local_number(inv, "--after-index")?,
        limit: local_limit(inv, "--limit", 20, 200)?,
    };
    Ok(LocalRequest {
        snapshot,
        input: match (file, from) {
            (Some(file), None) => LocalInput::File(absolute(inv, file)),
            (None, Some(id)) => LocalInput::Capture(id.into()),
            _ => unreachable!("admitted cache source"),
        },
        table,
        filter,
        max_bytes,
        offline: inv.flag_bool("--offline"),
    })
}
pub(crate) fn admit(inv: &Invocation) -> Result<(DataContext, Request)> {
    inv.control.check()?;
    if inv.flag_bool("--scan") && inv.flag("--source") != Some("dbcache") {
        return Err(ToolkitError::arguments("--scan requires --source dbcache"));
    }
    let source = SourceOptions {
        snapshot: inv.flag("--snapshot").map(str::to_owned),
        offline: inv.flag_bool("--offline"),
        ..Default::default()
    };
    let ctx = crate::data_cli::context(inv, source);
    let request = match inv.required("--source")? {
        "dbcache" => {
            let page = local_request(inv)?;
            if inv.flag_bool("--scan") {
                let pages = inv
                    .flag("--max-pages")
                    .map(str::parse::<i64>)
                    .transpose()
                    .map_err(|_| ToolkitError::arguments("Invalid --max-pages"))?
                    .unwrap_or(100);
                if page.filter.after_index.is_some() || !(1..=1000).contains(&pages) {
                    return Err(fault(
                        "records.hotfix_limit",
                        3,
                        "Hotfix scan exceeds its bound",
                    ));
                }
                Request::Scan(ScanRequest {
                    page,
                    cursor: inv.flag("--cursor").unwrap_or("").into(),
                    max_pages: pages as usize,
                })
            } else {
                Request::Local(page)
            }
        }
        source @ ("wago" | "raidbots") => {
            if inv.flag_bool("--scan") {
                return Err(ToolkitError::arguments("--scan requires --source dbcache"));
            }
            let query = remote_query(inv)?;
            let provider = if source == "wago" {
                remote::Provider::Wago
            } else {
                remote::Provider::Raidbots(absolute(inv, inv.required("--raidbots")?))
            };
            let fetch = remote::FetchBudget {
                max_requests: inv
                    .flag("--max-requests")
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| ToolkitError::arguments("--max-requests must be at least 1"))?
                    .unwrap_or(0),
                max_bytes: inv
                    .flag("--max-bytes")
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| {
                        ToolkitError::arguments("--max-bytes must be between 1 and 536870912")
                    })?
                    .unwrap_or(128 << 20),
                ..Default::default()
            };
            let max_pages = inv
                .flag("--max-pages")
                .map(str::parse)
                .transpose()
                .map_err(|_| error("budget", 3, "budgets must be positive"))?
                .unwrap_or(2048);
            let csv = if inv.flag("--encoding") == Some("csv") {
                Some(absolute(
                    inv,
                    inv.flag("--output")
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| {
                            ToolkitError::arguments("data hotfix --encoding csv requires --output")
                        })?,
                ))
            } else {
                None
            };
            Request::Remote(remote::Request {
                provider,
                query,
                max_pages,
                fetch,
                csv,
            })
        }
        _ => {
            return Err(ToolkitError::arguments(
                "--source must be wago, dbcache, or raidbots",
            ));
        }
    };
    Ok((ctx, request))
}
pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let (ctx, request) = admit(inv)?;
    hotfix::dispatch(&ctx, &request)
}
fn output_failure(original: ToolkitError) -> ToolkitError {
    fault("command.output_failed", 5, original.message.clone())
        .with_details(json!({"originalCause":original}))
}
pub(crate) fn render(
    inv: &Invocation,
    timeout: Option<u64>,
    out: &mut impl std::io::Write,
) -> Result<Outcome> {
    let (ctx, request) = admit(inv)?;
    let Request::Remote(request) = request else {
        return Err(ToolkitError::arguments(
            "Hotfix terminal stream requires a remote source",
        ));
    };
    remote::with_result(&ctx, &request, |result, budget| {
        render_result(inv, result, timeout, out, budget).map_err(output_failure)?;
        Ok(remote::outcome(result, Value::Null))
    })
}
#[derive(Serialize)]
struct Context<'a> {
    query: &'a Query,
    coverage: &'a Coverage,
    change: &'a selection::ChangePin,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame<'a> {
    schema: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    operation_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<Context<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<&'a Record>,
    #[serde(skip_serializing_if = "Option::is_none")]
    complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
}
fn jsonl(
    result: &RemoteResult,
    control: &OperationControl,
    budget: &Budget,
    emit: &mut impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let frame = |kind| Frame {
        schema: "lycheedev.jsonl.v1",
        kind,
        operation_id: &result.snapshot,
        context: None,
        index: None,
        record: None,
        complete: None,
        truncated: None,
        count: None,
        warnings: &[],
    };
    let mut send = |value: Frame<'_>| -> Result<()> {
        control.check()?;
        let mut raw = remote::wire_bytes(&value, budget, usize::MAX)?;
        raw.push(b'\n');
        control.check()?;
        budget.check()?;
        emit(&raw)
    };
    let mut begin = frame("begin");
    begin.context = Some(Context {
        query: &result.query,
        coverage: &result.coverage,
        change: &result.change,
    });
    send(begin)?;
    for (index, record) in result.records.iter().enumerate() {
        let mut row = frame("record");
        row.index = Some(index);
        row.record = Some(record);
        send(row)?;
    }
    let mut end = frame("end");
    end.complete = Some(result.complete);
    end.truncated = Some(result.truncated);
    end.count = Some(result.records.len());
    end.warnings = &result.warnings;
    send(end)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    schema: &'static str,
    ok: bool,
    operation_id: &'static str,
    context: Value,
    result: &'a RemoteResult,
    captures: &'a [Value],
    warnings: &'a [Value],
    error: Option<()>,
}
fn render_result(
    inv: &Invocation,
    result: &RemoteResult,
    timeout: Option<u64>,
    out: &mut impl std::io::Write,
    budget: &Budget,
) -> Result<()> {
    if inv.flag("--format") == Some("jsonl") {
        return jsonl(result, &inv.control, budget, &mut |raw| {
            out.write_all(raw)?;
            out.flush()?;
            Ok(())
        });
    }
    inv.control.check()?;
    if inv.flag("--format") == Some("json") {
        let outcome = remote::outcome(result, Value::Null);
        let mut context = outcome.selection;
        if let Some(seconds) = timeout {
            context["timeoutSeconds"] = json!(seconds);
        }
        let envelope = Envelope {
            schema: "lycheedev.result.v1",
            ok: true,
            operation_id: "",
            context,
            result,
            captures: &outcome.provenance,
            warnings: &outcome.warnings,
            error: None,
        };
        lycheedev_core::public_json::write_serializable_line(out, &envelope)?;
    } else {
        lycheedev_core::public_json::write_pretty_serializable_line(out, result)?;
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_hotfix_source_is_required_before_input_or_pin_reads() {
        let home = std::env::temp_dir().join(format!("cli-admission-unused-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let words = ["data", "hotfix", "--source", "dbcache", "--snapshot", "PIN-explicit", "--dbcache", "not-created.bin"].map(str::to_owned);
        let mut inv = crate::parse(&words).unwrap();
        inv.home = home.clone();
        let (_, Request::Local(local)) = admit(&inv).unwrap() else { panic!("local hotfix"); };
        assert_eq!(local.snapshot, "PIN-explicit");
        assert!(matches!(local.input, LocalInput::File(ref file) if file == &inv.cwd.join("not-created.bin")));
        let mut incomplete = words.to_vec();
        incomplete.drain(2..4);
        let mut inv = crate::parse(&incomplete).unwrap();
        inv.home = home.clone();
        let error = admit(&inv).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("--source"));
        assert!(!home.exists());
    }
    use lycheedev_data::resource::Cost;
    use std::{
        collections::BTreeMap,
        time::{Duration, Instant},
    };
    fn invocation(source: &str) -> Invocation {
        Invocation {
            command: "data hotfix".into(),
            positional: None,
            home: PathBuf::from("missing-hotfix-home"),
            cwd: PathBuf::from("fixture-cwd"),
            control: Default::default(),
            flags: BTreeMap::from([
                ("--source".into(), source.into()),
                ("--product".into(), "retail".into()),
                ("--build".into(), "12.0.7.68974".into()),
                ("--region".into(), "us".into()),
                ("--locale".into(), "enUS".into()),
            ]),
        }
    }
    fn local(scan: bool) -> Invocation {
        let mut inv = invocation("dbcache");
        for key in ["--product", "--build", "--region", "--locale"] {
            inv.flags.remove(key);
        }
        inv.flags.insert("--snapshot".into(), "PIN-fixture".into());
        inv.flags.insert("--dbcache".into(), "cache.bin".into());
        if scan {
            inv.flags.insert("--scan".into(), "true".into());
        }
        inv
    }
    #[test]
    fn source_flags_and_unknown_source_fail_before_io() {
        let mut inv = invocation("raidbots");
        assert_eq!(
            admit(&inv).unwrap_err().message,
            "data hotfix --source raidbots requires --raidbots"
        );
        inv.flags.insert("--source".into(), "other".into());
        assert_eq!(
            admit(&inv).unwrap_err().message,
            "--source must be wago, dbcache, or raidbots"
        );
        inv.flags.insert("--source".into(), "wago".into());
        inv.flags.insert("--dbcache".into(), "x".into());
        assert!(admit(&inv).unwrap_err().message.contains("--dbcache"));
        inv.flags.remove("--dbcache");
        inv.flags.insert("--scan".into(), "true".into());
        assert_eq!(
            admit(&inv).unwrap_err().message,
            "--scan requires --source dbcache"
        );
    }
    #[test]
    fn local_argument_combinations_keep_precise_errors_without_workspace_io() {
        for (flag, value, code) in [
            ("--table-hash", "0000002A", "records.hotfix_identity"),
            ("--after-index", "0", "records.hotfix_identity"),
            ("--cursor", "0", "command.invalid_arguments"),
            ("--encoding", "csv", "command.invalid_arguments"),
        ] {
            let mut inv = local(false);
            inv.flags.insert("--table".into(), "Example".into());
            inv.flags.insert(flag.into(), value.into());
            assert_eq!(admit(&inv).unwrap_err().code, code, "{flag}");
        }
        for (flag, value) in [("--limit", "0"), ("--limit", "201"), ("--max-bytes", "0")] {
            let mut inv = local(false);
            inv.flags.insert(flag.into(), value.into());
            let error = admit(&inv).unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("records.hotfix_limit", 2)
            );
        }
    }
    #[test]
    fn scan_argument_limits_keep_query_errors_and_scan_errors_distinct() {
        for (flag, value, exit) in [
            ("--max-pages", "-1", 3),
            ("--max-pages", "1001", 3),
            ("--limit", "0", 2),
            ("--limit", "201", 2),
            ("--max-bytes", "0", 2),
            ("--max-bytes", "536870913", 2),
            ("--after-index", "0", 2),
        ] {
            let mut inv = local(true);
            inv.flags.insert(flag.into(), value.into());
            assert_eq!(admit(&inv).unwrap_err().exit_code, exit, "{flag}={value}");
        }
        for flag in [
            "--raidbots",
            "--product",
            "--build",
            "--region",
            "--locale",
            "--search",
            "--to",
            "--page",
            "--max-requests",
        ] {
            let mut inv = local(true);
            inv.flags.insert(flag.into(), "1".into());
            assert_eq!(admit(&inv).unwrap_err().exit_code, 2, "{flag}");
        }
        for source in [None, Some("wago"), Some("raidbots"), Some("other")] {
            let mut inv = local(true);
            inv.flags.remove("--source");
            if let Some(source) = source {
                inv.flags.insert("--source".into(), source.into());
            }
            let error = admit(&inv).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.message, "--scan requires --source dbcache");
            assert_eq!(error.exit_code, 2);
        }
    }
    #[test]
    fn query_time_window_normalizes_typed_rfc3339_instants() {
        let mut inv = invocation("wago");
        inv.flags.insert("--offline".into(), "true".into());
        inv.flags
            .insert("--from".into(), "2026-10-16T00:00:00+02:00".into());
        inv.flags
            .insert("--to".into(), "2026-10-15T22:00:00Z".into());
        let q = remote_query(&inv).unwrap();
        assert_eq!(q.from.as_deref(), Some("2026-10-16T00:00:00+02:00"));
        assert_eq!(q.to.as_deref(), Some("2026-10-15T22:00:00Z"));
        let scope = q.scope().unwrap();
        assert!(scope.contains("from:2026-10-15T22:00:00Z"));
        assert!(scope.contains("to:2026-10-15T22:00:00Z"));
        assert_eq!(
            remote::normalize_time(q.from.as_deref().unwrap()).unwrap(),
            "2026-10-16T00:00:00+02:00"
        );
        for invalid in ["2026-10-16T00:00:00+24:00", "not-a-time"] {
            let mut bad = inv.clone();
            bad.flags.insert("--from".into(), invalid.into());
            assert!(remote_query(&bad).is_err(), "{invalid}");
        }
        inv.flags
            .insert("--from".into(), "2026-10-15 01:02:03".into());
        let q = remote_query(&inv).unwrap();
        assert_eq!(q.from.as_deref(), Some("2026-10-15T01:02:03Z"));
        assert!(q.scope().unwrap().contains("from:2026-10-15T01:02:03Z"));
        assert!(serde_json::to_value(q).is_ok());
        assert!(remote::normalize_time("2026-10-15T25:00:00Z").is_err());
        assert!(remote::normalize_time("").is_err());
    }
    fn sample_result() -> RemoteResult {
        let q = Query {
            product: "retail".into(),
            full_build: "12.0.7.68974".into(),
            region: "us".into(),
            locale: "enUS".into(),
            limit: 50,
            ..Default::default()
        };
        let coverage = Coverage::new("wago", &q);
        RemoteResult::new(
            "wago",
            q,
            vec![Record::default(), Record::default()],
            coverage,
            String::new(),
        )
        .unwrap()
    }
    fn ledger() -> Budget {
        Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        )
    }
    #[test]
    fn jsonl_end_only_after_all_records_and_success() {
        let r = sample_result();
        let control = OperationControl::default();
        let budget = ledger();
        let mut frames = Vec::new();
        jsonl(&r, &control, &budget, &mut |raw| {
            frames.push(serde_json::from_slice::<Value>(raw).unwrap());
            Ok(())
        })
        .unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0]["schema"], "lycheedev.jsonl.v1");
        assert_eq!(frames[1]["index"], 0);
        assert_eq!(frames[3]["type"], "end");
        assert_eq!(frames[3]["count"], 2);
        let mut types = Vec::new();
        let result = jsonl(&r, &control, &ledger(), &mut |raw| {
            let v: Value = serde_json::from_slice(raw).unwrap();
            types.push(v["type"].as_str().unwrap().to_owned());
            if types.len() == 2 {
                Err(ToolkitError::io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "closed",
                )))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(!types.contains(&"end".into()));
    }
    #[test]
    fn typed_terminal_output_preserves_payload_tokens_and_duplicate_members() {
        let mut r = sample_result();
        r.records[0].payload = Some(
            serde_json::value::RawValue::from_string(
                r#" { "dup":1e+02, "dup":2, "html":"<&>" } "#.into(),
            )
            .unwrap(),
        );
        let budget = ledger();
        let mut lines = Vec::new();
        jsonl(&r, &OperationControl::default(), &budget, &mut |raw| {
            lines.extend_from_slice(raw);
            Ok(())
        })
        .unwrap();
        assert!(
            String::from_utf8(lines)
                .unwrap()
                .contains(r#""dup":1e+02,"dup":2"#)
        );
        let mut inv = invocation("wago");
        inv.flags.insert("--format".into(), "json".into());
        let mut public = Vec::new();
        render_result(&inv, &r, Some(300), &mut public, &budget).unwrap();
        let public = String::from_utf8(public).unwrap();
        assert!(public.contains(r#""dup":1e+02,"dup":2"#));
        assert!(public.contains(r#""html":"<&>""#));
        assert!(public.contains(r#""timeoutSeconds":300"#));
    }
    #[test]
    fn terminal_output_projection_preserves_original_typed_cause() {
        let original =
            ToolkitError::new("records.query_resource_budget", "stopped during output", 3)
                .with_details(json!({"reason":"fixture"}));
        let error = output_failure(original.clone());
        assert_eq!(error.code, "command.output_failed");
        assert_eq!(error.exit_code, 5);
        assert_eq!(
            error.details["originalCause"],
            serde_json::to_value(original).unwrap()
        );
    }
    #[test]
    fn hotfix_limits_do_not_change_the_physical_source_budget_and_paths_use_cli_cwd() {
        let mut inv = invocation("raidbots");
        inv.flags.insert("--raidbots".into(), "cache.bin".into());
        inv.flags.insert("--max-bytes".into(), "1".into());
        let (ctx, Request::Remote(request)) = admit(&inv).unwrap() else {
            panic!("remote")
        };
        assert_eq!(request.fetch.max_bytes, 1);
        assert_eq!(
            ctx.source.max_file_bytes,
            SourceOptions::default().max_file_bytes
        );
        let remote::Provider::Raidbots(path) = request.provider else {
            panic!("raidbots")
        };
        assert_eq!(path, inv.cwd.join("cache.bin"));
    }
}
