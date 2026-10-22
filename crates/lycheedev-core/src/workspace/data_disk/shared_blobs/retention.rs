//! Read-only protection for an independently owned Data body during explicit
//! CAP removal. The caller holds its canonical blob lease. Never acquire quota
//! here: publishers use quota -> blob, and may be waiting for that lease.
use super::*;

pub(crate) fn protects_evidence_body(
    home: &Path,
    digest: &str,
    bytes: u64,
    control: &OperationControl,
) -> Result<bool> {
    let checks = Checks {
        control,
        supplemental: None,
    };
    checks.check()?;
    // Preserve the historical missing/corrupt-blob diagnostics when this is not
    // a possible Data key. In particular, no production-size limit is applied
    // to an old capture before looking for an actual Data attribution record.
    if digest.len() != 64 {
        return Ok(false);
    }
    // Only this independent Data ownership key is canonicalized. The caller
    // has authenticated the original signed CAP; its identity, other-CAP and
    // retention comparisons, and actual blob consumers stay case-sensitive.
    let digest = digest.to_ascii_lowercase();
    if !valid_digest(&digest, 64) {
        return Ok(false);
    }
    let root = Directory::open_shared_controlled(home, &mut Controlled(&checks))?;
    let result = descend(
        &root,
        &["indexes", "data-derived-v1", "shared-blobs-v1"],
        home,
        &digest,
        bytes,
        &checks,
    );
    join(result, close_directory(root), &checks)
}

fn descend(
    parent: &Directory,
    components: &[&str],
    home: &Path,
    digest: &str,
    bytes: u64,
    checks: &Checks<'_>,
) -> Result<bool> {
    let Some((name, tail)) = components.split_first() else {
        return inspect_reference(parent, home, digest, bytes, checks);
    };
    // Native optional entry acquisition alone proves absence. An attribute-only
    // original handle is returned before the post-open check, so every acquired
    // owner is checked closed on cancellation/error. No directory is created.
    let Some(anchor) = parent
        .open_optional_inspection_file_controlled(OsStr::new(*name), &mut Controlled(checks))?
    else {
        checks.check()?;
        return Ok(false);
    };
    let opened = (|| {
        checks.check()?;
        if !anchor.metadata()?.is_dir() {
            return Err(integrity());
        }
        Directory::open_shared_controlled(&parent.path().join(*name), &mut Controlled(checks))
    })();
    let closed = finish_file(anchor, Ok(()), checks);
    let directory = match opened {
        Ok(directory) => match closed {
            Ok(()) => directory,
            Err(error) => return join(Err(error), close_directory(directory), checks),
        },
        Err(error) => return join(Err(error), closed.err(), checks),
    };
    let result = descend(&directory, tail, home, digest, bytes, checks);
    join(result, close_directory(directory), checks)
}

fn inspect_reference(
    directory: &Directory,
    home: &Path,
    digest: &str,
    bytes: u64,
    checks: &Checks<'_>,
) -> Result<bool> {
    let name = format!("{digest}.intent");
    let raw = read_record(directory, &name, INTENT_BYTES, checks)?;
    let complete = read_record(
        directory,
        &format!("{digest}.complete"),
        COMPLETE_BYTES,
        checks,
    )?;
    let Some(raw) = raw else {
        return if complete.is_none() {
            Ok(false)
        } else {
            Err(integrity())
        };
    };
    let reference = decode(&name, &raw)?;
    if bytes != reference.bytes
        || complete
            .as_ref()
            .is_some_and(|done| done.as_slice() != completion(&raw))
    {
        return Err(integrity());
    }
    // A valid pending intent also owns its possible body until checked recovery.
    // A live original WRITE/DELETE intent handle may reject read_record's
    // immutable Reader. Such an error is retained, never treated as absence.
    // Authenticate the independent retention claim and its present byte charge.
    // Like an existing other-CAP reference, this is permission to RETAIN, never
    // evidence that the body's content is healthy. No body is consumed here;
    // real readers still perform their original full SHA/EOF verification.
    if !inspect_body(home, &reference, checks, false)? {
        return Err(integrity());
    }
    checks.check()?;
    Ok(true)
}
