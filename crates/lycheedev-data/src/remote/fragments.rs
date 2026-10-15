//! Original CDN range bytes, their immutable cache identity and one route refresh.
//! A fragment authenticates only its stored bytes. EKey/page/chunk/CKey verification
//! belongs to the encoded/index/decoded consumer, never to this cache entry.
use super::{
    availability_failure,
    cache::{
        self, CachedBlob, MetadataFetcher, PublicMetadata, Route, Store, pure_expected,
        wire::{Object, ObjectSeed, folded, scalar},
    },
    catalog, invalid, limit, target,
    transport::{self, RangeReply, RangeRequest},
};
use crate::{
    fault,
    installed::BuildMetadata,
    resource::{Budget, RetainedReservation},
};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence::GoSignedInteger, selection::DataPin,
    storage,
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};
use std::{path::Path, sync::Arc};

pub(crate) trait RangeFetcher: Sync {
    fn fetch(
        &self,
        locator: &str,
        request: RangeRequest,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<RangeReply>;
}
pub(crate) struct PublicRanges;
impl RangeFetcher for PublicRanges {
    fn fetch(
        &self,
        locator: &str,
        request: RangeRequest,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<RangeReply> {
        transport::fetch_range(locator, request, control, budget)
    }
}
fn range() -> ToolkitError {
    fault("records.remote_range", 4, "records.remote_range")
}
fn missing() -> ToolkitError {
    fault(
        "records.remote_object_missing",
        3,
        "records.remote_object_missing",
    )
}
fn retryable(error: &ToolkitError) -> bool {
    // A lease-close failure must not turn into a successful refresh/retry just
    // because the primary body error remains an availability diagnostic.
    availability_failure(error) && error.details.get("cleanup").is_none()
}
fn context(mut error: ToolkitError, object: &str) -> ToolkitError {
    error.message = format!("CDN object {object}: {}", error.message);
    error
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedRange {
    object_bytes: i64,
    blob: CachedBlob,
}
impl Object for SavedRange {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "objectbytes" => {
                    let mut n = GoSignedInteger(self.object_bytes);
                    scalar(&mut map, &mut n)?;
                    self.object_bytes = n.0;
                }
                "blob" => map.next_value_seed(ObjectSeed(&mut self.blob))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for SavedRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}

// The retained reservation travels with the bytes and is refunded on Drop.
// No bare Vec conversion detaches the allocation from its safety owner.
pub(crate) struct Fragment<'a> {
    reply: RangeReply,
    _allocation: Option<RetainedReservation<'a>>,
}
impl Fragment<'_> {
    pub fn bytes(&self) -> &[u8] {
        &self.reply.bytes
    }
    pub fn object_bytes(&self) -> i64 {
        self.reply.object_bytes
    }
}
impl std::fmt::Debug for Fragment<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fragment")
            .field("reply", &self.reply)
            .finish_non_exhaustive()
    }
}

pub(crate) fn data_identity(pin: &DataPin) -> Result<(&'static str, &'static str)> {
    // Fixed DataIdentity validates the exact pin before product/locale lookup.
    if pin.product.is_empty()
        || pin.region.is_empty()
        || pin.language.is_empty()
        || catalog::build_components(&pin.full_build).is_none()
        || !catalog::valid_key(&pin.build_config)
        || !catalog::valid_key(&pin.cdn_config)
        || !lycheedev_core::selection::valid_digest(&pin.definition_commit, 40)
    {
        return Err(fault(
            "selection.unresolved_data",
            4,
            "selection.unresolved_data",
        ));
    }
    target::release_product(&pin.product, &pin.full_build)?;
    let identity = target::product(&pin.product)?;
    target::Request {
        product: pin.product.clone(),
        region: pin.region.clone(),
        locale: pin.language.clone(),
        full_build: pin.full_build.clone(),
        definitions: pin.definition_commit.clone(),
        ..Default::default()
    }
    .validate()?;
    Ok(identity)
}

