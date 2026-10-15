//! Independent bounded workspace checks. Provider-specific checks are composed at the entry point.
use crate::{
    Invocation, Outcome, Result, ToolkitError, cache, metadata::Metadata, storage, workspace,
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};
fn check(
    id: &str,
    ok: bool,
    status: &str,
    code: &str,
    detail: impl Into<String>,
    next: &str,
) -> Value {
    let mut value = json!({"id":id,"ok":ok,"status":status,"detail":detail.into()});
    if !code.is_empty() {
        value["code"] = json!(code);
    }
    if !next.is_empty() {
        value["nextStep"] = json!(next);
    }
    value
}
fn files(root: &Path, control: &crate::control::OperationControl) -> Result<usize> {
    if !root.exists() {
        return Ok(0);
    }
    storage::guard_path(root, root)?;
    let mut stack = vec![root.to_path_buf()];
    let mut count = 0usize;
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            control.check()?;
            let entry = entry?;
            storage::guard_path(root, &entry.path())?;
            count += 1;
            if count > 100000 {
                return Err(ToolkitError::new(
                    "vault.check_limit",
                    "doctor file scan exceeds 100000 entries",
                    3,
                ));
            }
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(count)
}
pub fn inspect(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let mut checks = vec![];
    let identity = workspace::open(&inv.home);
    match identity {
        Err(error) => {
            let code = if !inv.home.exists() {
                "vault.workspace_missing"
            } else if !inv.home.join("workspace.json").exists() {
                "vault.legacy_detected"
            } else {
                error.code.as_str()
            };
            checks.push(check(
                "vault.workspace",
                false,
                "error",
                code,
                error.message,
                "run explicit init into an empty root, or inspect the selected marker",
            ));
        }
        Ok(identity) => {
            checks.push(check(
                "vault.workspace",
                true,
                "ok",
                "",
                format!(
                    "workspace marker valid: schema {}, id {}",
                    identity["schema"].as_str().unwrap_or(""),
                    identity["workspaceId"].as_str().unwrap_or("")
                ),
                "",
            ));
            match Metadata::open(&inv.home, false) {
                Ok(metadata) => {
                    checks.push(check(
                        "vault.metadata",
                        true,
                        "ok",
                        "",
                        "metadata readable; schema version 1",
                        "",
                    ));
                    let targets: Vec<Value> = metadata.list("target/", 10001)?;
                    if targets.len() > 10000 {
                        return Err(ToolkitError::new(
                            "selection.target_scan_limit",
                            "doctor target scan exceeds 10000 entries",
                            3,
                        ));
                    }
                    let mut problems = vec![];
                    let mut remote = vec![];
                    for target in &targets {
                        if target["schema"] != "lycheedev.target.v1"
                            || !crate::selection::valid_product(
                                target["product"].as_str().unwrap_or(""),
                            )
                        {
                            problems.push(target["name"].clone());
                        }
                        if target["source"] == "installation"
                            && !Path::new(target["installation"].as_str().unwrap_or("")).is_dir()
                        {
                            problems.push(target["name"].clone());
                        }
                        if target["source"] == "remote" {
                            remote.push(target["name"].clone());
                        }
                    }
                    let mut item = check(
                        "selection.named_targets",
                        problems.is_empty(),
                        if problems.is_empty() { "ok" } else { "error" },
                        if problems.is_empty() {
                            ""
                        } else {
                            "selection.target_unresolvable"
                        },
                        format!(
                            "{} named target(s); remote currency is unknown without a provider observation",
                            targets.len()
                        ),
                        "inspect target list and resolve only the needed target",
                    );
                    item["data"] = json!({"remote":remote,"unresolvable":problems});
                    checks.push(item);
                }
                Err(error) => checks.push(check(
                    "vault.metadata",
                    false,
                    if error.code == "vault.record_missing" {
                        "warn"
                    } else {
                        "error"
                    },
                    &error.code,
                    error.message,
                    "inspect state/toolkit.sqlite; a fresh workspace may have no database",
                )),
            }
            let tmp = inv.home.join("tmp");
            let probe = tmp.join(format!(
                ".lycheedev-write-probe-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let writable = (|| -> Result<()> {
                storage::guard_path(&inv.home, &tmp)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&probe)?;
                file.write_all(b"probe")?;
                drop(file);
                fs::remove_file(&probe)?;
                Ok(())
            })();
            match writable {
                Ok(()) => checks.push(check(
                    "vault.writable",
                    true,
                    "ok",
                    "",
                    "workspace tmp/ accepts writes",
                    "",
                )),
                Err(error) => checks.push(check(
                    "vault.writable",
                    false,
                    "error",
                    "vault.workspace_readonly",
                    error.message,
                    "check selected workspace permissions",
                )),
            }
            let lock_count = files(&inv.home.join("locks"), &inv.control)?;
            checks.push(check(
                "vault.lock_hygiene",
                true,
                "ok",
                "",
                format!("{lock_count} lock files retained to preserve lock identity"),
                "",
            ));
            let temporary =
                files(&tmp, &inv.control)? + files(&inv.home.join("cache/tmp"), &inv.control)?;
            checks.push(check(
                "vault.tmp_hygiene",
                temporary == 0,
                if temporary == 0 { "ok" } else { "warn" },
                if temporary == 0 {
                    ""
                } else {
                    "vault.stale_temporary_files"
                },
                format!("{temporary} leftover temporary entries"),
                "unfinished files are not deleted by doctor",
            ));
            let mut verify = inv.clone();
            verify.command = "cache verify".into();
            match cache::dispatch(&verify) {
                Ok(report) => checks.push(check(
                    "vault.cache_integrity",
                    true,
                    "ok",
                    "",
                    format!(
                        "objects={} bytes={}",
                        report.data["checked"], report.data["bytes"]
                    ),
                    "",
                )),
                Err(error) => {
                    let mut item = check(
                        "vault.cache_integrity",
                        false,
                        "error",
                        &error.code,
                        error.message,
                        "run cache verify for the original integrity report",
                    );
                    item["data"] = error.details;
                    checks.push(item);
                }
            }
        }
    }
    let journal = crate::archive::journal_path(&inv.home)?;
    if journal.exists() {
        let plan = crate::archive::plan(&inv.home);
        match plan {
            Ok(plan) => checks.push(check(
                "vault.archive_state",
                plan["state"] == "complete",
                if plan["state"] == "complete" {
                    "ok"
                } else {
                    "warn"
                },
                if plan["state"] == "complete" {
                    ""
                } else {
                    "vault.archive_incomplete"
                },
                format!("archive switch state={}", plan["state"]),
                "resume the exact explicit fresh initialization",
            )),
            Err(error) => checks.push(check(
                "vault.archive_state",
                false,
                "warn",
                &error.code,
                error.message,
                "inspect the selected archive-state journal",
            )),
        }
    } else {
        checks.push(check(
            "vault.archive_state",
            true,
            "ok",
            "",
            "no archive switch recorded for this root",
            "",
        ));
    }
    let baselines = &crate::client::baselines::VERIFIED_BASELINES;
    let problems = crate::client::baselines::catalog_problems(baselines);
    checks.push(if problems.is_empty() {
        check(
            "selection.catalog",
            true,
            "ok",
            "",
            format!(
                "{} verified client baselines are self-consistent",
                baselines.len()
            ),
            "",
        )
    } else {
        check(
            "selection.catalog",
            false,
            "error",
            "selection.catalog_invalid",
            problems.join("; "),
            "regenerate or repair the verified client baselines before resolving targets",
        )
    });
    let healthy = checks.iter().all(|item| item["status"] != "error");
    let result = json!({"healthy":healthy,"checks":checks});
    if !healthy {
        return Err(ToolkitError::new(
            "doctor.unhealthy",
            "workspace has error-status health checks",
            3,
        )
        .with_details(json!({"result":result,"context":{"workspace":inv.home}})));
    }
    let mut outcome = Outcome::new(result);
    outcome.selection = json!({"workspace":inv.home});
    Ok(outcome)
}
/// Entry adapters supply provider-owned observations; Core remains independent
/// of all business crates and owns the common health/status/exit aggregation.
pub fn inspect_with_checks(inv: &Invocation, providers: Vec<Value>) -> Result<Outcome> {
    if providers.len() > 64 {
        return Err(ToolkitError::new(
            "doctor.check_budget",
            "provider check count exceeds64",
            3,
        ));
    }
    let mut outcome = match inspect(inv) {
        Ok(outcome) => outcome,
        Err(error) if error.code == "doctor.unhealthy" => {
            Outcome::new(error.details["result"].clone())
        }
        Err(error) => return Err(error),
    };
    for provider in providers {
        inv.control.check()?;
        if provider["id"].as_str().is_none_or(|id| id.is_empty())
            || !provider["ok"].is_boolean()
            || !["ok", "warn", "error", "unknown"]
                .contains(&provider["status"].as_str().unwrap_or("ok"))
        {
            return Err(ToolkitError::invalid(
                "provider health check shape is invalid",
            ));
        }
        outcome.data["checks"]
            .as_array_mut()
            .ok_or_else(|| ToolkitError::invalid("base health check shape is invalid"))?
            .push(provider);
    }
    let healthy = outcome.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] != "error");
    outcome.data["healthy"] = json!(healthy);
    outcome.selection = json!({"workspace":inv.home});
    if !healthy {
        return Err(ToolkitError::new(
            "doctor.unhealthy",
            "workspace has error-status health checks",
            3,
        )
        .with_details(json!({"result":outcome.data,"context":outcome.selection})));
    }
    Ok(outcome)
}
