//! Identity-bound retirement of the existing derived CAS and typed generations.
//! No retention ledger: a completed object may precede its generation after a
//! crash. Only canonical CAS files are candidates; every generation must first
//! pass the domain's strict reference projection. Quota remains held throughout.
use super::*;
use lycheedev_windows::filesystem::owned_cleanup::AuthenticatedCleanup;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::Read,
    path::Component,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct DerivedRetirementOptions {
    /// Remaining bytes in objects + generations, including retained unknowns.
    pub target_bytes: u64,
    /// Distinct object groups with a deletion attempt; zero means no count cap.
    pub max_objects: usize,
    pub dry_run: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedRetirementReport<A = ()> {
    pub scope: &'static str,
    pub target_bytes: u64,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub reclaimed_bytes: u64,
    pub data_before_bytes: u64,
    pub data_after_bytes: u64,
    pub retained_other_data_bytes: u64,
    pub unattributed_scope_bytes: u64,
    pub attempted_objects: usize,
    pub removed_objects: usize,
    pub removed_generations: usize,
    pub removed_orphan_objects: usize,
    pub skipped_in_use: usize,
    pub truncated: bool,
    pub complete: bool,
    pub dry_run: bool,
    pub ordering: &'static str,
    pub coverage: Coverage,
    pub deletions: Vec<Deletion>,
    // Travels with returned receipts through domain serialization. The bound
    // includes both the original typed records and output conversion scratch.
    #[serde(skip)]
    receipt_allocation: Option<A>,
}
impl<A> std::fmt::Debug for DerivedRetirementReport<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedRetirementReport")
            .field("before_bytes", &self.before_bytes)
            .field("after_bytes", &self.after_bytes)
            .field("deletions", &self.deletions)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deletion {
    kind: &'static str,
    identity: String,
    bytes: u64,
    /// requested-close-unconfirmed is never reported as reclaimed bytes.
    state: &'static str,
}
struct Manifest {
    key: String,
    sha256: String,
    bytes: u64,
}
struct Object {
    bytes: u64,
    manifests: Vec<Manifest>,
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("vault.data_cache_integrity", message, 4)
}
fn lowercase_digest(value: &str, length: usize) -> bool {
    valid_digest(value, length) && !value.bytes().any(|b| b.is_ascii_uppercase())
}
fn directory<'a>(owner: &'a DerivedReservation<'_>, path: &Path) -> &'a Directory {
    owner
        .directories
        .iter()
        .find(|d| d.path() == path)
        .expect("held retirement root")
}
fn scoped_usage(owner: &DerivedReservation<'_>) -> Result<u64> {
    let mut entries = 0;
    let mut files = 0;
    let mut bytes = 0;
    for root in [&owner.paths.objects, &owner.paths.generations] {
        bytes = sum([
            bytes,
            scan(
                root,
                &owner.paths.staging,
                &owner.paths.metadata,
                0,
                &mut entries,
                &mut files,
                &owner.control,
                &owner.directories,
            )?,
        ])?;
    }
    Ok(bytes)
}
// Per retained entry covers BTree/Vec growth, digest/key strings and its guard.
// Raw manifest buffers and final receipt capacity have separate exact owners.
fn retain_allocation<A>(
    allocations: &mut Vec<A>,
    reserve: &mut impl FnMut(usize) -> Result<A>,
    bytes: usize,
) -> Result<()> {
    let bound = bytes
        .checked_add(std::mem::size_of::<A>().saturating_mul(2))
        .ok_or_else(|| invalid("Retirement allocation overflow"))?;
    let guard = reserve(bound)?;
    allocations
        .try_reserve(1)
        .map_err(|_| invalid("Cannot allocate retirement budget owners"))?;
    allocations.push(guard);
    Ok(())
}
fn snapshot<A>(
    owner: &DerivedReservation<'_>,
    reference: &mut impl FnMut(&str, &[u8]) -> Result<DerivedObject>,
    reserve: &mut impl FnMut(usize) -> Result<A>,
    allocations: &mut Vec<A>,
) -> Result<BTreeMap<String, Object>> {
    let checks = &owner.control;
    let root = directory(owner, &owner.paths.objects);
    root.verify_controlled(&mut Controlled(checks))?;
    let mut objects = BTreeMap::new();
    let mut entries = 0usize;
    // A canonical CAS entry is produced before its manifest. Unknown names or
    // nesting are charged by inventory but acquire no deletion authority.
    for entry in fs::read_dir(root.path())? {
        checks.check()?;
        let entry = entry?;
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(invalid("Retirement inventory exceeds its bound"));
        }
        let name = entry.file_name();
        let Some(prefix) = name.to_str().filter(|s| lowercase_digest(s, 2)) else {
            continue;
        };
        let shard = Directory::open_controlled(&entry.path(), &mut Controlled(checks))?;
        let result = (|| {
            for child in fs::read_dir(shard.path())? {
                checks.check()?;
                let child = child?;
                entries += 1;
                if entries > MAX_ENTRIES {
                    return Err(invalid("Retirement inventory exceeds its bound"));
                }
                let name = child.file_name();
                let Some(suffix) = name.to_str().filter(|s| lowercase_digest(s, 62)) else {
                    continue;
                };
                let file = shard
                    .open_optional_reader_owner_controlled(&name, &mut Controlled(checks))?
                    .ok_or_else(|| invalid("Canonical Data object disappeared during inventory"))?;
                let result = (|| {
                    checks.check()?;
                    let info = file.metadata()?;
                    if !info.is_file() {
                        return Err(invalid("Canonical Data object is not a regular file"));
                    }
                    retain_allocation(allocations, reserve, 1024)?;
                    objects.insert(
                        format!("{prefix}{suffix}"),
                        Object {
                            bytes: info.len(),
                            manifests: Vec::new(),
                        },
                    );
                    Ok(())
                })();
                finish_file(file, result, checks)?;
            }
            Ok(())
        })();
        join(result, close_directory(shard), checks)?;
    }
    // This proof is deliberately not truncated by --max-objects. One unknown
    // or malformed generation prevents *all* deletion, including orphan cleanup.
    let root = directory(owner, &owner.paths.generations);
    root.verify_controlled(&mut Controlled(checks))?;
    for entry in fs::read_dir(root.path())? {
        checks.check()?;
        let entry = entry?;
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(invalid("Retirement inventory exceeds its bound"));
        }
        let name = entry.file_name();
        let key = name
            .to_str()
            .and_then(|s| s.strip_suffix(".json"))
            .filter(|s| lowercase_digest(s, 64))
            .ok_or_else(|| invalid("Unknown generation prevents proving Data references"))?;
        let mut raw_allocation = None;
        let bytes = owner
            .read_generation_with_admission(key, |length| {
                raw_allocation = Some(reserve(length)?);
                Ok(())
            })?
            .ok_or_else(|| invalid("Generation disappeared during retirement inventory"))?;
        let referenced = reference(key, &bytes)?;
        checks.check()?;
        if !lowercase_digest(&referenced.sha256, 64) {
            return Err(invalid("Generation has a noncanonical object identity"));
        }
        let object = objects
            .get_mut(&referenced.sha256)
            .ok_or_else(|| invalid("Generation references an absent canonical Data object"))?;
        if object.bytes != referenced.bytes {
            return Err(invalid(
                "Generation references a conflicting Data object length",
            ));
        }
        retain_allocation(allocations, reserve, 1024)?;
        object
            .manifests
            .try_reserve(1)
            .map_err(|_| invalid("Cannot allocate bounded retirement references"))?;
        object.manifests.push(Manifest {
            key: key.into(),
            sha256: storage::sha256(&bytes),
            bytes: bytes.len() as u64,
        });
        drop(bytes);
        drop(raw_allocation);
    }
    for object in objects.values_mut() {
        object.manifests.sort_by(|a, b| a.key.cmp(&b.key));
    }
    checks.check()?;
    Ok(objects)
}
fn authenticate(
    directory: &Directory,
    name: &OsStr,
    digest: &str,
    bytes: u64,
    checks: &Checks<'_>,
) -> Result<Option<AuthenticatedCleanup>> {
    checks.check()?;
    let Some(candidate) =
        directory.open_cleanup_candidate_owner_controlled(name, &mut Controlled(checks))?
    else {
        return Ok(None);
    };
    match candidate.authenticate::<ToolkitError>(|file| {
        checks.check()?;
        if file.metadata()?.len() != bytes {
            return Err(invalid("Retirement length changed"));
        }
        let mut hash = Sha256::new();
        let mut remaining = bytes;
        let mut buffer = [0u8; 65536];
        while remaining != 0 {
            checks.check()?;
            let count = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..count])?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
            checks.check()?;
        }
        if file.read(&mut [0])? != 0 || hex::encode(hash.finalize()) != digest {
            return Err(invalid("Retirement SHA-256 or EOF differs"));
        }
        checks.check()
    }) {
        Ok(held) => Ok(Some(held)),
        Err((error, candidate)) => finish_file(candidate.into_file(), Err(error), checks),
    }
}
fn discard<A>(
    held: AuthenticatedCleanup,
    kind: &'static str,
    identity: &str,
    bytes: u64,
    report: &mut DerivedRetirementReport<A>,
    checks: &Checks<'_>,
) -> Result<()> {
    discard_with_close(
        held,
        kind,
        identity,
        bytes,
        report,
        checks,
        storage::close_file,
    )
}
fn discard_with_close<A>(
    held: AuthenticatedCleanup,
    kind: &'static str,
    identity: &str,
    bytes: u64,
    report: &mut DerivedRetirementReport<A>,
    checks: &Checks<'_>,
    close: impl FnOnce(fs::File) -> std::io::Result<()>,
) -> Result<()> {
    if let Err(error) = checks.check() {
        return finish_file(held.into_file(), Err(error), checks);
    }
    if report.dry_run {
        // Same original-handle authentication and pin gating, but no disposition.
        finish_file(held.into_file(), Ok(()), checks)?;
        report.deletions.push(Deletion {
            kind,
            identity: identity.into(),
            bytes,
            state: "planned",
        });
    } else {
        let (file, requested) = held.discard();
        let requested_ok = requested.is_ok();
        let closed = close(file).map_err(ToolkitError::io);
        let closed_ok = closed.is_ok();
        report.deletions.push(Deletion {
            kind,
            identity: identity.into(),
            bytes,
            state: if !requested_ok {
                "not-requested"
            } else if closed_ok {
                "closed"
            } else {
                "requested-close-unconfirmed"
            },
        });
        if requested_ok && closed_ok {
            account(kind, bytes, report);
        }
        return join(requested.map_err(ToolkitError::io), closed.err(), checks);
    }
    account(kind, bytes, report);
    checks.check()
}
fn account<A>(kind: &str, bytes: u64, report: &mut DerivedRetirementReport<A>) {
    report.after_bytes -= bytes;
    report.data_after_bytes -= bytes;
    report.reclaimed_bytes += bytes;
    if kind == "generation" {
        report.removed_generations += 1;
    } else {
        report.removed_objects += 1;
        if kind == "orphan-object" {
            report.removed_orphan_objects += 1;
        }
    }
}
fn retire_object<A>(
    owner: &DerivedReservation<'_>,
    sha: &str,
    object: &Object,
    report: &mut DerivedRetirementReport<A>,
) -> Result<()> {
    let checks = &owner.control;
    let path = owner.paths.object(sha)?;
    let shard = Directory::open_controlled(
        path.parent().expect("object shard"),
        &mut Controlled(checks),
    )?;
    let result = (|| {
        let Some(body) = authenticate(&shard, OsStr::new(&sha[2..]), sha, object.bytes, checks)?
        else {
            report.skipped_in_use += 1;
            return Ok(());
        };
        let mut body = Some(body);
        let mut attempted = false;
        let result = (|| {
            let root = directory(owner, &owner.paths.generations);
            for manifest in &object.manifests {
                checks.check()?;
                let name = format!("{}.json", manifest.key);
                let Some(held) = authenticate(
                    root,
                    OsStr::new(&name),
                    &manifest.sha256,
                    manifest.bytes,
                    checks,
                )?
                else {
                    report.skipped_in_use += 1;
                    // A remaining reference retains the body. Earlier retirement
                    // facts survive; retry scans the remaining references again.
                    return Ok(());
                };
                if !attempted {
                    report.attempted_objects += 1;
                    attempted = true;
                }
                discard(
                    held,
                    "generation",
                    &manifest.key,
                    manifest.bytes,
                    report,
                    checks,
                )?;
            }
            if !attempted {
                report.attempted_objects += 1;
            }
            let kind = if object.manifests.is_empty() {
                "orphan-object"
            } else {
                "object"
            };
            discard(
                body.take().expect("held body"),
                kind,
                sha,
                object.bytes,
                report,
                checks,
            )
        })();
        match body {
            Some(body) => finish_file(body.into_file(), result, checks),
            None => result,
        }
    })();
    join(result, close_directory(shard), checks)
}
// Directory's ordinary ancestors deny rename, but deliberately allow reparse
// writers. This short retirement owner additionally pins EVERY path component
// used by full-path enumeration, before scanning references or deleting files.
// Reuse the existing cleanup mechanism; no path verification-only TOCTOU claim.
fn hold_enumeration_ancestors(
    owner: &DerivedReservation<'_>,
    held: &mut Vec<fs::File>,
) -> Result<()> {
    let checks = &owner.control;
    let mut seen = BTreeSet::new();
    for root in owner.paths.roots().into_iter().chain([
        owner.paths.objects.as_path(),
        owner.paths.generations.as_path(),
    ]) {
        if root.components().count() > 1024 {
            return Err(invalid("Retirement ancestry exceeds its bound"));
        }
        let mut current = PathBuf::new();
        for component in root.components() {
            checks.check()?;
            current.push(component.as_os_str());
            match component {
                Component::Prefix(_) => continue,
                Component::RootDir | Component::Normal(_) => (),
                _ => return Err(invalid("Noncanonical retirement ancestry")),
            }
            if seen.insert(current.clone()) {
                held.try_reserve(1)
                    .map_err(|_| invalid("Cannot allocate retirement ancestry"))?;
                held.push(storage::hold_cleanup_ancestor(&current, checks.control)?);
                checks.check()?;
            }
        }
    }
    Ok(())
}
pub(in crate::workspace) fn retire<A>(
    resources: &DataResources,
    home: &Path,
    limit: u64,
    control: &OperationControl,
    options: DerivedRetirementOptions,
    check: &dyn Fn() -> Result<()>,
    reserve: impl FnMut(usize) -> Result<A>,
    reference: impl FnMut(&str, &[u8]) -> Result<DerivedObject>,
) -> Result<DerivedRetirementReport<A>> {
    let mut owner = DerivedReservation::acquire_inner(
        Some(resources),
        home,
        limit,
        AdditionalPeak::default(),
        control,
        Some(check),
        true,
    )?;
    let (mut report, result) = retire_held(&owner, options, reserve, reference, false);
    let cleanup = owner.cleanup();
    match join(result, cleanup, &owner.control) {
        Ok(()) => Ok(report),
        Err(mut error) => {
            report.complete = false;
            error.details = serde_json::json!({"schema":"lycheedev.data-derived-prune-error.v1",
                "primary":error.clone(), "result":report});
            Err(error)
        }
    }
}

