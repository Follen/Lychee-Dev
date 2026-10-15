//! Fixed remote build resolution. Catalog observations never replace an exact pin.
use super::{
    cache::{self, MetadataFetcher, Observation, ObservationTime, PublicMetadata, Store},
    catalog, invalid,
};
use crate::{definitions, fault, resource::Budget};
use lycheedev_core::{
    Result, ToolkitError, client,
    control::OperationControl,
    evidence::{self, BlobRef, CaptureRef},
    selection::{self, DataPin, PinnedSet, SelectionSpec},
    storage,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub product: String,
    pub region: String,
    pub locale: String,
    pub full_build: String,
    pub definitions: String,
    pub parent: String,
    pub offline: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTarget {
    pub pin: PinnedSet,
    pub observed_at: String,
    pub offline: bool,
    pub capture: CaptureRef,
}
fn selection_error(code: &str) -> ToolkitError {
    fault(code, 4, code)
}
pub(crate) fn product(value: &str) -> Result<(&'static str, &'static str)> {
    let profile = client::product(value)
        .ok_or_else(|| selection_error("selection.unsupported_data_product"))?;
    Ok((
        if value == "forever" {
            "wow_forever"
        } else {
            profile.catalog_product
        },
        profile.catalog_product,
    ))
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        product(&self.product)?;
        if ![
            "enUS", "koKR", "frFR", "deDE", "zhCN", "esES", "zhTW", "enGB", "esMX", "ruRU", "ptBR",
            "itIT", "ptPT",
        ]
        .contains(&self.locale.as_str())
        {
            return Err(selection_error("selection.unsupported_data_language"));
        }
        if !["us", "eu", "cn", "kr", "tw"].contains(&self.region.as_str()) {
            return Err(selection_error("selection.unsupported_data_region"));
        }
        if !self.full_build.is_empty() && catalog::build_components(&self.full_build).is_none() {
            return Err(invalid());
        }
        definitions::normalized_reference(&self.definitions)?;
        Ok(())
    }
}
pub(crate) fn release_product(product: &str, build: &str) -> Result<()> {
    let profile = client::product(product)
        .ok_or_else(|| selection_error("selection.unsupported_data_product"))?;
    if profile.reused_slot
        && (catalog::build_components(build).is_none()
            || !build.starts_with(&format!("{}.", profile.series)))
    {
        return Err(fault(
            "selection.data_product_build_mismatch",
            4,
            format!(
                "selection.data_product_build_mismatch: {product} slot {} serves {build}; verified series is {}",
                profile.catalog_product, profile.series
            ),
        ));
    }
    Ok(())
}
fn scoped_control(control: &OperationControl, budget: Option<&Budget>) -> Result<OperationControl> {
    control.check()?;
    if let Some(budget) = budget {
        budget.check()?;
    }
    let deadline = control
        .deadline()
        .min(Instant::now() + Duration::from_secs(120));
    cache::child_control(
        control,
        budget.map_or(deadline, |budget| deadline.min(budget.deadline)),
    )
}

