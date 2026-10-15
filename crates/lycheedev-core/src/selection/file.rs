//! Default --file pin resolution. The held input remains open until the pin
//! operation completes; every acquired file/metadata owner has a checked close.
use super::*;
use crate::control::OperationControl;
use std::io::Read;
mod wire;
fn failed(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("command.failed", message, 5)
}
fn finish<T>(result: Result<T>, cleanup: Result<()>, control: &OperationControl) -> Result<T> {
    let stopped = control.check();
    let already_stopped = matches!((&result, &stopped), (Err(first), Err(stop))
        if first.code == stop.code && first.message == stop.message
            && first.exit_code == stop.exit_code && first.details == stop.details);
    let result = crate::metadata::historical_write::finish_historical(result, cleanup);
    if already_stopped {
        result
    } else {
        selection_finish(result, stopped)
    }
}
fn read(file: &mut impl Read, control: &OperationControl) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(65537);
    let mut buffer = [0u8; 8192];
    while result.len() < 65537 {
        control.check()?;
        let maximum = buffer.len().min(65537 - result.len());
        let n = file.read(&mut buffer[..maximum])?;
        result.extend_from_slice(&buffer[..n]);
        control.check()?;
        if n == 0 {
            break;
        }
    }
    if result.len() > 65536 {
        return Err(failed("selection.spec_limit"));
    }
    Ok(result)
}
// Use the shared validation rules and preserve Go's plain-error messages at
// this new consumer. Digit-only build admission is stricter than Rust parsing.
pub(super) fn validate_spec(spec: &SelectionSpec) -> Result<()> {
    // Source errors precede the other fields in fixed validatePins.
    let mut validation = spec.clone();
    if let Some(data) = validation.data.as_mut() {
        if !data
            .full_build
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
        {
            data.full_build.clear();
        }
    }
    if let Some(changes) = validation.changes.as_mut() {
        if !changes
            .full_build
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
        {
            changes.full_build.clear();
        }
    }
    validate(&validation).map_err(|error| {
        if error.code == "selection.product_mismatch" {
            let data = spec.data.as_ref().map(|p| p.product.as_str());
            let changes = spec.changes.as_ref().map(|p| p.product.as_str());
            let (left, right) = match (data, changes) {
                (Some(a), Some(b)) if a != b => (a, b),
                _ => (
                    spec.source
                        .as_ref()
                        .map(|p| p.product.as_str())
                        .unwrap_or(""),
                    data.or(changes).unwrap_or(""),
                ),
            };
            failed(format!("selection.product_mismatch: {left} / {right}"))
        } else {
            failed(error.code)
        }
    })
}
pub(super) fn stored_pin(raw: &[u8], requested: &str, control: &OperationControl) -> Result<PinnedSet> {
    let (pin, unmarshal_only) = wire::stored(raw, control)?;
    if pin.id != requested
        || pin.schema != "lycheedev.selection.v1"
        || !pin
            .id
            .strip_prefix("PIN-")
            .is_some_and(|id| valid_digest(id, 64))
    {
        return Err(failed("selection.invalid_pin"));
    }
    validate_spec(&pin.spec)?;
    if unmarshal_only {
        return Err(failed(
            "Time.MarshalJSON: timezone hour outside of range [0,23]",
        ));
    }
    if identity(&pin)? != pin.id {
        return Err(failed("selection.pin_integrity"));
    }
    control.check()?;
    Ok(pin)
}
fn pin(
    writer: &crate::metadata::historical_write::HistoricalWriter<'_>,
    decoded: wire::Decoded,
    control: &OperationControl,
    published: &mut Option<String>,
) -> Result<PinnedSet> {
    let mut spec = decoded.spec;
    control.check()?;
    if !spec.parent.is_empty() {
        let raw = writer.document(&format!("pin/{}", spec.parent))?;
        let parent = stored_pin(&raw.bytes, &spec.parent, control)?;
        inherit_parent(&mut spec, parent).map_err(|error| failed(error.code))?;
    }
    validate_spec(&spec)?;
    if decoded.time_unmarshal_only {
        return Err(failed(
            "Time.MarshalJSON: timezone hour outside of range [0,23]",
        ));
    }
    let mut pin = PinnedSet {
        schema: "lycheedev.selection.v1".into(),
        id: String::new(),
        spec,
    };
    pin.id = identity(&pin)?;
    let raw = serde_json::to_vec(&pin)?;
    let key = format!("pin/{}", pin.id);
    match writer.commit(&[crate::metadata::historical_write::HistoricalMutation {
        key: &key,
        expected_generation: 0,
        bytes: &raw,
    }]) {
        Ok(()) => *published = Some(pin.id.clone()),
        Err(error) if error.code == "vault.generation_conflict" && error.details.is_null() => {
            // Fixed Go reads/authenticates the actual persisted winner even on
            // a byte-identical conflict. A corrupt row is never overwritten.
            let winner = writer.document(&key)?;
            let winner = stored_pin(&winner.bytes, &pin.id, control)?;
            *published = Some(winner.id.clone());
            return Ok(winner);
        }
        Err(error) => return Err(error),
    }
    control.check()?;
    Ok(pin)
}
/// Explicit source-file path has the same normal read/share/replace behavior as
/// Go os.Open, including symlinks. No immutable-blob reparse policy is added.
/// Synchronous OS open/read are checked before and after; they are not claimed
/// to be interrupted halfway through the kernel call.
pub fn resolve_selection_file_controlled(
    home: &Path,
    path: &Path,
    control: &OperationControl,
) -> Result<PinnedSet> {
    control.check()?;
    let mut file = fs::File::open(path)?;
    let mut published = None;
    let result = (|| {
        control.check()?;
        let raw = read(&mut file, control)?;
        let decoded = wire::decode(&raw, control)?;
        // Go opens writable metadata before resolving parents/validating pins.
        // Keep one checked connection through parent reads, CAS and winner reads.
        Metadata::with_historical_write(home, control, |writer| {
            pin(writer, decoded, control, &mut published)
        })
    })();
    let result = finish(
        result,
        storage::close_file(file).map_err(ToolkitError::from),
        control,
    );
    completed(result, published)
}
fn completed<T>(result: Result<T>, published: Option<String>) -> Result<T> {
    match (result, published) {
        (Err(mut error), Some(id)) => {
            let primary = error.clone();
            error.message = format!("{}\nselection pin already committed: {}", error.message, id);
            error.details = json!({"schema":"lycheedev.selection-file-error.v1","primary":primary,"completedPin":id});
            Err(error)
        }
        (result, _) => result,
    }
}
#[cfg(test)]
mod tests;
