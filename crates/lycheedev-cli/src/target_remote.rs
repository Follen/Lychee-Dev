//! Fixed Go entry.go local/remote and named target adapters.
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

pub(super) fn local(inv: &Invocation) -> Result<Outcome> {
    let request = lycheedev_data::installed::TargetRequest {
        installation: inv.flag("--installation").unwrap_or("").into(),
        product: inv.flag("--product").unwrap_or("").into(),
        full_build: inv.flag("--build").unwrap_or("").into(),
        region: inv.flag("--region").unwrap_or("").into(),
        locale: inv.flag("--locale").unwrap_or("").into(),
        definitions: inv.flag("--definitions").unwrap_or("").into(),
        parent: inv.flag("--from").unwrap_or("").into(),
        offline: inv.flag_bool("--offline"),
    };
    request.validate().map_err(|mut error| {
        error.exit_code = 2;
        error
    })?;
    let result = lycheedev_data::installed::prepare_target(&inv.home, &request, &inv.control)?;
    let mut outcome = Outcome::new(serde_json::to_value(&result.pin)?);
    outcome.selection =
        json!({"snapshot":result.pin.id,"installation":result.installation,"client":result.client});
    outcome
        .provenance
        .push(serde_json::to_value(result.capture)?);
    Ok(outcome)
}
pub(super) fn named(inv: &Invocation, key: &str) -> Result<Outcome> {
    let budget = Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    );
    let resolved = lycheedev_core::selection::target::resolve_controlled_with_check(
        &inv.home,
        key,
        inv.flag("--definitions").unwrap_or(""),
        &inv.control,
        || budget.check(),
        |config, definitions| {
            if config.source == "installation" {
                let resolved = lycheedev_data::installed::prepare_target(
                    &inv.home,
                    &lycheedev_data::installed::TargetRequest {
                        installation: config.installation.clone().into(),
                        product: config.product.clone(),
                        full_build: config.full_build.clone(),
                        region: config.region.clone(),
                        locale: config.locale.clone(),
                        definitions: definitions.into(),
                        parent: inv.flag("--from").unwrap_or("").into(),
                        offline: inv.flag_bool("--offline"),
                    },
                    &inv.control,
                )?;
                return Ok(resolved.pin);
            }
            target::resolve(
                &inv.home,
                &target::Request {
                    product: config.product.clone(),
                    region: config.region.clone(),
                    locale: config.locale.clone(),
                    full_build: config.full_build.clone(),
                    definitions: definitions.into(),
                    parent: inv.flag("--from").unwrap_or("").into(),
                    offline: inv.flag_bool("--offline"),
                },
                &inv.control,
                Some(&budget),
            )
            .map(|resolved| resolved.pin)
            .map_err(|mut error| {
                if !config.full_build.is_empty()
                    && matches!(
                        error.code.as_str(),
                        "records.build_unavailable" | "records.remote_object_missing"
                    )
                {
                    let original = error.clone();
                    error.code = "selection.target_build_unavailable".into();
                    error.exit_code = 3;
                    error.message =
                        format!("selection.target_build_unavailable: {}", original.message);
                    // Keep existing multi-cause/cleanup details intact.
                }
                error
            })
        },
    )?;
    let context = json!({"target":resolved.config.name,"snapshot":resolved.pin.id});
    let mut outcome = Outcome::new(serde_json::to_value(resolved)?);
    outcome.selection = context;
    // Go named resolution deliberately does not append the preparer's capture
    // or the direct-remote warning. Those immutable captures remain persisted.
    Ok(outcome)
}

// Named resolution reaches Go queryFault without direct-request validation's
// special exit-2 bypass. Preserve full cleanup details while adding its stage.
pub(super) fn named_fault(mut error: ToolkitError) -> ToolkitError {
    if matches!(
        error.code.as_str(),
        "selection.target_build_unavailable"
            | "selection.target_missing"
            | "selection.target_exists"
            | "selection.target_ambiguous"
            | "selection.target_in_use"
            | "selection.target_format"
            | "selection.data_product_build_mismatch"
            | "selection.installation_missing"
            | "selection.installation_ambiguous"
            | "selection.installation_product_mismatch"
            | "selection.installation_metadata_conflict"
            | "records.remote_range"
            | "records.remote_object_missing"
            | "records.remote_identity"
            | "records.remote_unavailable_offline"
            | "records.remote_http"
            | "records.invalid_metadata"
            | "records.metadata_limit"
            | "records.configuration_integrity"
            | "records.build_unavailable"
            | "records.build_ambiguous"
            | "records.pinned_build_changed"
            | "records.definition_unavailable_offline"
            | "records.definition_identity"
            | "records.query_resource_budget"
    ) {
        if !error.details.is_object() {
            error.details = if error.details.is_null() {
                json!({})
            } else {
                json!({"primaryDetails":error.details})
            };
        }
        error.details["stage"] = json!("selection");
    } else if matches!(
        error.code.as_str(),
        "selection.unsupported_data_product"
            | "selection.unsupported_data_language"
            | "selection.unsupported_data_region"
            | "selection.product_mismatch"
    ) {
        // These Go errors are plain errors.New values, not queryFault sentinels.
        error.code = "command.failed".into();
        error.exit_code = 5;
    }
    error
}
