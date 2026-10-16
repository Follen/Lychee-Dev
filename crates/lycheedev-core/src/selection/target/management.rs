//! Complete named configuration lifecycle, using the same configuration wire
//! decoder as resolution and the fixed Go document/CAS boundaries.
use super::*;
use crate::metadata::historical_write::{HistoricalMutation, HistoricalWriter};
use serde::ser::SerializeStruct;
use serde_json::value::RawValue;

trait Pages {
    fn page(
        &self,
        prefix: &str,
        after: &str,
        control: &OperationControl,
        visit: &mut dyn FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)>;
}
impl Pages for Metadata {
    fn page(
        &self,
        prefix: &str,
        after: &str,
        control: &OperationControl,
        visit: &mut dyn FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        self.visit_raw_page_controlled_with_check(prefix, after, 1000, control, || Ok(()), visit)
    }
}
impl Pages for HistoricalWriter<'_> {
    fn page(
        &self,
        prefix: &str,
        after: &str,
        _: &OperationControl,
        visit: &mut dyn FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        self.visit_raw_page(prefix, after, visit)
    }
}
fn configs(reader: &impl Pages, control: &OperationControl) -> Result<Vec<TargetConfig>> {
    let mut result = Vec::new();
    let mut after = "target/".to_owned();
    loop {
        let (count, last) = reader.page("target/", &after, control, &mut |_, _, raw| {
            result.push(decode(raw, control)?);
            Ok(())
        })?;
        after = last;
        if count < 1000 {
            break;
        }
    }
    control.check()?;
    result.sort_by(|a, b| a.name.cmp(&b.name));
    control.check()?;
    Ok(result)
}
fn choose(mut configs: Vec<TargetConfig>, key: &str) -> Result<TargetConfig> {
    if key.is_empty() {
        return Err(error_missing(key));
    }
    if let Some(i) = configs.iter().position(|c| c.name == key) {
        return Ok(configs.remove(i));
    }
    let folded_key = folded(key);
    let exact: Vec<_> = configs
        .iter()
        .enumerate()
        .filter(|(_, c)| folded(&c.name) == folded_key)
        .map(|(i, _)| i)
        .collect();
    if exact.len() == 1 {
        return Ok(configs.remove(exact[0]));
    }
    if exact.len() > 1 {
        return Err(error(
            "selection.target_ambiguous",
            format!("selection.target_ambiguous: {key}"),
        ));
    }
    let key_lower = lower(key);
    let matching: Vec<_> = configs
        .iter()
        .enumerate()
        .filter(|(_, c)| lower(&c.name).starts_with(&key_lower))
        .map(|(i, _)| i)
        .collect();
    match matching.len() {
        0 => Err(error_missing(key)),
        1 => Ok(configs.remove(matching[0])),
        _ => Err(error(
            "selection.target_ambiguous",
            format!(
                "selection.target_ambiguous: {key} matches {}",
                matching
                    .iter()
                    .map(|i| configs[*i].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}
#[derive(Default)]
struct Reference {
    schema: String,
    name: String,
    kind: String,
    id: String,
    created_at: crate::evidence::GoTimestamp,
}
impl Serialize for Reference {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut value = s.serialize_struct("TargetReference", 5)?;
        value.serialize_field("schema", &self.schema)?;
        value.serialize_field("name", &self.name)?;
        value.serialize_field("kind", &self.kind)?;
        value.serialize_field("id", &self.id)?;
        let time = self.created_at.canonical().ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom(
                "Time.MarshalJSON: timezone hour outside of range [0,23]",
            )
        })?;
        value.serialize_field("createdAt", time)?;
        value.end()
    }
}
impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Reference;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a target reference")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(Reference::default())
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut out = Reference::default();
                while let Some(key) = m.next_key::<String>()? {
                    let field = match folded(&key).as_str() {
                        "schema" => &mut out.schema,
                        "name" => &mut out.name,
                        "kind" => &mut out.kind,
                        "id" => &mut out.id,
                        "createdat" => {
                            let raw = m.next_value::<Box<RawValue>>()?;
                            if raw.get() != "null" {
                                let token = raw.get();
                                let raw = token
                                    .strip_prefix('"')
                                    .and_then(|t| t.strip_suffix('"'))
                                    .ok_or_else(|| {
                                        serde::de::Error::custom(
                                            "Time.UnmarshalJSON: input is not a JSON string",
                                        )
                                    })?;
                                out.created_at = crate::evidence::parse_go_timestamp(raw)
                                    .ok_or_else(|| {
                                        serde::de::Error::custom("invalid Go reference timestamp")
                                    })?;
                            }
                            continue;
                        }
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
fn references(
    reader: &impl Pages,
    name: &str,
    control: &OperationControl,
) -> Result<Vec<Reference>> {
    let prefix = format!("target-ref/{name}/");
    let mut after = prefix.clone();
    let mut result = Vec::new();
    loop {
        let (count, last) = reader.page(&prefix, &after, control, &mut |_, _, raw| {
            if !crate::raw_json::is_valid(raw) {
                return Err(format("invalid JSON document"));
            }
            let normalized = crate::evidence::normalize_go_json_strings(raw, control)?;
            let reference: Reference = serde_json::from_slice(&normalized).map_err(format)?;
            if reference.schema != "lycheedev.target-ref.v1" || reference.name != name {
                return Err(format("reference identity"));
            }
            // Go validates schema/name only; unknown kind and empty ID remain
            // historical data, with their original remove/show behavior.
            result.push(reference);
            Ok(())
        })?;
        after = last;
        if count < 1000 {
            break;
        }
    }
    control.check()?;
    result.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.id.cmp(&b.id)));
    control.check()?;
    Ok(result)
}
fn read<T>(
    home: &Path,
    control: &OperationControl,
    run: impl FnOnce(&Metadata) -> Result<T>,
) -> Result<T> {
    let mut scope = Scope {
        control,
        check: || Ok(()),
    };
    scope.check()?;
    let metadata = read_metadata(home, control, || Ok(()))?;
    let result = run(&metadata);
    scope.finish(result, metadata.close())
}
fn write<T>(
    home: &Path,
    control: &OperationControl,
    run: impl FnOnce(&HistoricalWriter<'_>) -> Result<T>,
) -> Result<T> {
    let result = Metadata::with_historical_write(home, control, run);
    super::super::selection_finish(result, control.check())
}
fn absolute(path: &Path, cwd: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(cwd.join(path))?;
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}
pub(crate) fn list_configurations(home: &Path, control: &OperationControl) -> Result<Vec<TargetConfig>> {
    match read(home, control, |reader| configs(reader, control)) {
        Err(error) if pure(&error, "vault.record_missing") => Ok(Vec::new()),
        other => other,
    }
}
fn list(inv: &Invocation) -> Result<Outcome> {
    Ok(Outcome::new(json!({"targets":list_configurations(&inv.home, &inv.control)?})))
}

fn add(inv: &Invocation) -> Result<Outcome> {
    let name = inv
        .positional
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolkitError::arguments("target add requires a name argument"))?;
    let (product, region, locale) = (
        inv.flag("--product").unwrap_or(""),
        inv.flag("--region").unwrap_or(""),
        inv.flag("--locale").unwrap_or(""),
    );
    if product.is_empty() || region.is_empty() || locale.is_empty() {
        return Err(ToolkitError::arguments(
            "target add requires --product, --region, and --locale",
        ));
    }
    if crate::client::product(product).is_none() {
        return Err(ToolkitError::arguments(
            "target add requires a supported --product",
        ));
    }
    if validate_locale(region, locale).is_err() {
        return Err(ToolkitError::arguments(
            "target add requires a supported --region and --locale pair",
        ));
    }
    let installation = inv.flag("--installation").unwrap_or("");
    if inv.flag_bool("--remote") == !installation.is_empty() {
        return Err(ToolkitError::arguments(
            "target add requires exactly one of --installation or --remote",
        ));
    }
    let mut config = TargetConfig {
        schema: "lycheedev.target.v1".into(),
        name: name.into(),
        product: product.into(),
        region: region.into(),
        locale: locale.into(),
        full_build: inv.flag("--build").unwrap_or("").into(),
        source: if inv.flag_bool("--remote") {
            "remote"
        } else {
            "installation"
        }
        .into(),
        installation: installation.into(),
        definitions: inv.flag("--definitions").unwrap_or("").into(),
        repository: String::new(),
    };
    config.validate()?;
    if config.source == "installation" {
        config.installation = absolute(Path::new(&config.installation), &inv.cwd)?
            .to_string_lossy()
            .into_owned();
    }
    let raw = serde_json::to_vec(&config)?;
    let key = format!("target/{}", config.name);
    write(&inv.home, &inv.control, |writer| {
        let expected = match writer.document(&key) {
            Ok(_) if !inv.flag_bool("--replace") => {
                return Err(error("selection.target_exists", "selection.target_exists"));
            }
            Ok(document) => document.generation,
            Err(error) if pure(&error, "vault.record_missing") => 0,
            Err(error) => return Err(error),
        };
        writer.commit(&[HistoricalMutation {
            key: &key,
            expected_generation: expected,
            bytes: &raw,
        }])
    })?;
    Ok(Outcome::new(serde_json::to_value(config)?))
}
fn show(inv: &Invocation) -> Result<Outcome> {
    let key = inv.argument()?;
    if key.is_empty() {
        return Err(ToolkitError::arguments(
            "missing selection file or record ID; use describe",
        ));
    }
    if key.starts_with("PIN-") {
        return Ok(Outcome::new(serde_json::to_value(
            super::super::load_pinned_controlled(&inv.home, key, &inv.control)?,
        )?));
    }
    let result = read(&inv.home, &inv.control, |reader| {
        let config = choose(configs(reader, &inv.control)?, key)?;
        let refs = references(reader, &config.name, &inv.control)?;
        let mut value = json!({"config":config});
        // Serialize only on show: remove may legally consume a timestamp which
        // Go accepts at Unmarshal but cannot subsequently Marshal.
        value["references"] = serde_json::to_value(&refs)
            .map_err(|e| ToolkitError::new("command.failed", e.to_string(), 5))?;
        let active: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == "operation")
            .map(|r| &r.id)
            .collect();
        let pins: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == "pin")
            .map(|r| &r.id)
            .collect();
        if !active.is_empty() {
            value["activeOperations"] = json!(active);
        }
        if !pins.is_empty() {
            value["resolvedPins"] = json!(pins);
        }
        Ok(Outcome::new(value))
    });
    match result {
        Err(error) if pure(&error, "vault.record_missing") => Err(error_missing(key)),
        other => other,
    }
}
fn remove(inv: &Invocation) -> Result<Outcome> {
    let key = inv.argument()?;
    if key.is_empty() {
        return Err(ToolkitError::arguments(
            "target remove requires a name argument",
        ));
    }
    write(&inv.home, &inv.control, |writer| {
        let config = choose(configs(writer, &inv.control)?, key)?;
        let refs = references(writer, &config.name, &inv.control)?;
        let active: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == "operation")
            .map(|r| r.id.as_str())
            .collect();
        if !active.is_empty() {
            return Err(error(
                "selection.target_in_use",
                format!("selection.target_in_use: {}", active.join(", ")),
            ));
        }
        let target = format!("target/{}", config.name);
        let generation = writer.document(&target)?.generation;
        writer.delete(&[(target, generation)])?;
        let mut removals = Vec::new();
        for reference in &refs {
            inv.control.check()?;
            let key = format!(
                "target-ref/{}/{}/{}",
                config.name, reference.kind, reference.id
            );
            let generation = writer.document(&key)?.generation;
            removals.push((key, generation));
        }
        let mut count = 0;
        for batch in removals.chunks(64) {
            writer.delete(batch)?;
            count += batch.len();
        }
        let mut value = json!({"name":config.name,"removedRefCount":count});
        if !refs.is_empty() {
            value["removedPins"] = json!(refs.iter().map(|r| &r.id).collect::<Vec<_>>());
        }
        Ok(Outcome::new(value))
    })
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let result = match inv.command.as_str() {
        "target list" => list(inv),
        "target add" => add(inv),
        "target show" => show(inv),
        "target remove" => remove(inv),
        _ => Err(ToolkitError::arguments("unknown target command")),
    };
    super::super::selection_finish(result, inv.control.check())
}
#[cfg(test)]
mod tests;
