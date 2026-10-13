// SPDX-License-Identifier: MIT
// DBD contracts ported from Lychee Dev's MIT wowdata-derived implementation.
use crate::fault;
use lycheedev_core::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub name: String,
    pub kind: String,
    pub foreign_table: String,
    pub foreign_column: String,
    pub verified: bool,
    pub bits: u16,
    pub signed: bool,
    pub elements: u32,
    pub array: bool,
    pub identity: bool,
    pub inline: bool,
    pub relation: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub sha256: String,
    pub r#match: String,
    pub build: String,
    pub layout: String,
    pub fields: Vec<Field>,
}
#[derive(Debug)]
struct Variant {
    builds: Vec<([u32; 4], [u32; 4])>,
    layouts: Vec<String>,
    fields: Vec<Field>,
}
#[derive(Debug)]
pub struct Document {
    digest: String,
    variants: Vec<Variant>,
}
fn invalid<T>() -> Result<T> {
    Err(fault(
        "schema.invalid_definition",
        4,
        "Invalid DBD definition",
    ))
}
fn limit<T>() -> Result<T> {
    Err(fault(
        "schema.limit_exceeded",
        3,
        "DBD resource limit exceeded",
    ))
}
pub fn version(value: &str) -> Result<[u32; 4]> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 4 {
        return invalid();
    }
    let mut out = [0; 4];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return invalid();
        }
        out[i] = p
            .parse()
            .map_err(|_| fault("schema.invalid_definition", 4, "Invalid full build"))?;
    }
    Ok(out)
}
fn layout(value: &str) -> Result<String> {
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return invalid();
    }
    Ok(value.to_ascii_uppercase())
}
fn intervals(value: &str) -> Result<Vec<([u32; 4], [u32; 4])>> {
    let mut out = Vec::new();
    for p in value.split(',') {
        let ends: Vec<_> = p.trim().split('-').collect();
        if ends.len() > 2 {
            return invalid();
        }
        let low = version(ends[0].trim())?;
        let high = if ends.len() == 2 {
            version(ends[1].trim())?
        } else {
            low
        };
        if low > high {
            return invalid();
        }
        out.push((low, high));
    }
    Ok(out)
}
pub fn matches_build(build: &str, constraints: &str) -> Result<bool> {
    let v = version(build)?;
    Ok(intervals(constraints)?
        .iter()
        .any(|(l, h)| v >= *l && v <= *h))
}
impl Document {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 4 << 20 {
            return limit();
        }
        if raw.is_empty() || raw.contains(&0) {
            return invalid();
        }
        let text = std::str::from_utf8(raw)
            .map_err(|_| fault("schema.invalid_definition", 4, "DBD must be UTF-8"))?;
        let mut chunks = Vec::<Vec<&str>>::new();
        let mut chunk = Vec::new();
        for rawline in text.split('\n') {
            if rawline.len() >= 64 << 10 {
                return limit();
            }
            let line = rawline.trim();
            if line.is_empty() {
                if !chunk.is_empty() {
                    chunks.push(std::mem::take(&mut chunk));
                }
                continue;
            }
            let line = line.split("//").next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            chunk.push(line);
            if chunk.len() > 16384 {
                return limit();
            }
        }
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        static COLUMN: OnceLock<Regex> = OnceLock::new();
        static FIELD: OnceLock<Regex> = OnceLock::new();
        let col=COLUMN.get_or_init(||Regex::new(r"^(int|float|string|locstring)(?:<([A-Za-z_][A-Za-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)>)?\s+([A-Za-z_][A-Za-z0-9_]*)(\?)?$").unwrap());
        let field=FIELD.get_or_init(||Regex::new(r"^(?:\$([^$]+)\$)?([A-Za-z_][A-Za-z0-9_]*)(?:<([uf]?)([0-9]+)>)?(?:\[([0-9]+)\])?$").unwrap());
        let mut columns = BTreeMap::<String, Field>::new();
        let mut variants = Vec::new();
        for chunk in chunks {
            if chunk[0] == "COLUMNS" {
                if !columns.is_empty() || !variants.is_empty() || chunk.len() == 1 {
                    return invalid();
                }
                if chunk.len() > 4097 {
                    return limit();
                }
                for line in &chunk[1..] {
                    let c = col.captures(line).ok_or_else(|| {
                        fault("schema.invalid_definition", 4, "Invalid DBD column")
                    })?;
                    let name = c[4].to_string();
                    let f = Field {
                        name: name.clone(),
                        kind: c[1].into(),
                        foreign_table: c.get(2).map_or("", |m| m.as_str()).into(),
                        foreign_column: c.get(3).map_or("", |m| m.as_str()).into(),
                        verified: c.get(5).is_none(),
                        bits: 0,
                        signed: true,
                        elements: 1,
                        array: false,
                        identity: false,
                        inline: true,
                        relation: false,
                    };
                    if columns.insert(name, f).is_some() {
                        return invalid();
                    }
                }
                continue;
            }
            if columns.is_empty() {
                return invalid();
            }
            if variants.len() >= 4096 {
                return limit();
            }
            let mut v = Variant {
                builds: vec![],
                layouts: vec![],
                fields: vec![],
            };
            let mut seen = BTreeSet::new();
            let mut identity = false;
            let mut layout_seen = false;
            for line in chunk {
                if line.starts_with("COMMENT ") {
                    continue;
                }
                if let Some(value) = line.strip_prefix("BUILD ") {
                    if !v.fields.is_empty() {
                        return invalid();
                    }
                    v.builds.extend(intervals(value)?);
                    continue;
                }
                if let Some(value) = line.strip_prefix("LAYOUT ") {
                    if layout_seen || !v.fields.is_empty() {
                        return invalid();
                    }
                    layout_seen = true;
                    for key in value.split(',') {
                        v.layouts.push(layout(key.trim())?);
                    }
                    continue;
                }
                let c = field
                    .captures(line)
                    .ok_or_else(|| fault("schema.invalid_definition", 4, "Invalid DBD field"))?;
                let mut f = columns
                    .get(&c[2])
                    .cloned()
                    .ok_or_else(|| fault("schema.invalid_definition", 4, "Unknown DBD column"))?;
                if !seen.insert(f.name.clone()) {
                    return invalid();
                }
                if let Some(a) = c.get(1) {
                    let mut annotations = BTreeSet::new();
                    for a in a.as_str().split(',').map(str::trim) {
                        if !annotations.insert(a) {
                            return invalid();
                        }
                        match a {
                            "id" => f.identity = true,
                            "noninline" => f.inline = false,
                            "relation" => f.relation = true,
                            _ => return invalid(),
                        }
                    }
                }
                if f.identity {
                    if identity || f.kind != "int" {
                        return invalid();
                    }
                    identity = true;
                }
                if !f.inline && ((!f.identity && !f.relation) || f.kind != "int") {
                    return invalid();
                }
                if let Some(b) = c.get(4) {
                    f.bits = b.as_str().parse().map_err(|_| {
                        fault("schema.invalid_definition", 4, "Invalid field width")
                    })?;
                    if ![8, 16, 32, 64].contains(&f.bits) {
                        return invalid();
                    }
                    let prefix = c.get(3).map_or("", |m| m.as_str());
                    match f.kind.as_str() {
                        "int" => {
                            if prefix == "f" {
                                return invalid();
                            }
                            f.signed = prefix != "u";
                        }
                        "float" => {
                            if f.bits != 32 || prefix == "u" {
                                return invalid();
                            }
                        }
                        _ => return invalid(),
                    }
                } else {
                    if f.kind == "int" && f.inline {
                        return invalid();
                    }
                    if f.kind == "int" || f.kind == "float" {
                        f.bits = 32;
                    }
                }
                if let Some(n) = c.get(5) {
                    f.elements = n
                        .as_str()
                        .parse()
                        .map_err(|_| fault("schema.limit_exceeded", 3, "Invalid array width"))?;
                    if f.elements == 0 || f.elements > 65536 {
                        return limit();
                    }
                    f.array = true;
                }
                if !f.inline && f.array {
                    return invalid();
                }
                v.fields.push(f);
                if v.fields.len() > 4096 {
                    return limit();
                }
            }
            if v.fields.is_empty() || (v.builds.is_empty() && v.layouts.is_empty()) {
                return invalid();
            }
            variants.push(v);
        }
        if columns.is_empty() || variants.is_empty() {
            return invalid();
        }
        Ok(Self {
            digest: format!("{:x}", Sha256::digest(raw)),
            variants,
        })
    }
    pub fn select(&self, build: &str, layout_key: &str) -> Result<Definition> {
        let ver = version(build)?;
        let layout_key = if layout_key.is_empty() {
            String::new()
        } else {
            layout(layout_key)?
        };
        let mut selected = None;
        for v in &self.variants {
            let matched = if layout_key.is_empty() {
                v.builds.iter().any(|(l, h)| ver >= *l && ver <= *h)
            } else {
                v.layouts.contains(&layout_key)
            };
            if matched {
                if selected.is_some() {
                    return Err(fault(
                        "schema.ambiguous_definition",
                        4,
                        "Multiple DBD variants match",
                    ));
                }
                selected = Some(v);
            }
        }
        let v = selected
            .ok_or_else(|| fault("schema.definition_not_found", 3, "No exact DBD variant"))?;
        Ok(Definition {
            sha256: self.digest.clone(),
            r#match: if layout_key.is_empty() {
                "build"
            } else {
                "layout"
            }
            .into(),
            build: build.into(),
            layout: layout_key,
            fields: v.fields.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_and_authoritative_layout() {
        let d=Document::parse(b"COLUMNS\nint ID\nstring Name\nint<Other::ID> Parent?\n\nBUILD 12.1.0.69875-12.1.0.69900\nLAYOUT 1234abcd\n$id,noninline$ID\nName\nParent<u32>[2]\n").unwrap();
        let a = d.select("12.1.0.69875", "").unwrap();
        assert!(!a.fields[0].inline);
        assert_eq!(a.fields[2].foreign_table, "Other");
        assert!(!a.fields[2].verified);
        assert_eq!(d.select("1.2.3.4", "1234ABCD").unwrap().r#match, "layout");
        assert_eq!(
            d.select("12.1.0.69875", "00000000").unwrap_err().code,
            "schema.definition_not_found"
        );
    }
    #[test]
    fn invalid_duplicates_and_widths() {
        for raw in [
            "COLUMNS\nint ID\nint ID\n\nBUILD 1.2.3.4\nID<32>",
            "COLUMNS\nint ID\n\nBUILD 1.2.3.4\nID",
            "COLUMNS\nstring Name\n\nBUILD 1.2.3.4\nName<32>",
        ] {
            assert!(Document::parse(raw.as_bytes()).is_err());
        }
    }
    #[test]
    fn ambiguous_and_ranges() {
        let d =
            Document::parse(b"COLUMNS\nint ID\n\nBUILD 1.2.3.4\nID<32>\n\nBUILD 1.2.3.4\nID<32>")
                .unwrap();
        assert_eq!(
            d.select("1.2.3.4", "").unwrap_err().code,
            "schema.ambiguous_definition"
        );
        assert!(matches_build("1.2.3.9", "1.2.3.4-1.2.3.10").unwrap());
        assert!(matches_build("1.2.3.9", "1.2.3.10-1.2.3.4").is_err());
    }
}
