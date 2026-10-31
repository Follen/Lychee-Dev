//! Fixed Go 3.2 semantic metadata: pinned enum/flag definitions are rows in
//! meta.Table. Applicability conditions stay visible and never filter DB2 rows.
use super::*;
use crate::{
    query_value::{Scalar, Source},
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
#[cfg(test)]
fn parse_mappings(raw: &[u8], table: &str, budget: &Budget) -> Result<Vec<Mapping>> {
    parse_mappings_with_check(raw, table, budget, || Ok(()))
}
fn parse_mappings_with_check(
    raw: &[u8],
    table: &str,
    budget: &Budget,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Vec<Mapping>> {
    check()?;

    let text = text(raw, budget)?;
    let mut mappings = Vec::new();
    for line in text.split_terminator('\n') {
        check()?;
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
    check()?;
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
// WoWDBDefs b9a63096cdd99b26967f17f9515377ae62cadfd7,
// code/C#/DBDefsLib/BuildRange.cs: presetRanges and Contains. These are metadata
// version selectors, never toolkit products or aliases. Presets use upstream's
// independent expansion/major/build bounds (minor is ignored); ordinary numeric
// selectors retain the existing schema::matches_build contract.
fn matches_metadata_build(build: &str, constraints: &str) -> Result<bool> {
    let version = schema::version(build)?;
    let mut matched = false;
    for constraint in constraints.split(',').map(str::trim) {
        let preset = match constraint {
            "Pre" => Some(([0, 3, 4, 2953], [0, 12, 0, 3988])),
            "Vanilla" => Some(([1, 0, 0, 3980], [1, 12, 3, 6141])),
            "TBC" => Some(([2, 0, 0, 5610], [2, 4, 3, 8606])),
            "WotLK" => Some(([3, 0, 0, 8120], [3, 3, 5, 12340])),
            "Cata" => Some(([4, 0, 0, 11927], [4, 3, 4, 15595])),
            "MoP" => Some(([5, 0, 1, 15464], [5, 4, 8, 18414])),
            "WoD" => Some(([6, 0, 1, 18125], [6, 2, 4, 21742])),
            "Legion" => Some(([7, 0, 1, 20740], [7, 3, 5, 26972])),
            "BfA" => Some(([8, 0, 1, 25703], [8, 3, 7, 35662])),
            "SL" => Some(([9, 0, 1, 33411], [9, 2, 7, 45745])),
            "DF" => Some(([10, 0, 0, 43342], [10, 2, 7, 55664])),
            "TWW" => Some(([11, 0, 0, 54311], [11, 2, 7, 65299])),
            _ => None,
        };
        // Validate every selector, even after a match, so malformed trailing
        // constraints cannot disappear through short-circuit evaluation.
        matched |= if let Some((low, high)) = preset {
            [0, 1, 3]
                .into_iter()
                .all(|index| version[index] >= low[index] && version[index] <= high[index])
        } else {
            schema::matches_build(build, constraint)?
        };
    }
    Ok(matched)
}
#[cfg(test)]
fn parse_values(raw: &[u8], build: &str, budget: &Budget) -> Result<Vec<Vec<Scalar>>> {
    parse_values_with_check(raw, build, budget, || Ok(()))
}
fn parse_values_with_check(
    raw: &[u8],
    build: &str,
    budget: &Budget,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Vec<Vec<Scalar>>> {
    check()?;

    let text = text(raw, budget)?;
    let mut rows = Vec::new();
    for line in text.split_terminator('\n') {
        check()?;
        budget.check()?;
        let (code, comment) = line.split_once("//").unwrap_or((line, ""));
        let mut code = code.trim();
        if code.is_empty() {
            continue;
        }
        if code.starts_with('(') {
            let (annotation, rest) = code.split_once(')').ok_or_else(identity)?;
            let constraints = annotation.strip_prefix("(BUILD ").ok_or_else(identity)?;
            if !matches_metadata_build(build, constraints)? {
                continue;
            }
            code = rest.trim();
        }
        // DBDE/DBDF Name is the whole remainder, including internal whitespace,
        // punctuation and a possible trailing '?'. Mapping tokens stay strict.
        let (value, name) = code
            .split_once(char::is_whitespace)
            .map_or((code, ""), |(value, name)| (value, name.trim_start()));
        let value = number(value)?;
        if rows.len() >= 100_000 {
            return Err(limit());
        }
        let comment = comment.trim();
        // Keep the fixed logical row estimate, but admit the actual owned text
        // before allocating it. The raw input remains separately accounted.
        budget.charge_split(
            Cost {
                retained_bytes: 160,
                metadata_bytes: 160,
                decode_work: 1,
                ..Default::default()
            },
            Cost {
                retained_bytes: 160 + name.len() as u64 + comment.len() as u64,
                metadata_bytes: 160,
                decode_work: 1,
                ..Default::default()
            },
        )?;
        rows.push(vec![
            value,
            if name.is_empty() {
                Scalar::Null
            } else {
                Scalar::String(name.into())
            },
            Scalar::String(comment.into()),
        ]);
    }
    check()?;
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
    check_definition(budget.deadline, Some(budget), control)?;
    // The pinned table's ordinary definition is required even for empty meta.
    let (definition, _) = prepare_many_controlled(
        home,
        commit,
        &[table],
        offline,
        budget.deadline,
        Some(budget),
        control,
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
            mappings = parse_mappings_with_check(raw, &table, budget, || control.check())?;
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
        check_definition(budget.deadline, Some(budget), control)?;
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
                    values = parse_values_with_check(raw, build, budget, || control.check())?;
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
            check_definition(budget.deadline, Some(budget), control)?;
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
            budget.charge_split(
                Cost {
                    retained_bytes: 256,
                    metadata_bytes: 256,
                    decode_work: 1,
                    ..Default::default()
                },
                Cost {
                    retained_bytes: 256 + strings as u64,
                    metadata_bytes: 256,
                    decode_work: 1,
                    ..Default::default()
                },
            )?;
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
    check_definition(budget.deadline, Some(budget), control)?;
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

#[cfg(test)]
#[path = "semantics/metadata_compatibility_tests.rs"]
mod metadata_compatibility_tests;
