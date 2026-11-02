//! Production repair validation composes the existing load-closure/XML facts
//! and pinned compile-only Lua 5.1 tool. No project configuration is executed.
#![forbid(unsafe_code)]
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use lycheedev_delivery::{formal_repair::AddonValidator, release::Release};
use lycheedev_source::{closure::Closure, facts, luac};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const TOC: &str = "Lychee Dev.toc";
static NEXT: AtomicU64 = AtomicU64::new(0);
pub struct CliAddonValidator;
impl AddonValidator for CliAddonValidator {
    fn validate(
        &mut self,
        prepared_release: &Path,
        version: &str,
        control: &OperationControl,
    ) -> Result<()> {
        control.check()?;
        let release =
            lycheedev_delivery::release::inspect_with_control(prepared_release, version, control)?;
        let mut scratch = Scratch::new(control)?;
        let result = (|| {
            let home = scratch.0.join("workspace");
            lycheedev_core::workspace::initialize(&home)?;
            let closure = load_closure(prepared_release, version, &release, &home, control)?;
            let (inputs, _) = lycheedev_source::xml_inline::inputs(&closure.documents)?;
            let compiler = compiler(prepared_release, control)?;
            // If the compiler unwinds after entering its runner, Drop must not
            // remove the parent of potentially live child/pipe scratch either.
            scratch.1 = false;
            let compiled =
                compiler.check_with_control(&scratch.0.join("compile"), &inputs, control);
            scratch.1 = !compiled
                .as_ref()
                .err()
                .is_some_and(|e| luac::requires_scratch_retention(e));
            let syntax = compiled?;
            if syntax["passed"] != true || syntax["complete"] != true {
                return Err(payload("invalid Lua syntax in addon load graph")
                    .with_details(json!({"syntax":syntax})));
            }
            control.check()
        })();
        scratch.finish(result, control)
    }
}
fn payload(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("delivery.invalid_payload", message, 4)
}
fn read(path: &Path, limit: u64, control: &OperationControl) -> Result<Vec<u8>> {
    control.check()?;
    let mut file = File::open(path)?;
    let result = (|| {
        let info = file.metadata()?;
        if !info.is_file() || info.len() > limit {
            return Err(payload("release source is not a bounded regular file"));
        }
        let mut bytes = Vec::with_capacity(info.len() as usize);
        (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != info.len() {
            return Err(payload("release source changed during bounded read"));
        }
        Ok(bytes)
    })();
    storage::finish_file(file, result, control)
}
pub(super) fn compiler(root: &Path, control: &OperationControl) -> Result<luac::Runtime> {
    // Authentication is the executable's build-time tool identity, never a
    // self-supplied release hash or PATH executable. The full snapshot inventory
    // was checked before this manifest is interpreted.
    let pinned: Value = serde_json::from_str(include_str!("../../../release/tools/luac.json"))?;
    let bytes = read(
        &root.join("payload/tool/luac/runtime.json"),
        1 << 20,
        control,
    )?;
    let packaged: Value = serde_json::from_slice(&bytes)?;
    if packaged != pinned["runtime"] {
        return Err(ToolkitError::new(
            "luac.unavailable",
            "bundled compiler manifest differs from pinned identity",
            3,
        ));
    }
    let runtime = luac::Runtime::verified_with_ordered_control(
        &root.join("payload/tool/luac"),
        serde_json::from_value(packaged)?,
        control,
    )?;
    control.check()?;
    Ok(runtime)
}
fn manifest(
    facts: &facts::Facts,
    version: &str,
    files: &BTreeMap<&str, &lycheedev_delivery::installation::Resource>,
) -> Result<()> {
    let mut headers = BTreeMap::new();
    for header in &facts.headers {
        let key = header.key.to_lowercase();
        if headers.insert(key.clone(), header.value.as_str()).is_some() {
            return Err(payload(format!(
                "duplicate TOC header {key} in addon/{TOC}"
            )));
        }
    }
    let declared: BTreeSet<_> = headers
        .get("interface")
        .copied()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect();
    let expected: BTreeSet<_> = lycheedev_delivery::slots::INTERFACES
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect();
    if declared != expected {
        return Err(payload(format!(
            "TOC addon/{TOC} must declare every supported interface and no unsupported interface"
        )));
    }
    if headers.get("version") != Some(&version)
        || headers.get("savedvariables") != Some(&"LycheeToolkitDB")
        || !matches!(
            headers
                .get("savedvariablespercharacter")
                .copied()
                .unwrap_or(""),
            "" | "LycheeToolkitBridgeDB"
        )
    {
        return Err(payload(format!(
            "TOC identity/version/storage mismatch in addon/{TOC}"
        )));
    }
    let first = facts
        .loads
        .first()
        .ok_or_else(|| payload(format!("empty TOC addon/{TOC}")))?;
    if !first
        .path
        .replace('\\', "/")
        .eq_ignore_ascii_case("Core/ClientGate.lua")
    {
        return Err(payload(format!(
            "addon/{TOC} must load Core/ClientGate.lua first"
        )));
    }
    let mut seen = BTreeSet::new();
    for load in &facts.loads {
        let name = load.path.replace('\\', "/");
        if name.is_empty()
            || name.starts_with('/')
            || name.contains([':', '\0'])
            || name
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(payload(format!("invalid TOC load {:?}", load.path)));
        }
        let extension = Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !extension.eq_ignore_ascii_case("lua") && !extension.eq_ignore_ascii_case("xml") {
            return Err(payload(format!("unsupported TOC load {name:?}")));
        }
        if !files.contains_key(format!("addon/{name}").as_str())
            || !seen.insert(name.to_lowercase())
        {
            return Err(payload(format!("MISSING/REPEATED TOC load {name:?}")));
        }
    }
    Ok(())
}
fn load_closure(
    root: &Path,
    version: &str,
    release: &Release,
    home: &Path,
    control: &OperationControl,
) -> Result<Closure> {
    let files: BTreeMap<_, _> = release
        .resources
        .iter()
        .map(|r| (r.path.as_str(), r))
        .collect();
    let name = format!("addon/{TOC}");
    let resource = files
        .get(name.as_str())
        .ok_or_else(|| payload(format!("TOC size or presence {name}")))?;
    if resource.bytes > 1 << 20 {
        return Err(payload(format!("TOC size or presence {name}")));
    }
    let bytes = read(&root.join("payload").join(&name), 1 << 20, control)?;
    if bytes.len() as u64 != resource.bytes || storage::sha256(&bytes) != resource.sha256 {
        return Err(payload(format!("changed TOC {name}")));
    }
    manifest(&facts::toc(&bytes), version, &files)?;
    let closure = Closure::inspect_with_control(home, &root.join("payload/addon"), TOC, control)?;
    if !closure.valid {
        return Err(payload(format!("invalid load graph for {TOC}"))
            .with_details(json!({"issues":closure.issues})));
    }
    for doc in &closure.documents {
        control.check()?;
        let name = format!("addon/{}", doc.path);
        let resource = files
            .get(name.as_str())
            .ok_or_else(|| payload(format!("changed or unlisted load {}", doc.path)))?;
        if doc.bytes.len() as u64 != resource.bytes
            || storage::sha256(&doc.bytes) != resource.sha256
        {
            return Err(payload(format!("changed or unlisted load {}", doc.path)));
        }
    }
    Ok(closure)
}
const VALIDATION_RETAINED_SCHEMA: &str = "lycheedev.addon-validator-retained-scratch.v1";
fn retain_validation_scratch(error: ToolkitError, root: &Path, reason: &str) -> ToolkitError {
    let mut retained = error.clone();
    retained.details = json!({"schema":VALIDATION_RETAINED_SCHEMA,
        "retainedScratch":root,"owner":"formal-addon-validator",
        "ownerProcessId":std::process::id(),"reason":reason,"cause":error});
    retained
}
fn requires_validation_retention(error: &ToolkitError) -> bool {
    fn check(details: &Value, depth: usize, remaining: &mut usize) -> bool {
        // Bounded inspection must fail closed rather than erase deeper evidence.
        if depth >= 32 || *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        match details["schema"].as_str() {
            Some(
                "lycheedev.atomic-temporary-retained.v1"
                | "lycheedev.luac-retained-scratch.v1"
                | VALIDATION_RETAINED_SCHEMA,
            ) => true,
            Some(storage::FILE_CLEANUP_SCHEMA) => {
                ["primary", "cleanup", "control"].iter().any(|key| {
                    !details[key].is_null() && check(&details[key]["details"], depth + 1, remaining)
                })
            }
            _ => false,
        }
    }
    check(&error.details, 0, &mut 128)
}
struct Scratch(PathBuf, bool);
impl Scratch {
    fn new(control: &OperationControl) -> Result<Self> {
        // Production validation reuses one installation-level parent so stale
        // roots are admitted deliberately. Unit tests run in parallel, so each
        // test gets its own parent to avoid lock contention between fixtures.
        static PARENTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let parent = std::env::temp_dir().join(format!(
            "lycheedev-addon-validator-v1-{}-{}",
            std::process::id(),
            PARENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        Self::new_in(&parent, control)
    }
    fn new_in(parent: &Path, control: &OperationControl) -> Result<Self> {
        Self::new_in_with(parent, control, storage::atomic_write_controlled)
    }
    fn new_in_with(
        parent: &Path,
        control: &OperationControl,
        publish_owner: impl FnOnce(&Path, &[u8], &OperationControl) -> Result<()>,
    ) -> Result<Self> {
        control.check()?;
        fs::create_dir_all(parent)?;
        storage::guard_path(parent, parent)?;
        storage::guard_path(parent, &parent.join(".locks"))?;
        let lock = storage::ExclusiveLock::resource_controlled(
            &parent.join(".locks"),
            "scratch-admission",
            control,
        )?;
        let admitted = (|| {
            let mut existing = Vec::new();
            for (index, entry) in fs::read_dir(parent)?.enumerate() {
                control.check()?;
                if index >= 32 {
                    return Err(ToolkitError::new(
                        "luac.scratch_capacity",
                        "validator scratch admission entry budget exceeded; retained work requires explicit recovery",
                        3,
                    ));
                }
                let entry = entry?;
                if entry.file_name() == ".locks" {
                    continue;
                }
                storage::guard_path(parent, &entry.path())?;
                existing.push(entry.path());
                if existing.len() >= 8 {
                    let blocked_by: Vec<_> = existing
                        .iter()
                        .map(|path| {
                            json!({
                                "path": path,
                                "reason": "terminal_ownership_not_proven",
                                "disposition": "preserved_without_pid_or_age_inference"
                            })
                        })
                        .collect();
                    return Err(ToolkitError::new(
                        "luac.scratch_capacity",
                        "at least eight active, retained or unknown validator roots exist; no root was reclaimed",
                        3,
                    ).with_details(json!({
                        "owner":"formal-addon-validator", "root":parent,
                        "retainedOrActive":existing, "blockedBy":blocked_by,
                        "maximumWorkspaces":8, "scanStoppedAtCapacity":true
                    })));
                }
            }
            let root = parent.join(format!(
                "validation-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| ToolkitError::invalid(e.to_string()))?
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root)?;
            // Establish conservative parent ownership before publication can
            // leave an explicitly retained atomic temporary or unwind.
            let mut scratch = Self(root, false);
            let owner = json!({"schema":"lycheedev.addon-validator-scratch.v1","owner":"formal-addon-validator","ownerProcessId":std::process::id(),"root":scratch.0,"phase":"admitted","maximumWorkspaces":8,"sourceClosureMaximumBytes":67108864,"sourceClosureMaximumDocuments":10000});
            let published = storage::bounded_json(&owner, 65536)
                .and_then(|bytes| publish_owner(&scratch.0.join("owner.json"), &bytes, control));
            if let Err(error) = published {
                return Err(retain_validation_scratch(
                    error,
                    &scratch.0,
                    "owner_publication_not_confirmed",
                ));
            }
            scratch.1 = true;
            Ok(scratch)
        })();
        let closed = lock.close();
        match (admitted, closed) {
            (Ok(scratch), Ok(())) => Ok(scratch),
            (Err(error), Ok(())) => Err(error),
            (Ok(mut scratch), Err(error)) => {
                scratch.1 = false;
                Err(retain_validation_scratch(
                    error,
                    &scratch.0,
                    "admission_lock_close_not_confirmed",
                ))
            }
            (Err(primary), Err(cleanup)) => {
                let mut error = primary.clone();
                error.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":cleanup,"control":null});
                Err(error)
            }
        }
    }
    fn finish(mut self, result: Result<()>, control: &OperationControl) -> Result<()> {
        // An error is not a lifetime proof. Retention propagates from the
        // compiler through checked-close aggregates to this parent owner.
        let retain = !self.1
            || result
                .as_ref()
                .err()
                .is_some_and(requires_validation_retention);
        self.1 = false;
        if retain {
            let cause = result.err().unwrap_or_else(|| {
                ToolkitError::new(
                    "luac.process_unresolved",
                    "validator lacks compiler terminal proof",
                    5,
                )
            });
            let retained = retain_validation_scratch(
                cause,
                &self.0,
                "nested_retained_resource_or_terminal_proof_missing",
            );
            let record = storage::bounded_json(&retained, 1 << 20).and_then(|bytes| {
                storage::atomic_write(&self.0.join("retained-owner.json"), &bytes)
            });
            return join(join(Err(retained), record), control.check());
        }
        let cleanup = fs::remove_dir_all(&self.0).map_err(ToolkitError::io);
        let result = join(result, cleanup);
        join(result, control.check())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn join(result: Result<()>, cleanup: Result<()>) -> Result<()> {
    match (result, cleanup) {
        (Ok(()), v) | (v, Ok(())) => v,
        (Err(primary), Err(cleanup)) => {
            let mut result = primary.clone();
            result.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":cleanup,"control":null});
            Err(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn toc(extra: &str) -> facts::Facts {
        facts::toc(format!("## Interface: 120105,120100,50504,11509,20506,38002,16001\n## Version: test\n## SavedVariables: LycheeToolkitDB\n{extra}Core/ClientGate.lua\nMain.xml\n").as_bytes())
    }
    fn resources() -> Vec<lycheedev_delivery::installation::Resource> {
        ["addon/Core/ClientGate.lua", "addon/Main.xml"]
            .into_iter()
            .map(|path| {
                serde_json::from_value(json!({"path":path,"sha256":"0".repeat(64),"bytes":1}))
                    .unwrap()
            })
            .collect()
    }
    #[test]
    fn repair_manifest_accepts_exact_reference_identity_and_all_interfaces() {
        let rows = resources();
        let files = rows.iter().map(|r| (r.path.as_str(), r)).collect();
        assert!(manifest(&toc(""), "test", &files).is_ok());
        assert!(
            manifest(
                &toc("## SavedVariablesPerCharacter: LycheeToolkitBridgeDB\n"),
                "test",
                &files
            )
            .is_ok()
        );
    }
    #[test]
    fn repair_manifest_rejects_duplicate_headers_and_missing_or_extra_interfaces() {
        let rows = resources();
        let files = rows.iter().map(|r| (r.path.as_str(), r)).collect();
        assert!(manifest(&toc("## Version: test\n"), "test", &files).is_err());
        // Previous interface lists remain maintenance input only; a newly
        // validated payload must include both supported Retail series.
        for interface in [
            "120100",
            "120100,50504,11509,20506,38002,16001",
            "120105,120100,50504,11509,20506,38002,16001,42",
        ] {
            let mut f = toc("");
            f.headers[0].value = interface.into();
            assert!(manifest(&f, "test", &files).is_err());
        }
    }
    #[test]
    fn repair_manifest_rejects_reordered_repeated_unlisted_or_escaping_loads() {
        let rows = resources();
        let files = rows.iter().map(|r| (r.path.as_str(), r)).collect();
        for path in [
            "../escape.lua",
            "Core/../Main.lua",
            "Missing.lua",
            "Core/ClientGate.lua",
        ] {
            let mut f = toc("");
            f.loads[1].path = path.into();
            assert!(manifest(&f, "test", &files).is_err());
        }
        let mut f = toc("");
        f.loads.swap(0, 1);
        assert!(manifest(&f, "test", &files).is_err());
    }
    #[test]
    fn repair_compiler_identity_is_pinned_not_release_self_attested() {
        let scratch = Scratch::new(&OperationControl::default()).unwrap();
        fs::create_dir_all(scratch.0.join("payload/tool/luac")).unwrap();
        fs::write(scratch.0.join("payload/tool/luac/runtime.json"), b"{}").unwrap();
        let error = match compiler(&scratch.0, &OperationControl::default()) {
            Ok(_) => panic!("untrusted compiler accepted"),
            Err(e) => e,
        };
        assert_eq!(error.code, "luac.unavailable");
    }
    #[test]
    fn repair_cleanup_error_cannot_hide_original_validation_failure() {
        let error = join(
            Err(payload("bad syntax")),
            Err(ToolkitError::io(std::io::Error::other("close"))),
        )
        .unwrap_err();
        assert_eq!(error.code, "delivery.invalid_payload");
        assert_eq!(error.details["schema"], storage::FILE_CLEANUP_SCHEMA);
        assert_eq!(error.details["cleanup"]["message"], "close");
    }
    #[test]
    fn unresolved_compiler_retention_crosses_checked_close_to_parent_owner() {
        let control = OperationControl::default();
        let scratch = Scratch::new(&control).unwrap();
        let root = scratch.0.clone();
        let child = root.join("compile/owned-child");
        fs::create_dir_all(&child).unwrap();
        fs::write(child.join("evidence"), b"keep").unwrap();
        let retained = luac::retain_scratch(
            ToolkitError::new("process.cancelled", "cancelled", 7),
            &child,
        );
        let mut combined = retained.clone();
        combined.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":retained,"cleanup":ToolkitError::io(std::io::Error::other("modeled close")),"control":null});
        let error = scratch.finish(Err(combined), &control).unwrap_err();
        assert_eq!(error.exit_code, 7);
        assert!(requires_validation_retention(&error));
        assert_eq!(fs::read(child.join("evidence")).unwrap(), b"keep");
        assert!(root.join("owner.json").is_file());
        assert!(root.join("retained-owner.json").is_file());
        // The fixture supplied modeled proof and never spawned a child.
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn retained_and_active_validation_owners_bound_future_admission() {
        let parent = std::env::temp_dir().join(format!(
            "lycheedev-validator-cap-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let control = OperationControl::default();
        let mut owners = vec![];
        for _ in 0..8 {
            owners.push(Scratch::new_in(&parent, &control).unwrap());
        }
        let error = match Scratch::new_in(&parent, &control) {
            Ok(_) => panic!("capacity admitted ninth workspace"),
            Err(e) => e,
        };
        assert_eq!(error.code, "luac.scratch_capacity");
        assert_eq!(
            error.details["retainedOrActive"].as_array().unwrap().len(),
            8
        );
        let blockers = error.details["blockedBy"].as_array().unwrap();
        assert_eq!(blockers.len(), 8);
        for row in blockers {
            assert_eq!(row["reason"], "terminal_ownership_not_proven");
            assert!(owners.iter().any(|owner| row["path"] == json!(owner.0)));
        }
        assert!(owners.iter().all(|s| s.0.join("owner.json").is_file()));
        drop(owners);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn parent_drop_after_unproven_compiler_entry_retains_ownership() {
        let parent = std::env::temp_dir().join(format!(
            "lycheedev-validator-retention-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut scratch = Scratch::new_in(&parent, &OperationControl::default()).unwrap();
        let root = scratch.0.clone();
        scratch.1 = false;
        drop(scratch);
        assert!(root.join("owner.json").is_file());
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn unknown_and_retained_validation_roots_are_never_reclaimed_for_capacity() {
        let parent = std::env::temp_dir().join(format!(
            "lycheedev-validator-unknown-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let control = OperationControl::default();
        fs::create_dir_all(&parent).unwrap();
        let unknown = parent.join("unknown-owner");
        fs::create_dir(&unknown).unwrap();
        fs::write(unknown.join("evidence"), b"unknown").unwrap();
        let mut retained = vec![];
        for _ in 0..7 {
            let mut scratch = Scratch::new_in(&parent, &control).unwrap();
            retained.push(scratch.0.clone());
            scratch.1 = false;
            drop(scratch);
        }
        for _ in 0..2 {
            let error = match Scratch::new_in(&parent, &control) {
                Ok(_) => panic!("unknown retained roots were reclaimed"),
                Err(e) => e,
            };
            assert_eq!(error.code, "luac.scratch_capacity");
            assert!(
                retained
                    .iter()
                    .all(|root| root.join("owner.json").is_file())
            );
            assert_eq!(fs::read(unknown.join("evidence")).unwrap(), b"unknown");
        }
        // Every directory is an owned synthetic fixture, with no process ever
        // started. Production admission never performs this cleanup.
        fs::remove_dir_all(parent).unwrap();
    }
    fn publication_fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lycheedev-validator-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn atomic_retained(path: &Path, reason: &str) -> ToolkitError {
        let cause = ToolkitError::io(std::io::Error::other("modeled uncertain close"));
        cause.clone().with_details(json!({
            "schema":"lycheedev.atomic-temporary-retained.v1", "temporary":path,
            "state":"unconfirmed", "detail":reason, "cause":cause
        }))
    }
    #[test]
    fn owner_publication_retained_temporary_preserves_parent_and_original_reason() {
        let parent = publication_fixture("atomic-owner");
        let control = OperationControl::default();
        let mut temporary = PathBuf::new();
        let error = match Scratch::new_in_with(&parent, &control, |path, _, _| {
            temporary = path.with_file_name(".modeled-atomic.tmp");
            fs::write(&temporary, b"uncertain").unwrap();
            Err(atomic_retained(
                &temporary,
                "publication failed after original handle closed",
            ))
        }) {
            Ok(_) => panic!("failed publication admitted"),
            Err(error) => error,
        };
        assert_eq!(error.code, "external.io");
        assert_eq!(error.exit_code, 5);
        assert_eq!(error.details["reason"], "owner_publication_not_confirmed");
        assert_eq!(
            error.details["cause"]["details"]["temporary"],
            json!(temporary)
        );
        assert_eq!(
            error.details["cause"]["details"]["detail"],
            "publication failed after original handle closed"
        );
        assert_eq!(fs::read(&temporary).unwrap(), b"uncertain");
        assert!(requires_validation_retention(&error));
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn unknown_owner_publication_error_or_unwind_never_drops_parent() {
        let parent = publication_fixture("unknown-publication");
        let control = OperationControl::default();
        let mut created = PathBuf::new();
        let error = match Scratch::new_in_with(&parent, &control, |path, _, _| {
            created = path.parent().unwrap().to_owned();
            Err(ToolkitError::io(std::io::Error::other(
                "unknown publication failure",
            )))
        }) {
            Ok(_) => panic!("failed publication admitted"),
            Err(error) => error,
        };
        assert!(created.is_dir());
        assert_eq!(
            error.details["cause"]["message"],
            "unknown publication failure"
        );
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Scratch::new_in_with(&parent, &control, |path, _, _| {
                created = path.parent().unwrap().to_owned();
                panic!("modeled publication unwind");
            });
        }));
        assert!(unwind.is_err());
        assert!(created.is_dir());
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn validator_finish_retains_atomic_leaf_in_every_checked_close_branch() {
        let control = OperationControl::default();
        for branch in ["primary", "cleanup", "control"] {
            let parent = publication_fixture(branch);
            let scratch = Scratch::new_in(&parent, &control).unwrap();
            let root = scratch.0.clone();
            let temporary = root.join(".modeled-atomic.tmp");
            fs::write(&temporary, b"uncertain").unwrap();
            let leaf = atomic_retained(&temporary, "close or post-close control failed");
            let mut details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,
                "primary":null,"cleanup":null,"control":null});
            details[branch] = serde_json::to_value(&leaf).unwrap();
            let original = ToolkitError::new("delivery.invalid_payload", "original validation", 4)
                .with_details(details);
            let error = scratch.finish(Err(original.clone()), &control).unwrap_err();
            assert_eq!(error.code, original.code);
            assert_eq!(error.exit_code, 4);
            assert_eq!(
                error.details["cause"],
                serde_json::to_value(original).unwrap()
            );
            assert_eq!(fs::read(&temporary).unwrap(), b"uncertain");
            assert!(root.join("retained-owner.json").is_file());
            fs::remove_dir_all(parent).unwrap();
        }
    }
    #[test]
    fn retained_owner_publication_faults_exhaust_bounded_admission_without_reclaim() {
        let parent = publication_fixture("atomic-capacity");
        let control = OperationControl::default();
        let mut retained = vec![];
        for _ in 0..8 {
            let failed = Scratch::new_in_with(&parent, &control, |path, _, _| {
                let temporary = path.with_file_name(".modeled-atomic.tmp");
                fs::write(&temporary, b"uncertain").unwrap();
                retained.push(temporary.clone());
                Err(atomic_retained(&temporary, "close unconfirmed"))
            });
            assert!(failed.is_err());
        }
        let error = match Scratch::new_in_with(&parent, &control, |_, _, _| {
            panic!("ninth publication must not run")
        }) {
            Ok(_) => panic!("capacity admitted ninth owner"),
            Err(error) => error,
        };
        assert_eq!(error.code, "luac.scratch_capacity");
        assert_eq!(error.details["blockedBy"].as_array().unwrap().len(), 8);
        assert!(
            retained
                .iter()
                .all(|path| fs::read(path).unwrap() == b"uncertain")
        );
        // Injected fault fixtures own these files; no uncertain OS close or
        // process is induced. Production never reclaims these roots by path.
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn retained_cause_walk_is_bounded_and_fails_closed() {
        let ordinary = ToolkitError::new("delivery.invalid_payload", "plain input error", 4);
        assert!(!requires_validation_retention(&ordinary));
        let mut deep = ordinary;
        for _ in 0..34 {
            deep = ToolkitError::new("delivery.invalid_payload", "wrapped", 4)
                .with_details(json!({"schema":storage::FILE_CLEANUP_SCHEMA,
                    "primary":deep,"cleanup":null,"control":null}));
        }
        assert!(requires_validation_retention(&deep));
    }
}
