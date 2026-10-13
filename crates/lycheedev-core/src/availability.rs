//! Bounded current release observations. HTTP is supplied by an entry adapter.
use crate::{Result, ToolkitError, client::PRODUCTS, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const MAX_BYTES: usize = 1 << 20;
pub type ManifestFetcher<'a> = dyn FnMut(&str, &str, &OperationControl) -> Result<Vec<u8>> + 'a;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRow {
    pub region: String,
    pub full_build: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build_id: String,
    pub build_config: String,
    #[serde(rename = "cdnConfig")]
    pub cdn_config: String,
    pub current: bool,
}
fn fault(code: &str) -> ToolkitError {
    ToolkitError::new(code, code, 4)
}
pub fn valid_region(region: &str) -> bool {
    ["us", "eu", "cn", "kr", "tw"].contains(&region)
}
pub fn locator(region: &str, slot: &str) -> String {
    if region == "cn" {
        format!("https://cn.version.battlenet.com.cn/{slot}/versions")
    } else {
        format!("https://{region}.version.battle.net/{slot}/versions")
    }
}
fn full_build(build: &str) -> bool {
    let parts = build.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
fn key(value: &str) -> bool {
    crate::selection::valid_digest(value, 32)
}
pub fn parse(
    raw: &[u8],
    region: &str,
    control: &OperationControl,
) -> Result<(Vec<ReleaseRow>, usize)> {
    control.check()?;
    if raw.len() > MAX_BYTES {
        return Err(fault("selection.manifest_limit"));
    }
    let text = std::str::from_utf8(raw).map_err(|_| fault("selection.manifest_format"))?;
    if text.contains('\0') {
        return Err(fault("selection.manifest_format"));
    }
    let mut header = None;
    let mut columns = BTreeMap::new();
    let mut records = 0usize;
    let mut rejected = 0;
    let mut rows = vec![];
    for line in text.trim_start_matches('\u{feff}').split('\n') {
        control.check()?;
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields = line.split('|').collect::<Vec<_>>();
        if header.is_none() {
            if fields.len() > 128 {
                return Err(fault("selection.manifest_limit"));
            }
            for (i, field) in fields.iter().enumerate() {
                let (name, _) = field
                    .trim()
                    .split_once('!')
                    .ok_or_else(|| fault("selection.manifest_format"))?;
                let name = name.trim();
                if name.is_empty() || columns.insert(name.to_owned(), i).is_some() {
                    return Err(fault("selection.manifest_format"));
                }
            }
            for required in ["Region", "VersionsName", "BuildConfig", "CDNConfig"] {
                if !columns.contains_key(required) {
                    return Err(fault("selection.manifest_format"));
                }
            }
            header = Some(fields.len());
            continue;
        }
        if records == 4096 {
            return Err(fault("selection.manifest_limit"));
        }
        if fields.len() != header.unwrap() {
            rejected += 1;
            continue;
        }
        records += 1;
        let fields = fields.iter().map(|field| field.trim()).collect::<Vec<_>>();
        if !fields[columns["Region"]]
            .split(',')
            .any(|item| item.trim() == region)
        {
            continue;
        }
        let row = ReleaseRow {
            region: region.into(),
            full_build: fields[columns["VersionsName"]].into(),
            build_config: fields[columns["BuildConfig"]].into(),
            cdn_config: fields[columns["CDNConfig"]].into(),
            build_id: columns
                .get("BuildId")
                .map(|col| fields[*col].to_owned())
                .unwrap_or_default(),
            current: false,
        };
        let id_matches = row.build_id.is_empty()
            || row
                .full_build
                .rsplit('.')
                .next()
                .and_then(|part| part.parse::<u32>().ok())
                .zip(row.build_id.parse::<u32>().ok())
                .is_some_and(|(want, got)| want == got);
        if !full_build(&row.full_build)
            || !key(&row.build_config)
            || !key(&row.cdn_config)
            || !id_matches
        {
            rejected += 1;
            continue;
        }
        rows.push(row);
    }
    if header.is_none() {
        return Err(fault("selection.manifest_format"));
    }
    rows.sort_by(|a, b| {
        for (left, right) in a.full_build.split('.').zip(b.full_build.split('.')) {
            let order = right
                .parse::<u32>()
                .unwrap_or(0)
                .cmp(&left.parse::<u32>().unwrap_or(0));
            if !order.is_eq() {
                return order;
            }
        }
        b.full_build.cmp(&a.full_build)
    });
    Ok((rows, rejected))
}
pub fn list(
    region: &str,
    offline: bool,
    max_rows: usize,
    control: &OperationControl,
    fetch: &mut ManifestFetcher<'_>,
) -> Result<Value> {
    if !valid_region(region) {
        return Err(ToolkitError::arguments(
            "target available requires --region <us|eu|cn|kr|tw>",
        ));
    }
    if offline {
        return Err(ToolkitError::new(
            "selection.listing_requires_network",
            "selection.listing_requires_network",
            3,
        ));
    }
    let max_rows = if max_rows == 0 {
        64
    } else {
        max_rows.min(4096)
    };
    let mut products = vec![];
    for profile in &PRODUCTS {
        control.check()?;
        let product = profile.id;
        let slot = profile.catalog_product;
        let series = profile.series;
        let mut entry = json!({"product":product,"productCode":slot,"locator":locator(region,slot),"rows":null,"rejected":0,"totalRows":0,"truncated":false});
        if !profile.channel.is_empty() {
            entry["displayName"] = json!(profile.display_name);
            entry["channel"] = json!(profile.channel);
            entry["catalogProduct"] = json!(slot);
        }
        let parsed = fetch(region, slot, control).and_then(|raw| parse(&raw, region, control));
        control.check()?;
        match parsed {
            Err(error) => entry["error"] = json!(error.message),
            Ok((mut rows, mut rejected)) => {
                if !profile.channel.is_empty() {
                    rows.retain(|row|{if row.full_build.starts_with(&format!("{series}.")){true}else{rejected+=1;entry["error"]=json!(format!("selection.data_product_build_mismatch: {product} slot {slot} serves {}; verified series is {series}",row.full_build));false}});
                }
                entry["totalRows"] = json!(rows.len());
                entry["truncated"] = json!(rows.len() > max_rows);
                rows.truncate(max_rows);
                for (i, row) in rows.iter_mut().enumerate() {
                    row.current = i == 0;
                }
                entry["rows"] = serde_json::to_value(rows)?;
                entry["rejected"] = json!(rejected);
            }
        }
        products.push(entry);
    }
    Ok(json!({"region":region,"observedAt":storage::utc_now(),"products":products}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> String {
        format!(
            "\u{feff}# observed\r\nRegion!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\r\nus, cn|{}|{}|70009|12.1.0.70009\r\ncn|{}|{}|69933|12.1.0.69933\r\ncn|{}|{}|99999|12.1.0.69933\r\nbad|shape\r\n",
            "a".repeat(32),
            "b".repeat(32),
            "a".repeat(32),
            "b".repeat(32),
            "a".repeat(32),
            "b".repeat(32)
        )
    }
    #[test]
    fn current_observation_bounds_region_sort_truncation_and_reused_forever_slot() {
        let control = OperationControl::default();
        let mut calls = vec![];
        let mut fetch = |region: &str, slot: &str, _: &OperationControl| {
            calls.push((region.to_owned(), slot.to_owned()));
            Ok(manifest().into_bytes())
        };
        let result = list("cn", false, 1, &control, &mut fetch).unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[3].1, "wow_classic_beta");
        assert_eq!(calls[4].1, "wow_cn_beta");
        assert_eq!(result["products"][0]["totalRows"], 2);
        assert_eq!(result["products"][0]["truncated"], true);
        assert_eq!(
            result["products"][0]["rows"][0]["fullBuild"],
            "12.1.0.70009"
        );
        assert_eq!(result["products"][0]["rows"][0]["current"], true);
        assert_eq!(result["products"][3]["rows"], json!([]));
        assert!(result["products"][3]["error"].is_string());
        assert_eq!(
            list("cn", true, 0, &control, &mut |_, _, _| panic!(
                "offline must not fetch"
            ))
            .unwrap_err()
            .exit_code,
            3
        );
    }
    #[test]
    fn equal_build_products_are_observed_independently_at_their_catalog_slots() {
        let mut slots = Vec::new();
        let result = list("cn", false, 20, &OperationControl::default(), &mut |region, slot, _| {
            slots.push(slot.to_owned());
            Ok(format!("Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\n{region}|{}|{}|70338|1.60.1.70338\n", "a".repeat(32), "b".repeat(32)).into_bytes())
        }).unwrap();
        assert_eq!(
            slots
                .iter()
                .filter(|s| s.as_str() == "wow_classic_beta")
                .count(),
            1
        );
        assert_eq!(
            slots.iter().filter(|s| s.as_str() == "wow_cn_beta").count(),
            1
        );
        let asia = &result["products"][3];
        let cn = &result["products"][4];
        assert_eq!(asia["product"], "forever");
        assert_eq!(cn["product"], "cn-beta");
        assert_eq!(asia["displayName"], "Forever Asia Beta");
        assert_eq!(cn["displayName"], "Forever CN Beta");
        assert_eq!(asia["catalogProduct"], "wow_classic_beta");
        assert_eq!(cn["catalogProduct"], "wow_cn_beta");
        assert_eq!(asia["rows"][0]["fullBuild"], cn["rows"][0]["fullBuild"]);
        assert!(
            cn["locator"]
                .as_str()
                .unwrap()
                .contains("/wow_cn_beta/versions")
        );
    }
    #[test]
    fn malformed_utf8_duplicate_header_and_row_budget_are_rejected() {
        let control = OperationControl::default();
        assert!(parse(&[255], "us", &control).is_err());
        assert!(parse(b"Region!STR|Region!STR\n", "us", &control).is_err());
        let row = format!(
            "us|{}|{}|70009|12.1.0.70009\n",
            "a".repeat(32),
            "b".repeat(32)
        );
        let body = format!(
            "Region!STR|BuildConfig!HEX|CDNConfig!HEX|BuildId!DEC|VersionsName!STR\n{}",
            row.repeat(4097)
        );
        assert_eq!(
            parse(body.as_bytes(), "us", &control).unwrap_err().code,
            "selection.manifest_limit"
        );
    }
}
