use super::*;
use crate::{input::invalid, local_backup, zip_contract};
use lycheedev_core::storage;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub(super) const MAX_ZIP: u64 = 200 * 1024 * 1024;
pub(super) fn open(path: &Path, control: &OperationControl) -> Result<(File, Value)> {
    let (mut file, sha256) = storage::locked_sha256(path, MAX_ZIP, control)?;
    let result = (|| {
        let md5 = local_backup::md5(&mut file, control)?;
        let mut raw = file.try_clone()?;
        let mut archive = zip::ZipArchive::new(&mut file)
            .map_err(|_| invalid("$.file", "file must be an addon ZIP"))?;
        let mut paths = zip_contract::Paths::default();
        let mut groups: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
        let mut unzip = 0u64;
        for i in 0..archive.len() {
            control.check()?;
            let (name, local, central, directory, encrypted, size) = {
                let m = archive
                    .by_index_raw(i)
                    .map_err(|_| invalid("$.file", "invalid ZIP member"))?;
                (
                    m.name_raw().to_vec(),
                    m.header_start(),
                    m.central_header_start(),
                    m.is_dir(),
                    m.encrypted(),
                    m.size(),
                )
            };
            let parts = paths.admit(&mut raw, &name, local, central, directory, encrypted)?;
            unzip = unzip
                .checked_add(size)
                .filter(|n| *n <= 5_000_000_000)
                .ok_or_else(|| invalid("$.file", "uncompressed addon ZIP exceeds 5 GB"))?;
            if directory {
                continue;
            }
            let name = parts.join("/").to_lowercase();
            let toc = name.ends_with(".toc")
                && !parts[..parts.len() - 1]
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case("libs"));
            let mut member = archive
                .by_index(i)
                .map_err(|_| invalid("$.file", "cannot decode ZIP member"))?;
            let mut bytes = Vec::new();
            let mut count = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                control.check()?;
                let n = member
                    .read(&mut buffer)
                    .map_err(|_| invalid("$.file", "ZIP member failed CRC/decompression"))?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                if count > size {
                    return Err(invalid("$.file", "ZIP member exceeds its declared size"));
                }
                if toc {
                    bytes.extend_from_slice(&buffer[..n]);
                }
            }
            if count != size {
                return Err(invalid("$.file", "ZIP member size is incomplete"));
            }
            if toc {
                let text = decode(&bytes)?;
                let mut values = BTreeSet::new();
                for line in text.trim_start_matches('\u{feff}').lines() {
                    if let Some((key, value)) = line
                        .trim()
                        .strip_prefix("##")
                        .and_then(|s| s.split_once(':'))
                    {
                        if key.trim().eq_ignore_ascii_case("Interface") {
                            values.extend(interfaces(value)?);
                        }
                    }
                }
                if values.is_empty() {
                    return Err(invalid("$.file", "addon TOC is missing Interface"));
                }
                let mut group = name.trim_end_matches(".toc").to_owned();
                if let Some((prefix, flavor)) = group.rsplit_once('_') {
                    if [
                        "mainline", "vanilla", "classic", "bcc", "tbc", "wrath", "cata", "mists",
                    ]
                    .contains(&flavor)
                    {
                        group = prefix.into();
                    }
                }
                groups.entry(group).or_default().extend(values);
            }
        }
        let first = groups
            .values()
            .next()
            .ok_or_else(|| invalid("$.file", "ZIP has no addon TOC outside Libs"))?;
        if groups.values().any(|v| v != first) {
            return Err(invalid(
                "$.file",
                "independent addons must declare the same Interface union",
            ));
        }
        let toc = first
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let games = derive(&toc)?;
        drop(archive);
        drop(raw);
        file.seek(SeekFrom::Start(0))?;
        Ok(
            json!({"sha256":sha256,"md5":md5,"zip_size":file.metadata()?.len(),"unzip_size":unzip,"toc_version":toc,"derived_games":games}),
        )
    })();
    match result {
        Ok(info) => Ok((file, info)),
        Err(e) => storage::finish_file(file, Err(e), control),
    }
}
fn decode(bytes: &[u8]) -> Result<String> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.into());
    }
    // Windows-1252 is the Creator's one supported legacy TOC encoding.
    const CP: &[u32; 32] = &[
        0x20ac, 0, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0, 0x17d, 0, 0, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc,
        0x2122, 0x161, 0x203a, 0x153, 0, 0x17e, 0x178,
    ];
    bytes
        .iter()
        .map(|b| {
            let c = if (0x80..=0x9f).contains(b) {
                CP[usize::from(*b - 0x80)]
            } else {
                u32::from(*b)
            };
            if c == 0 && *b != 0 {
                Err(invalid("$.file", "TOC is neither UTF-8 nor Windows-1252"))
            } else {
                Ok(char::from_u32(c).unwrap())
            }
        })
        .collect()
}
fn interfaces(text: &str) -> Result<BTreeSet<u64>> {
    let mut values = BTreeSet::new();
    for token in text
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        if !token.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid(
                "$.toc_version",
                "Interface values must be decimal codes",
            ));
        }
        values.insert(
            token
                .parse()
                .map_err(|_| invalid("$.toc_version", "Interface code is out of range"))?,
        );
    }
    if values.is_empty() {
        return Err(invalid("$.toc_version", "Interface codes are required"));
    }
    Ok(values)
}
pub(super) fn derive(toc: &str) -> Result<Value> {
    let mut games = Vec::new();
    for code in interfaces(toc)? {
        let (version, server) = match code {
            110000 => ("11.0.0", "wow_retail"),
            110002 => ("11.0.2", "wow_retail"),
            111000 => ("11.1.0", "wow_retail"),
            111005 => ("11.1.5", "wow_retail"),
            120100 => ("12.1.0", "wow_retail"),
            120105 => ("12.1.5", "wow_retail"),
            11506..=11508 => ("Classic Era", "wow_classic_era"),
            11509 => ("1.15.9", "wow_classic_era"),
            40401 => ("Cataclysm Classic", "wow_classic_cata"),
            40402 => ("4.4.2", "wow_classic_cata"),
            16001 => ("1.60.1", "wow_forever"),
            20506 => ("2.5.6", "wow_anniversary"),
            38002 => ("3.80.2", "wow_classic_titan"),
            50504 => ("5.5.4", "wow_classic"),
            30405 => continue,
            _ => {
                return Err(invalid(
                    "$.file",
                    "TOC Interface has no confirmed ModUs mapping; no game branch was guessed",
                ));
            }
        };
        let game = json!({"gameVersion":version,"server":server});
        if !games.contains(&game) {
            games.push(game);
        }
    }
    Ok(json!(games))
}
pub(super) fn games(value: &Value) -> Result<Value> {
    let rows = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
        invalid(
            "$.supported_game_versions",
            "select a nonempty game-version array",
        )
    })?;
    let mut out = Vec::new();
    for row in rows {
        let obj = row
            .as_object()
            .filter(|o| o.len() == 2 && o.contains_key("game_version") && o.contains_key("server"))
            .ok_or_else(|| {
                invalid(
                    "$.supported_game_versions",
                    "each choice requires only game_version and server",
                )
            })?;
        if obj
            .values()
            .any(|v| !v.as_str().is_some_and(|s| !s.trim().is_empty()))
        {
            return Err(invalid(
                "$.supported_game_versions",
                "game_version and server must be nonblank strings",
            ));
        }
        let game = json!({"gameVersion":row["game_version"],"server":row["server"]});
        if out.contains(&game) {
            return Err(invalid(
                "$.supported_game_versions",
                "duplicate game-version choice",
            ));
        }
        out.push(game);
    }
    Ok(json!(out))
}
pub(super) fn select(derived: &Value, options: &Value, explicit: Option<&Value>) -> Result<Value> {
    let rows = read::rows(options)?;
    let config = rows
        .iter()
        .find(|r| r["key"] == "wow_builds")
        .and_then(|r| r["value"].as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .ok_or_else(|| malformed("current wow_builds options are missing or malformed"))?;
    let builds = config
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or_else(|| malformed("wow_builds must contain current game choices"))?;
    let mut choices = Vec::new();
    for (server, record) in builds {
        let versions = record["versions"]
            .as_array()
            .filter(|a| {
                a.iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            })
            .ok_or_else(|| malformed("wow_builds contains malformed versions"))?;
        if derived
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["server"] == server.as_str())
        {
            for v in versions {
                choices.push(json!({"gameVersion":v,"server":server}));
            }
        }
    }
    let selected = explicit.map(games).transpose()?.unwrap_or_else(|| {
        json!(
            derived
                .as_array()
                .unwrap()
                .iter()
                .filter(|g| choices.contains(g))
                .cloned()
                .collect::<Vec<_>>()
        )
    });
    if selected.as_array().unwrap().is_empty()
        || selected
            .as_array()
            .unwrap()
            .iter()
            .any(|g| !choices.contains(g))
    {
        return Err(invalid(
            "$.supported_game_versions",
            "select current game versions on client branches actually declared by the ZIP; defaults use exact TOC versions",
        ));
    }
    Ok(selected)
}
