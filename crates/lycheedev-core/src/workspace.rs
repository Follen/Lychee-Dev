//! Workspace settings, budgets, and retention.
use crate::{
    Invocation, Outcome, Result, ToolkitError,
    metadata::Metadata,
    selection,
    storage::{self, ExclusiveLock},
};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub mod data_resources;
pub fn open(home: &Path) -> Result<Value> {
    let path = home.join("workspace.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if fs::read_dir(home)
                .ok()
                .and_then(|entries| entries.collect::<std::io::Result<Vec<_>>>().ok())
                .is_some_and(|entries| !entries.is_empty())
            {
                return Err(ToolkitError::new(
                    "vault.legacy_detected",
                    "vault.legacy_detected",
                    3,
                ));
            }
            return Err(error.into());
        }
        Err(error) => return Err(error.into()),
    };
    let unsupported = || {
        ToolkitError::new(
            "vault.unsupported_workspace",
            "vault.unsupported_workspace",
            4,
        )
    };
    let (identity, zero_time) =
        crate::evidence::decode_workspace_identity(&bytes).map_err(|_| unsupported())?;
    let id = identity["workspaceId"].as_str().unwrap_or("");
    // Fixed Go checks decoded length and time.IsZero before schema, then hex.
    if id.len() != 32 || zero_time {
        return Err(unsupported());
    }
    let schema = identity["schema"].as_str().unwrap_or("");
    if schema != "lycheedev.workspace.v1" {
        if workspace_schema_newer(schema) {
            return Err(ToolkitError::new(
                "vault.workspace_schema_newer",
                "vault.workspace_schema_newer\nvault.unsupported_workspace",
                4,
            ));
        }
        return Err(unsupported());
    }
    if hex::decode(id).is_err() {
        return Err(unsupported());
    }
    Ok(identity)
}
fn workspace_schema_newer(schema: &str) -> bool {
    let Some(version) = schema.strip_prefix("lycheedev.workspace.v") else {
        return false;
    };
    let mut value = 0i64;
    for digit in version.bytes() {
        if !digit.is_ascii_digit() {
            return false;
        }
        value = value.wrapping_mul(10).wrapping_add(i64::from(digit - b'0'));
    }
    value > 1
}
pub fn initialize(home: &Path) -> Result<Value> {
    let home = if home.is_absolute() {
        home.to_path_buf()
    } else {
        std::env::current_dir()?.join(home)
    };
    let parent = home
        .parent()
        .ok_or_else(|| ToolkitError::invalid("workspace cannot be a filesystem root"))?;
    fs::create_dir_all(parent)?;
    let _guard = ExclusiveLock::resource(
        &parent.join(".lycheedev-locks"),
        &format!(
            "initialize:{}",
            home.file_name().unwrap_or_default().to_string_lossy()
        ),
    )?;
    initialize_unlocked(&home)
}
pub(crate) fn initialize_unlocked(home: &Path) -> Result<Value> {
    if home.join("workspace.json").exists() {
        return open(home);
    }
    if home.exists() && fs::read_dir(home)?.next().is_some() {
        return Err(ToolkitError::new(
            "vault.legacy_detected",
            "existing data is never adopted or imported",
            4,
        ));
    }
    let parent = home
        .parent()
        .ok_or_else(|| ToolkitError::invalid("workspace cannot be filesystem root"))?;
    fs::create_dir_all(parent)?;
    if home.join("workspace.json").exists() {
        return open(home);
    }
    let now = chrono::Utc::now();
    let identity = json!({"schema":"lycheedev.workspace.v1","workspaceId":storage::sha256(format!("{now:?}:{}:{}",std::process::id(),home.display()).as_bytes())[..32],"createdAt":now.to_rfc3339_opts(chrono::SecondsFormat::Nanos,true)});
    let stage = parent.join(format!(
        ".lycheedev-init-{}",
        identity["workspaceId"].as_str().unwrap()
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        for name in [
            "state", "pins", "mirrors", "blobs", "indexes", "cache", "runs", "captures", "locks",
            "tmp",
        ] {
            fs::create_dir(stage.join(name))?;
        }
        storage::atomic_json(&stage.join("workspace.json"), &identity)?;
        if home.exists() {
            fs::remove_dir(home)?;
        }
        fs::rename(&stage, home)?;
        Ok(identity)
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}
pub fn default_config() -> Value {
    json!({"schema":"lycheedev.config.v1","cache":{"maxBytes":20u64<<30},"download":{"workers":0}})
}
pub fn config(home: &Path) -> Result<Value> {
    open(home)?;
    let value = if home.join("config.json").exists() {
        storage::read_json(&home.join("config.json"))?
    } else {
        default_config()
    };
    if value["schema"] != "lycheedev.config.v1" {
        return Err(ToolkitError::new(
            "vault.config_schema_newer",
            "unsupported configuration schema",
            4,
        ));
    }
    if value["cache"]["maxBytes"]
        .as_u64()
        .is_none_or(|n| n > 1u64 << 45)
        || value["download"]["workers"]
            .as_u64()
            .is_none_or(|n| n > 256)
    {
        return Err(ToolkitError::new(
            "vault.config_format",
            "budget out of range",
            4,
        ));
    }
    Ok(value)
}
pub fn commands() -> &'static [&'static str] {
    &[
        "init",
        "doctor",
        "config show",
        "config set",
        "target list",
        "target add",
        "target remove",
        "target show",
        "target resolve",
    ]
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    match inv.command.as_str() {
        "doctor" => crate::doctor::inspect(inv),
        "init" => {
            if inv.flag_bool("--fresh") {
                return Ok(Outcome::new(if inv.flag_bool("--plan") {
                    crate::archive::plan(&inv.home)?
                } else {
                    crate::archive::fresh(&inv.home)?
                }));
            }
            if inv.flag_bool("--plan") {
                return Err(ToolkitError::arguments("init --plan requires --fresh"));
            }
            let identity = initialize(&inv.home)?;
            Ok(Outcome::new(
                json!({"root":fs::canonicalize(&inv.home)?,"identity":identity}),
            ))
        }
        "config show" => Ok(Outcome::new(config(&inv.home)?)),
        "config set" => {
            let _lock = ExclusiveLock::acquire(&inv.home.join("locks/config.lock"))?;
            let mut value = config(&inv.home)?;
            if inv.flag("--cache-max-bytes").is_some() {
                value["cache"]["maxBytes"] =
                    json!(inv.usize_flag("--cache-max-bytes", 0, 0, 1usize << 45)?);
            }
            if inv.flag("--download-workers").is_some() {
                value["download"]["workers"] =
                    json!(inv.usize_flag("--download-workers", 0, 0, 256)?);
            }
            storage::atomic_json(&inv.home.join("config.json"), &value)?;
            Ok(Outcome::new(value))
        }
        command if command.starts_with("target ") => target(inv),
        _ => Err(ToolkitError::new(
            "core.migration_incomplete",
            "requested workspace handler has not yet been migrated",
            3,
        )),
    }
}
fn target(inv: &Invocation) -> Result<Outcome> {
    open(&inv.home)?;
    if inv.command == "target resolve" {
        if let Some(file) = inv.flag("--file") {
            if inv.flag("--target").is_some()
                || inv.flag("--installation").is_some()
                || inv.flag("--product").is_some()
            {
                return Err(ToolkitError::arguments(
                    "target resolve --file cannot combine with target/provider selectors",
                ));
            }
            if fs::metadata(file)?.len() > 65536 {
                return Err(ToolkitError::new(
                    "selection.spec_limit",
                    "selection file exceeds 64 KiB",
                    4,
                ));
            }
            let raw = fs::read(file)?;
            let value: Value = serde_json::from_slice(&raw)?;
            if value.as_object().is_none_or(|fields| {
                fields
                    .keys()
                    .any(|key| !["parent", "source", "data", "changes"].contains(&key.as_str()))
            }) {
                return Err(ToolkitError::invalid(
                    "selection spec contains unknown fields",
                ));
            }
            let spec: selection::SelectionSpec = serde_json::from_value(value)?;
            let pin = selection::pin_selection(&inv.home, spec)?;
            let mut outcome = Outcome::new(serde_json::to_value(&pin)?);
            outcome.selection = serde_json::to_value(pin)?;
            return Ok(outcome);
        }
        return Err(ToolkitError::new(
            "selection.provider_required",
            "this target needs an entry-point data provider to resolve exact identities",
            3,
        ));
    }
    if inv.command == "target show" && inv.argument()?.starts_with("PIN-") {
        return Ok(Outcome::new(serde_json::to_value(selection::load_pinned(
            &inv.home,
            inv.argument()?,
        )?)?));
    }
    if inv.command == "target list" && !inv.home.join("state/toolkit.sqlite").exists() {
        return Ok(Outcome::new(json!({"targets":[]})));
    }
    let mut metadata = Metadata::open(
        &inv.home,
        inv.command == "target add" || inv.command == "target remove",
    )?;
    let all: Vec<Value> = metadata.list("target/", 10_000)?;
    match inv.command.as_str() {
        "target list" => Ok(Outcome::new(json!({"targets":all}))),
        "target add" => {
            let name = inv.argument()?;
            if name.is_empty()
                || name.len() > 100
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            {
                return Err(ToolkitError::arguments(
                    "target name must be bounded ASCII identifier",
                ));
            }
            let product = crate::client::canonical_product(inv.required("--product")?)
                .ok_or_else(|| ToolkitError::arguments("unsupported product"))?;
            let region = inv.required("--region")?;
            let locale = inv.required("--locale")?;
            if !selection::valid_product(product)
                || !["us", "eu", "cn", "kr", "tw"].contains(&region)
                || ![
                    "enUS", "enGB", "deDE", "esES", "esMX", "frFR", "itIT", "ptBR", "ruRU", "koKR",
                    "zhCN", "zhTW",
                ]
                .contains(&locale)
            {
                return Err(ToolkitError::arguments(
                    "unsupported product, region, or locale",
                ));
            }
            if inv.flag_bool("--remote") == inv.flag("--installation").is_some() {
                return Err(ToolkitError::arguments(
                    "exactly one of --remote or --installation is required",
                ));
            }
            if inv
                .flag("--build")
                .is_some_and(|s| !selection::valid_build(s))
            {
                return Err(ToolkitError::arguments(
                    "--build requires full numeric build",
                ));
            }
            let key = format!("target/{name}");
            let generation = metadata.generation(&key)?;
            if generation.is_some() && !inv.flag_bool("--replace") {
                return Err(ToolkitError::new(
                    "selection.target_exists",
                    "target exists; explicit --replace required",
                    4,
                ));
            }
            let mut value = json!({"schema":"lycheedev.target.v1","name":name,"product":product,"region":region,"locale":locale,"source":if inv.flag_bool("--remote"){"remote"}else{"installation"}});
            for (flag, key) in [
                ("--build", "fullBuild"),
                ("--installation", "installation"),
                ("--definitions", "definitions"),
            ] {
                if let Some(v) = inv.flag(flag) {
                    value[key] = json!(v);
                }
            }
            metadata.put(&key, &value, generation)?;
            Ok(Outcome::new(value))
        }
        "target show" | "target remove" => {
            let key = inv.argument()?.to_ascii_lowercase();
            let matches: Vec<_> = all
                .iter()
                .filter(|v| {
                    v["name"]
                        .as_str()
                        .is_some_and(|s| s.to_ascii_lowercase().starts_with(&key))
                })
                .collect();
            let exact = matches.iter().find(|v| {
                v["name"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&key))
            });
            let selected = if let Some(v) = exact {
                *v
            } else if matches.len() == 1 {
                matches[0]
            } else {
                return Err(ToolkitError::new(
                    if matches.is_empty() {
                        "selection.target_missing"
                    } else {
                        "selection.target_ambiguous"
                    },
                    "target selection must match exactly one name",
                    2,
                ));
            };
            let name = selected["name"].as_str().unwrap();
            let refs: Vec<Value> = metadata.list(&format!("target-ref/{name}/"), 10001)?;
            if refs.len() > 10000
                || refs.iter().any(|reference| {
                    reference["schema"] != "lycheedev.target-ref.v1"
                        || reference["name"] != name
                        || !["pin", "operation"].contains(&reference["kind"].as_str().unwrap_or(""))
                })
            {
                return Err(ToolkitError::new(
                    "selection.target_format",
                    "named target reference ledger is invalid or exceeds bounds",
                    4,
                ));
            }
            let active: Vec<_> = refs
                .iter()
                .filter(|v| v["kind"] == "operation")
                .map(|v| v["id"].clone())
                .collect();
            if inv.command == "target show" {
                return Ok(Outcome::new(
                    json!({"config":selected,"references":refs,"activeOperations":active}),
                ));
            }
            if !active.is_empty() {
                return Err(ToolkitError::new(
                    "selection.target_in_use",
                    "active operations block target removal",
                    6,
                ));
            }
            let target_key = format!("target/{name}");
            let generation = metadata.generation(&target_key)?.ok_or_else(|| {
                ToolkitError::new("selection.target_missing", "target disappeared", 2)
            })?;
            let mut keys = vec![target_key.clone()];
            for reference in &refs {
                keys.push(format!(
                    "target-ref/{name}/{}/{}",
                    reference["kind"].as_str().unwrap(),
                    reference["id"]
                        .as_str()
                        .ok_or_else(|| ToolkitError::invalid("target reference ID missing"))?
                ));
            }
            metadata.delete_checked(&keys, &[(target_key, generation)], |documents| {
                if documents.iter().any(|doc| {
                    doc.key.starts_with(&format!("target-ref/{name}/"))
                        && doc.value["kind"] == "operation"
                }) {
                    return Err(ToolkitError::new(
                        "selection.target_in_use",
                        "a concurrent operation still owns the named target",
                        6,
                    ));
                }
                Ok(())
            })?;
            Ok(Outcome::new(
                json!({"name":name,"removedRefCount":refs.len(),"keptPins":refs.iter().filter(|v|v["kind"]=="pin").map(|v|v["id"].clone()).collect::<Vec<_>>()}),
            ))
        }
        _ => Err(ToolkitError::new(
            "selection.migration_incomplete",
            "target resolution belongs to provider adapters and is not complete",
            3,
        )),
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
