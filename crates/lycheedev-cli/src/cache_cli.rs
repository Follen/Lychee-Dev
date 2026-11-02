//! Command admission stays in the CLI; Core owns storage operations.
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};

fn parse_request(inv: &Invocation) -> Result<lycheedev_core::cache::CacheRequest> {
    use lycheedev_core::cache::CacheRequest as C;
    Ok(match inv.command.as_str() {
        "cache status" if inv.flags.contains_key("--data") => C::DataStatus,
        "cache status" => C::Status,
        "cache verify" => C::Verify,
        "cache prune" => C::Prune {
            target_bytes: inv.usize_flag("--target-bytes", 0, 0, usize::MAX)? as u64,
            dry_run: inv.flag_bool("--dry-run"),
            max_objects: inv.usize_flag("--max-objects", 0, 0, 100000)?,
            include_uncommitted: inv.flag_bool("--uncommitted"),
        },
        _ => return Err(ToolkitError::arguments("unknown cache verb")),
    })
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<lycheedev_core::Outcome> {
    let ctx = crate::core_cli::context(inv);
    lycheedev_core::cache::dispatch(&ctx, parse_request(inv)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_pruning_numbers_fail_before_storage() {
        for (flag, value) in [
            ("--target-bytes", "-1"),
            ("--target-bytes", "18446744073709551616"),
            ("--max-objects", "-1"),
            ("--max-objects", "100001"),
            ("--max-objects", "bad"),
        ] {
            let args = ["cache", "prune", flag, value].map(str::to_owned);
            let error = crate::parse(&args)
                .and_then(|inv| parse_request(&inv))
                .unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
        }
    }
}
