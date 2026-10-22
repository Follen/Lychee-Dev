//! Host declaration policy lives here, outside the pinned analysis engine.
use crate::{CallableInput, DeclarationFailure, DeclarationLookup, DeclarationProvenance};
use emmylua_code_analysis::{AsyncState, LuaFunctionType, LuaType, LuaUnionType, VariadicType};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_DEPTH: usize = 32;
const MAX_SLOTS: usize = 32;
const MAX_WORK: usize = 4096;
const MAX_METADATA_NODES: usize = 16_384;
const MAX_METADATA_BYTES: usize = 8 << 20;
const MAX_TEXT: usize = 1024;

type ProjectResult<T, E> = Result<T, Failure<E>>;
enum Failure<E> {
    Interrupted(E),
    Unavailable(&'static str),
}
use Failure::Unavailable;

pub(crate) struct Projection {
    pub(crate) signature: Result<Arc<LuaFunctionType>, &'static str>,
    pub(crate) declarations: Vec<DeclarationProvenance>,
}

pub(crate) fn project<E>(
    input: &CallableInput<'_>,
    check: &dyn Fn() -> Result<(), E>,
    step: &mut dyn FnMut() -> Result<(), E>,
    resolve: &mut dyn FnMut(&str) -> Result<DeclarationLookup, E>,
) -> Result<Projection, DeclarationFailure<E>> {
    let mut projector = Projector {
        aliases: input.aliases,
        catalog: input.catalog_identity,
        check,
        step,
        resolve,
        work: 0,
        metadata_nodes: 0,
        metadata_bytes: 0,
        active: BTreeSet::new(),
        records: BTreeMap::new(),
        declarations: BTreeMap::new(),
    };
    let result = (|| {
        projector.tick()?;
        if input.origin.identity.is_empty()
            || input.origin.material_sha256.len() != 64
            || input.declaration.get("kind").and_then(Value::as_str) != Some("function")
        {
            return Err(Unavailable("environment_callable_identity_invalid"));
        }
        if input.catalog_identity.len() > MAX_TEXT {
            return Err(Unavailable("environment_declaration_budget"));
        }
        projector.metadata(input.declaration, 0)?;
        projector.function(input.declaration, 0)
    })();
    // Cancellation wins even if the last resolver/step made the answer unavailable.
    check().map_err(DeclarationFailure::Interrupted)?;
    let signature = match result {
        Ok(signature) => Ok(signature),
        Err(Failure::Unavailable(reason)) => Err(reason),
        Err(Failure::Interrupted(error)) => return Err(DeclarationFailure::Interrupted(error)),
    };
    Ok(Projection {
        signature,
        declarations: projector.declarations.into_values().collect(),
    })
}

struct Projector<'a, E> {
    aliases: &'a BTreeMap<String, String>,
    catalog: &'a str,
    check: &'a dyn Fn() -> Result<(), E>,
    step: &'a mut dyn FnMut() -> Result<(), E>,
    resolve: &'a mut dyn FnMut(&str) -> Result<DeclarationLookup, E>,
    work: usize,
    metadata_nodes: usize,
    metadata_bytes: usize,
    active: BTreeSet<String>,
    // Query-local: resolve each name once, but charge every expanded use. Caching
    // an expanded Arc without charging its subtree would permit exponential work
    // in the downstream renderer. No catalog or Source graph is copied here.
    records: BTreeMap<String, Arc<Value>>,
    declarations: BTreeMap<String, DeclarationProvenance>,
}
impl<E> Projector<'_, E> {
    fn checkpoint(&mut self) -> ProjectResult<(), E> {
        (self.check)().map_err(Failure::Interrupted)?;
        (self.step)().map_err(Failure::Interrupted)?;
        (self.check)().map_err(Failure::Interrupted)
    }
    fn tick(&mut self) -> ProjectResult<(), E> {
        self.checkpoint()?;
        self.work += 1;
        if self.work > MAX_WORK {
            return Err(Unavailable("environment_projection_budget"));
        }
        Ok(())
    }
    fn metadata(&mut self, value: &Value, depth: usize) -> ProjectResult<(), E> {
        self.checkpoint()?;
        self.metadata_nodes += 1;
        if depth >= MAX_DEPTH || self.metadata_nodes > MAX_METADATA_NODES {
            return Err(Unavailable("environment_declaration_budget"));
        }
        match value {
            Value::String(text) => self.metadata_bytes += text.len(),
            Value::Array(values) => {
                for value in values {
                    self.metadata(value, depth + 1)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.metadata_bytes += key.len();
                    if self.metadata_bytes > MAX_METADATA_BYTES {
                        return Err(Unavailable("environment_declaration_budget"));
                    }
                    self.metadata(value, depth + 1)?;
                }
            }
            Value::Number(number) => self.metadata_bytes += number.to_string().len(),
            _ => self.metadata_bytes += 8,
        }
        if self.metadata_bytes > MAX_METADATA_BYTES {
            return Err(Unavailable("environment_declaration_budget"));
        }
        Ok(())
    }
    fn function(&mut self, record: &Value, depth: usize) -> ProjectResult<Arc<LuaFunctionType>, E> {
        self.tick()?;
        if depth >= MAX_DEPTH {
            return Err(Unavailable("environment_type_depth"));
        }
        if record.get("owner").is_some_and(|owner| !owner.is_null()) {
            return Err(Unavailable("environment_receiver_unavailable"));
        }
        text::<E>(record, "name")?;
        let parameters = slots(record, "parameters")?;
        let returns = slots(record, "returns")?;
        if parameters.len() > MAX_SLOTS || returns.len() > MAX_SLOTS {
            return Err(Unavailable("environment_signature_budget"));
        }
        let mut params = Vec::with_capacity(parameters.len());
        for (index, slot) in parameters.iter().enumerate() {
            if slot.get("index").and_then(Value::as_u64) != Some(index as u64 + 1) {
                return Err(Unavailable("environment_parameter_index_invalid"));
            }
            let name = text(slot, "name")?;
            params.push((name.to_owned(), Some(self.slot(slot, depth)?)));
        }
        let mut results = Vec::with_capacity(returns.len());
        for (index, slot) in returns.iter().enumerate() {
            if slot.get("index").and_then(Value::as_u64) != Some(index as u64 + 1) {
                return Err(Unavailable("environment_return_index_invalid"));
            }
            text::<E>(slot, "name")?;
            results.push(self.slot(slot, depth)?);
        }
        self.tick()?;
        // Keep the old cardinality uncertainty: MayReturnNothing must not be
        // rewritten into a single nil or a falsely fixed return count.
        let returns = if record
            .get("raw")
            .and_then(|raw| raw.get("MayReturnNothing"))
            == Some(&Value::Bool(true))
        {
            VariadicType::Base(LuaType::Unknown)
        } else {
            VariadicType::Multi(results)
        };
        Ok(Arc::new(LuaFunctionType::new(
            AsyncState::None,
            false,
            false,
            params,
            LuaType::Variadic(Arc::new(returns)),
            None,
        )))
    }
    fn slot(&mut self, slot: &Value, depth: usize) -> ProjectResult<LuaType, E> {
        self.tick()?;
        let name = text(slot, "type")?;
        let inner = optional_text(slot, "innerType")?;
        let mut ty = self.named(name, inner, depth)?;
        let nilable = match slot.get("nilable") {
            None | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            _ => return Err(Unavailable("environment_declaration_invalid")),
        };
        if nilable {
            self.tick()?;
            ty = LuaType::Union(Arc::new(LuaUnionType::from_vec(vec![ty, LuaType::Nil])));
        }
        Ok(ty)
    }
    fn named(&mut self, name: &str, inner: &str, depth: usize) -> ProjectResult<LuaType, E> {
        self.tick()?;
        if depth >= MAX_DEPTH {
            return Err(Unavailable("environment_type_depth"));
        }
        if !self.active.insert(name.to_owned()) {
            return Err(Unavailable("environment_type_cycle"));
        }
        let result = self.named_inner(name, inner, depth);
        self.active.remove(name);
        result
    }
    fn named_inner(&mut self, name: &str, inner: &str, depth: usize) -> ProjectResult<LuaType, E> {
        let aliases = self.aliases;
        if let Some(alias) = aliases.get(name) {
            if alias.len() > MAX_TEXT {
                return Err(Unavailable("environment_declaration_budget"));
            }
            return self.named(alias, inner, depth + 1);
        }
        let primitive = match name {
            "number" => Some(LuaType::Number),
            "integer" => Some(LuaType::Integer),
            "string" => Some(LuaType::String),
            "bool" | "boolean" => Some(LuaType::Boolean),
            "nil" => Some(LuaType::Nil),
            "any" => Some(LuaType::Any),
            "function" | "LuaFunction" => Some(LuaType::Function),
            "table" if inner.is_empty() => Some(LuaType::Table),
            _ => None,
        };
        if let Some(ty) = primitive {
            return Ok(ty);
        }
        if !inner.is_empty() {
            return Err(Unavailable("environment_type_unavailable"));
        }
        let record = if let Some(record) = self.records.get(name) {
            record.clone()
        } else {
            self.tick()?;
            let resolved = (self.resolve)(name);
            (self.check)().map_err(Failure::Interrupted)?;
            let record = match resolved.map_err(Failure::Interrupted)? {
                DeclarationLookup::Missing => {
                    return Err(Unavailable("environment_type_unavailable"));
                }
                DeclarationLookup::Ambiguous => {
                    return Err(Unavailable("environment_declaration_ambiguous"));
                }
                DeclarationLookup::Unique(record) => record,
            };
            self.metadata(&record, 0)?;
            let record = Arc::new(record);
            self.records.insert(name.to_owned(), record.clone());
            record
        };
        if text(&record, "name")? != name {
            return Err(Unavailable("environment_declaration_identity_invalid"));
        }
        let kind = text(&record, "kind")?;
        if !matches!(kind, "callback" | "callbacktype") {
            return Err(Unavailable("environment_type_unavailable"));
        }
        if self.catalog.is_empty() {
            return Err(Unavailable("environment_declaration_identity_invalid"));
        }
        self.declarations
            .entry(name.into())
            .or_insert(DeclarationProvenance {
                name: name.into(),
                kind: kind.into(),
                catalog_identity: self.catalog.into(),
                path: optional_text(&record, "path")?.into(),
                line: match record.get("line") {
                    None => 0,
                    Some(line) => line
                        .as_u64()
                        .ok_or(Unavailable("environment_declaration_invalid"))?,
                },
            });
        Ok(LuaType::DocFunction(self.function(&record, depth + 1)?))
    }
}
fn slots<'a, E>(record: &'a Value, field: &str) -> ProjectResult<&'a [Value], E> {
    match record.get(field) {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        _ => Err(Unavailable("environment_declaration_invalid")),
    }
}
fn text<'a, E>(value: &'a Value, field: &str) -> ProjectResult<&'a str, E> {
    let text = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(Unavailable("environment_declaration_invalid"))?;
    if text.len() > MAX_TEXT {
        return Err(Unavailable("environment_declaration_budget"));
    }
    Ok(text)
}
fn optional_text<'a, E>(value: &'a Value, field: &str) -> ProjectResult<&'a str, E> {
    if value.get(field).is_none() {
        Ok("")
    } else {
        text(value, field)
    }
}

#[cfg(test)]
mod tests;
