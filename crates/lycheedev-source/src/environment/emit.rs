use super::{ApiRecord, Gap, Mapping, Slot, budget};
use lycheedev_core::{Result, control::OperationControl};
use std::collections::{BTreeMap, BTreeSet};
struct Writer {
    text: String,
    line: usize,
    max: usize,
}
impl Writer {
    fn write(&mut self, line: &str) -> Result<()> {
        if self.text.len().saturating_add(line.len()).saturating_add(1) > self.max {
            return Err(budget());
        }
        self.text.push_str(line);
        self.text.push('\n');
        self.line += 1;
        Ok(())
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && ![
            "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
            "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
        ]
        .contains(&s)
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
}
fn type_name(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(identifier)
}
fn return_label(name: &str) -> String {
    // Return labels are metadata, not part of the type expression. In Emmy's
    // annotation grammar `number in` starts a binary type expression, so the
    // real GetNetStats return named `in` invalidated the entire API model.
    // Keep ordinary labels byte-identical and put non-name tokens behind the
    // description delimiter. The original name/ordinal/type remain in facts.
    if identifier(name) && !["keyof", "extends", "as"].contains(&name) {
        name.into()
    } else {
        format!("# {}", name.replace(['\r', '\n'], " "))
    }
}
fn issue(record: &ApiRecord, line: usize, code: &str, detail: &str) -> Gap {
    Gap {
        path: record.path.clone(),
        line,
        code: code.into(),
        detail: detail.into(),
    }
}
struct Types<'a> {
    known: BTreeSet<&'a str>,
    unknown: BTreeSet<String>,
    gaps: Vec<Gap>,
}
impl Types<'_> {
    fn slot(&mut self, record: &ApiRecord, slot: &Slot) -> String {
        let value = slot.type_name.trim();
        let mut value = match value {
            "bool" => "boolean".into(),
            "cstring" | "string" | "localizedstring" | "cstringOptional" => "string".into(),
            "number" | "float" | "double" => "number".into(),
            "int" | "integer" => "integer".into(),
            "function" | "boolean" | "unknown" | "any" | "nil" | "thread" | "userdata" => {
                value.into()
            }
            "table" if !slot.inner_type.is_empty() => {
                let mut inner = slot.clone();
                inner.type_name = slot.inner_type.clone();
                inner.inner_type.clear();
                inner.nilable = false;
                format!("{}[]", self.slot(record, &inner))
            }
            "table" => "table".into(),
            "" => {
                self.gaps
                    .push(issue(record, slot.line, "missing_slot_type", &slot.name));
                "unknown".into()
            }
            value => {
                if !type_name(value) || !self.known.contains(value) {
                    self.unknown.insert(value.into());
                    self.gaps
                        .push(issue(record, slot.line, "unknown_type", value));
                    "unknown".into()
                } else {
                    value.into()
                }
            }
        };
        if slot.nilable && value != "nil" {
            value.push_str("|nil");
        }
        value
    }
    fn parameter(&mut self, record: &ApiRecord, slot: &Slot) -> String {
        if identifier(&slot.name) {
            slot.name.clone()
        } else {
            self.gaps.push(issue(
                record,
                slot.line,
                "invalid_parameter_name",
                &slot.name,
            ));
            format!("arg{}", slot.index)
        }
    }
}
type Emitted = (String, Vec<Mapping>, Vec<String>, Vec<Gap>);
pub(super) fn definitions(
    records: &[ApiRecord],
    max: usize,
    control: &OperationControl,
) -> Result<Emitted> {
    let mut w = Writer {
        text: String::new(),
        line: 0,
        max,
    };
    for s in [
        "---@meta",
        "-- Fixed Blizzard API declarations. Unknown metadata remains in coverage/facts.",
        "---@alias UnitToken string",
        "---@alias UnitTokenVariant string",
        "---@alias luaIndex integer",
        "---@alias fileID integer",
        "---@alias textureID integer",
        "",
    ] {
        w.write(s)?;
    }
    let mut types = Types {
        known: [
            "UnitToken",
            "UnitTokenVariant",
            "luaIndex",
            "fileID",
            "textureID",
        ]
        .into_iter()
        .collect(),
        unknown: BTreeSet::new(),
        gaps: vec![],
    };
    for record in records {
        if ["structure", "enumeration", "callback", "callbacktype", "script-object"].contains(&record.kind.as_str())
            && record.declaration_role != "script-object-root"
            && type_name(&record.name)
        {
            types.known.insert(&record.name);
        }
    }
    let mut enum_namespace = false;
    for record in records {
        control.check()?;
        if ["function", "event"].contains(&record.kind.as_str()) {
            continue;
        }
        if record.declaration_role == "script-object-root" {
            // A documentation owner name does not establish its runtime type
            // name or inheritance. Keep the declaration in facts, not an empty
            // structural class that would accept arbitrary Lua tables.
            types.gaps.push(issue(record,record.line,"script_object_nominal_identity_unavailable",&record.name));
            continue;
        }
        if !type_name(&record.name) {
            types.gaps.push(issue(
                record,
                record.line,
                "invalid_type_name",
                &record.name,
            ));
            continue;
        }
        match record.kind.as_str() {
            "structure" | "script-object" => {
                w.write(&format!("---@class {}", record.name))?;
                for slot in &record.fields {
                    if !identifier(&slot.name) {
                        types
                            .gaps
                            .push(issue(record, slot.line, "invalid_field_name", &slot.name));
                        continue;
                    }
                    let t = types.slot(record, slot);
                    w.write(&format!("---@field {} {t}", slot.name))?;
                }
                w.write("")?;
            }
            "callback" | "callbacktype" => {
                let mut args = vec![];
                for slot in &record.parameters {
                    let name = types.parameter(record, slot);
                    let t = types.slot(record, slot);
                    args.push(format!("{name}:{t}"));
                }
                let returns: Vec<_> = record
                    .returns
                    .iter()
                    .map(|s| types.slot(record, s))
                    .collect();
                let mut alias = format!("---@alias {} fun({})", record.name, args.join(","));
                if !returns.is_empty() {
                    alias.push(':');
                    alias.push_str(&returns.join(","));
                }
                w.write(&alias)?;
                w.write("")?;
            }
            "enumeration" => {
                let valid = !record.values.is_empty()
                    && record.values.iter().all(|s| {
                        let raw = s
                            .raw
                            .get("EnumValue")
                            .filter(|v| v.is_number())
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        identifier(&s.name)
                            && !raw.is_empty()
                            && raw.bytes().all(|b| b.is_ascii_digit())
                            && (raw == "0" || !raw.starts_with('0'))
                    });
                w.write(&format!("---@alias {} integer", record.name))?;
                if !valid {
                    types.gaps.push(issue(
                        record,
                        record.line,
                        "enum_values_unknown",
                        &record.name,
                    ));
                    w.write("")?;
                    continue;
                }
                if !enum_namespace {
                    w.write("Enum = Enum or {}")?;
                    enum_namespace = true;
                }
                w.write(&format!("Enum.{} = {{", record.name))?;
                for slot in &record.values {
                    w.write(&format!("  {} = {},", slot.name, slot.raw["EnumValue"]))?;
                }
                w.write("}")?;
                w.write("")?;
            }
            _ => types.gaps.push(issue(
                record,
                record.line,
                "unsupported_record_kind",
                &record.kind,
            )),
        }
    }
    let mut counts = BTreeMap::new();
    for record in records.iter().filter(|r| r.kind == "function") {
        *counts.entry(&record.name).or_insert(0) += 1;
    }
    let mut namespaces = BTreeSet::new();
    let mut mappings = vec![];
    for record in records.iter().filter(|r| r.kind == "function") {
        control.check()?;
        if counts[&record.name] > 1 {
            types.gaps.push(issue(
                record,
                record.line,
                "ambiguous_callable",
                &record.name,
            ));
            continue;
        }
        let parts: Vec<_> = record.name.split('.').collect();
        if !(1..=2).contains(&parts.len()) || !parts.iter().all(|s| identifier(s)) {
            types
                .gaps
                .push(issue(record, record.line, "invalid_callable", &record.name));
            continue;
        }
        if parts.len() == 2 && namespaces.insert(parts[0]) {
            w.write(&format!("{} = {} or {{}}", parts[0], parts[0]))?;
        }
        let mut args = vec![];
        for slot in &record.parameters {
            let name = types.parameter(record, slot);
            let t = types.slot(record, slot);
            w.write(&format!("---@param {name} {t}"))?;
            args.push(name);
        }
        for slot in &record.returns {
            let t = types.slot(record, slot);
            w.write(&format!(
                "---@return {t} {}",
                return_label(&slot.name)
            ))?;
        }
        if record
            .raw
            .get("MayReturnNothing")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            types.gaps.push(issue(
                record,
                record.line,
                "may_return_nothing",
                "LuaLS annotation cannot express zero returns separately from nil",
            ));
        }
        mappings.push(Mapping {
            callable: record.name.clone(),
            source_path: record.path.clone(),
            source_line: record.line,
            generated_line: w.line,
            generated_utf16_column: 9 + if parts.len() == 2 {
                parts[0].len() + 1
            } else {
                0
            },
            kind: "function".into(),
        });
        w.write(&format!(
            "function {}({}) end",
            record.name,
            args.join(", ")
        ))?;
        w.write("")?;
    }
    Ok((
        w.text,
        mappings,
        types.unknown.into_iter().collect(),
        types.gaps,
    ))
}
