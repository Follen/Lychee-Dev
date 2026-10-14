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
    pub fetched_at: String,
    #[serde(rename = "sha256")]
    pub sha256: String,
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
    [
        ("retail", "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b", 120100),
        ("classic", "1028c1e687f721ba9d3af14d1b12a5745e4227c7", 50504),
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
        ("titan", "825d29d3662b372f0bead725ee6abd339e4a77b5", 38002),
        ("forever", "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069", 16001),
    ]
    .into_iter()
    .find(|(product, commit, _)| {
        *product == crate::client::source_product_family(&pin.product)
            && *commit == pin.exact_commit
    })
    .map(|(_, _, interface)| interface)
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
    let mut unsigned = pin.clone();
    unsigned.id.clear();
    Ok(format!(
        "PIN-{}",
        storage::sha256(&storage::identity_json(&unsigned)?)
    ))
}
pub fn load_pinned(home: &Path, id: &str) -> Result<PinnedSet> {
    let pin: PinnedSet = Metadata::open(home, false)?.read(&format!("pin/{id}"))?;
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
pub fn pin_selection(home: &Path, mut spec: SelectionSpec) -> Result<PinnedSet> {
    if let Some(change) = spec.changes.as_mut()
        && let Ok(time) = chrono::DateTime::parse_from_rfc3339(&change.fetched_at)
    {
        change.fetched_at = storage::timestamp_offset(time);
    }
    if !spec.parent.is_empty() {
        let parent = load_pinned(home, &spec.parent)?;
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
        spec.source = spec.source.or(parent.spec.source);
        spec.data = spec.data.or(parent.spec.data);
        spec.changes = spec.changes.or(parent.spec.changes);
    }
    validate(&spec)?;
    let mut pin = PinnedSet {
        schema: "lycheedev.selection.v1".into(),
        id: String::new(),
        spec,
    };
    pin.id = identity(&pin)?;
    Metadata::open(home, true)?.put(&format!("pin/{}", pin.id), &pin, None)?;
    Ok(pin)
}
pub fn find_project(start: &Path) -> Result<PathBuf> {
    for parent in start.ancestors() {
        if parent.join("lycheedev.json").exists() {
            return Ok(parent.into());
        }
    }
    Err(ToolkitError::new(
        "selection.project_missing",
        "no parent project declaration found",
        2,
    ))
}
pub fn inspect_project(path: &Path) -> Result<Value> {
    let project: Value = storage::read_json(&path.join("lycheedev.json"))?;
    if project["schema"] != "lycheedev.project.v1"
        || !valid_product(project["product"].as_str().unwrap_or(""))
    {
        return Err(ToolkitError::new(
            "selection.project_format",
            "invalid project declaration",
            4,
        ));
    }
    let mut result = json!({"directory":path,"project":project,"state":"unlocked"});
    if path.join("lycheedev.lock.json").exists() {
        let lock: Value = storage::read_json(&path.join("lycheedev.lock.json"))?;
        let pin: PinnedSet = serde_json::from_value(lock["selection"].clone())?;
        validate(&pin.spec)?;
        if lock["schema"] != "lycheedev.project-lock.v1" || identity(&pin)? != pin.id {
            return Err(ToolkitError::new(
                "selection.project_format",
                "invalid project lock",
                4,
            ));
        }
        if pin_product(&pin.spec) != project["product"].as_str() {
            return Err(ToolkitError::new(
                "selection.project_conflict",
                "project product differs from lock",
                4,
            ));
        }
        result["state"] = json!("locked");
        result["lock"] = lock;
    }
    Ok(result)
}
pub fn resolve_pinned(inv: &Invocation) -> Result<PinnedSet> {
    if let Some(id) = inv.flag("--snapshot") {
        return load_pinned(&inv.home, id);
    }
    let path = inv
        .flag("--project")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| find_project(&inv.cwd))?;
    let status = inspect_project(&path)?;
    if status["state"] != "locked" {
        return Err(ToolkitError::new(
            "selection.project_unlocked",
            "project has no fixed lock",
            2,
        ));
    }
    let pin: PinnedSet = serde_json::from_value(status["lock"]["selection"].clone())?;
    Metadata::open(&inv.home, true)?.put(&format!("pin/{}", pin.id), &pin, None)?;
    Ok(pin)
}

