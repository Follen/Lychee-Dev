//! Fixed context resolution and project identities, using the existing SQLite document contract.
#![forbid(unsafe_code)]
use crate::{
    Invocation, Outcome, Result, ToolkitError,
    metadata::Metadata,
    storage::{self, ExclusiveLock},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod file;
pub mod target;
pub use file::resolve_selection_file_controlled;
mod project_controlled;
pub use project_controlled::{
    resolve_pin_request_controlled_with_check, resolve_pinned_controlled_with_check,
};

/// Borrowed inputs for resolving an immutable snapshot or project selection.
///
/// Nonempty snapshots take precedence over projects; empty selectors retain
/// nearest-project fallback. The caller keeps ownership of paths and selectors,
/// and supplies the original operation control separately.
#[derive(Debug, Clone, Copy)]
pub struct PinSelectionRequest<'a> {
    pub home: &'a Path,
    pub cwd: &'a Path,
    pub snapshot: Option<&'a str>,
    pub project: Option<&'a str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePin {
    pub repository: String,
    pub product: String,
    pub requested_ref: String,
    pub exact_commit: String,
    pub parser_revision: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPin {
    pub product: String,
    pub region: String,
    pub full_build: String,
    pub build_config: String,
    #[serde(rename = "cdnConfig")]
    pub cdn_config: String,
    pub language: String,
    pub definition_commit: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePin {
    pub provider: String,
    pub product: String,
    pub full_build: String,
    pub region: String,
    pub scope: String,
    #[serde(serialize_with = "serialize_change_time")]
    pub fetched_at: String,
    #[serde(rename = "sha256")]
    pub sha256: String,
}
fn change_time_marshal_error() -> ToolkitError {
    ToolkitError::new(
        "command.failed",
        "json: error calling MarshalJSON for type time.Time: Time.MarshalJSON: timezone hour outside of range [0,23]",
        5,
    )
}
fn serialize_change_time<S: serde::Serializer>(
    value: &str,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match crate::evidence::parse_go_timestamp(value) {
        Some(time) => match time.canonical() {
            Some(canonical) => serializer.serialize_str(canonical),
            None => Err(serde::ser::Error::custom(
                change_time_marshal_error().message,
            )),
        },
        // Historical public Rust callers use String; raw Go wire readers own
        // malformed-input admission. Do not invent a new string parse policy here.
        None => serializer.serialize_str(value),
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectionSpec {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourcePin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<DataPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<ChangePin>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedSet {
    pub schema: String,
    pub id: String,
    #[serde(flatten)]
    pub spec: SelectionSpec,
}
impl std::ops::Deref for PinnedSet {
    type Target = SelectionSpec;
    fn deref(&self) -> &Self::Target {
        &self.spec
    }
}
pub fn valid_digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn valid_build(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|s| !s.is_empty() && s.parse::<u32>().is_ok())
}
pub fn valid_product(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
}
/// Approved interface observations belong to exact source commits, not version arithmetic.
pub fn source_interface(pin: &SourcePin) -> Option<u32> {
    if pin.repository != "wow-ui-source" {
        return None;
    }
    crate::client::baselines::VERIFIED_BASELINES
        .iter()
        .find(|baseline| {
            baseline.product == crate::client::source_product_family(&pin.product)
                && baseline.source_commit == pin.exact_commit
        })
        .map(|baseline| baseline.interface)
}
/// Family-wide Blizzard source pins support each delivery track, but an exact
/// regional pin never grants access to the other regional track.
pub fn source_supports_product(source: &SourcePin, product: &str) -> bool {
    source.product == product
        || source.repository == "wow-ui-source"
            && source.product == crate::client::source_product_family(product)
}
fn pin_product(spec: &SelectionSpec) -> Option<&str> {
    spec.data
        .as_ref()
        .map(|p| p.product.as_str())
        .or(spec.changes.as_ref().map(|p| p.product.as_str()))
        .or(spec.source.as_ref().map(|p| p.product.as_str()))
}
fn validate(spec: &SelectionSpec) -> Result<()> {
    if spec.source.is_none() && spec.data.is_none() && spec.changes.is_none() {
        return Err(ToolkitError::new(
            "selection.empty",
            "no fixed selection provided",
            4,
        ));
    }
    let mut products = vec![];
    if let Some(source) = &spec.source {
        if source.repository.is_empty()
            || source.product.is_empty()
            || source.parser_revision.is_empty()
            || !valid_digest(&source.exact_commit, 40)
        {
            return Err(ToolkitError::new(
                "selection.unresolved_source",
                "source requires a fixed commit",
                4,
            ));
        }
    }
    if let Some(data) = &spec.data {
        if data.product.is_empty()
            || !valid_build(&data.full_build)
            || !valid_digest(&data.build_config, 32)
            || !valid_digest(&data.cdn_config, 32)
            || !valid_digest(&data.definition_commit, 40)
            || data.region.is_empty()
            || data.language.is_empty()
        {
            return Err(ToolkitError::new(
                "selection.unresolved_data",
                "data requires exact build/config/definition identities",
                4,
            ));
        }
        products.push(&data.product);
    }
    if let Some(changes) = &spec.changes {
        if changes.product.is_empty()
            || changes.provider.is_empty()
            || changes.region.is_empty()
            || changes.scope.is_empty()
            || !valid_build(&changes.full_build)
            || !valid_digest(&changes.sha256, 64)
            || changes.fetched_at.is_empty()
            || crate::evidence::parse_go_timestamp(&changes.fetched_at)
                .is_some_and(|time| time.is_zero())
        {
            return Err(ToolkitError::new(
                "selection.unresolved_changes",
                "changes require exact capture identity",
                4,
            ));
        }
        products.push(&changes.product);
    }
    if products.windows(2).any(|pair| pair[0] != pair[1])
        || spec.source.as_ref().is_some_and(|source| {
            products
                .first()
                .is_some_and(|product| !source_supports_product(source, product))
        })
    {
        return Err(ToolkitError::new(
            "selection.product_mismatch",
            "selection products disagree",
            4,
        ));
    }
    Ok(())
}
fn identity(pin: &PinnedSet) -> Result<String> {
    // Fixed Go validates/merges the selection before its first json.Marshal.
    // A parsed >=24h offset is comparable but fails only at this publication
    // identity boundary, not while reading or choosing the latest Wago page.
    if pin
        .changes
        .as_ref()
        .and_then(|change| crate::evidence::parse_go_timestamp(&change.fetched_at))
        .is_some_and(|time| time.canonical().is_none())
    {
        return Err(change_time_marshal_error());
    }
    let mut unsigned = pin.clone();
    unsigned.id.clear();
    Ok(format!(
        "PIN-{}",
        storage::sha256(&storage::identity_json(&unsigned)?)
    ))
}
pub fn load_pinned(home: &Path, id: &str) -> Result<PinnedSet> {
    let pin = Metadata::open(home, false)?.read(&format!("pin/{id}"))?;
    validate_loaded_pin(pin, id)
}
/// Preserve parent pin identity validation while bounding open and row reads
/// by the invoking operation. Always check the acquired connection's close.
pub fn load_pinned_controlled(
    home: &Path,
    id: &str,
    control: &crate::control::OperationControl,
) -> Result<PinnedSet> {
    load_pinned_controlled_with_check(home, id, control, || Ok(()))
}
/// Keep the caller's independent stop active through open, row waits and close.
pub fn load_pinned_controlled_with_check(
    home: &Path,
    id: &str,
    control: &crate::control::OperationControl,
    mut check: impl FnMut() -> Result<()>,
) -> Result<PinnedSet> {
    selection_check(Some(control), &mut check)?;
    let metadata = Metadata::open_controlled_with_check(home, false, control, &mut check)?;
    let result = metadata
        .read_controlled_with_check(&format!("pin/{id}"), control, &mut check)
        .and_then(|pin| validate_loaded_pin(pin, id));
    let result = crate::metadata::historical_write::finish_historical(result, metadata.close());
    selection_finish(result, selection_check(Some(control), &mut check))
}
fn validate_loaded_pin(pin: PinnedSet, id: &str) -> Result<PinnedSet> {
    validate(&pin.spec)?;
    if pin.schema != "lycheedev.selection.v1" || pin.id != id || identity(&pin)? != id {
        return Err(ToolkitError::new(
            "selection.pin_integrity",
            "fixed selection identity differs",
            4,
        ));
    }
    Ok(pin)
}
pub fn pin_selection(home: &Path, spec: SelectionSpec) -> Result<PinnedSet> {
    pin_selection_inner(home, spec, None)
}
/// Publish a derived selection with the invoking operation's cancellation and deadline.
pub fn pin_selection_controlled(
    home: &Path,
    spec: SelectionSpec,
    control: &crate::control::OperationControl,
) -> Result<PinnedSet> {
    pin_selection_controlled_with_check(home, spec, control, || Ok(()))
}
/// Supplemental checks use invocation-first ordering and retain the same
/// parent-read and independent pin publication transactions.
pub fn pin_selection_controlled_with_check(
    home: &Path,
    spec: SelectionSpec,
    control: &crate::control::OperationControl,
    check: impl FnMut() -> Result<()>,
) -> Result<PinnedSet> {
    pin_selection_observed_with_check(home, spec, Some(control), check, |_| {})
}
fn selection_check(
    control: Option<&crate::control::OperationControl>,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    if let Some(control) = control {
        control.check()?;
    }
    check()
}
// Check after cleanup on success and failure. Keep the first typed cause and
// append a distinct stop, while avoiding a duplicate observation of one stop.
pub(crate) fn selection_finish<T>(result: Result<T>, stopped: Result<()>) -> Result<T> {
    if let (Err(first), Err(stop)) = (&result, &stopped)
        && first.code == stop.code
        && first.message == stop.message
        && first.exit_code == stop.exit_code
        && first.details == stop.details
    {
        return result;
    }
    crate::metadata::historical_write::finish_historical(result, stopped)
}
fn pin_selection_inner(
    home: &Path,
    spec: SelectionSpec,
    control: Option<&crate::control::OperationControl>,
) -> Result<PinnedSet> {
    pin_selection_observed(home, spec, control, |_| {})
}
fn pin_selection_observed(
    home: &Path,
    spec: SelectionSpec,
    control: Option<&crate::control::OperationControl>,
    published: impl FnMut(&PinnedSet),
) -> Result<PinnedSet> {
    pin_selection_observed_with_check(home, spec, control, || Ok(()), published)
}
fn pin_selection_observed_with_check(
    home: &Path,
    mut spec: SelectionSpec,
    control: Option<&crate::control::OperationControl>,
    mut check: impl FnMut() -> Result<()>,
    mut published: impl FnMut(&PinnedSet),
) -> Result<PinnedSet> {
    selection_check(control, &mut check)?;
    if let Some(change) = spec.changes.as_mut()
        && let Some(time) = crate::evidence::parse_go_timestamp(&change.fetched_at)
        && let Some(canonical) = time.canonical()
    {
        change.fetched_at = canonical.to_owned();
    }
    if !spec.parent.is_empty() {
        let parent = match control {
            Some(control) => {
                load_pinned_controlled_with_check(home, &spec.parent, control, &mut check)?
            }
            None => load_pinned(home, &spec.parent)?,
        };
        inherit_parent(&mut spec, parent)?;
    }
    validate(&spec)?;
    let mut pin = PinnedSet {
        schema: "lycheedev.selection.v1".into(),
        id: String::new(),
        spec,
    };
    pin.id = identity(&pin)?;
    selection_check(control, &mut check)?;
    let key = format!("pin/{}", pin.id);
    if let Some(control) = control {
        let mut metadata = Metadata::open_controlled_with_check(home, true, control, &mut check)?;
        let result = metadata.put_controlled_with_check(&key, &pin, None, control, &mut check);
        if result.is_ok() {
            published(&pin);
        }
        let result = crate::metadata::historical_write::finish_historical(result, metadata.close());
        selection_finish(result, selection_check(Some(control), &mut check))?;
    } else {
        Metadata::open(home, true)?.put(&key, &pin, None)?;
        published(&pin);
    }
    Ok(pin)
}
fn inherit_parent(spec: &mut SelectionSpec, parent: PinnedSet) -> Result<()> {
    if spec.source.is_some() && parent.source.is_some() && spec.source != parent.source {
        return Err(ToolkitError::new(
            "selection.fixed_source_conflict",
            "cannot change an inherited source",
            4,
        ));
    }
    if spec.data.is_some() && parent.data.is_some() && spec.data != parent.data {
        return Err(ToolkitError::new(
            "selection.fixed_data_conflict",
            "cannot change inherited data",
            4,
        ));
    }
    if spec.changes.is_some() && parent.changes.is_some() && spec.changes != parent.changes {
        return Err(ToolkitError::new(
            "selection.fixed_changes_conflict",
            "cannot change inherited changes",
            4,
        ));
    }
    spec.source = spec.source.take().or(parent.spec.source);
    spec.data = spec.data.take().or(parent.spec.data);
    spec.changes = spec.changes.take().or(parent.spec.changes);
    Ok(())
}
pub fn find_project(start: &Path) -> Result<PathBuf> {
    find_project_controlled(start, &crate::control::OperationControl::default())
}
pub fn find_project_controlled(
    start: &Path,
    control: &crate::control::OperationControl,
) -> Result<PathBuf> {
    project_controlled::find_controlled(start, control)
}
pub fn inspect_project(path: &Path) -> Result<Value> {
    inspect_project_controlled(path, &crate::control::OperationControl::default())
}
pub fn inspect_project_controlled(
    path: &Path,
    control: &crate::control::OperationControl,
) -> Result<Value> {
    project_controlled::inspect_value(path, control)
}
pub fn resolve_pinned(inv: &Invocation) -> Result<PinnedSet> {
    resolve_pinned_controlled_with_check(inv, || Ok(()))
}
/// Nearest-project fallback for actual Source/catalog consumers, sharing the
/// controlled Data path and exact immutable import/winner authentication.
pub fn resolve_optional_project(inv: &Invocation) -> Result<Option<(PathBuf, PinnedSet)>> {
    project_controlled::optional(inv)
}
pub fn dispatch_project(inv: &Invocation) -> Result<Outcome> {
    project_controlled::dispatch(inv)
}

#[cfg(test)]
mod product_tests {
    use super::*;
    fn source(product: &str, repository: &str) -> SourcePin {
        SourcePin {
            repository: repository.into(),
            product: product.into(),
            requested_ref: "forever".into(),
            exact_commit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069".into(),
            parser_revision: "fixture-v1".into(),
        }
    }
    fn data(product: &str) -> DataPin {
        DataPin {
            product: product.into(),
            region: "cn".into(),
            language: "zhCN".into(),
            full_build: "1.60.1.70338".into(),
            build_config: "c".repeat(32),
            cdn_config: "d".repeat(32),
            definition_commit: "e".repeat(40),
        }
    }
    fn changes(product: &str) -> ChangePin {
        ChangePin {
            product: product.into(),
            region: "cn".into(),
            provider: "fixture".into(),
            scope: "full".into(),
            full_build: "1.60.1.70338".into(),
            fetched_at: "2026-10-14T00:00:00Z".into(),
            sha256: "f".repeat(64),
        }
    }
    #[test]
    fn change_time_admission_comparison_and_identity_marshal_have_distinct_boundaries() {
        let mut change = changes("retail");
        change.fetched_at = "2026-10-16T00:00:00+24:00".into();
        let mut spec = SelectionSpec {
            changes: Some(change),
            ..Default::default()
        };
        validate(&spec).unwrap();
        let failure = identity(&PinnedSet {
            schema: "lycheedev.selection.v1".into(),
            id: String::new(),
            spec: spec.clone(),
        })
        .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit_code),
            ("command.failed", 5)
        );
        assert!(serde_json::to_value(&spec).is_err());
        spec.changes.as_mut().unwrap().fetched_at = "0001-01-02T00:00:00+24:00".into();
        assert_eq!(
            validate(&spec).unwrap_err().code,
            "selection.unresolved_changes"
        );
        spec.changes.as_mut().unwrap().fetched_at = "2026-10-16T00:00:00+00:60".into();
        let normalized = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            normalized["changes"]["fetchedAt"],
            "2026-10-16T00:00:00+01:00"
        );
    }

    #[test]
    fn shared_source_supports_each_exact_track_without_collapsing_pin_identity() {
        let mut identities = vec![];
        for product in ["forever-asia-beta", "forever-cn-beta"] {
            let spec = SelectionSpec {
                source: Some(source("forever", "wow-ui-source")),
                data: Some(data(product)),
                changes: Some(changes(product)),
                ..Default::default()
            };
            validate(&spec).unwrap();
            assert_eq!(pin_product(&spec), Some(product));
            identities.push(
                identity(&PinnedSet {
                    schema: "lycheedev.selection.v1".into(),
                    id: String::new(),
                    spec,
                })
                .unwrap(),
            );
            let regional = source(product, "wow-ui-source");
            assert!(source_supports_product(&regional, product));
            assert_eq!(source_interface(&regional), Some(16001));
            let changes_only = SelectionSpec {
                source: Some(source("forever", "wow-ui-source")),
                changes: Some(changes(product)),
                ..Default::default()
            };
            validate(&changes_only).unwrap();
            assert_eq!(pin_product(&changes_only), Some(product));
        }
        assert_ne!(identities[0], identities[1]);
        let source_only = SelectionSpec {
            source: Some(source("forever", "wow-ui-source")),
            ..Default::default()
        };
        validate(&source_only).unwrap();
        assert_eq!(pin_product(&source_only), Some("forever"));
    }
    #[test]
    fn regional_and_unrelated_sources_cannot_borrow_another_tracks_data() {
        for (source_product, repository, data_product) in [
            ("forever-asia-beta", "wow-ui-source", "forever-cn-beta"),
            ("forever-cn-beta", "wow-ui-source", "forever-asia-beta"),
            ("forever", "elvui", "forever-cn-beta"),
            ("forever", "wow-ui-source", "classic-era"),
            ("forever-asia-beta", "wow-ui-source", "forever"),
        ] {
            let spec = SelectionSpec {
                source: Some(source(source_product, repository)),
                data: Some(data(data_product)),
                ..Default::default()
            };
            assert_eq!(
                validate(&spec).unwrap_err().code,
                "selection.product_mismatch"
            );
        }
    }
    #[test]
    fn shared_source_never_relaxes_data_changes_track_equality() {
        for (data_product, change_product) in [
            ("forever-asia-beta", "forever-cn-beta"),
            ("forever-cn-beta", "forever-asia-beta"),
        ] {
            for with_source in [false, true] {
                let spec = SelectionSpec {
                    source: with_source.then(|| source("forever", "wow-ui-source")),
                    data: Some(data(data_product)),
                    changes: Some(changes(change_product)),
                    ..Default::default()
                };
                assert_eq!(
                    validate(&spec).unwrap_err().code,
                    "selection.product_mismatch"
                );
            }
        }
    }
    #[test]
    fn exact_source_commit_and_repository_remain_required_for_interface() {
        for (product, commit, interface) in [
            ("retail", "18c6d677b605b312f3d813d498404631fb941977", 120105),
            ("retail", "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b", 120100),
            (
                "classic-era",
                "8165d4cd6e48d606369336cc3a7977902310e81e",
                11509,
            ),
            (
                "anniversary",
                "2cfec7c00f0f24762d5ca35a4beb1e93c0cde328",
                20506,
            ),
        ] {
            let mut pin = source(product, "wow-ui-source");
            pin.exact_commit = commit.into();
            assert_eq!(source_interface(&pin), Some(interface));
            pin.repository = "fixture".into();
            assert_eq!(source_interface(&pin), None);
        }
        let mut pin = source("forever-cn-beta", "wow-ui-source");
        pin.exact_commit = "a".repeat(40);
        assert_eq!(source_interface(&pin), None);
    }
    #[test]
    fn empty_identity_fields_are_unresolved_not_family_permissions() {
        let spec = SelectionSpec {
            source: Some(source("", "wow-ui-source")),
            ..Default::default()
        };
        assert_eq!(
            validate(&spec).unwrap_err().code,
            "selection.unresolved_source"
        );
        let spec = SelectionSpec {
            data: Some(data("")),
            ..Default::default()
        };
        assert_eq!(
            validate(&spec).unwrap_err().code,
            "selection.unresolved_data"
        );
        for field in ["provider", "product", "region", "scope"] {
            let mut change = changes("forever-cn-beta");
            match field {
                "provider" => change.provider.clear(),
                "product" => change.product.clear(),
                "region" => change.region.clear(),
                _ => change.scope.clear(),
            }
            let spec = SelectionSpec {
                changes: Some(change),
                ..Default::default()
            };
            assert_eq!(
                validate(&spec).unwrap_err().code,
                "selection.unresolved_changes"
            );
        }
    }
}

#[cfg(test)]
mod controlled_tests;

#[cfg(test)]
mod supplemental_tests;
