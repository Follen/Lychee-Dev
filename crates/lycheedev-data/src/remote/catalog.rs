// SPDX-License-Identifier: MIT
//! Retained remote-catalog contract: selected-region validation and immutable
//! full-build selection, separate from the broader availability listing API.
use super::{ambiguous, invalid, limit, unavailable};
use lycheedev_core::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

pub const MAX_BYTES: usize = 1 << 20;
const MAX_COLUMNS: usize = 128;
const MAX_ROWS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub full_build: String,
    pub build_config: String,
    #[serde(rename = "cdnConfig")]
    pub cdn_config: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distribution {
    pub path: String,
    pub hosts: Vec<String>,
}
impl Distribution {
    pub fn validate(&self) -> Result<()> {
        if !valid_path(&self.path) || self.hosts.is_empty() || self.hosts.len() > 16 {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        if self
            .hosts
            .iter()
            .any(|host| !valid_host(host) || !seen.insert(host.to_ascii_lowercase()))
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn config_url(&self, host: &str, key: &str) -> Result<String> {
        self.validate()?;
        if !self.hosts.iter().any(|value| value == host) || !valid_key(key) {
            return Err(invalid());
        }
        Ok(format!(
            "https://{host}/{}/config/{}/{}/{key}",
            self.path,
            &key[..2],
            &key[2..4]
        ))
    }
    pub(crate) fn data_url(&self, host: &str, object: &str) -> Result<String> {
        self.validate()?;
        if !self.hosts.iter().any(|value| value == host)
            || !valid_key(object.strip_suffix(".index").unwrap_or(object))
        {
            return Err(invalid());
        }
        Ok(format!(
            "https://{host}/{}/data/{}/{}/{object}",
            self.path,
            &object[..2],
            &object[2..4]
        ))
    }
}

struct Bpsv<'a> {
    columns: BTreeMap<&'a str, usize>,
    rows: Vec<Vec<&'a str>>,
}
fn parse<'a>(raw: &'a [u8], required: &[&str]) -> Result<Bpsv<'a>> {
    if raw.len() > MAX_BYTES {
        return Err(limit());
    }
    let text = std::str::from_utf8(raw).map_err(|_| invalid())?;
    if text.contains('\0') {
        return Err(invalid());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut columns = BTreeMap::new();
    let mut header_len = None;
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.contains('\r') {
            return Err(invalid());
        }
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }
        if let Some(count) = header_len {
            if rows.len() == MAX_ROWS {
                return Err(limit());
            }
            let values: Vec<_> = line.split('|').map(str::trim).collect();
            if values.len() != count {
                return Err(invalid());
            }
            rows.push(values);
        } else {
            let parts: Vec<_> = line.split('|').collect();
            if parts.len() > MAX_COLUMNS {
                return Err(limit());
            }
            for (index, part) in parts.iter().enumerate() {
                let (name, spec) = part.trim().split_once('!').ok_or_else(invalid)?;
                let name = name.trim();
                let (kind, width) = spec.trim().split_once(':').ok_or_else(invalid)?;
                if name.is_empty()
                    || kind.is_empty()
                    || !kind
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    || !decimal(width)
                    || columns.insert(name, index).is_some()
                {
                    return Err(invalid());
                }
            }
            if required.iter().any(|key| !columns.contains_key(key)) {
                return Err(invalid());
            }
            header_len = Some(parts.len());
        }
    }
    if header_len.is_none() {
        return Err(invalid());
    }
    Ok(Bpsv { columns, rows })
}

pub fn release(raw: &[u8], region: &str, expected_build: &str) -> Result<Release> {
    let table = parse(
        raw,
        &[
            "Region",
            "BuildConfig",
            "CDNConfig",
            "BuildId",
            "VersionsName",
        ],
    )?;
    let region_col = table.columns["Region"];
    let build_col = table.columns["VersionsName"];
    let rows: Vec<_> = table
        .rows
        .iter()
        .filter(|row| !region.is_empty() && row[region_col] == region)
        .collect();
    if rows.is_empty() {
        return Err(unavailable());
    }
    let row = if !expected_build.is_empty() {
        let mut exact = rows
            .iter()
            .copied()
            .filter(|row| row[build_col] == expected_build);
        let selected = exact.next().ok_or_else(unavailable)?;
        if exact.next().is_some() {
            return Err(ambiguous());
        }
        selected
    } else if rows.len() == 1 {
        rows[0]
    } else {
        let mut current = None;
        let mut greatest = [0u32; 4];
        let mut tied = false;
        for row in rows {
            let Some(build) = build_components(row[build_col]) else {
                continue;
            };
            if current.is_none() || build > greatest {
                current = Some(row);
                greatest = build;
                tied = false;
            } else if build == greatest {
                tied = true;
            }
        }
        if tied {
            return Err(ambiguous());
        }
        current.ok_or_else(ambiguous)?
    };
    let full_build = row[build_col];
    let build_config = row[table.columns["BuildConfig"]];
    let cdn_config = row[table.columns["CDNConfig"]];
    let build_id = row[table.columns["BuildId"]];
    let parsed = build_components(full_build).ok_or_else(invalid)?;
    if !valid_key(build_config)
        || !valid_key(cdn_config)
        || !decimal(build_id)
        || build_id.parse::<u32>().ok() != Some(parsed[3])
    {
        return Err(invalid());
    }
    Ok(Release {
        full_build: full_build.into(),
        build_config: build_config.into(),
        cdn_config: cdn_config.into(),
    })
}

pub fn distribution(raw: &[u8], region: &str) -> Result<Distribution> {
    let table = parse(raw, &["Name", "Path", "Hosts"])?;
    let mut matches = table
        .rows
        .iter()
        .filter(|row| !region.is_empty() && row[table.columns["Name"]] == region);
    let selected = matches.next().ok_or_else(unavailable)?;
    if matches.next().is_some() {
        return Err(ambiguous());
    }
    let route = Distribution {
        path: selected[table.columns["Path"]].into(),
        hosts: selected[table.columns["Hosts"]]
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    };
    route.validate()?;
    Ok(route)
}

pub(crate) fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}
pub(crate) fn valid_key(value: &str) -> bool {
    lycheedev_core::selection::valid_digest(value, 32)
}
pub(crate) fn build_components(value: &str) -> Option<[u32; 4]> {
    let mut result = [0; 4];
    let mut parts = value.split('.');
    for field in &mut result {
        let part = parts.next()?;
        if !decimal(part) {
            return None;
        }
        *field = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(result)
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        })
}
fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.parse::<IpAddr>().is_ok() {
        return false;
    }
    let labels: Vec<_> = host.split('.').collect();
    labels.len() >= 2
        && !labels
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
        && labels.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part.as_bytes()[0].is_ascii_alphanumeric()
                && part.as_bytes()[part.len() - 1].is_ascii_alphanumeric()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests;
