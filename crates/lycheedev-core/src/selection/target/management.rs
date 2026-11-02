//! Complete named configuration lifecycle, using the same configuration wire
//! decoder as resolution and the owned document/CAS boundaries.
use super::*;
use crate::metadata::owned_write::{OwnedMutation, OwnedWriter};

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
impl Pages for OwnedWriter<'_> {
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
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference { schema: String, name: String, kind: String, id: String, created_at: String }
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
            let mut reference: Reference = serde_json::from_slice(raw).map_err(format)?;
            let time = storage::parse_timestamp(&reference.created_at).ok_or_else(|| format("invalid reference timestamp"))?;
            reference.created_at = storage::timestamp_offset(time);
            if reference.schema != "lycheedev.target-ref.v1" || reference.name != name {
                return Err(format("reference identity"));
            }
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
    run: impl FnOnce(&OwnedWriter<'_>) -> Result<T>,
) -> Result<T> {
    let result = Metadata::with_owned_write(home, control, run);
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
#[derive(Clone, Debug)]
pub enum TargetRequest {
    List,
    Add { config: TargetConfig, replace: bool },
    Show { key: String },
    Remove { key: String },
}
fn list(ctx: &RequestContext) -> Result<Outcome> {
    Ok(Outcome::new(json!({"targets":list_configurations(&ctx.home, &ctx.control)?})))
}
fn add(ctx: &RequestContext, supplied: &TargetConfig, replace: bool) -> Result<Outcome> {
    let mut config = supplied.clone();
    config.validate()?;
    if config.source == "installation" {
        config.installation = absolute(Path::new(&config.installation), &ctx.cwd)?
            .to_string_lossy().into_owned();
    }
    let raw = serde_json::to_vec(&config)?;
    let key = format!("target/{}", config.name);
    write(&ctx.home, &ctx.control, |writer| {
        let expected = match writer.document(&key) {
            Ok(_) if !replace => {
                return Err(error("selection.target_exists", "selection.target_exists"));
            }
            Ok(document) => document.generation,
            Err(error) if pure(&error, "vault.record_missing") => 0,
            Err(error) => return Err(error),
        };
        writer.commit(&[OwnedMutation {
            key: &key,
            expected_generation: expected,
            bytes: &raw,
        }])
    })?;
    Ok(Outcome::new(serde_json::to_value(config)?))
}
fn show(ctx: &RequestContext, key: &str) -> Result<Outcome> {
    if key.starts_with("PIN-") {
        return Ok(Outcome::new(serde_json::to_value(
            super::super::load_pinned_controlled(&ctx.home, key, &ctx.control)?,
        )?));
    }
    let result = read(&ctx.home, &ctx.control, |reader| {
        let config = choose(configs(reader, &ctx.control)?, key)?;
        let refs = references(reader, &config.name, &ctx.control)?;
        let mut value = json!({"config":config});
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
fn remove(ctx: &RequestContext, key: &str) -> Result<Outcome> {
    write(&ctx.home, &ctx.control, |writer| {
        let config = choose(configs(writer, &ctx.control)?, key)?;
        let refs = references(writer, &config.name, &ctx.control)?;
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
            ctx.control.check()?;
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
pub fn execute(ctx: &RequestContext, request: &TargetRequest) -> Result<Outcome> {
    ctx.control.check()?;
    let result = match request {
        TargetRequest::List => list(ctx),
        TargetRequest::Add { config, replace } => add(ctx, config, *replace),
        TargetRequest::Show { key } => show(ctx, key),
        TargetRequest::Remove { key } => remove(ctx, key),
    };
    super::super::selection_finish(result, ctx.control.check())
}

#[cfg(test)]
mod tests;