/// Public snapshot consumers try the nearest project before checking their
/// required source/selector flags. No nearby declaration is an absent fallback;
/// an explicitly selected or discovered unlocked project is an error.
pub fn resolve_optional_project(inv: &Invocation) -> Result<Option<(PathBuf, PinnedSet)>> {
    inv.control.check()?;
    if inv.flag("--snapshot").is_some_and(|id| !id.is_empty()) {
        return Ok(None);
    }
    let path = if let Some(path) = inv.flag("--project").filter(|path| !path.is_empty()) {
        std::path::absolute(inv.cwd.join(path))?
    } else {
        match find_project(&inv.cwd) {
            Ok(path) => std::path::absolute(path)?,
            Err(error) if error.code == "selection.project_missing" => return Ok(None),
            Err(error) => return Err(error),
        }
    };
    if !fs::metadata(&path)?.is_dir() {
        return Err(ToolkitError::new(
            "selection.project_format",
            "selection.project_format",
            4,
        ));
    }
    match fs::symlink_metadata(path.join("lycheedev.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ToolkitError::new(
                "selection.project_missing",
                "selection.project_missing",
                3,
            ));
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let status = inspect_project(&path).map_err(|mut error| {
        if error.code.starts_with("selection.project_") {
            error.message = error.code.clone();
        }
        error
    })?;
    if status["state"] != "locked" {
        return Err(ToolkitError::new(
            "selection.project_unlocked",
            "selection.project_unlocked",
            3,
        ));
    }
    let pin: PinnedSet = serde_json::from_value(status["lock"]["selection"].clone())?;
    inv.control.check()?;
    Metadata::open(&inv.home, true)?.put(&format!("pin/{}", pin.id), &pin, None)?;
    Ok(Some((path, pin)))
}
pub fn dispatch_project(inv: &Invocation) -> Result<Outcome> {
    let path = inv
        .flag("--path")
        .map(PathBuf::from)
        .unwrap_or_else(|| inv.cwd.clone());
    match inv.command.as_str() {
        "project init" => {
            let product = inv.required("--product")?;
            if !valid_product(product) {
                return Err(ToolkitError::arguments("unsupported product"));
            }
            fs::create_dir_all(&path)?;
            let _lock = ExclusiveLock::acquire(&path.join(".lycheedev-locks/project.lock"))?;
            if path.join("lycheedev.json").exists() {
                let status = inspect_project(&path)?;
                if status["project"]["product"] != product {
                    return Err(ToolkitError::new(
                        "selection.project_conflict",
                        "project already declares another product",
                        4,
                    ));
                }
                return Ok(Outcome::new(status));
            }
            if path.join("lycheedev.lock.json").exists() {
                return Err(ToolkitError::new(
                    "selection.project_conflict",
                    "orphan project lock exists",
                    4,
                ));
            }
            storage::atomic_json(
                &path.join("lycheedev.json"),
                &json!({"schema":"lycheedev.project.v1","product":product}),
            )?;
            Ok(Outcome::new(inspect_project(&path)?))
        }
        "project status" => Ok(Outcome::new(inspect_project(
            &if inv.flag("--path").is_some() {
                path
            } else {
                find_project(&inv.cwd)?
            },
        )?)),
        "project lock" => {
            let path = if inv.flag("--path").is_some() {
                path
            } else {
                find_project(&inv.cwd)?
            };
            let _lock = ExclusiveLock::acquire(&path.join(".lycheedev-locks/project.lock"))?;
            let status = inspect_project(&path)?;
            let pin = load_pinned(&inv.home, inv.required("--snapshot")?)?;
            let product = pin_product(&pin.spec);
            if product != status["project"]["product"].as_str() {
                return Err(ToolkitError::new(
                    "selection.project_conflict",
                    "pin product differs",
                    4,
                ));
            }
            storage::atomic_json(
                &path.join("lycheedev.lock.json"),
                &json!({"schema":"lycheedev.project-lock.v1","selection":pin}),
            )?;
            Ok(Outcome::new(inspect_project(&path)?))
        }
        _ => Err(ToolkitError::arguments("unknown project verb")),
    }
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
