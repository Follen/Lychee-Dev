//! Independently assembled workspace and selection checks. Provider probes run
//! afterward; an unhealthy workspace does not suppress unrelated diagnostics.
use crate::control::OperationControl;
use crate::{
    Invocation, Outcome, Result, ToolkitError, cache, metadata::Metadata, storage, workspace,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
mod legacy;
pub use legacy::{LegacyRoot, legacy_candidates};
mod probes;
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
        value["code"] = json!(code)
    }
    if !next.is_empty() {
        value["nextStep"] = json!(next)
    }
    value
}
fn finish<T>(result: Result<T>, control: &OperationControl) -> Result<T> {
    crate::selection::selection_finish(result, control.check())
}
// Classify diagnostics and terminal cancellation from one observation. A stop
// arriving after a successful probe must never enter an unreachable branch.
fn diagnostic<T>(result: Result<T>, control: &OperationControl) -> Result<Result<T>> {
    match (result, control.check()) {
        (result, Ok(())) => Ok(result),
        (Ok(_), Err(stop)) => Err(stop),
        (Err(primary), Err(stop)) => {
            crate::selection::selection_finish::<T>(Err(primary), Err(stop)).map(Ok)
        }
    }
}
fn workspace_checks(
    inv: &Invocation,
    legacy: &[LegacyRoot],
    checks: &mut Vec<Value>,
) -> Result<()> {
    let (identity, missing) = workspace::inspect_controlled(&inv.home, &inv.control);
    let identity = diagnostic(identity, &inv.control)?;
    let available = identity.is_ok();
    checks.push(match identity {
        Ok(identity)=>check("vault.workspace",true,"ok","",format!("workspace marker valid: schema {}, id {}",identity["schema"].as_str().unwrap_or(""),identity["workspaceId"].as_str().unwrap_or("")),""),
        Err(error)=>match error.code.as_str(){
            "vault.workspace_schema_newer"=>check("vault.workspace",false,"error","vault.workspace_schema_newer","workspace marker was written by a newer toolkit","upgrade the toolkit; writes are refused to protect the data"),
            "vault.unsupported_workspace"=>check("vault.workspace",false,"error","vault.unsupported_workspace","workspace marker is corrupt or unsupported","inspect workspace.json by hand or reinitialize into a fresh root; nothing is overwritten automatically"),
            "vault.legacy_detected"=>check("vault.workspace",false,"error","vault.legacy_detected","root is occupied without the new workspace marker","run an explicit fresh init to isolate the legacy root, or pick another --home"),
            _ if missing=>check("vault.workspace",false,"error","vault.workspace_missing",format!("no workspace at {}",inv.home.display()),"run init to create a workspace"),
            _=>check("vault.workspace",false,"error","vault.unsupported_workspace",error.message,"check the workspace path and permissions"),
        }
    });
    if available {
        checks.push(probes::metadata(inv)?);
        checks.push(probes::writable(inv)?);
        checks.extend(probes::hygiene(inv)?);
        checks.push(probes::cache(inv)?);
    }
    checks.push(probes::archive(inv)?);
    if !legacy.is_empty() {
        checks.push(legacy::inspect(legacy, &inv.control)?)
    }
    inv.control.check()
}
fn selection_checks(inv: &Invocation, checks: &mut Vec<Value>) -> Result<()> {
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
    let result = crate::selection::target::list_configurations(&inv.home, &inv.control);
    let result = diagnostic(result, &inv.control)?;
    let configs = match result {
        Ok(v) => v,
        Err(e) => {
            checks.push(check(
                "selection.named_targets",
                false,
                "error",
                "selection.target_format",
                e.message,
                "run target list against a valid workspace root",
            ));
            return Ok(());
        }
    };
    if configs.is_empty() {
        checks.push(check(
            "selection.named_targets",
            true,
            "ok",
            "",
            "no named target configurations",
            "",
        ));
        return Ok(());
    }
    let mut problems = Vec::new();
    let mut remote = Vec::new();
    for config in &configs {
        inv.control.check()?;
        if let Err(e) = config.validate() {
            problems.push(format!("{}: {}", config.name, e.message));
            continue;
        }
        match config.source.as_str() {
            "installation" => {
                if !fs::symlink_metadata(&config.installation).is_ok_and(|i| i.is_dir()) {
                    problems.push(format!(
                        "{}: installation directory is missing ({})",
                        config.name, config.installation
                    ));
                }
            }
            "remote" => remote.push(config.name.clone()),
            _ => {}
        }
        inv.control.check()?;
    }
    checks.push(if !problems.is_empty(){check("selection.named_targets",false,"error","selection.target_unresolvable",problems.join("; "),"fix or re-add the named target (target add replaces it explicitly)")}else{
        let mut detail=format!("{} named target(s) validate and resolve locally",configs.len());
        if !remote.is_empty(){detail.push_str(&format!("; remote currency unknown offline for {}",remote.join(", ")))}
        let mut item=check("selection.named_targets",true,"ok","",detail,"run target resolve (or the remote availability listing) to observe current remote releases");item["data"]=json!({"remote":remote});item
    });
    Ok(())
}
fn partial(mut error: ToolkitError, inv: &Invocation, checks: Vec<Value>) -> ToolkitError {
    let primary = error.clone();
    error.details = json!({"schema":"lycheedev.doctor-error.v1","primary":primary,"context":{"workspace":inv.home},"result":{"healthy":false,"checks":checks}});
    error
}
pub fn inspect(inv: &Invocation) -> Result<Outcome> {
    inspect_with_provider(inv, &[], || Ok(Vec::new()))
}
/// Existing callers keep their provider shape contract. The actual entrypoint
/// supplies a closure so expensive provider observations follow Core checks.
pub fn inspect_with_checks(inv: &Invocation, providers: Vec<Value>) -> Result<Outcome> {
    inspect_with_provider(inv, &[], || Ok(providers))
}
pub fn inspect_with_provider(
    inv: &Invocation,
    legacy: &[LegacyRoot],
    provider: impl FnOnce() -> Result<Vec<Value>>,
) -> Result<Outcome> {
    inv.control.check()?;
    let mut checks = Vec::new();
    let result = (|| {
        workspace_checks(inv, legacy, &mut checks)?;
        selection_checks(inv, &mut checks)?;
        let providers = provider()?;
        for item in providers {
            inv.control.check()?;
            if item["id"].as_str().is_none_or(str::is_empty)
                || !item["ok"].is_boolean()
                || !["ok", "warn", "error", "unknown"]
                    .contains(&item["status"].as_str().unwrap_or("ok"))
            {
                return Err(ToolkitError::invalid(
                    "provider health check shape is invalid",
                ));
            }
            checks.push(item);
        }
        inv.control.check()
    })();
    if let Err(error) = result {
        return Err(partial(error, inv, checks));
    }
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
#[cfg(test)]
mod tests;
