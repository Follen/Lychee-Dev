//! Fixed Go entry.go direct remote target adapter. Named intent remains on its
//! separate existing path until controlled configuration lookup/history is ready.
use super::*;
use lycheedev_data::{
    remote::target,
    resource::{Budget, Cost},
};

pub(super) fn validate_flags(flags: &BTreeMap<String, String>) -> Result<()> {
    if flags.contains_key("--file") {
        for flag in [
            "--target",
            "--installation",
            "--product",
            "--build",
            "--region",
            "--locale",
            "--definitions",
            "--from",
            "--offline",
        ] {
            if flags.contains_key(flag) {
                return Err(ToolkitError::arguments(format!(
                    "target resolve --file cannot be combined with {flag}"
                )));
            }
        }
    }
    if flags.contains_key("--target") {
        for flag in [
            "--file",
            "--installation",
            "--product",
            "--build",
            "--region",
            "--locale",
        ] {
            if flags.contains_key(flag) {
                return Err(ToolkitError::arguments(format!(
                    "target resolve --target cannot be combined with {flag}"
                )));
            }
        }
    }
    if flags.contains_key("--build") && !flags.contains_key("--product") {
        return Err(ToolkitError::arguments(
            "target resolve --build requires --product",
        ));
    }
    Ok(())
}
fn request(inv: &Invocation) -> Option<target::Request> {
    if inv.flag("--file").is_some_and(|value| !value.is_empty())
        || inv.flag("--target").is_some_and(|value| !value.is_empty())
        || inv
            .flag("--installation")
            .is_some_and(|value| !value.is_empty())
        || inv.flag("--product").is_none_or(str::is_empty)
    {
        return None;
    }
    Some(target::Request {
        product: inv.flag("--product").unwrap_or("").into(),
        region: inv.flag("--region").unwrap_or("").into(),
        locale: inv.flag("--locale").unwrap_or("").into(),
        full_build: inv.flag("--build").unwrap_or("").into(),
        definitions: inv.flag("--definitions").unwrap_or("").into(),
        parent: inv.flag("--from").unwrap_or("").into(),
        offline: inv.flag_bool("--offline"),
    })
}
pub(super) fn direct(inv: &Invocation) -> Result<Option<Outcome>> {
    let Some(request) = request(inv) else {
        return Ok(None);
    };
    // Fixed command entry validates a direct request before opening workspace,
    // and returns exit 2 for admission even when the provider's error is exit 4.
    request.validate().map_err(|mut error| {
        error.exit_code = 2;
        error
    })?;
    let budget = Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    );
    let resolved = target::resolve(&inv.home, &request, &inv.control, Some(&budget))?;
    let mut outcome = Outcome::new(serde_json::to_value(&resolved.pin)?);
    outcome.selection = json!({"snapshot":resolved.pin.id,"observedAt":resolved.observed_at,"offline":resolved.offline});
    outcome
        .provenance
        .push(serde_json::to_value(&resolved.capture)?);
    outcome.warnings.push(json!("Remote target metadata is pinned; this does not download data archives. Offline resolution reuses a recorded observation, not the server's current state."));
    inv.control.check()?;
    budget.check()?;
    Ok(Some(outcome))
}
#[cfg(test)]
mod tests;
