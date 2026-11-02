//! Source manifest and single-use startup persistence. Each ordinary metadata
//! owner is closed before return, including every early error and sticky stop.
use lycheedev_core::{
    Result,
    control::OperationControl,
    metadata::{Metadata, owned_write::finish_owned},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::path::Path;

pub(crate) fn read<T: DeserializeOwned>(
    home: &Path,
    key: &str,
    control: &OperationControl,
) -> Result<T> {
    let store = Metadata::open_controlled(home, false, control)?;
    let result = store.read_controlled(key, control);
    finish_control(finish_owned(result, store.close()), control.check())
}

pub(crate) fn put_manifest(
    home: &Path,
    key: &str,
    value: &impl Serialize,
    control: &OperationControl,
) -> Result<()> {
    write(home, key, value, true, control).map(|_| ())
}
pub(crate) fn create_intent(
    home: &Path,
    key: &str,
    value: &impl Serialize,
    control: &OperationControl,
) -> Result<i64> {
    write(home, key, value, false, control)
}
fn write(
    home: &Path,
    key: &str,
    value: &impl Serialize,
    replace: bool,
    control: &OperationControl,
) -> Result<i64> {
    let mut store = Metadata::open_controlled(home, true, control)?;
    let mut state = "not_attempted";
    let result = (|| {
        let generation = if replace {
            store.generation_controlled(key, control)?
        } else {
            None
        };
        control.check()?;
        state = "unconfirmed";
        store.put_controlled(key, value, generation, control)?;
        state = "committed";
        store.generation_controlled(key, control)?.ok_or_else(|| {
            lycheedev_core::ToolkitError::invalid("published Source metadata missing")
        })
    })();
    finish_control(finish_owned(result, store.close()), control.check()).map_err(|mut error| {
        if state != "not_attempted" {
            error.details = json!({"schema":"lycheedev.source-persistence.v1","key":key,"mutation":"put","state":state,"cause":error.clone()});
        }
        error
    })
}
pub(crate) fn consume<T: DeserializeOwned, U>(
    home: &Path,
    key: &str,
    expected: Option<i64>,
    control: &OperationControl,
    validate: impl FnOnce(T) -> Result<U>,
) -> Result<U> {
    let mut store = Metadata::open_controlled(home, true, control)?;
    let result = match expected {
        Some(generation) => store.consume_generation_controlled(key, generation, control, validate),
        None => store.consume_controlled(key, control, validate),
    };
    let consumed = result.is_ok();
    finish_control(finish_owned(result, store.close()), control.check()).map_err(|mut error| {
        if consumed {
            error.details = json!({"schema":"lycheedev.source-persistence.v1","key":key,"mutation":"consume","state":"committed","cause":error.clone()});
        }
        error
    })
}

/// The first cause remains primary; a repeated observation of the same sticky
/// stop is not a second cleanup event.
pub(crate) fn finish_control<T>(result: Result<T>, stopped: Result<()>) -> Result<T> {
    if let (Err(first), Err(stop)) = (&result, &stopped)
        && first.code == stop.code
        && first.message == stop.message
        && first.exit_code == stop.exit_code
        && first.details == stop.details
    {
        return result;
    }
    finish_owned(result, stopped)
}

/// Argument/cwd validation can fail after the durable intent was committed but
/// before a process was launched. Keep that fact without deleting or replaying.
pub(crate) fn retained_intent(mut error: lycheedev_core::ToolkitError, key: &str, generation: i64) -> lycheedev_core::ToolkitError {
    error.details = json!({"schema":"lycheedev.source-persistence.v1","key":key,"generation":generation,"mutation":"put","state":"committed","phase":"before_launch","cause":error.clone()});
    error
}

#[cfg(test)]
mod tests;
