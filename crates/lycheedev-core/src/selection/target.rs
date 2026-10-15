//! Configuration-only named target resolution. History is written only after
//! the preparer returns an immutable pin; it is never read to choose a target.
use super::*;
use crate::control::OperationControl;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use std::fmt;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetConfig {
    pub schema: String,
    pub name: String,
    pub product: String,
    pub region: String,
    pub locale: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub full_build: String,
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub installation: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub repository: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub definitions: String,
}
// Go typed Unmarshal ignores unknown fields, merges duplicate/case-folded
// string fields in source order, and leaves an existing string unchanged for
// null. The original raw grammar is checked before this projection.
impl<'de> Deserialize<'de> for TargetConfig {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = TargetConfig;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a target configuration")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(TargetConfig::default())
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut out = TargetConfig::default();
                while let Some(key) = m.next_key::<String>()? {
                    let field = match folded(&key).as_str() {
                        "schema" => &mut out.schema,
                        "name" => &mut out.name,
                        "product" => &mut out.product,
                        "region" => &mut out.region,
                        "locale" => &mut out.locale,
                        "fullbuild" => &mut out.full_build,
                        "source" => &mut out.source,
                        "installation" => &mut out.installation,
                        "repository" => &mut out.repository,
                        "definitions" => &mut out.definitions,
                        _ => {
                            m.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    if let Some(value) = m.next_value::<Option<String>>()? {
                        *field = value;
                    }
                }
                Ok(out)
            }
        }
        d.deserialize_any(Fields)
    }
}
fn folded(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
fn lower(value: &str) -> String {
    value
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}
fn error(code: &str, message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(
        code,
        message,
        match code {
            "selection.target_missing"
            | "selection.target_build_unavailable"
            | "selection.target_exists"
            | "selection.target_in_use" => 3,
            "selection.target_format" | "selection.target_ambiguous" => 2,
            _ => 5,
        },
    )
}
fn format(message: impl fmt::Display) -> ToolkitError {
    error(
        "selection.target_format",
        format!("selection.target_format: {message}"),
    )
}
impl TargetConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.schema.is_empty() && self.schema != "lycheedev.target.v1" {
            return Err(format(format!("schema {:?}", self.schema)));
        }
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || !self.name.bytes().any(|c| c.is_ascii_alphanumeric())
        {
            return Err(format(format!("name {:?}", self.name)));
        }
        if crate::client::product(&self.product).is_none() {
            return Err(error(
                "selection.unsupported_data_product",
                "selection.unsupported_data_product",
            ));
        }
        validate_locale(&self.region, &self.locale)?;
        if !self.full_build.is_empty()
            && (!super::valid_build(&self.full_build)
                || !self
                    .full_build
                    .bytes()
                    .all(|c| c.is_ascii_digit() || c == b'.'))
        {
            return Err(format(format!("fullBuild {:?}", self.full_build)));
        }
        match self.source.as_str() {
            "installation" if self.installation.trim().is_empty() => {
                return Err(format("source installation requires an installation path"));
            }
            "installation" => {}
            "remote" if !self.installation.is_empty() => {
                return Err(format("remote target must not name an installation"));
            }
            "remote" => {}
            _ => return Err(format(format!("source {:?}", self.source))),
        }
        if self.repository.len() > 200 || self.definitions.len() > 200 {
            return Err(format("field too long"));
        }
        Ok(())
    }
}
pub fn validate_locale(region: &str, language: &str) -> Result<()> {
    if ![
        "enUS", "koKR", "frFR", "deDE", "zhCN", "esES", "zhTW", "enGB", "esMX", "ruRU", "ptBR",
        "itIT", "ptPT",
    ]
    .contains(&language)
    {
        return Err(error(
            "selection.unsupported_data_language",
            "selection.unsupported_data_language",
        ));
    }
    if !["us", "eu", "cn", "kr", "tw"].contains(&region) {
        return Err(error(
            "selection.unsupported_data_region",
            "selection.unsupported_data_region",
        ));
    }
    Ok(())
}
struct Scope<'a, C> {
    control: &'a OperationControl,
    check: C,
}
impl<C: FnMut() -> Result<()>> Scope<'_, C> {
    fn check(&mut self) -> Result<()> {
        self.control.check()?;
        (self.check)()
    }
    fn finish<T>(&mut self, result: Result<T>, cleanup: Result<()>) -> Result<T> {
        let result = crate::metadata::historical_write::finish_historical(result, cleanup);
        let result = super::selection_finish(result, self.control.check());
        super::selection_finish(result, (self.check)())
    }
}
fn pure(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
fn decode(raw: &[u8], control: &OperationControl) -> Result<TargetConfig> {
    if !crate::raw_json::is_valid(raw) {
        return Err(format("invalid JSON document"));
    }
    let normalized = crate::evidence::normalize_go_json_strings(raw, control)?;
    let config: TargetConfig = serde_json::from_slice(&normalized).map_err(format)?;
    config.validate()?;
    Ok(config)
}
fn read_metadata(
    home: &Path,
    control: &OperationControl,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Metadata> {
    let metadata = Metadata::open_controlled_with_check(home, false, control, &mut check).map_err(
        |mut error| {
            if matches!(
                error.code.as_str(),
                "vault.schema_newer" | "vault.schema_invalid"
            ) {
                error.code = "vault.unsupported_workspace".into();
                if error.details.is_null() {
                    error.message = "vault.unsupported_workspace".into();
                }
                error.exit_code = 4;
            }
            error
        },
    )?;
    let result = metadata.require_historical_schema_with_check(control, &mut check);
    match result {
        Ok(()) => Ok(metadata),
        Err(error) => {
            let result =
                crate::metadata::historical_write::finish_historical(Err(error), metadata.close());
            let result = super::selection_finish(result, control.check());
            super::selection_finish(result, check())
        }
    }
}
/// Lookup scans all configuration pages, as fixed Go ListTargets does, even
/// after an exact match. An invalid unrelated configuration is still an error.
/// Only candidate configurations/names survive a page; no history is read.
pub fn lookup_controlled_with_check(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: impl FnMut() -> Result<()>,
) -> Result<TargetConfig> {
    let mut scope = Scope { control, check };
    scope.check()?;
    let metadata = match read_metadata(home, control, || (scope.check)()) {
        Ok(metadata) => metadata,
        Err(error) if pure(&error, "vault.record_missing") => {
            return scope.finish(Err(error_missing(key)), Ok(()));
        }
        Err(error) => return scope.finish(Err(error), Ok(())),
    };
    let result = (|| {
        let mut after = "target/".to_owned();
        let mut exact = None;
        let mut folded_match = None;
        let mut folded_count = 0;
        let mut prefix_match = None;
        let mut names = Vec::new();
        let folded_key = folded(key);
        let prefix = lower(key);
        loop {
            let (count, last) = metadata.visit_raw_page_controlled_with_check(
                "target/",
                &after,
                1000,
                control,
                || (scope.check)(),
                |_, _, raw| {
                    let config = decode(raw, control)?;
                    if config.name == key && exact.is_none() {
                        exact = Some(config.clone());
                    }
                    if folded(&config.name) == folded_key {
                        folded_count += 1;
                        folded_match = Some(config.clone());
                    }
                    if lower(&config.name).starts_with(&prefix) {
                        names.push(config.name.clone());
                        if prefix_match.is_none() {
                            prefix_match = Some(config);
                        }
                    }
                    Ok(())
                },
            )?;
            after = last;
            if count < 1000 {
                break;
            }
        }
        scope.check()?;
        if key.is_empty() {
            return Err(error_missing(key));
        }
        if let Some(config) = exact {
            return Ok(config);
        }
        if folded_count == 1 {
            return Ok(folded_match.expect("one folded candidate"));
        }
        if folded_count > 1 {
            return Err(error(
                "selection.target_ambiguous",
                format!("selection.target_ambiguous: {key}"),
            ));
        }
        match names.len() {
            0 => Err(error_missing(key)),
            1 => Ok(prefix_match.expect("one prefix candidate")),
            _ => {
                names.sort();
                Err(error(
                    "selection.target_ambiguous",
                    format!(
                        "selection.target_ambiguous: {key} matches {}",
                        names.join(", ")
                    ),
                ))
            }
        }
    })();
    scope.finish(result, metadata.close())
}
fn error_missing(key: &str) -> ToolkitError {
    error(
        "selection.target_missing",
        if key.is_empty() {
            "selection.target_missing".into()
        } else {
            format!("selection.target_missing: {key}")
        },
    )
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTarget {
    pub config: TargetConfig,
    pub pin: PinnedSet,
    pub resolved_at: String,
}
/// The preparer owns actual local/remote identity and its evidence. The named
/// result exposes only config/pin/time, just as selection.ResolveTarget.
pub fn resolve_controlled_with_check(
    home: &Path,
    key: &str,
    definitions: &str,
    control: &OperationControl,
    mut check: impl FnMut() -> Result<()>,
    prepare: impl FnOnce(&TargetConfig, &str) -> Result<PinnedSet>,
) -> Result<ResolvedTarget> {
    let config = lookup_controlled_with_check(home, key, control, &mut check)?;
    let mut scope = Scope { control, check };
    let result = (|| {
        if config.source == "installation" && !Path::new(&config.installation).is_absolute() {
            return Err(format(format!(
                "target {} has an old relative installation path; replace it with an explicit installation path",
                config.name
            )));
        }
        let definitions = if definitions.is_empty() {
            &config.definitions
        } else {
            definitions
        };
        let pin = prepare(&config, definitions).map_err(|mut error| {
            error.message = if error.code == "selection.target_build_unavailable" {
                format!(
                    "selection.target_build_unavailable: {} ({})",
                    config.full_build, error.message
                )
            } else {
                format!("selection.target_prepare_failed: {}", error.message)
            };
            error
        })?;
        if pin.id.is_empty() {
            return Err(error(
                "command.failed",
                "selection.target_prepare_failed: preparer returned no identity",
            ));
        }
        let at = storage::utc_now();
        let mut metadata =
            Metadata::open_controlled_with_check(home, true, control, || (scope.check)())?;
        let written=metadata.create_controlled_with_check(
            &format!("target-ref/{}/pin/{}",config.name,pin.id),
            &json!({"schema":"lycheedev.target-ref.v1","name":config.name,"kind":"pin","id":pin.id,"createdAt":at}),
            control,|| (scope.check)(),
        );
        let written = match written {
            Err(error) if pure(&error, "vault.generation_conflict") => Ok(()),
            other => other,
        };
        // Never re-read the mutable target or old ref. Removal during prepare
        // and an existing immutable reference are both successful Go outcomes.
        scope.finish(written, metadata.close())?;
        Ok(ResolvedTarget {
            config,
            pin,
            resolved_at: at,
        })
    })();
    scope.finish(result, Ok(()))
}
#[cfg(test)]
mod tests;

mod management;
pub use management::dispatch;
