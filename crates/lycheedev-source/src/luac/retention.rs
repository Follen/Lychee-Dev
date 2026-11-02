//! Owned compiler scratch disposition. A returned error is not terminal-process
//! proof. This marker survives the core's checked-close aggregates so a caller
//! that owns a parent directory cannot erase unresolved child evidence.
use lycheedev_core::{ToolkitError, storage};
use serde_json::{Value, json};
use std::path::Path;
pub const SCHEMA: &str = "lycheedev.luac-retained-scratch.v1";
pub fn retain_scratch(error: ToolkitError, root: &Path) -> ToolkitError {
    let mut retained = error.clone();
    retained.details = json!({"schema":SCHEMA,"reason":"process_terminal_not_confirmed","retainedScratch":root,"ownerProcessId":std::process::id(),"cause":error});
    retained
}
pub fn requires_scratch_retention(error: &ToolkitError) -> bool {
    fn check(details: &Value, depth: usize) -> bool {
        // Unknown overly deep provenance is not safe cleanup proof.
        if depth >= 32 {
            return true;
        }
        if details["schema"] == SCHEMA {
            return true;
        }
        if details["schema"] == storage::FILE_CLEANUP_SCHEMA {
            return ["primary", "cleanup", "control"]
                .iter()
                .any(|key| !details[key].is_null() && check(&details[key]["details"], depth + 1));
        }
        false
    }
    check(&error.details, 0)
}
