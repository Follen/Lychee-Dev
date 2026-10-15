//! Fixed Go 3.2 semantic metadata: pinned enum/flag definitions are rows in
//! meta.Table. Applicability conditions stay visible and never filter DB2 rows.
use super::*;
use crate::{
    relational::{Scalar, Source},
    resource::{Budget, Cost},
};

#[derive(Debug, Serialize)]
pub(crate) struct SemanticBundle {
    pub table: String,
    pub commit: String,
    pub build: String,
    pub mapping: Value,
    pub definitions: BTreeMap<String, Value>,
}
#[derive(Debug, PartialEq)]
struct Mapping {
    field: String,
    kind: String,
    name: String,
    condition_field: String,
    condition_value: String,
}
fn identity() -> lycheedev_core::ToolkitError {
    fault(
        "records.definition_identity",
        4,
        "records.definition_identity",
    )
}
fn limit() -> lycheedev_core::ToolkitError {
    fault("records.metadata_limit", 3, "records.metadata_limit")
}
fn name(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().enumerate().all(|(i, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || i > 0 && byte.is_ascii_digit()
        })
}
fn field(text: &str) -> bool {
    if let Some((base, index)) = text.split_once('[') {
        name(base)
            && index.strip_suffix(']').is_some_and(|index| {
                !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
            })
    } else {
        name(text)
    }
}
fn text<'a>(raw: &'a [u8], budget: &Budget) -> Result<&'a str> {
    if raw.len() > 4 << 20 {
        return Err(identity());
    }
    let text = std::str::from_utf8(raw).map_err(|_| identity())?;
    budget.charge(Cost::metadata(raw.len()))?;
    Ok(text)
}
// Manifest table names are ASCII. Go strings.EqualFold uses Unicode simple
// folding: only long s and Kelvin sign join a fold class containing ASCII.
// Do not use upper/lower composition (which would also admit dotless i), full
// case folding (which expands sharp s), or compatibility normalization.
fn table_matches(selected: &str, canonical: &str) -> bool {
    if !canonical.is_ascii() {
        return false;
    }
    let mut selected = selected.chars();
    for expected in canonical.bytes() {
        let actual = match selected.next() {
            Some('\u{017f}') => b's',
            Some('\u{212a}') => b'k',
            Some(actual) if actual.is_ascii() => actual as u8,
            _ => return false,
        };
        if !actual.eq_ignore_ascii_case(&expected) {
            return false;
        }
    }
    selected.next().is_none()
}
fn parse_mappings(raw: &[u8], table: &str, budget: &Budget) -> Result<Vec<Mapping>> {
    let text = text(raw, budget)?;
    let mut mappings = Vec::new();
    for line in text.split_terminator('\n') {
        budget.check()?;
        let code = line.split_once("//").map_or(line, |(code, _)| code);
        // Only enough tokens to reject the grammar, even for a huge bad line.
        let parts = code.split_whitespace().take(5).collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let Some((selected, column)) = parts[1].split_once("::") else {
            continue;
        };
        if !table_matches(selected, table) || parts[0] == "COLOR" {
            continue;
        }
        if !(3..=4).contains(&parts.len())
            || !matches!(parts[0], "FLAGS" | "ENUM")
            || !name(parts[2])
            || !field(column)
        {
            return Err(identity());
        }
        let (mut condition_field, mut condition_value) = ("", "");
        if parts.len() == 4 {
            let (lhs, value) = parts[3].split_once('=').ok_or_else(identity)?;
            let (table, column) = lhs.split_once("::").ok_or_else(identity)?;
            if !name(table) || !field(column) || value.len() > 128 {
                return Err(identity());
            }
            condition_field = lhs;
            condition_value = value;
        }
        if mappings.len() >= 4096 {
            return Err(limit());
        }
        budget.charge(Cost {
            retained_bytes: 256,
            metadata_bytes: 256,
            decode_work: 1,
            ..Default::default()
        })?;
        mappings.push(Mapping {
            field: column.into(),
            kind: parts[0].into(),
            name: parts[2].into(),
            condition_field: condition_field.into(),
            condition_value: condition_value.into(),
        });
    }
    Ok(mappings)
}
fn number(text: &str) -> Result<Scalar> {
    let (base, text) = text
        .strip_prefix("0x")
        .map_or((10, text), |text| (16, text));
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty()
        || !digits.bytes().all(|byte| {
            if base == 16 {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return Err(identity());
    }
    if text.starts_with('-') {
        i64::from_str_radix(text, base)
            .map(Scalar::Int)
            .map_err(|_| identity())
    } else {
        u64::from_str_radix(text, base)
            .map(Scalar::UInt)
            .map_err(|_| identity())
    }
}
fn parse_values(raw: &[u8], build: &str, budget: &Budget) -> Result<Vec<Vec<Scalar>>> {
    let text = text(raw, budget)?;
    let mut rows = Vec::new();
    for line in text.split_terminator('\n') {
        budget.check()?;
        let (code, comment) = line.split_once("//").unwrap_or((line, ""));
        let mut code = code.trim();
        if code.is_empty() {
            continue;
        }
        if code.starts_with('(') {
            let (annotation, rest) = code.split_once(')').ok_or_else(identity)?;
            let constraints = annotation.strip_prefix("(BUILD ").ok_or_else(identity)?;
            if !schema::matches_build(build, constraints)? {
                continue;
            }
            code = rest.trim();
        }
        let parts = code.split_whitespace().take(3).collect::<Vec<_>>();
        if !(1..=2).contains(&parts.len()) {
            return Err(identity());
        }
        let value = number(parts[0])?;
        if rows.len() >= 100_000 {
            return Err(limit());
        }
        budget.charge(Cost {
            retained_bytes: 160,
            metadata_bytes: 160,
            decode_work: 1,
            ..Default::default()
        })?;
        rows.push(vec![
            value,
            parts
                .get(1)
                .map_or(Scalar::Null, |name| Scalar::String((*name).into())),
            Scalar::String(comment.trim().into()),
        ]);
    }
    Ok(rows)
}

pub(crate) fn source(
    home: &Path,
    commit: &str,
    build: &str,
    table: &str,
    offline: bool,
    budget: &Budget,
    control: &OperationControl,
) -> Result<(Source, SemanticBundle)> {
    budget.check()?;
    // The pinned table's ordinary definition is required even for empty meta.
    let (definition, _) = prepare_many_controlled(
        home,
        commit,
        &[table],
        offline,
        budget.deadline,
        Some(budget),
        Some(control),
    )?
    .remove(0);
    let table = definition.identity.name;
    let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/meta/");
    let mut mappings = Vec::new();
    let (mapping, _) = load(
        home,
        &format!("{base}mapping.dbdm"),
        offline,
        budget.deadline,
        Some(budget),
        Some(control),
        |raw| {
            mappings = parse_mappings(raw, &table, budget)?;
            Ok(())
        },
    )?;
    let mut bundle = SemanticBundle {
        table,
        commit: commit.into(),
        build: build.into(),
        mapping,
        definitions: BTreeMap::new(),
    };
    let mut cache: BTreeMap<String, Vec<Vec<Scalar>>> = BTreeMap::new();
    let mut rows = Vec::new();
    let mut retained = 0usize;
    for mapping in mappings {
        budget.check()?;
        let path = if mapping.kind == "FLAGS" {
            format!("flags/{}.dbdf", mapping.name)
        } else {
            format!("enums/{}.dbde", mapping.name)
        };
        if !cache.contains_key(&path) {
            if cache.len() >= 128 {
                return Err(limit());
            }
            let mut values = Vec::new();
            let (reference, raw) = load(
                home,
                &format!("{base}{path}"),
                offline,
                budget.deadline,
                Some(budget),
                Some(control),
                |raw| {
                    values = parse_values(raw, build, budget)?;
                    Ok(())
                },
            )?;
            retained = retained.checked_add(raw.len()).ok_or_else(limit)?;
            if retained > 16 << 20 {
                return Err(limit());
            }
            bundle.definitions.insert(path.clone(), reference);
            cache.insert(path.clone(), values);
        }
        for value in &cache[&path] {
            budget.check()?;
            if rows.len() >= 100_000 {
                return Err(limit());
            }
            // Go's fixed 256-byte row estimate is retained, with actual owned
            // strings admitted too before cloning repeated definitions.
            let strings = mapping.field.len()
                + mapping.kind.len()
                + mapping.name.len()
                + mapping.condition_field.len()
                + mapping.condition_value.len()
                + value
                    .iter()
                    .map(|value| match value {
                        Scalar::String(text) => text.len(),
                        _ => 0,
                    })
                    .sum::<usize>();
            budget.charge(Cost {
                retained_bytes: 256 + strings as u64,
                metadata_bytes: 256,
                decode_work: 1,
                ..Default::default()
            })?;
            rows.push(vec![
                Scalar::String(mapping.field.clone()),
                Scalar::String(mapping.kind.clone()),
                Scalar::String(mapping.name.clone()),
                value[0].clone(),
                value[1].clone(),
                value[2].clone(),
                Scalar::String(mapping.condition_field.clone()),
                Scalar::String(mapping.condition_value.clone()),
            ]);
        }
    }
    budget.check()?;
    Ok((
        Source {
            columns: [
                "Field",
                "Kind",
                "Definition",
                "Value",
                "Name",
                "Comment",
                "ConditionField",
                "ConditionValue",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            rows,
            complete: true,
        },
        bundle,
    ))
}

#[cfg(test)]
mod tests;