// Runs inside the caller's SAME quota lease. No read/publication can acquire
// references between retirement and reservation of the new generation's peak.
fn retire_held<A>(
    owner: &DerivedReservation<'_>,
    options: DerivedRetirementOptions,
    mut reserve: impl FnMut(usize) -> Result<A>,
    mut reference: impl FnMut(&str, &[u8]) -> Result<DerivedObject>,
    require_possible_target: bool,
) -> (DerivedRetirementReport<A>, Result<()>) {
    let data_before = owner.report.admitted_peak_bytes;
    let mut report = DerivedRetirementReport {
        scope: "data-derived-v1-objects-and-generations",
        target_bytes: options.target_bytes,
        before_bytes: 0,
        after_bytes: 0,
        reclaimed_bytes: 0,
        data_before_bytes: data_before,
        data_after_bytes: data_before,
        retained_other_data_bytes: data_before,
        unattributed_scope_bytes: 0,
        attempted_objects: 0,
        removed_objects: 0,
        removed_generations: 0,
        removed_orphan_objects: 0,
        skipped_in_use: 0,
        truncated: false,
        complete: false,
        dry_run: options.dry_run,
        ordering: "object-sha256-then-generation-key; no-last-used-policy",
        coverage: Coverage {
            automatic_eviction_enabled: require_possible_target,
            ..Default::default()
        },
        deletions: Vec::new(),
        receipt_allocation: None,
    };
    let mut ancestors = Vec::new();
    let mut allocations = Vec::new();
    let result = (|| {
        hold_enumeration_ancestors(&owner, &mut ancestors)?;
        let actual = owner.verify_usage()?.total()?;
        report.data_before_bytes = actual;
        report.data_after_bytes = actual;
        report.before_bytes = scoped_usage(&owner)?;
        report.after_bytes = report.before_bytes;
        report.retained_other_data_bytes = report
            .data_before_bytes
            .checked_sub(report.before_bytes)
            .ok_or_else(|| invalid("Retirement scope exceeds Data inventory"))?;
        let objects = snapshot(&owner, &mut reference, &mut reserve, &mut allocations)?;
        let mut known = 0;
        let mut count = objects.len();
        for object in objects.values() {
            known = sum([known, object.bytes])?;
            count = count
                .checked_add(object.manifests.len())
                .ok_or_else(|| invalid("Retirement receipt bound overflow"))?;
            for manifest in &object.manifests {
                known = sum([known, manifest.bytes])?;
            }
        }
        report.unattributed_scope_bytes = report
            .before_bytes
            .checked_sub(known)
            .ok_or_else(|| invalid("Retirement inventory changed during scan"))?;
        if require_possible_target && report.unattributed_scope_bytes > options.target_bytes {
            return Err(budget_error(&owner.report));
        }
        report.receipt_allocation = Some(reserve(
            count
                .checked_mul(std::mem::size_of::<Deletion>() + 1024)
                .ok_or_else(|| invalid("Retirement receipt allocation overflow"))?,
        )?);
        report
            .deletions
            .try_reserve_exact(count)
            .map_err(|_| invalid("Cannot allocate retirement receipts"))?;
        for (sha, object) in objects {
            owner.control.check()?;
            if report.after_bytes <= options.target_bytes {
                break;
            }
            if options.max_objects != 0 && report.attempted_objects >= options.max_objects {
                report.truncated = true;
                break;
            }
            retire_object(&owner, &sha, &object, &mut report)?;
        }
        report.complete = !report.truncated && report.after_bytes <= options.target_bytes;
        owner.control.check()
    })();
    // Verify while the enumeration ancestry is STILL pinned. Only afterwards
    // close these pins, the ordinary directories, and finally the quota owner.
    let result = result.and_then(|()| {
        let current = owner.verify_usage()?;
        let scope = scoped_usage(&owner)?;
        let expected = if report.dry_run {
            report.before_bytes
        } else {
            report.after_bytes
        };
        if scope != expected {
            return Err(invalid("Retirement occupancy changed outside its receipts"));
        }
        if !report.dry_run {
            report.data_after_bytes = current.total()?;
        }
        Ok(())
    });
    let mut cleanup = Vec::new();
    while let Some(file) = ancestors.pop() {
        cleanup.extend(storage::close_file(file).err().map(ToolkitError::io));
    }
    let result = join(result, cleanup, &owner.control);
    if result.is_err() {
        report.complete = false;
    }
    (report, result)
}
/// Admit growth with quota-triggered retirement of canonical typed cold caches.
/// Never delete for a pre-existing over-limit home or an impossible peak. Other
/// retained Data bytes (including evidence and unknown files) remain charged.
pub(in crate::workspace) fn admit<'a, A>(
    resources: &'a DataResources,
    home: &Path,
    limit: u64,
    control: &'a OperationControl,
    peak: AdditionalPeak,
    check: &'a dyn Fn() -> Result<()>,
    reserve: impl FnMut(usize) -> Result<A>,
    reference: impl FnMut(&str, &[u8]) -> Result<DerivedObject>,
    mut retain_receipt: impl FnMut(A),
) -> Result<(DerivedReservation<'a>, Option<DerivedRetirementReport<A>>)> {
    let mut owner = DerivedReservation::acquire_inner(
        Some(resources),
        home,
        limit,
        peak,
        control,
        Some(check),
        true,
    )?;
    let mut retirement = None;
    let result = (|| {
        let existing = owner.report.existing.total()?;
        let maximum = owner.report.configured_max_bytes;
        let additional = peak.total()?;
        if owner.report.admitted_peak_bytes > maximum {
            if existing > maximum || additional > maximum {
                return Err(budget_error(&owner.report));
            }
            let scope = scoped_usage(&owner)?;
            let retained = existing
                .checked_sub(scope)
                .ok_or_else(|| invalid("Retirement scope exceeds Data inventory"))?;
            // If protected/non-typed bytes alone make admission impossible,
            // do not delete even one otherwise reclaimable typed generation.
            let Some(target) = maximum.checked_sub(sum([retained, additional])?) else {
                return Err(budget_error(&owner.report));
            };
            let (report, result) = retire_held(
                &owner,
                DerivedRetirementOptions {
                    target_bytes: target,
                    max_objects: 0,
                    dry_run: false,
                },
                reserve,
                reference,
                true,
            );
            retirement = Some(report);
            // The serialized receipt may outlive this quota owner, including
            // producer/outer cleanup failures. Transfer its charge now, even
            // when retirement stopped after only some completed deletions.
            if let Some(allocation) = retirement
                .as_mut()
                .and_then(|r| r.receipt_allocation.take())
            {
                retain_receipt(allocation);
            }
            result?;
            owner.report.existing = owner.verify_usage()?;
            owner.report.admitted_peak_bytes = sum([owner.report.existing.total()?, additional])?;
            if owner.report.admitted_peak_bytes > maximum {
                let mut error = budget_error(&owner.report);
                error.message = "Data capacity remains insufficient after typed cache retirement; see completed retirement facts".into();
                return Err(error);
            }
        }
        // This is a per-admission policy, not a claim of automatic shared-blob
        // reclamation or complete whole-Data accounting.
        owner.report.coverage.automatic_eviction_enabled = true;
        owner.control.check()
    })();
    match result {
        Ok(()) => Ok((owner, retirement)),
        Err(error) => {
            let cleanup = owner.cleanup();
            let mut error = join::<()>(Err(error), cleanup, &owner.control).unwrap_err();
            if let Some(report) = retirement {
                error.details = serde_json::json!({"schema":"lycheedev.data-derived-auto-admission-error.v1",
                    "primary":error.clone(), "retirement":report});
            }
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests;
