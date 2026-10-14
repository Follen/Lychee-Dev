#![forbid(unsafe_code)]
//! Authenticated game data formats and bounded queries.
pub mod assets;
pub mod availability;
pub mod blte;
mod capture;
pub mod casc;
pub mod db2;
pub mod decoded_cache;
pub mod definitions;
pub mod hotfix;
pub mod installed;
pub mod keys;
pub mod local_query;
pub mod metadata_format;
pub mod navigate;
pub mod relational;
pub mod resource;
pub mod schema;
pub mod stream;
pub mod table;
#[cfg(test)]
mod unicode_checks;
mod unicode_simple;

use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};

pub(crate) fn fault(code: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    ToolkitError {
        code: code.into(),
        message: message.into(),
        exit_code: exit,
        details: serde_json::Value::Null,
    }
}

pub fn commands() -> &'static [&'static str] {
    &[
        "data hotfix",
        "data sql",
        "data db2",
        "data db2 schema",
        "data db2 search",
        "data db2 foreign-key",
        "data db2 stream",
        "data spell info",
        "data item get",
        "data item models",
        "data item geosets",
        "data item textures",
        "data decor list",
        "data decor get",
        "data creature display",
        "data creature model",
        "data encounter get",
        "asset inspect",
        "asset search",
        "asset export",
        "asset demux",
    ]
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if matches!(
        inv.command.as_str(),
        "asset search" | "asset export" | "asset demux"
    ) {
        return assets::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data spell info"
            | "data spell auras"
            | "data spell summons"
            | "data item get"
            | "data item models"
            | "data item geosets"
            | "data item textures"
            | "data decor list"
            | "data decor get"
            | "data creature display"
            | "data creature model"
            | "data encounter get"
    ) {
        return navigate::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data db2" | "data db2 schema" | "data db2 search" | "data db2 foreign-key"
    ) {
        return db2::dispatch(inv);
    }
    if inv.command == "data sql" {
        return local_query::sql(inv);
    }
    if inv.command == "asset inspect" {
        return local_query::asset(inv);
    }
    if inv.command == "data hotfix" && inv.required("--source")? == "dbcache" {
        use lycheedev_core::{evidence, selection, storage};
        use serde_json::json;
        let pin = selection::load_pinned(&inv.home, inv.required("--snapshot")?)?;
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("records.hotfix_identity", 4, "Hotfix requires pinned data"))?;
        let build = schema::version(&data.full_build)?[3];
        let max = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
        if inv.flag("--dbcache").is_some() == inv.flag("--from").is_some() {
            return Err(fault(
                "records.hotfix_identity",
                4,
                "Select exactly one --dbcache or archived --from",
            ));
        }
        if inv.flag("--table").is_some() || inv.flag_bool("--scan") {
            return Err(fault(
                "command.unsupported",
                3,
                "Named hotfix definitions and resumable scans have not completed Rust parity",
            ));
        }
        let (source, raw) = if let Some(id) = inv.flag("--from") {
            let source = evidence::read_capture(&inv.home, id)?;
            if source["provenance"]["kind"] != "hotfix-cache"
                || source["mediaType"] != "application/octet-stream"
                || source["complete"] != true
                || source["truncated"] != false
                || source["provenance"]["dataBuild"] != data.full_build
            {
                return Err(fault(
                    "records.hotfix_identity",
                    4,
                    "Capture is not complete cache for pinned build",
                ));
            }
            let origin = selection::load_pinned(
                &inv.home,
                source["provenance"]["snapshot"].as_str().unwrap_or(""),
            )?;
            if origin.data.as_ref() != Some(data) {
                return Err(fault(
                    "records.hotfix_identity",
                    4,
                    "Capture data context differs",
                ));
            }
            let raw = evidence::read_blob(&inv.home, &source["blob"], max)?;
            (source, raw)
        } else {
            if inv.flag("--cursor").is_some() {
                return Err(fault(
                    "records.hotfix_identity",
                    4,
                    "Pagination requires archived --from",
                ));
            }
            let path = std::path::Path::new(inv.required("--dbcache")?);
            let meta = std::fs::symlink_metadata(path)?;
            if !meta.is_file() || meta.len() > max as u64 {
                return Err(fault(
                    "records.hotfix_limit",
                    3,
                    "Cache file is not regular or exceeds budget",
                ));
            }
            let raw = std::fs::read(path)?;
            if raw.len() > max {
                return Err(fault(
                    "records.hotfix_limit",
                    3,
                    "Cache changed beyond byte budget",
                ));
            }
            // Validate complete framing before publishing evidence or answering a limited page.
            hotfix::inspect(
                &raw,
                hotfix::CacheFilter {
                    build,
                    limit: 1,
                    ..Default::default()
                },
            )?;
            let source = evidence::commit_capture(
                &inv.home,
                &raw,
                "application/octet-stream",
                json!({"kind":"hotfix-cache","locator":path.to_string_lossy(),"snapshot":pin.id,"dataBuild":data.full_build}),
                true,
                false,
            )?;
            (source, raw)
        };
        let number = |flag: &str| -> Result<Option<u32>> {
            inv.flag(flag)
                .map(|v| {
                    if let Some(v) = v.strip_prefix("0x") {
                        u32::from_str_radix(v, 16)
                    } else {
                        v.parse()
                    }
                })
                .transpose()
                .map_err(|_| fault("records.hotfix_query", 2, format!("Invalid {flag}")))
        };
        let status = number("--status")?
            .map(|v| {
                u8::try_from(v)
                    .map_err(|_| fault("records.hotfix_query", 2, "Status outside 0..255"))
            })
            .transpose()?;
        let filter = hotfix::CacheFilter {
            build,
            table_hash: number("--table-hash")?,
            record_id: number("--record")?,
            push: inv
                .flag("--push")
                .map(str::parse)
                .transpose()
                .map_err(|_| fault("records.hotfix_query", 2, "Invalid signed push"))?,
            region: number("--region-id")?,
            status,
            latest: inv.flag_bool("--latest"),
            after_index: number("--cursor")?,
            limit: inv.usize_flag("--limit", 20, 1, 200)?,
        };
        let page = hotfix::inspect(&raw, filter)?;
        let complete = page.complete;
        let truncated = page.truncated;
        let result = json!({"snapshot":pin.id,"source":source,"page":page});
        let capture = evidence::commit_capture(
            &inv.home,
            &serde_json::to_vec(&result)?,
            "application/json",
            json!({"kind":"hotfix-page","locator":storage::sha256(&raw),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete,
            truncated,
        )?;
        let mut out = Outcome::new(result);
        out.complete = complete;
        out.selection = serde_json::to_value(pin)?;
        out.provenance = vec![source, capture];
        out.coverage = json!({"scope":"selected_cache","serverCoverage":"unknown"});
        return Ok(out);
    }
    Err(fault(
        "command.unsupported",
        3,
        format!(
            "Rust data command has not completed parity: {}",
            inv.command
        ),
    ))
}
