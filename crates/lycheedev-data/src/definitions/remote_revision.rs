//! Controlled immutable definition revision resolution for remote target preparation.
//! The legacy entry point remains unchanged; this path keeps its caller's deadline.
use super::*;
use crate::{
    remote::{
        cache::{
            Store, pure_expected,
            wire::{Object, ObjectSeed, folded, scalar},
        },
        transport::{self, BodyPolicy, GetPolicy, LocatorPolicy},
    },
    resource::Budget,
};
use lycheedev_core::ToolkitError;
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};

fn identity() -> ToolkitError {
    fault(
        "records.definition_identity",
        4,
        "records.definition_identity",
    )
}
pub(crate) fn normalized_reference(value: &str) -> Result<&str> {
    if lycheedev_core::selection::valid_digest(value, 40) {
        return Ok(value);
    }
    reference(value).ok_or_else(identity)
}
#[derive(Default, Serialize)]
struct Remembered {
    reference: String,
    commit: String,
}
impl Object for Remembered {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "reference" => scalar(&mut map, &mut self.reference)?,
                "commit" => scalar(&mut map, &mut self.commit)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Remembered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}

fn cache_error(error: ToolkitError) -> ToolkitError {
    if pure_expected(&error, "records.remote_identity") {
        identity()
    } else {
        error
    }
}
fn cached(store: &Store<'_>, key: &str, reference: &str) -> Result<Option<(Remembered, i64)>> {
    let value = store.read::<Remembered>(key).map_err(cache_error)?;
    if value.as_ref().is_some_and(|(v, _)| {
        v.reference != reference || !lycheedev_core::selection::valid_digest(&v.commit, 40)
    }) {
        return Err(identity());
    }
    Ok(value)
}
fn escaped(reference: &str) -> String {
    let mut result = String::new();
    for b in reference.bytes() {
        // Go url.PathEscape's encodePathSegment, not query encoding.
        if b.is_ascii_alphanumeric() || b"-_.~$&+:=@".contains(&b) {
            result.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(result, "%{b:02X}");
        }
    }
    result
}
trait Fetch {
    fn fetch(
        &self,
        reference: &str,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>>;
}
struct Public;
impl Fetch for Public {
    fn fetch(
        &self,
        reference: &str,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        let locator = format!(
            "https://api.github.com/repos/wowdev/WoWDBDefs/commits/{}",
            escaped(reference)
        );
        let deadline = transport::request_deadline(control, budget)?;
        transport::run(async {
            let response = transport::get_once(
                &locator,
                GetPolicy {
                    locator: LocatorPolicy::HttpsOnly,
                    body: BodyPolicy::GoAutomaticGzip,
                    accept: Some("application/vnd.github.sha"),
                    referer: None,
                    user_agent: None,
                    authorization: None,
                },
                control,
                budget,
                deadline,
            )
            .await?;
            if response.status().as_u16() != 200 {
                return Err(fault(
                    "records.definition_http",
                    5,
                    format!("records.definition_http: {}", response.status().as_u16()),
                ));
            }
            transport::read_response(
                response,
                BodyPolicy::GoAutomaticGzip,
                128,
                control,
                budget,
                deadline,
            )
            .await
        })
    }
}
pub fn resolve_revision_controlled(
    home: &Path,
    value: &str,
    offline: bool,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<String> {
    resolve(home, value, offline, control, budget, &Public)
}
fn resolve(
    home: &Path,
    value: &str,
    offline: bool,
    control: &OperationControl,
    budget: Option<&Budget>,
    fetch: &dyn Fetch,
) -> Result<String> {
    control.check()?;
    if let Some(budget) = budget {
        budget.check()?;
    }
    let value = normalized_reference(value)?;
    if lycheedev_core::selection::valid_digest(value, 40) {
        return Ok(value.into());
    }
    let store = Store {
        home,
        control,
        budget,
    };
    let key = format!("definition-ref/{}", storage::sha256(value.as_bytes()));
    let observed = cached(&store, &key, value)?;
    if offline {
        return observed.map(|(v, _)| v.commit).ok_or_else(|| {
            fault(
                "records.definition_unavailable_offline",
                3,
                "records.definition_unavailable_offline",
            )
        });
    }
    let bytes = fetch.fetch(value, control, budget)?;
    store.check()?;
    if bytes.len() > 128 {
        return Err(fault("records.metadata_limit", 3, "records.metadata_limit"));
    }
    let commit = std::str::from_utf8(&bytes)
        .map_err(|_| identity())?
        .trim_end_matches(['\r', '\n']);
    if !lycheedev_core::selection::valid_digest(commit, 40) {
        return Err(identity());
    }
    let document = Remembered {
        reference: value.into(),
        commit: commit.into(),
    };
    if let Err(error) = store.put(&key, &document, observed.map(|(_, generation)| generation)) {
        if !pure_expected(&error, "vault.generation_conflict") {
            return Err(error);
        }
        cached(&store, &key, value)?.ok_or_else(identity)?;
    }
    store.check()?;
    Ok(commit.into())
}
#[cfg(test)]
mod tests;
