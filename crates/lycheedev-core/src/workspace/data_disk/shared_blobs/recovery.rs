//! Recovery of the closed-body / missing-completion crash window. This never
//! deletes a shared body or an unknown stage. Quota excludes every Data
//! publisher; the canonical blob lease excludes same-body evidence publishers.
use super::*;

pub(super) fn complete_closed_publication(
    owner: &mut DerivedReservation<'_>,
    reference: &BlobRef,
    raw: &[u8; INTENT_BYTES],
) -> Result<bool> {
    let extra = COMPLETE_BYTES as u64;
    if sum([owner.report.existing.total()?, extra])? > owner.report.configured_max_bytes {
        // Reuse in an over-limit home remains zero-growth. Never silently add
        // even a completion record without preadmission.
        return Ok(false);
    }
    let guard = ExclusiveLock::resource_wait_checked(
        &owner.home.join("locks"),
        &format!("blob:{}", reference.sha256),
        Some(owner.control.control.deadline()),
        || owner.control.check(),
    )?;
    let result = (|| {
        let path = evidence::blob_path(&owner.home, &reference.sha256)?;
        let shard_path = path.parent().expect("canonical shard");
        let shard = Directory::open_controlled(shard_path, &mut Controlled(&owner.control))?;
        // Full-path enumeration is protected by the same ancestor pins used by
        // retirement. Do not infer ownership/liveness from filename, age or PID.
        let mut ancestors = Vec::new();
        let inspected = (|| {
            if shard_path.components().count() > 1024 {
                return Err(integrity());
            }
            let mut current = PathBuf::new();
            for component in shard_path.components() {
                current.push(component.as_os_str());
                if matches!(component, std::path::Component::Prefix(_)) {
                    continue;
                }
                ancestors.try_reserve(1).map_err(|_| integrity())?;
                ancestors.push(storage::hold_cleanup_ancestor(
                    &current,
                    owner.control.control,
                )?);
                owner.control.check()?;
            }
            for (index, entry) in fs::read_dir(shard.path())?.enumerate() {
                owner.control.check()?;
                if index >= MAX_ENTRIES {
                    return Err(integrity());
                }
                let entry = entry?;
                let name = entry.file_name();
                let Some(name) = name.to_str().filter(|name| {
                    name.len() == 62
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                }) else {
                    return Ok(false);
                };
                // Unknown/reparse/directory entries cannot become proof that
                // no possible retained shard stage exists.
                let file = shard
                    .open_file_controlled(OsStr::new(name), &mut Controlled(&owner.control))?;
                let regular = file
                    .metadata()
                    .map(|m| m.is_file())
                    .map_err(ToolkitError::io);
                if !finish_file(file, regular, &owner.control)? {
                    return Ok(false);
                }
            }
            // Verify the winner while the canonical body lease is still held.
            inspect_body(&owner.home, reference, &owner.control, true)
        })();
        let mut cleanup = Vec::new();
        while let Some(anchor) = ancestors.pop() {
            cleanup.extend(storage::close_file(anchor).err().map(ToolkitError::io));
        }
        cleanup.extend(close_directory(shard));
        if !join(inspected, cleanup, &owner.control)? {
            return Ok(false);
        }
        owner.admit_shared(extra, 0)?;
        owner.publish_shared_record(&format!("{}.complete", reference.sha256), &completion(raw))?;
        Ok(true)
    })();
    join(result, guard.close().err(), &owner.control)
}
