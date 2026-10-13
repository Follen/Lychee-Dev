// SPDX-License-Identifier: MIT
// Three explicit listfile serializations; never infer a filename or merge origins.
use super::fault;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde::Serialize;
use std::collections::BTreeMap;

pub const COMPONENTS: [&str; 8] = [
    "listfile-id-index.dat",
    "listfile-strings.dat",
    "listfile-pf-models.dat",
    "listfile-pf-textures.dat",
    "listfile-pf-sounds.dat",
    "listfile-pf-videos.dat",
    "listfile-pf-text.dat",
    "listfile-pf-fonts.dat",
];
pub fn valid_kind(kind: &str) -> bool {
    ["community-csv", "wowexport-text", "wowexport-binary"].contains(&kind)
}
pub fn source_url(kind: &str, component: &str) -> Result<String> {
    Ok(match kind {
        "community-csv" => {
            "https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv"
                .into()
        }
        "wowexport-text" => "https://www.kruithne.net/wow.export/data/listfile/master".into(),
        "wowexport-binary" if COMPONENTS.contains(&component) => {
            format!("https://www.kruithne.net/wow.export/data/listfile/bin/{component}")
        }
        _ => return Err(fault("records.listfile_query", 2)),
    })
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub bytes: usize,
    pub entries: usize,
    pub line_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 64 << 20,
            entries: 4 << 20,
            line_bytes: 1 << 20,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let defaults = Self::default();
        let value = Self {
            bytes: if self.bytes == 0 {
                defaults.bytes
            } else {
                self.bytes
            },
            entries: if self.entries == 0 {
                defaults.entries
            } else {
                self.entries
            },
            line_bytes: if self.line_bytes == 0 {
                defaults.line_bytes
            } else {
                self.line_bytes
            },
        };
        if !(1024..=1 << 30).contains(&value.bytes)
            || !(1..=16 << 20).contains(&value.entries)
            || !(128..=16 << 20).contains(&value.line_bytes)
        {
            return Err(fault("records.listfile_limit", 3));
        }
        Ok(value)
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(rename = "fileDataID")]
    pub file_id: u32,
    pub file_name: String,
    pub source_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub component: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub matched_via: String,
}
pub struct Index {
    pub entries: Vec<Entry>,
    pub rejected: usize,
    pub total_bytes: usize,
}
pub fn lower(text: &str) -> String {
    text.chars()
        .map(|c| crate::unicode_simple::map(c, false))
        .collect()
}
pub fn normalize(text: &str) -> String {
    lower(&text.trim().replace('\\', "/"))
}
// Go's rune iteration replaces each invalid UTF-8 byte independently. This also
// preserves the JSON-visible sourceName behavior on tolerated malformed lines.
fn text(mut raw: &[u8]) -> String {
    let mut result = String::new();
    while !raw.is_empty() {
        match std::str::from_utf8(raw) {
            Ok(valid) => {
                result.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                result.push_str(std::str::from_utf8(&raw[..valid]).expect("valid prefix"));
                result.push('\u{fffd}');
                raw = &raw[valid + 1..];
            }
        }
    }
    result
}
impl Index {
    fn add(&mut self, id: u32, name: String, component: &str, limits: Limits) -> Result<()> {
        if id == 0 || name.is_empty() || name.contains('\0') {
            self.rejected += 1;
            return Ok(());
        }
        if self.entries.len() >= limits.entries {
            return Err(fault("records.listfile_limit", 3));
        }
        let normalized = normalize(&name);
        if normalized.is_empty() {
            self.rejected += 1;
            return Ok(());
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| fault("records.listfile_limit", 3))?;
        self.entries.push(Entry {
            file_id: id,
            file_name: normalized,
            source_name: name,
            component: component.into(),
            matched_via: String::new(),
        });
        Ok(())
    }
    pub fn names(&self, id: u32, control: &OperationControl) -> Result<Vec<Entry>> {
        let mut result = vec![];
        for (index, entry) in self.entries.iter().enumerate() {
            if index & 4095 == 0 {
                control.check()?;
            }
            if entry.file_id == id {
                result.push(entry.clone());
            }
        }
        Ok(result)
    }
    pub fn resolve(&self, name: &str, control: &OperationControl) -> Result<Vec<Entry>> {
        let normalized = normalize(name);
        let mut result = vec![];
        for (index, entry) in self.entries.iter().enumerate() {
            if index & 4095 == 0 {
                control.check()?;
            }
            if entry.file_name == normalized {
                result.push(entry.clone());
            }
        }
        if !result.is_empty() {
            return Ok(result);
        }
        if normalized.ends_with(".mdl") || normalized.ends_with(".mdx") {
            let alias = format!("{}.m2", &normalized[..normalized.len() - 4]);
            for (index, entry) in self.entries.iter().enumerate() {
                if index & 4095 == 0 {
                    control.check()?;
                }
                if entry.file_name == alias {
                    let mut found = entry.clone();
                    found.matched_via = "m2-alias".into();
                    result.push(found);
                }
            }
        }
        Ok(result)
    }
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        extension: bool,
        control: &OperationControl,
    ) -> Result<(usize, Vec<Entry>)> {
        let query = if extension {
            format!(
                ".{}",
                lower(query.trim().strip_prefix('.').unwrap_or(query.trim()))
            )
        } else {
            lower(query)
        };
        let mut total = 0;
        let mut found = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if index & 4095 == 0 {
                control.check()?;
            }
            let matched = if extension {
                entry.file_name.ends_with(&query)
            } else {
                entry.file_name.contains(&query)
            };
            if matched {
                total += 1;
                if extension || found.len() < limit {
                    found.push(entry);
                }
            }
        }
        if extension {
            found.sort_by(|a, b| {
                a.file_name
                    .cmp(&b.file_name)
                    .then(a.file_id.cmp(&b.file_id))
            });
            found.truncate(limit);
        }
        control.check()?;
        Ok((total, found.into_iter().cloned().collect()))
    }
}
fn format_error(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(
        "records.listfile_format",
        format!("records.listfile_format: {}", message.into()),
        4,
    )
}
pub fn parse(
    kind: &str,
    payloads: &BTreeMap<String, Vec<u8>>,
    limits: Limits,
    control: &OperationControl,
) -> Result<Index> {
    control.check()?;
    let limits = limits.validate()?;
    if !valid_kind(kind) {
        return Err(fault("records.listfile_query", 2));
    }
    let mut result = Index {
        entries: vec![],
        rejected: 0,
        total_bytes: 0,
    };
    if kind != "wowexport-binary" {
        let raw = payloads
            .get("listfile")
            .ok_or_else(|| format_error("missing text payload"))?;
        if raw.len() > limits.bytes {
            return Err(fault("records.listfile_limit", 3));
        }
        result.total_bytes = raw.len();
        let body = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
        for (index, line) in body.split(|byte| *byte == b'\n').enumerate() {
            if index & 4095 == 0 {
                control.check()?;
            }
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.len() > limits.line_bytes {
                result.rejected += 1;
                continue;
            }
            let line = text(line);
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(separator) = line.find(';').or_else(|| line.find(',')) else {
                result.rejected += 1;
                continue;
            };
            let raw_id = line[..separator].trim();
            let source_name = line[separator + 1..].trim();
            let id = raw_id.parse::<u32>().ok().filter(|id| *id != 0);
            if raw_id.is_empty()
                || !raw_id.bytes().all(|c| c.is_ascii_digit())
                || id.is_none()
                || source_name.is_empty()
            {
                result.rejected += 1;
                continue;
            }
            let source_name = if source_name.len() >= 2
                && source_name.starts_with('"')
                && source_name.ends_with('"')
            {
                source_name[1..source_name.len() - 1].replace("\"\"", "\"")
            } else {
                source_name.into()
            };
            if source_name.is_empty() {
                result.rejected += 1;
                continue;
            }
            result.add(id.unwrap(), source_name, "", limits)?;
        }
    } else {
        for (name, raw) in payloads {
            if name == "listfile-tree-nodes.dat" {
                continue;
            }
            result.total_bytes = result
                .total_bytes
                .checked_add(raw.len())
                .ok_or_else(|| fault("records.listfile_limit", 3))?;
        }
        if result.total_bytes > limits.bytes {
            return Err(fault("records.listfile_limit", 3));
        }
        let raw = payloads
            .get(COMPONENTS[0])
            .ok_or_else(|| format_error(format!("missing {}", COMPONENTS[0])))?;
        if raw.is_empty() || raw.len() % 9 != 0 {
            return Err(format_error(format!(
                "id index has invalid length {}",
                raw.len()
            )));
        }
        for name in &COMPONENTS[1..] {
            if !payloads.contains_key(*name) {
                return Err(format_error(format!("missing {name}")));
            }
        }
        for (index, row) in raw.as_chunks::<9>().0.iter().enumerate() {
            if index & 4095 == 0 {
                control.check()?;
            }
            let id = u32::from_be_bytes(row[..4].try_into().unwrap());
            if id == 0 {
                result.rejected += 1;
                continue;
            }
            let offset = u32::from_be_bytes(row[4..8].try_into().unwrap()) as usize;
            let pool = usize::from(row[8]);
            if pool >= COMPONENTS.len() - 1 {
                return Err(format_error(format!(
                    "file {id} references invalid component index {pool}"
                )));
            }
            let component = COMPONENTS[1 + pool];
            let data = &payloads[component];
            if offset >= data.len() {
                return Err(format_error(format!(
                    "file {id}: string offset out of range"
                )));
            }
            let Some(length) = data[offset..].iter().position(|byte| *byte == 0) else {
                return Err(format_error(format!("file {id}: unterminated string")));
            };
            result.add(id, text(&data[offset..offset + length]), component, limits)?;
        }
    }
    control.check()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parsed(raw: &[u8]) -> Index {
        parse(
            "community-csv",
            &BTreeMap::from([("listfile".into(), raw.to_vec())]),
            Limits::default(),
            &OperationControl::default(),
        )
        .unwrap()
    }
    #[test]
    fn text_retains_duplicates_quotes_original_names_and_marks_aliases() {
        let index=parsed(b"\xef\xbb\xbf# hi\r\n1;Creature\\Wolf.M2\n2;\"A, B.blp\"\n1;Other.BLP\n3;A, B.blp\n0;bad\ninvalid\n4;\"\"\n");
        assert_eq!(index.entries.len(), 4);
        assert_eq!(index.rejected, 3);
        let control = OperationControl::default();
        let names = index.names(1, &control).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].source_name, "Creature\\Wolf.M2");
        assert_eq!(
            index
                .resolve(" a, b.BLP ", &control)
                .unwrap()
                .iter()
                .map(|e| e.file_id)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            index.resolve("CREATURE/wolf.mdx", &control).unwrap()[0].matched_via,
            "m2-alias"
        );
        let (total, found) = index.search("BLP", 1, false, &control).unwrap();
        assert_eq!(total, 3);
        assert_eq!(found.len(), 1);
        let (total, found) = index.search(" .BLP ", 2, true, &control).unwrap();
        assert_eq!(total, 3);
        assert_eq!(found.iter().map(|e| e.file_id).collect::<Vec<_>>(), [2, 3]);
    }
    #[test]
    fn binary_offsets_pools_bounds_and_invalid_utf8_are_explicit() {
        let control = OperationControl::default();
        let mut files: BTreeMap<String, Vec<u8>> =
            COMPONENTS.iter().map(|s| ((*s).into(), vec![])).collect();
        files.insert(COMPONENTS[1].into(), b"Creature/Bear.m2\0".to_vec());
        let mut row = 7u32.to_be_bytes().to_vec();
        row.extend(0u32.to_be_bytes());
        row.push(0);
        files.insert(COMPONENTS[0].into(), row);
        let index = parse("wowexport-binary", &files, Limits::default(), &control).unwrap();
        assert_eq!(index.entries[0].component, COMPONENTS[1]);
        files.get_mut(COMPONENTS[0]).unwrap()[8] = 7;
        assert!(
            parse("wowexport-binary", &files, Limits::default(), &control)
                .err()
                .unwrap()
                .message
                .contains("invalid component index 7")
        );
        files.get_mut(COMPONENTS[0]).unwrap()[8] = 0;
        files.get_mut(COMPONENTS[1]).unwrap().pop();
        assert!(
            parse("wowexport-binary", &files, Limits::default(), &control)
                .err()
                .unwrap()
                .message
                .contains("unterminated string")
        );
        assert_eq!(
            parsed(b"1;\xe1\x80.BLP\n").entries[0].file_name,
            "\u{fffd}\u{fffd}.blp"
        );
        assert_eq!(lower("İΣ"), "iσ");
    }
    #[test]
    fn entry_limit_long_line_and_cancellation_do_not_silently_drop_valid_entries() {
        let control = OperationControl::default();
        let raw = BTreeMap::from([("listfile".into(), b"1;a\n2;b".to_vec())]);
        assert_eq!(
            parse(
                "wowexport-text",
                &raw,
                Limits {
                    entries: 1,
                    ..Limits::default()
                },
                &control
            )
            .err()
            .unwrap()
            .code,
            "records.listfile_limit"
        );
        let mut long = b"1;".to_vec();
        long.extend([b'x'; 200]);
        let files = BTreeMap::from([("listfile".into(), long)]);
        assert_eq!(
            parse(
                "wowexport-text",
                &files,
                Limits {
                    line_bytes: 128,
                    ..Limits::default()
                },
                &control
            )
            .unwrap()
            .rejected,
            1
        );
        control
            .token()
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            parse("community-csv", &raw, Limits::default(), &control)
                .err()
                .unwrap()
                .exit_code,
            7
        );
    }
}