pub(crate) struct Fragments<'a> {
    pub store: Store<'a>,
    pub route: Route,
    pub build: Arc<BuildMetadata>,
    pub offline: bool,
    pub requests: u64,
    pub transferred: i64,
    pub fragment_loads: u64,
    metadata: &'a (dyn MetadataFetcher + Sync),
    ranges: &'a dyn RangeFetcher,
}
impl<'a> Fragments<'a> {
    /// Archive workers share immutable route/build data and the original
    /// atomic Budget; refreshed route values and counters remain worker-local.
    pub(crate) fn fork_for_lookup(&self) -> Self {
        Self {
            store: Store {
                home: self.store.home,
                control: self.store.control,
                budget: self.store.budget,
            },
            route: self.route.clone(),
            build: self.build.clone(),
            offline: self.offline,
            requests: self.requests,
            transferred: self.transferred,
            fragment_loads: 0,
            metadata: self.metadata,
            ranges: self.ranges,
        }
    }

    pub fn prepare(
        home: &'a Path,
        pin: &DataPin,
        offline: bool,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
    ) -> Result<(Self, Arc<BuildMetadata>)> {
        Self::prepare_with(
            home,
            pin,
            offline,
            control,
            budget,
            &PublicMetadata,
            &PublicRanges,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_with(
        home: &'a Path,
        pin: &DataPin,
        offline: bool,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
        metadata: &'a (dyn MetadataFetcher + Sync),
        ranges: &'a dyn RangeFetcher,
    ) -> Result<(Self, Arc<BuildMetadata>)> {
        // The public Go wrapper opens Metadata before c.prepare/DataIdentity.
        // Retain that admitted owner through preparation and consume-close it
        // before exposing a successful backend. Per-operation Store connections
        // use the same original control and supplemental Budget checks.
        let store = Store {
            home,
            control,
            budget,
        };
        store.check()?;
        let metadata_owner = lycheedev_core::metadata::Metadata::open_controlled_with_check(
            home,
            true,
            control,
            || store.check(),
        )?;
        let result = Self::prepare_opened(home, pin, offline, control, budget, metadata, ranges);
        store.finish(result, metadata_owner.close())
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare_opened(
        home: &'a Path,
        pin: &DataPin,
        offline: bool,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
        metadata: &'a (dyn MetadataFetcher + Sync),
        ranges: &'a dyn RangeFetcher,
    ) -> Result<(Self, Arc<BuildMetadata>)> {
        let (product, slot) = data_identity(pin)?;
        let store = Store {
            home,
            control,
            budget,
        };
        store.check()?;
        let mut route = Route::prepare(&store, product, &pin.region, slot, offline, metadata)?;
        let release = catalog::Release {
            full_build: pin.full_build.clone(),
            build_config: pin.build_config.clone(),
            cdn_config: pin.cdn_config.clone(),
        };
        let load = |route: &Route| {
            let (build, _) =
                store.configuration(&route.current, &pin.build_config, offline, metadata)?;
            let (cdn, _) =
                store.configuration(&route.current, &pin.cdn_config, offline, metadata)?;
            cache::complete_build(slot, &release, build, cdn)
        };
        let loaded = match load(&route) {
            Err(error) if retryable(&error) => {
                if route.refresh(&store, metadata)? {
                    load(&route)
                } else {
                    Err(error)
                }
            }
            result => result,
        }?;
        let loaded = Arc::new(loaded);
        store.check()?;
        Ok((
            Self {
                store,
                route,
                build: loaded.clone(),
                offline,
                requests: 0,
                transferred: 0,
                fragment_loads: 0,
                metadata,
                ranges,
            },
            loaded,
        ))
    }

    pub fn range(&mut self, object: &str, request: RangeRequest) -> Result<Fragment<'a>> {
        self.fragment_loads = self.fragment_loads.checked_add(1).ok_or_else(limit)?;
        self.store.check()?;
        if !catalog::valid_key(object.strip_suffix(".index").unwrap_or(object)) {
            return Err(invalid());
        }
        if request.length < 1
            || request.length > transport::MAX_RANGE_BYTES
            || request.total > transport::MAX_OBJECT_BYTES
        {
            return Err(limit());
        }
        if request.offset < 0
            || request.total < 0
            || request.offset.checked_add(request.length).is_none()
            || request.total != 0 && request.offset + request.length > request.total
        {
            return Err(range());
        }
        let route_path = self.route.current.path.clone();
        let key = format!(
            "cdn-range/{}",
            storage::sha256(
                format!(
                    "{route_path}/{object}/{}/{}",
                    request.offset, request.length
                )
                .as_bytes()
            )
        );
        let guard = self.store.lease(&key)?;
        let result = self.range_locked(object, request, &route_path, &key);
        self.finish_range(result, guard.close())
    }
    fn finish_range(
        &self,
        result: Result<(RangeReply, Option<RetainedReservation<'a>>)>,
        cleanup: Result<()>,
    ) -> Result<Fragment<'a>> {
        // Only a buffer actually returned to the caller remains retained.
        // Failed consuming close or post-close stop drops the refundable owner.
        let (reply, allocation) = self.store.finish(result, cleanup)?;
        Ok(Fragment {
            reply,
            _allocation: allocation,
        })
    }
    fn range_locked(
        &mut self,
        object: &str,
        request: RangeRequest,
        route_path: &str,
        key: &str,
    ) -> Result<(RangeReply, Option<RetainedReservation<'a>>)> {
        let saved = self.store.read::<SavedRange>(key).map_err(|error| {
            if pure_expected(&error, "records.remote_identity") {
                range()
            } else {
                error
            }
        })?;
        if let Some((saved, _)) = saved {
            if saved.object_bytes <= 0
                || saved.object_bytes > transport::MAX_OBJECT_BYTES
                || request.offset > saved.object_bytes
                || request.length > saved.object_bytes - request.offset
                || saved.blob.bytes != request.length
                || request.total != 0 && request.total != saved.object_bytes
            {
                return Err(range());
            }
            let allocation = self.admit(request.length)?;
            let bytes = self.store.read_blob(&saved.blob, request.length as usize)?;
            return Ok((
                RangeReply {
                    bytes,
                    object_bytes: saved.object_bytes,
                },
                allocation,
            ));
        }
        if self.offline {
            return Err(cache::offline());
        }
        loop {
            let mut last = missing();
            for host in &self.route.current.hosts {
                self.store.check()?;
                if self.requests >= 4096
                    || request.length
                        > (1i64 << 30)
                            .checked_sub(self.transferred)
                            .ok_or_else(limit)?
                {
                    return Err(limit());
                }
                self.requests += 1;
                self.transferred = self
                    .transferred
                    .checked_add(request.length)
                    .ok_or_else(limit)?;
                let locator = self.route.current.data_url(host, object)?;
                let allocation = self.admit(request.length)?;
                let reply = match self.ranges.fetch(
                    &locator,
                    request,
                    self.store.control,
                    self.store.budget,
                ) {
                    Ok(reply) => reply,
                    Err(error) => {
                        // Keep original cancellation/budget, including any actual body-read cause.
                        let error = if let Err(mut stopped) = self.store.check() {
                            stopped.details = serde_json::json!({"primaryDetails":stopped.details,"attemptError":error});
                            stopped
                        } else {
                            error
                        };
                        if !retryable(&error) {
                            return Err(context(error, object));
                        }
                        if error.code != "records.remote_object_missing"
                            || last.code == "records.remote_object_missing"
                        {
                            last = error;
                        }
                        continue;
                    }
                };
                self.store.check()?;
                if reply.bytes.len() as i64 != request.length
                    || reply.object_bytes <= 0
                    || reply.object_bytes > transport::MAX_OBJECT_BYTES
                    || request.offset + request.length > reply.object_bytes
                    || request.total != 0 && request.total != reply.object_bytes
                {
                    return Err(range());
                }
                // Route changes create a different fragment namespace. Do not
                // publish refreshed bytes beneath the old path's held lease.
                if self.route.current.path != route_path {
                    return Ok((reply, allocation));
                }
                let blob = self
                    .store
                    .publish_blob(&reply.bytes, request.length as usize)?
                    .try_into()?;
                self.store.put(
                    key,
                    &SavedRange {
                        object_bytes: reply.object_bytes,
                        blob,
                    },
                    None,
                )?;
                return Ok((reply, allocation));
            }
            if !self.route.refresh(&self.store, self.metadata)? {
                return Err(context(last, object));
            }
        }
    }
    fn admit(&self, bytes: i64) -> Result<Option<RetainedReservation<'a>>> {
        self.store
            .budget
            .map(|budget| budget.reserve_retained(bytes as usize))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
