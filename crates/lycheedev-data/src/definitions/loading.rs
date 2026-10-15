//! One load owns the original invocation and an independent Data ledger from
//! metadata wait through HTTP, blob verification, validation and publication.
use super::*;
use crate::{
    remote::{
        cache::{pure_expected, wire},
        transport::{self, BodyPolicy, GetPolicy, LocatorPolicy},
    },
    resource::{Budget, Cost},
};
use lycheedev_core::ToolkitError;
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};

const MAXIMUM: usize = 4 << 20;

pub(super) fn check_definition(
    deadline: Instant,
    budget: Option<&Budget>,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if let Some(budget) = budget {
        budget.check()?;
    }
    if Instant::now() >= deadline {
        return Err(fault(
            "records.resource_limit",
            3,
            "Definition deadline exceeded",
        ));
    }
    Ok(())
}
// A missing row/conflict with a close or stop cause is never ordinary absence
// or retry permission. Keep each distinct typed cause after checked cleanup.
fn finish<T>(
    result: Result<T>,
    cleanup: Result<()>,
    deadline: Instant,
    budget: Option<&Budget>,
    control: &OperationControl,
) -> Result<T> {
    let (value, primary) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let mut causes = [
        primary,
        cleanup.err(),
        control.check().err(),
        budget.and_then(|b| b.check().err()),
        (Instant::now() >= deadline)
            .then(|| fault("records.resource_limit", 3, "Definition deadline exceeded")),
    ];
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
        return Ok(value.expect("successful load retained its value"));
    };
    if causes.iter().flatten().count() > 1 {
        first.details = serde_json::json!({"schema":"lycheedev.definition-cleanup.v1",
            "primary":causes[0], "cleanup":causes[1], "control":causes[2],
            "budget":causes[3], "deadline":causes[4]});
    }
    Err(first)
}
fn identity() -> ToolkitError {
    fault(
        "records.definition_identity",
        4,
        "Cached definition identity differs",
    )
}
#[derive(Default)]
struct CachedObject {
    url: String,
    blob: wire::CachedBlob,
}
impl wire::Object for CachedObject {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match wire::folded(&key).as_str() {
                "url" => wire::scalar(&mut map, &mut self.url)?,
                "blob" => map.next_value_seed(wire::ObjectSeed(&mut self.blob))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for CachedObject {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        wire::ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
fn decode_cached_object(
    raw: &[u8],
    control: &OperationControl,
    budget: Option<&Budget>,
    mut check: impl FnMut() -> Result<()>,
) -> Result<CachedObject> {
    // This <=1 MiB metadata decode has the same Go scalar/merge grammar as
    // remote cache reads. Temporary normalization is a refundable owner;
    // definition work/metadata charges below remain cumulative.
    let _reserved = budget
        .map(|b| b.reserve_retained(raw.len() * 3))
        .transpose()?;
    let valid = lycheedev_core::raw_json::is_valid(raw);
    if !valid {
        return Err(identity());
    }
    check()?;
    let normalized = evidence::normalize_go_json_strings(raw, control)?;
    check()?;
    // Preserve a completed parse failure. The load's finish observes any
    // concurrent stop and retains it as a distinct typed secondary cause.
    let object = serde_json::from_slice(&normalized).map_err(|_| identity())?;
    check()?;
    Ok(object)
}
fn read_cached_definition(
    home: &Path,
    key: &str,
    url: &str,
    deadline: Instant,
    budget: Option<&Budget>,
    control: &OperationControl,
    validate: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<(Value, Vec<u8>)> {
    let check = || check_definition(deadline, budget, control);
    let document = Metadata::read_document_raw_controlled_with_check(home, key, control, check)?;
    check()?;
    let object = decode_cached_object(&document.bytes, control, budget, check)?;
    if object.url != url {
        return Err(identity());
    }
    if !(0..=MAXIMUM as i64).contains(&object.blob.bytes) {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Definition byte budget exceeded",
        ));
    }
    let bytes = object.blob.bytes as u64;
    if let Some(budget) = budget {
        budget.charge_split(
            definition_cost(bytes),
            Cost {
                retained_bytes: bytes * 8,
                ..definition_cost(bytes)
            },
        )?;
    }
    let blob = serde_json::to_value(object.blob)?;
    let raw = evidence::read_blob_bounded_with_check(home, &blob, MAXIMUM, control, check)?;
    check()?;
    let parsed = validate(&raw);
    finish(parsed, Ok(()), deadline, budget, control)?;
    Ok((blob, raw))
}
fn http_error(error: ToolkitError) -> ToolkitError {
    if pure_expected(&error, "records.remote_http") {
        fault("records.definition_http", 5, error.message)
    } else {
        error
    }
}
fn fetch(
    url: &str,
    deadline: Instant,
    budget: Option<&Budget>,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    let request_deadline = deadline.min(transport::request_deadline(control, budget)?);
    transport::run(async {
        let response = transport::get_once(
            url,
            GetPolicy {
                locator: LocatorPolicy::HttpsOnly,
                body: BodyPolicy::GoAutomaticGzip,
                accept: None,
                referer: None,
                user_agent: Some("Go-http-client/1.1"),
                authorization: None,
            },
            control,
            budget,
            request_deadline,
        )
        .await?;
        if response.status().as_u16() != 200 {
            return Err(fault(
                "records.definition_http",
                5,
                format!("Definition HTTP status {}", response.status()),
            ));
        }
        // Go's temporary 8 MiB read buffer is not persistent metadata. Actual
        // delivered bytes are charged by transport even if cancellation wins.
        let _logical_scratch = budget
            .map(|b| b.reserve_logical_scratch(8 << 20))
            .transpose()?;
        let mut admit = |bytes: usize| {
            check_definition(deadline, budget, control)?;
            if let Some(budget) = budget {
                budget.charge_split(Cost::default(), Cost::retained(bytes * 8))?;
            }
            Ok(())
        };
        // Go reads exactly a 4 MiB + 1 prefix, without an extra EOF probe.
        let raw = transport::read_response_prefix_with_admission(
            response,
            BodyPolicy::GoAutomaticGzip,
            MAXIMUM + 1,
            control,
            budget,
            request_deadline,
            &mut admit,
        )
        .await?;
        if raw.len() > MAXIMUM {
            return Err(fault(
                "records.metadata_limit",
                3,
                "Definition byte budget exceeded",
            ));
        }
        check_definition(deadline, budget, control)?;
        if let Some(budget) = budget {
            budget.charge_split(
                definition_cost(raw.len() as u64),
                Cost {
                    retained_bytes: 0,
                    ..definition_cost(raw.len() as u64)
                },
            )?;
        }
        Ok(raw)
    })
    .map_err(http_error)
}

pub(super) fn load(
    home: &Path,
    url: &str,
    offline: bool,
    deadline: Instant,
    budget: Option<&Budget>,
    control: Option<&OperationControl>,
    mut validate: impl FnMut(&[u8]) -> Result<()>,
) -> Result<(Value, Vec<u8>)> {
    // Legacy callers still get a bounded owner. Explicit invocation callers
    // use the original control unchanged; a Budget stop is not a new deadline.
    let fallback = OperationControl::uncancelable(deadline);
    let control = control.unwrap_or(&fallback);
    let check = || check_definition(deadline, budget, control);
    let result = (|| {
        check()?;
        let key = format!("definition/{}", storage::sha256(url.as_bytes()));
        match read_cached_definition(home, &key, url, deadline, budget, control, &mut validate) {
            Ok(cached) => return Ok(cached),
            Err(error) if pure_expected(&error, "vault.record_missing") => {}
            Err(error) => return Err(error),
        }
        if offline {
            return Err(fault(
                "records.definition_unavailable_offline",
                3,
                "Exact definition resource has not been cached",
            ));
        }
        let raw = fetch(url, deadline, budget, control)?;
        check()?;
        let parsed = validate(&raw);
        finish(parsed, Ok(()), deadline, budget, control)?;
        let blob = evidence::publish_blob_bounded_with_check(home, &raw, MAXIMUM, control, check)?;
        check()?;
        let object = Object {
            url: url.to_owned(),
            blob: blob.clone(),
        };
        let mut metadata = Metadata::open_controlled_with_check(home, true, control, check)?;
        let published = metadata.create_controlled_with_check(&key, &object, control, check);
        let published = finish(published, metadata.close(), deadline, budget, control);
        match published {
            Ok(()) => {}
            Err(error) if pure_expected(&error, "vault.generation_conflict") => {
                let (other, _) = read_cached_definition(
                    home,
                    &key,
                    url,
                    deadline,
                    budget,
                    control,
                    &mut validate,
                )?;
                if other != blob {
                    return Err(fault(
                        "records.definition_identity",
                        4,
                        "Concurrent definition content differs",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
        check()?;
        Ok((blob, raw))
    })();
    finish(result, Ok(()), deadline, budget, control)
}

#[cfg(test)]
mod tests;
