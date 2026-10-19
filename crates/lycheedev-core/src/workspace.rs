//! Workspace settings, budgets, and retention.
use crate::{
    Invocation, Outcome, Result, ToolkitError,
    control::OperationControl,
    selection,
    storage::{self, ExclusiveLock},
};
use serde_json::{Value, json};
use std::{fs, path::Path};
mod configuration;
mod controlled;
pub use configuration::{DEFAULT_DATA_CACHE_MAX_BYTES, WorkspaceConfig};
pub mod data_disk;
pub mod data_resources;
pub(crate) use controlled::inspect_controlled;
pub use controlled::{open_controlled, open_controlled_with_check};
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
    decode_identity(&bytes)
}
fn decode_identity(bytes: &[u8]) -> Result<Value> {
    let decoded =
        crate::evidence::decode_workspace_identity(bytes).map_err(|_| unsupported_identity())?;
    validate_identity(decoded)
}
fn unsupported_identity() -> ToolkitError {
    ToolkitError::new(
        "vault.unsupported_workspace",
        "vault.unsupported_workspace",
        4,
    )
}
fn validate_identity((identity, zero_time): (Value, bool)) -> Result<Value> {
    let unsupported = unsupported_identity;
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
/// Optional only for historical library callers. Controlled commands borrow the
/// original invocation; no new deadline or cancellation lineage is introduced.
#[derive(Clone, Copy)]
pub(crate) struct InitControl<'a>(pub(crate) Option<&'a OperationControl>);
impl InitControl<'_> {
    pub(crate) fn check(self) -> Result<()> {
        self.0.map_or(Ok(()), OperationControl::check)
    }
    pub(crate) fn finish<T>(self, result: Result<T>) -> Result<T> {
        selection::selection_finish(result, self.check())
    }
    pub(crate) fn open(self, home: &Path) -> Result<Value> {
        self.check()?;
        self.finish(match self.0 {
            Some(control) => open_controlled(home, control),
            None => open(home),
        })
    }
    pub(crate) fn lock(self, scope: &Path, resource: &str) -> Result<ExclusiveLock> {
        match self.0 {
            Some(control) => ExclusiveLock::resource_controlled(scope, resource, control),
            None => ExclusiveLock::resource(scope, resource),
        }
    }
    pub(crate) fn write(self, path: &Path, value: &Value) -> Result<()> {
        self.check()?;
        let bytes = serde_json::to_vec_pretty(value)?;
        self.check()?;
        // A successful durable replacement must be recorded by the caller
        // before its next stop check. Do not turn that success into an unknown.
        match self.0 {
            Some(control) => storage::atomic_write_controlled(path, &bytes, control),
            None => storage::atomic_write(path, &bytes),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitPhase {
    ParentPrepared,
    LeaseAcquired,
    StageCreated,
    DirectoryCreated,
    ManifestWriting,
    ManifestPublished,
    RemoveEmptyRoot,
    EmptyRootRemoved,
    RootRename,
    RootPublished,
    LeaseClosed,
}
fn init_checkpoint(
    phase: InitPhase,
    path: &Path,
    control: InitControl<'_>,
    observe: &mut impl FnMut(InitPhase, &Path) -> Result<()>,
) -> Result<()> {
    control.finish(observe(phase, path))
}
pub fn initialize(home: &Path) -> Result<Value> {
    initialize_observed(home, InitControl(None), &mut |_, _| Ok(()))
}
pub fn initialize_controlled(home: &Path, control: &OperationControl) -> Result<Value> {
    initialize_observed(home, InitControl(Some(control)), &mut |_, _| Ok(()))
}
fn initialize_observed(
    home: &Path,
    control: InitControl<'_>,
    observe: &mut impl FnMut(InitPhase, &Path) -> Result<()>,
) -> Result<Value> {
    control.check()?;
    let home = if home.is_absolute() {
        home.to_path_buf()
    } else {
        std::env::current_dir()?.join(home)
    };
    let parent = home
        .parent()
        .ok_or_else(|| ToolkitError::invalid("workspace cannot be a filesystem root"))?;
    control.check()?;
    control.finish(fs::create_dir_all(parent).map_err(ToolkitError::from))?;
    init_checkpoint(InitPhase::ParentPrepared, parent, control, observe)?;
    let guard = control.lock(
        &parent.join(".lycheedev-locks"),
        &format!(
            "initialize:{}",
            home.file_name().unwrap_or_default().to_string_lossy()
        ),
    )?;
    let result = (|| {
        init_checkpoint(InitPhase::LeaseAcquired, &home, control, observe)?;
        initialize_unlocked_observed(&home, control, observe, |stage| {
            fs::remove_dir_all(stage).map_err(ToolkitError::from)
        })
    })();
    let summary = initialization_result(&home, &result);
    let progress = result
        .as_ref()
        .err()
        .and_then(|error| error.details.get("initialization"))
        .cloned();
    // Once acquired, close is unconditional and checked, even after a stop.
    let result = selection::selection_finish(result, guard.close());
    let result = selection::selection_finish(
        result,
        init_checkpoint(InitPhase::LeaseClosed, &home, control, observe),
    );
    preserve_initialization_result(result, summary, progress)
}
pub(crate) fn initialization_result(home: &Path, result: &Result<Value>) -> Option<Value> {
    match result {
        Ok(identity) => Some(json!({"root":home,"identity":identity})),
        Err(error) => error
            .details
            .get("result")
            .filter(|value| !value.is_null())
            .cloned(),
    }
}
fn preserve_initialization_result(
    result: Result<Value>,
    summary: Option<Value>,
    progress: Option<Value>,
) -> Result<Value> {
    result.map_err(|error| {
        if summary.is_none() && progress.is_none() {
            return error;
        }
        let mut combined = error.clone();
        combined.details = json!({
            "schema":"lycheedev.workspace-initialization-error.v1",
            "primary":error,
            "initialization":progress,
            "result":summary,
        });
        combined
    })
}
/// Use the same Go time.Time marshal form as the identity reader. Keep the
/// original instant and ID input; only newly created identities are formatted.
fn created_identity(home: &Path, now: &chrono::DateTime<chrono::Utc>) -> Result<Value> {
    let raw = now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let created_at = crate::evidence::parse_go_timestamp(&raw)
        .and_then(|timestamp| timestamp.canonical().map(str::to_owned))
        .ok_or_else(|| ToolkitError::invalid("workspace creation timestamp cannot be encoded"))?;
    Ok(json!({
        "schema":"lycheedev.workspace.v1",
        "workspaceId":storage::sha256(format!("{now:?}:{}:{}",std::process::id(),home.display()).as_bytes())[..32],
        "createdAt":created_at,
    }))
}
pub(crate) fn initialize_unlocked_observed(
    home: &Path,
    control: InitControl<'_>,
    observe: &mut impl FnMut(InitPhase, &Path) -> Result<()>,
    cleanup: impl FnOnce(&Path) -> Result<()>,
) -> Result<Value> {
    control.check()?;
    if home.join("workspace.json").exists() {
        return control.open(home);
    }
    control.check()?;
    if home.exists() {
        let mut entries = fs::read_dir(home)?;
        control.check()?;
        let occupied = entries.next().transpose()?.is_some();
        control.check()?;
        if occupied {
            return Err(ToolkitError::new(
                "vault.legacy_detected",
                "existing data is never adopted or imported",
                4,
            ));
        }
    }
    let parent = home
        .parent()
        .ok_or_else(|| ToolkitError::invalid("workspace cannot be filesystem root"))?;
    control.check()?;
    control.finish(fs::create_dir_all(parent).map_err(ToolkitError::from))?;
    if home.join("workspace.json").exists() {
        return control.open(home);
    }
    control.check()?;
    let now = chrono::Utc::now();
    let identity = created_identity(home, &now)?;
    let stage = parent.join(format!(
        ".lycheedev-init-{}",
        identity["workspaceId"].as_str().unwrap()
    ));
    control.check()?;
    fs::create_dir(&stage)?;
    let mut published = false;
    let mut empty_target_removed = false;
    let mut retain_stage = false;
    let result = (|| {
        init_checkpoint(InitPhase::StageCreated, &stage, control, observe)?;
        for name in [
            "state", "pins", "mirrors", "blobs", "indexes", "cache", "runs", "captures", "locks",
            "tmp",
        ] {
            control.check()?;
            fs::create_dir(stage.join(name))?;
            init_checkpoint(InitPhase::DirectoryCreated, &stage, control, observe)?;
        }
        init_checkpoint(InitPhase::ManifestWriting, &stage, control, observe)?;
        if let Err(error) = control.write(&stage.join("workspace.json"), &identity) {
            // The atomic writer owns its uncertain temporary. Preserve the
            // containing private stage if close/publication/cleanup is unknown.
            retain_stage = !error.details.is_null();
            return Err(error);
        }
        init_checkpoint(InitPhase::ManifestPublished, &stage, control, observe)?;
        control.check()?;
        if home.exists() {
            init_checkpoint(InitPhase::RemoveEmptyRoot, home, control, observe)?;
            fs::remove_dir(home)?;
            empty_target_removed = true;
            init_checkpoint(InitPhase::EmptyRootRemoved, home, control, observe)?;
        }
        init_checkpoint(InitPhase::RootRename, &stage, control, observe)?;
        fs::rename(&stage, home)?;
        published = true;
        init_checkpoint(InitPhase::RootPublished, home, control, observe)?;
        Ok(identity.clone())
    })();
    let cleanup = if !published && !retain_stage {
        let result = cleanup(&stage);
        retain_stage = result.is_err();
        result
    } else {
        Ok(())
    };
    let result = control.finish(selection::selection_finish(result, cleanup));
    let progress = json!({
        "root":home,"stage":stage,"published":published,
        "emptyTargetRemoved":empty_target_removed,"stageRetained":retain_stage,
    });
    preserve_initialization_result(
        result,
        published.then(|| json!({"root":home,"identity":identity})),
        Some(progress),
    )
}
pub fn default_config() -> Value {
    WorkspaceConfig::default().to_value()
}
pub fn config(home: &Path) -> Result<Value> {
    open(home)?;
    let bytes = if home.join("config.json").exists() {
        Some(fs::read(home.join("config.json"))?)
    } else {
        None
    };
    legacy_configuration_value(bytes.as_deref())
}
/// Historical config show/set/cache callers keep their exact old field/default,
/// schema/error and unknown-field behavior. Only the new Data policy is shared
/// with strict admission, normalized without dropping unrelated user fields.
pub(crate) fn legacy_configuration_value(raw: Option<&[u8]>) -> Result<Value> {
    let value: Value = match raw {
        Some(raw) => serde_json::from_slice(raw)?,
        None => default_config(),
    };
    legacy_configuration_parsed(value, raw)
}
pub(crate) fn legacy_configuration_parsed(mut value: Value, raw: Option<&[u8]>) -> Result<Value> {
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
    let data_max_bytes = raw
        .map(configuration::data_policy_from_raw)
        .transpose()?
        .unwrap_or(DEFAULT_DATA_CACHE_MAX_BYTES);
    let fields = value
        .as_object_mut()
        .expect("valid configuration is an object");
    // Normalize only this new extension. Keeping differently cased duplicate
    // Data keys would make a later sorted Value serialization change precedence.
    fields.retain(|key, _| !configuration::is_data_policy_key(key));
    fields.insert("dataCache".into(), json!({"maxBytes":data_max_bytes}));
    Ok(value)
}
/// Strict Data admission, retaining its existing Go decoder. Historical public
/// Value APIs intentionally keep their pre-existing acceptance differences.
pub fn config_controlled(home: &Path, control: &OperationControl) -> Result<WorkspaceConfig> {
    configuration::read(home, control, &mut || Ok(()))
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
            if ["--plan", "--fresh", "--resume"]
                .into_iter()
                .filter(|flag| inv.flag_bool(flag))
                .count()
                > 1
            {
                return Err(ToolkitError::arguments(
                    "init accepts at most one of --plan, --fresh, --resume",
                ));
            }
            inv.control.check()?;
            if inv.flag_bool("--plan") {
                return Ok(Outcome::new(crate::archive::plan_controlled(
                    &inv.home,
                    &inv.control,
                )?));
            }
            if inv.flag_bool("--fresh") {
                return Ok(Outcome::new(crate::archive::fresh_controlled(
                    &inv.home,
                    &inv.control,
                )?));
            }
            if inv.flag_bool("--resume") {
                return Ok(Outcome::new(crate::archive::resume_controlled(
                    &inv.home,
                    &inv.control,
                )?));
            }
            let identity = initialize_controlled(&inv.home, &inv.control)?;
            let summary = json!({"root":inv.home,"identity":identity});
            let result = (|| {
                inv.control.check()?;
                let root = fs::canonicalize(&inv.home)?;
                inv.control.check()?;
                Ok(json!({"root":root,"identity":identity}))
            })();
            preserve_initialization_result(
                selection::selection_finish(result, inv.control.check()),
                Some(summary),
                None,
            )
            .map(Outcome::new)
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
    if inv.command != "target resolve" {
        return selection::target::dispatch(inv);
    }
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
            let pin = selection::resolve_selection_file_controlled(
                &inv.home,
                &inv.cwd.join(file),
                &inv.control,
            )?;
            return Ok(Outcome::new(serde_json::to_value(pin)?));
        }
        return Err(ToolkitError::new(
            "selection.provider_required",
            "this target needs an entry-point data provider to resolve exact identities",
            3,
        ));
    }
    selection::target::dispatch(inv)
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workspace_initialization_tests.rs"]
mod initialization_tests;

#[cfg(test)]
mod identity_creation_tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "lychee-init-canonical-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&base).unwrap();
            Self(base)
        }
        fn home(&self) -> PathBuf {
            self.0.join("home")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn created_identity_zero_seven_and_nine_digits_match_persisted_reopen() {
        for (raw, expected) in [
            ("2026-10-18T02:00:00.000000000Z", "2026-10-18T02:00:00Z"),
            (
                "2026-10-18T02:00:00.135524800Z",
                "2026-10-18T02:00:00.1355248Z",
            ),
            (
                "2026-10-18T02:00:00.135524801Z",
                "2026-10-18T02:00:00.135524801Z",
            ),
        ] {
            let f = Fixture::new();
            let home = f.home();
            let now = chrono::DateTime::parse_from_rfc3339(raw)
                .unwrap()
                .with_timezone(&chrono::Utc);
            let initial = created_identity(&home, &now).unwrap();
            assert_eq!(initial["createdAt"], expected);
            // Formatting must not feed back into the existing ID derivation.
            let expected_id = storage::sha256(
                format!("{now:?}:{}:{}", std::process::id(), home.display()).as_bytes(),
            );
            assert_eq!(initial["workspaceId"].as_str().unwrap(), &expected_id[..32]);
            let parsed = crate::evidence::parse_go_timestamp(expected).unwrap();
            assert_eq!(
                parsed.instant(),
                (now.timestamp(), now.timestamp_subsec_nanos())
            );

            fs::create_dir(&home).unwrap();
            let marker = home.join("workspace.json");
            // This is the production manifest writer, using the deterministic
            // identity helper. Persisted bytes are compared before/after reopen.
            InitControl(None).write(&marker, &initial).unwrap();
            fs::write(home.join("unchanged"), b"do not reconstruct this workspace").unwrap();
            let before = fs::read(&marker).unwrap();
            let persisted: Value = serde_json::from_slice(&before).unwrap();
            assert_eq!(persisted, initial);
            assert_eq!(open(&home).unwrap(), initial);
            assert_eq!(
                open_controlled(&home, &OperationControl::default()).unwrap(),
                initial
            );
            assert_eq!(initialize(&home).unwrap(), initial);
            assert_eq!(
                initialize_controlled(&home, &OperationControl::default()).unwrap(),
                initial
            );
            assert_eq!(fs::read(&marker).unwrap(), before);
            assert_eq!(
                fs::read(home.join("unchanged")).unwrap(),
                b"do not reconstruct this workspace"
            );
            assert_eq!(fs::read_dir(&home).unwrap().count(), 2);
        }
    }

    #[test]
    fn actual_initializer_returns_the_persisted_canonical_identity() {
        let f = Fixture::new();
        let home = f.home();
        let initial = initialize_controlled(&home, &OperationControl::default()).unwrap();
        let marker = home.join("workspace.json");
        let before = fs::read(&marker).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&before).unwrap(), initial);
        assert_eq!(open(&home).unwrap(), initial);
        assert_eq!(initialize(&home).unwrap(), initial);
        assert_eq!(fs::read(&marker).unwrap(), before);
        assert!(home.join("state").is_dir());
    }

    #[test]
    fn historical_fractional_spelling_is_canonicalized_without_rewriting_manifest() {
        let f = Fixture::new();
        let home = f.home();
        fs::create_dir(&home).unwrap();
        let marker = home.join("workspace.json");
        let raw = b"{ \"schema\":\"lycheedev.workspace.v1\", \"workspaceId\":\"0123456789abcdef0123456789abcdef\", \"createdAt\":\"2026-10-18T02:00:00.135524800Z\" }\n";
        fs::write(&marker, raw).unwrap();
        let expected = json!({
            "schema":"lycheedev.workspace.v1",
            "workspaceId":"0123456789abcdef0123456789abcdef",
            "createdAt":"2026-10-18T02:00:00.1355248Z",
        });
        assert_eq!(open(&home).unwrap(), expected);
        assert_eq!(initialize(&home).unwrap(), expected);
        assert_eq!(
            initialize_controlled(&home, &OperationControl::default()).unwrap(),
            expected
        );
        assert_eq!(fs::read(&marker).unwrap(), raw);
        assert_eq!(fs::read_dir(&home).unwrap().count(), 1);
    }
}
