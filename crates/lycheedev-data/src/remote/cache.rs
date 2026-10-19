// SPDX-License-Identifier: MIT
//! Content-addressed metadata and paired catalog observations. A corrupt hit
//! never authorizes a download, and routing never rewrites build identities.
use super::{
    availability_failure,
    catalog::{self, Distribution},
    invalid, transport,
};
use crate::{
    fault,
    installed::{self, BuildMetadata, ConfigDocument, InstalledBuild},
    resource::Budget,
};
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    evidence::{self, BlobRef},
    metadata::Metadata,
    storage::{self, ExclusiveLock},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

pub(crate) mod wire;
pub(crate) use wire::{CachedBlob, Observation, ObservationTime};

pub(crate) trait MetadataFetcher {
    fn fetch(
        &self,
        locator: &str,
        maximum: usize,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>>;
}
pub(crate) struct PublicMetadata;
impl MetadataFetcher for PublicMetadata {
    fn fetch(
        &self,
        locator: &str,
        maximum: usize,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        transport::fetch_metadata(locator, maximum, control, budget)
    }
}
pub(crate) fn identity() -> ToolkitError {
    fault("records.remote_identity", 4, "records.remote_identity")
}
pub(crate) fn offline() -> ToolkitError {
    fault(
        "records.remote_unavailable_offline",
        3,
        "records.remote_unavailable_offline",
    )
}

impl Observation {
    fn validate_route(&self, product: &str, region: &str) -> Result<()> {
        if self.product_code != product || self.region != region {
            return Err(identity());
        }
        Ok(())
    }
    fn validate(&self, product: &str, region: &str) -> Result<()> {
        self.validate_route(product, region)?;
        if self.observed_at.is_zero() {
            return Err(identity());
        }
        // Old catalog references are checked only when actually read offline.
        Ok(())
    }
}
pub(crate) fn pure_expected(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
pub(crate) fn child_control(
    control: &OperationControl,
    deadline: Instant,
) -> Result<OperationControl> {
    control.check()?;
    match control.ordered_child(deadline) {
        Ok(child) => Ok(child),
        Err(error) if pure_expected(&error, "control.cancellation_order_unavailable") => {
            Ok(control.child(deadline, Arc::new(AtomicBool::new(false))))
        }
        Err(error) => Err(error),
    }
}
// Same consuming-owner pattern as Core storage::finish_file: no close retry,
// full typed causes, and stop checked after cleanup has actually completed.
fn finish<T>(
    result: Result<T>,
    cleanup: Result<()>,
    control: Result<()>,
    budget: Result<()>,
) -> Result<T> {
    let (value, primary) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let mut causes = [primary, cleanup.err(), control.err(), budget.err()];
    for i in 1..causes.len() {
        if causes[..i].iter().flatten().any(|earlier| {
            causes[i].as_ref().is_some_and(|e| {
                earlier.code == e.code
                    && earlier.message == e.message
                    && earlier.exit_code == e.exit_code
                    && earlier.details == e.details
            })
        }) {
            causes[i] = None;
        }
    }
    let Some(mut first) = causes.iter().flatten().next().cloned() else {
        return Ok(value.expect("successful operation retained its value"));
    };
    if causes.iter().flatten().count() > 1 {
        first.details = serde_json::json!({"schema":"lycheedev.remote-cache-cleanup.v1", "primary":causes[0], "cleanup":causes[1], "control":causes[2], "budget":causes[3]});
    }
    Err(first)
}

pub(crate) struct Store<'a> {
    pub home: &'a Path,
    pub control: &'a OperationControl,
    pub budget: Option<&'a Budget>,
}
impl Store<'_> {
    pub fn check(&self) -> Result<()> {
        self.control.check()?;
        if let Some(budget) = self.budget {
            budget.check()?;
        }
        Ok(())
    }
    pub(crate) fn finish<T>(&self, result: Result<T>, cleanup: Result<()>) -> Result<T> {
        finish(
            result,
            cleanup,
            self.control.check(),
            self.budget.map_or(Ok(()), Budget::check),
        )
    }
    fn controlled<T>(&self, run: impl FnOnce(&OperationControl) -> Result<T>) -> Result<T> {
        self.check()?;
        // Budget is an independent stop cause, not an invocation deadline.
        // Core checks this original control before the supplemental callback.
        let result = run(self.control);
        self.finish(result, Ok(()))
    }
    // Refresh CAS needs only document identity/generation. Go does not decode
    // or revalidate the old alias value here, unlike initial route preparation.
    pub(crate) fn generation(&self, key: &str) -> Result<Option<i64>> {
        self.controlled(|control| {
            match Metadata::read_document_raw_controlled_with_check(self.home, key, control, || {
                self.check()
            }) {
                Ok(document) => Ok(Some(document.generation)),
                Err(error) if pure_expected(&error, "vault.record_missing") => Ok(None),
                Err(error) => Err(error),
            }
        })
    }
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<(T, i64)>> {
        self.controlled(|control| {
            let raw = match Metadata::read_document_raw_controlled_with_check(
                self.home,
                key,
                control,
                || self.check(),
            ) {
                Ok(document) => document,
                Err(error) if pure_expected(&error, "vault.record_missing") => return Ok(None),
                Err(error) => return Err(error),
            };
            self.check()?;
            // The normalization view never replaces original metadata bytes.
            let scratch = raw.bytes.len().checked_mul(3).ok_or_else(identity)?;
            let _reserved = self
                .budget
                .map(|b| b.reserve_retained(scratch))
                .transpose()?;
            // One bounded (<=1 MiB) Core Go-grammar admission pass over the
            // original bytes, including ignored fields and max depth 10000.
            // This CPU-only scanner has no internal callback; check both scopes
            // before/after rather than claiming mid-scan independent cancellation.
            self.check()?;
            let valid = lycheedev_core::raw_json::is_valid(&raw.bytes);
            self.check()?;
            if !valid {
                return Err(identity());
            }
            let normalized =
                evidence::normalize_go_json_strings(&raw.bytes, control).map_err(|error| {
                    if matches!(
                        error.code.as_str(),
                        "command.cancelled" | "command.deadline"
                    ) {
                        error
                    } else {
                        identity()
                    }
                })?;
            // Core's <=1 MiB string normalizer has only invocation callbacks.
            // Recheck the independent scope before starting deserialization.
            self.check()?;
            let value = serde_json::from_slice(&normalized).map_err(|_| identity())?;
            Ok(Some((value, raw.generation)))
        })
    }
    /// None is a strict absent-document CAS, not an unconditional replace.
    pub fn put<T: Serialize>(&self, key: &str, value: &T, generation: Option<i64>) -> Result<()> {
        self.controlled(|control| {
            let mut metadata =
                Metadata::open_controlled_with_check(self.home, true, control, || self.check())?;
            let result = if let Some(generation) = generation {
                metadata
                    .put_controlled_with_check(key, value, Some(generation), control, || {
                        self.check()
                    })
                    .map(|_| ())
            } else {
                metadata
                    .create_controlled_with_check(key, value, control, || self.check())
                    .map(|_| ())
            };
            self.finish(result, metadata.close())
        })
    }
    pub fn read_blob<T: Serialize>(&self, reference: &T, maximum: usize) -> Result<Vec<u8>> {
        self.controlled(|control| {
            evidence::read_blob_bounded_with_check(
                self.home,
                &serde_json::to_value(reference)?,
                maximum,
                control,
                || self.check(),
            )
        })
    }
    pub fn read_blob_optional<T: Serialize>(
        &self,
        reference: &T,
        maximum: usize,
    ) -> Result<Option<Vec<u8>>> {
        self.controlled(|control| {
            evidence::read_blob_bounded_optional_with_check(
                self.home,
                &serde_json::to_value(reference)?,
                maximum,
                control,
                || self.check(),
            )
        })
    }
    pub fn publish_blob(&self, bytes: &[u8], maximum: usize) -> Result<BlobRef> {
        self.controlled(|control| {
            let reference = lycheedev_core::workspace::data_disk::publish_shared_blob_with_check(
                self.home,
                bytes,
                maximum,
                control,
                &|| self.check(),
            )?;
            Ok(reference)
        })
    }
    pub fn lease(&self, key: &str) -> Result<ExclusiveLock> {
        loop {
            self.check()?;
            match ExclusiveLock::resource_controlled(&self.home.join("locks"), key, self.control) {
                Ok(guard) => {
                    return match self.check() {
                        Ok(()) => Ok(guard),
                        Err(error) => self.finish(Err(error), guard.close()),
                    };
                }
                Err(error) if pure_expected(&error, "workspace.busy") => {
                    self.check()?;
                    let mut pause = self.control.remaining()?.min(Duration::from_millis(25));
                    if let Some(budget) = self.budget {
                        // Shorten only the wait, never the original stop scope.
                        pause =
                            pause.min(budget.deadline.saturating_duration_since(Instant::now()));
                    }
                    std::thread::sleep(pause);
                }
                Err(error) => return self.finish(Err(error), Ok(())),
            }
        }
    }
    pub fn observation(&self, product: &str, region: &str) -> Result<Option<(Observation, i64)>> {
        let value = self.read::<Observation>(&format!("remote-catalog/{product}/{region}"))?;
        if let Some((observed, _)) = &value {
            observed.validate(product, region)?;
        }
        Ok(value)
    }
    /// Both catalog bytes are published before a single compare-and-swap.
    /// A losing refresh retains its own exact observation for its pin/capture.
    pub fn remember(&self, observed: &Observation, generation: Option<i64>) -> Result<()> {
        observed.validate(&observed.product_code, &observed.region)?;
        match self.put(
            &format!(
                "remote-catalog/{}/{}",
                observed.product_code, observed.region
            ),
            observed,
            generation,
        ) {
            Err(error) if pure_expected(&error, "vault.generation_conflict") => Ok(()),
            result => result,
        }
    }
    pub fn configuration(
        &self,
        route: &Distribution,
        key: &str,
        is_offline: bool,
        fetch: &dyn MetadataFetcher,
    ) -> Result<(ConfigDocument, BlobRef)> {
        self.check()?;
        if !catalog::valid_key(key) {
            return Err(invalid());
        }
        route.validate()?;
        let cache_key = format!("remote-config/{key}");
        let guard = self.lease(&cache_key)?;
        let result = (|| {
            if let Some((reference, _)) = self.read::<CachedBlob>(&cache_key)? {
                let raw = self.read_blob(&reference, 4 << 20)?;
                return Ok((
                    configuration(raw, key, self.budget)?,
                    reference.into_core()?,
                ));
            }
            if is_offline {
                return Err(offline());
            }
            let mut last = fault("records.remote_http", 5, "records.remote_http");
            for host in &route.hosts {
                self.check()?;
                let raw = match fetch.fetch(
                    &route.config_url(host, key)?,
                    4 << 20,
                    self.control,
                    self.budget,
                ) {
                    Ok(raw) => raw,
                    Err(error) if availability_failure(&error) => {
                        self.check()?;
                        last = error;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let parsed = configuration(raw, key, self.budget)?;
                let reference = self.publish_blob(&parsed.raw, 4 << 20)?;
                self.put(&cache_key, &reference, None)?;
                return Ok((parsed, reference));
            }
            Err(last)
        })();
        self.finish(result, guard.close())
    }
}
fn configuration(raw: Vec<u8>, key: &str, budget: Option<&Budget>) -> Result<ConfigDocument> {
    installed::parse_configuration_budgeted(raw, key, budget).map_err(|mut error| {
        // The shared local parser predates the fixed Go diagnostic spelling.
        if error.code == "records.metadata_invalid" {
            error.code = "records.invalid_metadata".into();
        }
        error
    })
}

pub(crate) fn complete_build(
    slot: &str,
    release: &catalog::Release,
    build: ConfigDocument,
    cdn: ConfigDocument,
) -> Result<BuildMetadata> {
    if build.key != release.build_config || cdn.key != release.cdn_config {
        return Err(identity());
    }
    let root = build.fields.get("root").ok_or_else(invalid)?;
    let encoding = build.fields.get("encoding").ok_or_else(invalid)?;
    let sizes = build.fields.get("encoding-size").ok_or_else(invalid)?;
    if root.len() != 1
        || encoding.len() != 2
        || sizes.len() != 2
        || !root
            .iter()
            .chain(encoding)
            .all(|key| catalog::valid_key(key))
    {
        return Err(invalid());
    }
    if build
        .fields
        .get("build-uid")
        .is_some_and(|uid| uid.as_slice() != [slot])
    {
        return Err(invalid());
    }
    let size = |value: &str| -> Result<usize> {
        if !catalog::decimal(value) {
            return Err(invalid());
        }
        let parsed = value.parse::<u64>().map_err(|_| invalid())?;
        if parsed == 0 || parsed > 1 << 50 {
            return Err(invalid());
        }
        usize::try_from(parsed).map_err(|_| invalid())
    };
    Ok(BuildMetadata {
        installed: InstalledBuild {
            product: slot.into(),
            full_build: release.full_build.clone(),
            build_config: release.build_config.clone(),
            cdn_config: release.cdn_config.clone(),
            active: false,
            metadata_branch: String::new(),
            channel: String::new(),
            display_name: String::new(),
        },
        catalog_sha256: String::new(),
        root_content_key: root[0].clone(),
        encoding_content_key: encoding[0].clone(),
        encoding_key: encoding[1].clone(),
        content_bytes: size(&sizes[0])?,
        encoding_bytes: size(&sizes[1])?,
        build_document: build,
        cdn_document: cdn,
    })
}

/// One route refresh per prepared reader. Only cdns is fetched; versions and
/// immutable config keys cannot change through this type.
#[derive(Clone)]
pub(crate) struct Route {
    pub current: Arc<Distribution>,
    key: Arc<str>,
    region: Arc<str>,
    locator: Arc<str>,
    offline: bool,
    refreshed: bool,
}
impl Route {
    pub fn prepare(
        store: &Store<'_>,
        product: &str,
        region: &str,
        slot: &str,
        is_offline: bool,
        fetch: &dyn MetadataFetcher,
    ) -> Result<Self> {
        let locator = catalog_url(region, slot, "cdns")?;
        let key = format!("cdn-route/{product}/{region}");
        let mut route = Self {
            current: Arc::new(Distribution {
                path: String::new(),
                hosts: Vec::new(),
            }),
            key: key.into(),
            region: region.into(),
            locator: locator.into(),
            offline: is_offline,
            refreshed: false,
        };
        let raw = if let Some((reference, _)) = store.read::<CachedBlob>(&route.key)? {
            store.read_blob(&reference, catalog::MAX_BYTES)?
        } else if let Some((observation, _)) =
            store.read::<Observation>(&format!("remote-catalog/{product}/{region}"))?
        {
            observation.validate_route(product, region)?;
            store.read_blob(&observation.distribution, catalog::MAX_BYTES)?
        } else if is_offline {
            return Err(offline());
        } else {
            route.fetch(store, fetch)?
        };
        route.current = Arc::new(catalog::distribution(&raw, region)?);
        Ok(route)
    }
    fn fetch(&self, store: &Store<'_>, fetch: &dyn MetadataFetcher) -> Result<Vec<u8>> {
        let raw = fetch.fetch(
            &self.locator,
            catalog::MAX_BYTES,
            store.control,
            store.budget,
        )?;
        catalog::distribution(&raw, &self.region)?;
        let generation = store.generation(&self.key)?;
        let reference = store.publish_blob(&raw, catalog::MAX_BYTES)?;
        match store.put(&self.key, &reference, generation) {
            Err(error) if pure_expected(&error, "vault.generation_conflict") => (),
            result => result?,
        }
        Ok(raw)
    }
    pub fn refresh(&mut self, store: &Store<'_>, fetch: &dyn MetadataFetcher) -> Result<bool> {
        if self.offline || self.refreshed {
            return Ok(false);
        }
        store.check()?;
        self.refreshed = true;
        let raw = self.fetch(store, fetch)?;
        self.current = Arc::new(catalog::distribution(&raw, &self.region)?);
        Ok(true)
    }
}
pub(crate) fn catalog_url(region: &str, slot: &str, suffix: &str) -> Result<String> {
    if !lycheedev_core::availability::valid_region(region)
        || slot.is_empty()
        || slot.len() > 128
        || !slot
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || !["versions", "cdns"].contains(&suffix)
    {
        return Err(identity());
    }
    Ok(if region == "cn" {
        format!("https://cn.version.battlenet.com.cn/{slot}/{suffix}")
    } else {
        format!("https://{region}.version.battle.net/{slot}/{suffix}")
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod data_disk_tests;
