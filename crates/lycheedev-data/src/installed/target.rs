//! Default local target preparation, in fixed records/target.go order. This
//! owns a Budget bound to the original invocation, so the existing installation
//! readers' per-IO Budget checks retain that invocation without a raw token.
use super::*;
use crate::{definitions, remote::cache::Store};
use lycheedev_core::{control::OperationControl, evidence, metadata::Metadata, selection};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default)]
pub struct TargetRequest {
    pub installation: PathBuf,
    pub product: String,
    pub full_build: String,
    pub region: String,
    pub locale: String,
    pub definitions: String,
    pub parent: String,
    pub offline: bool,
}
impl TargetRequest {
    pub fn validate(&self) -> Result<()> {
        if self.installation.as_os_str().is_empty() {
            return Err(fault(
                "records.target_installation_required",
                2,
                "records.target_installation_required",
            ));
        }
        if !self.product.is_empty() {
            return crate::remote::target::Request {
                product: self.product.clone(),
                region: self.region.clone(),
                locale: self.locale.clone(),
                full_build: self.full_build.clone(),
                ..Default::default()
            }
            .validate();
        }
        if !self.full_build.is_empty() {
            return Err(fault(
                "records.target_product_required_for_build",
                2,
                "records.target_product_required_for_build",
            ));
        }
        selection::target::validate_locale(&self.region, &self.locale)
    }
}
#[derive(Debug)]
pub struct LocalTarget {
    pub pin: selection::PinnedSet,
    pub client: Value,
    pub installation: String,
    pub capture: evidence::CaptureRef,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicClient<'a> {
    directory: &'a str,
    product: &'a str,
    product_code: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    catalog_product: &'a str,
    full_build: &'a str,
    interface: u32,
    toc: &'a str,
    identity_source: &'a str,
}
impl<'a> From<&'a ClientInstallation> for PublicClient<'a> {
    fn from(c: &'a ClientInstallation) -> Self {
        Self {
            directory: &c.directory,
            product: &c.product,
            product_code: &c.product_code,
            catalog_product: &c.catalog_product,
            full_build: &c.full_build,
            interface: c.interface,
            toc: &c.toc,
            identity_source: &c.identity_source,
        }
    }
}
/// No caller-supplied independent control is substituted for installation
/// checks: this operation's single Budget contains `control` from creation.
pub fn prepare_target(
    home: &Path,
    request: &TargetRequest,
    control: &OperationControl,
) -> Result<LocalTarget> {
    request.validate()?;
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let store = Store {
        home,
        control,
        budget: Some(&budget),
    };
    let metadata = Metadata::open_controlled_with_check(home, true, control, || budget.check())?;
    let result = (|| {
        store.check()?;
        let client =
            select_client_budgeted(&request.installation, &request.product, Some(&budget))?;
        if !request.full_build.is_empty() && request.full_build != client.full_build {
            return Err(fault(
                "selection.target_build_unavailable",
                3,
                "selection.target_build_unavailable",
            ));
        }
        let data_product = client::client_data_product(&client);
        let installation = Path::new(&client.directory).parent().ok_or_else(invalid)?;
        let build = resolve_local_budgeted(
            installation,
            observed_slot(&client),
            &client.full_build,
            Some(&budget),
        )?;
        let mut definitions = request.definitions.clone();
        if !request.parent.is_empty() {
            let parent = selection::load_pinned_controlled_with_check(
                home,
                &request.parent,
                control,
                || budget.check(),
            )?;
            if parent
                .source
                .as_ref()
                .is_some_and(|p| !selection::source_supports_product(p, data_product))
                || parent
                    .data
                    .as_ref()
                    .is_some_and(|p| p.product != data_product)
                || parent
                    .changes
                    .as_ref()
                    .is_some_and(|p| p.product != data_product)
            {
                return Err(fault(
                    "selection.product_mismatch",
                    4,
                    "selection.product_mismatch",
                ));
            }
            if definitions.is_empty()
                && let Some(data) = &parent.data
            {
                definitions = data.definition_commit.clone();
            }
        }
        let commit = definitions::resolve_revision_controlled(
            home,
            &definitions,
            request.offline,
            control,
            Some(&budget),
        )?;
        // Both observations surround definition resolution, which may download.
        let current =
            select_client_budgeted(&request.installation, &request.product, Some(&budget))?;
        if !same_client_identity(&current, &client) {
            return Err(pinned_changed(
                "Selected client changed during target preparation",
            ));
        }
        let after = resolve_local_budgeted(
            installation,
            observed_slot(&client),
            &client.full_build,
            Some(&budget),
        )?;
        if after.installed.public() != build.installed.public() {
            return Err(pinned_changed(
                "Active catalog identity changed during target preparation",
            ));
        }
        drop(build);
        let pin = selection::pin_selection_controlled_with_check(
            home,
            selection::SelectionSpec {
                parent: request.parent.clone(),
                data: Some(selection::DataPin {
                    product: data_product.into(),
                    region: request.region.clone(),
                    language: request.locale.clone(),
                    full_build: client.full_build.clone(),
                    build_config: after.installed.build_config.clone(),
                    cdn_config: after.installed.cdn_config.clone(),
                    definition_commit: commit,
                }),
                ..Default::default()
            },
            control,
            || budget.check(),
        )?;
        let mut configs = Vec::<evidence::BlobRef>::with_capacity(2);
        for document in [&after.build_document, &after.cdn_document] {
            let reference = lycheedev_core::workspace::data_disk::publish_shared_blob_with_check(
                home,
                &document.raw,
                4 << 20,
                control,
                &|| budget.check(),
            )?;
            if reference.sha256 != document.sha256 {
                return Err(fault(
                    "records.configuration_integrity",
                    4,
                    "records.configuration_integrity",
                ));
            }
            configs.push(reference);
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body<'a> {
            pin: &'a selection::PinnedSet,
            client: PublicClient<'a>,
            installation: &'a str,
            #[serde(rename = "catalogSHA256")]
            catalog_sha256: &'a str,
            configurations: &'a [evidence::BlobRef],
        }
        let directory = client::display_path(installation);
        let raw = storage::identity_json(&Body {
            pin: &pin,
            client: PublicClient::from(&client),
            installation: &directory,
            catalog_sha256: &after.catalog_sha256,
            configurations: &configs,
        })?;
        let capture = evidence::commit_data_capture_bounded_with_check(
            home,
            evidence::CaptureInput {
                bytes: &raw,
                maximum_bytes: 65536,
                media_type: "application/json",
                complete: true,
                truncated: false,
                provenance: json!({"kind":"local-target","locator":client.directory,"snapshot":pin.id,"dataBuild":client.full_build}),
            },
            control,
            || budget.check(),
        )?;
        Ok(LocalTarget {
            pin,
            client: serde_json::to_value(PublicClient::from(&client))?,
            installation: directory,
            capture: serde_json::from_value(capture)?,
        })
    })();
    store.finish(result, metadata.close())
}

#[cfg(test)]
mod tests;