trait DefinitionResolver {
    fn resolve(
        &self,
        home: &Path,
        reference: &str,
        offline: bool,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<String>;
}
struct PublicDefinitions;
impl DefinitionResolver for PublicDefinitions {
    fn resolve(
        &self,
        home: &Path,
        reference: &str,
        offline: bool,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<String> {
        definitions::resolve_revision_controlled(home, reference, offline, control, budget)
    }
}
pub fn resolve(
    home: &Path,
    request: &Request,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<RemoteTarget> {
    resolve_with(
        home,
        request,
        control,
        budget,
        &PublicMetadata,
        &PublicDefinitions,
    )
}
fn resolve_with(
    home: &Path,
    request: &Request,
    control: &OperationControl,
    budget: Option<&Budget>,
    fetch: &dyn MetadataFetcher,
    definitions: &dyn DefinitionResolver,
) -> Result<RemoteTarget> {
    request.validate()?;
    let control = scoped_control(control, budget)?;
    let store = Store {
        home,
        control: &control,
        budget,
    };
    store.check()?;
    let (product, slot) = product(&request.product)?;
    let mut expected_build = request.full_build.clone();
    let mut reference = request.definitions.clone();
    if !request.parent.is_empty() {
        let parent = selection::load_pinned_controlled(home, &request.parent, &control)?;
        if parent
            .source
            .as_ref()
            .is_some_and(|p| !selection::source_supports_product(p, &request.product))
            || parent
                .data
                .as_ref()
                .is_some_and(|p| p.product != request.product)
            || parent
                .changes
                .as_ref()
                .is_some_and(|p| p.product != request.product)
        {
            return Err(selection_error("selection.product_mismatch"));
        }
        if let Some(data) = &parent.data {
            if reference.is_empty() {
                reference = data.definition_commit.clone();
            }
            if expected_build.is_empty() {
                expected_build = data.full_build.clone();
            }
        }
    }
    let remembered = store.observation(product, &request.region)?;
    let generation = remembered.as_ref().map(|(_, generation)| *generation);
    let versions_url = cache::catalog_url(&request.region, slot, "versions")?;
    let distribution_url = cache::catalog_url(&request.region, slot, "cdns")?;
    let (versions, distribution, observed_at) = if request.offline {
        let (observation, _) = remembered.as_ref().ok_or_else(cache::offline)?;
        (
            store.read_blob(&observation.versions, catalog::MAX_BYTES)?,
            store.read_blob(&observation.distribution, catalog::MAX_BYTES)?,
            observation.observed_at.clone(),
        )
    } else {
        let versions = fetch.fetch(&versions_url, catalog::MAX_BYTES, &control, budget)?;
        let distribution = fetch.fetch(&distribution_url, catalog::MAX_BYTES, &control, budget)?;
        (
            versions,
            distribution,
            ObservationTime::parse(&storage::utc_now()).ok_or_else(cache::identity)?,
        )
    };
    store.check()?;
    let release = catalog::release(&versions, &request.region, &expected_build)?;
    release_product(&request.product, &release.full_build)?;
    let route = catalog::distribution(&distribution, &request.region)?;
    let (build, build_ref) =
        store.configuration(&route, &release.build_config, request.offline, fetch)?;
    let (cdn, cdn_ref) =
        store.configuration(&route, &release.cdn_config, request.offline, fetch)?;
    cache::complete_build(slot, &release, build, cdn)?;
    let commit = definitions.resolve(home, &reference, request.offline, &control, budget)?;
    let pin = selection::pin_selection_controlled(
        home,
        SelectionSpec {
            parent: request.parent.clone(),
            data: Some(DataPin {
                product: request.product.clone(),
                region: request.region.clone(),
                language: request.locale.clone(),
                full_build: release.full_build.clone(),
                build_config: release.build_config,
                cdn_config: release.cdn_config,
                definition_commit: commit,
            }),
            ..Default::default()
        },
        &control,
    )?;
    let observation = if request.offline {
        let (mut observation, _) = remembered.ok_or_else(cache::offline)?;
        observation.observed_at = observed_at.clone();
        observation
    } else {
        let observation = Observation {
            product_code: product.into(),
            region: request.region.clone(),
            observed_at: observed_at.clone(),
            versions: store
                .publish_blob(&versions, catalog::MAX_BYTES)?
                .try_into()?,
            distribution: store
                .publish_blob(&distribution, catalog::MAX_BYTES)?
                .try_into()?,
        };
        store.remember(&observation, generation)?;
        observation
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Capture<'a> {
        pin: &'a PinnedSet,
        observation: &'a Observation,
        #[serde(rename = "versionURL")]
        version_url: &'a str,
        #[serde(rename = "distributionURL")]
        distribution_url: &'a str,
        route: &'a catalog::Distribution,
        configurations: [BlobRef; 2],
        offline: bool,
    }
    let raw = storage::identity_json(&Capture {
        pin: &pin,
        observation: &observation,
        version_url: &versions_url,
        distribution_url: &distribution_url,
        route: &route,
        configurations: [build_ref, cdn_ref],
        offline: request.offline,
    })?;
    store.check()?;
    let capture = evidence::commit_capture_bounded(
        home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: 65536,
            media_type: "application/json",
            complete: true,
            truncated: false,
            provenance: serde_json::json!({"kind":"remote-target","locator":versions_url,"snapshot":pin.id,"dataBuild":release.full_build}),
        },
        &control,
    )?;
    let capture = serde_json::from_value(capture)?;
    store.check()?;
    Ok(RemoteTarget {
        pin,
        observed_at: observed_at.canonical().ok_or_else(cache::identity)?.into(),
        offline: request.offline,
        capture,
    })
}
#[cfg(test)]
mod tests;
