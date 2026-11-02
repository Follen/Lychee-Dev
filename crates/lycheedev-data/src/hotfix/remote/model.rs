//! Retained wire physical-record contract, independent of local scan state.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub(super) const CURSOR_SCHEMA: &str = "lycheedev.hotfix-cursor.v1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub product: String,
    pub full_build: String,
    pub region: String,
    pub locale: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_hash: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u32>,
    #[serde(rename = "push", skip_serializing_if = "Option::is_none")]
    pub push_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_optional_time")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_optional_time")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub search: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub latest: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub page: i64,
    pub limit: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cursor: String,
    #[serde(skip)]
    pub definition: Option<crate::schema::Definition>,
}
fn is_zero(n: &i64) -> bool {
    *n == 0
}
fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}
fn parsed_time(value: &str) -> Result<DateTime<FixedOffset>> {
    if let Ok(time) = DateTime::parse_from_rfc3339(value) {
        return Ok(time);
    }
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
        .map(|time| time.and_utc().fixed_offset())
        .map_err(|_| error("query", 2, "time must be RFC3339 or 2006-01-02 15:04:05"))
}
pub(super) fn time_wire(value: &str) -> Result<String> {
    Ok(parsed_time(value)?.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}
pub(super) fn parse_time(value: &str) -> Result<DateTime<FixedOffset>> {
    parsed_time(value)
}
fn utc_scope_time(value: &str) -> Result<String> {
    let time = parse_time(value)?.with_timezone(&Utc);
    // Go Format permits UTC normalization across year 0000/9999 without a
    // leading plus sign for a five-digit year. JSON marshal has its own check.
    let year = if time.year() < 0 {
        format!("-{:04}", -time.year())
    } else {
        format!("{:04}", time.year())
    };
    let mut out = format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year,
        time.month(),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    );
    if time.nanosecond() != 0 {
        out.push('.');
        out.push_str(format!("{:09}", time.nanosecond()).trim_end_matches('0'));
    }
    out.push('Z');
    Ok(out)
}
fn serialize_optional_time<S: serde::Serializer>(
    value: &Option<String>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        None => serializer.serialize_none(),
        Some(raw) => serializer.serialize_some(&time_wire(raw).map_err(serde::ser::Error::custom)?),
    }
}
impl Query {
    pub(super) fn check_time_marshal(&self) -> Result<()> {
        for raw in [&self.from, &self.to].into_iter().flatten() {
            parsed_time(raw)?;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if lycheedev_core::client::product(&self.product).is_none() {
            return Err(error("query", 2, "selection.unsupported_data_product"));
        }
        if ![
            "enUS", "koKR", "frFR", "deDE", "zhCN", "esES", "zhTW", "enGB", "esMX", "ruRU", "ptBR",
            "itIT", "ptPT",
        ]
        .contains(&self.locale.as_str())
        {
            return Err(error("query", 2, "selection.unsupported_data_language"));
        }
        if !["us", "eu", "cn", "kr", "tw"].contains(&self.region.as_str()) {
            return Err(error("query", 2, "selection.unsupported_data_region"));
        }
        self.build_number()?;
        if !self.table.is_empty() && self.table_hash.is_some() {
            return Err(error("query", 2, "select table or table-hash, not both"));
        }
        if let (Some(from), Some(to)) = (&self.from, &self.to) {
            if parse_time(from)? > parse_time(to)? {
                return Err(error("query", 2, "from must not be after to"));
            }
        }
        if self.page < 0 {
            return Err(error("query", 2, "page must be non-negative"));
        }
        if !(1..=200).contains(&self.limit) {
            return Err(error("query", 2, "limit must be 1..200"));
        }
        Ok(())
    }
    pub fn build_number(&self) -> Result<u32> {
        let pieces: Vec<_> = self.full_build.split('.').collect();
        if pieces.len() != 4
            || pieces.iter().any(|p| {
                p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.parse::<u32>().is_err()
            })
        {
            return Err(error("query", 2, "build must be four numeric segments"));
        }
        pieces[3].parse().map_err(|_| {
            error(
                "query",
                2,
                format!("build {:?} has no cache build number", self.full_build),
            )
        })
    }
    pub fn scope(&self) -> Result<String> {
        let mut p = vec![
            format!("product:{}", self.product),
            format!("build:{}", self.full_build),
            format!("region:{}", self.region),
            format!("locale:{}", self.locale),
        ];
        if !self.table.is_empty() {
            p.push(format!("table:{}", lower(&self.table)));
        }
        if let Some(v) = self.table_hash {
            p.push(format!("tableHash:{v}"));
        }
        if let Some(v) = self.record_id {
            p.push(format!("record:{v}"));
        }
        if let Some(v) = self.push_id {
            p.push(format!("push:{v}"));
        }
        if let Some(v) = self.region_id {
            p.push(format!("regionId:{v}"));
        }
        if let Some(v) = self.status {
            p.push(format!("status:{v}"));
        }
        if let Some(v) = &self.from {
            p.push(format!("from:{}", utc_scope_time(v)?));
        }
        if let Some(v) = &self.to {
            p.push(format!("to:{}", utc_scope_time(v)?));
        }
        if !self.search.is_empty() {
            p.push(format!("search:{}", lower(&self.search)));
        }
        if self.latest {
            p.push("latest".into());
        }
        Ok(p.join("|"))
    }
    pub fn scope_digest(&self) -> Result<String> {
        Ok(storage::sha256(self.scope()?.as_bytes()))
    }
    pub fn effective_search(&self) -> String {
        if !self.search.trim().is_empty() {
            return self.search.trim().into();
        }
        if let Some(v) = self.record_id {
            return v.to_string();
        }
        if let Some(v) = self.push_id {
            return v.to_string();
        }
        self.full_build
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .into()
    }
    pub fn matches(&self, r: &Record) -> Result<bool> {
        if self.record_id.is_some_and(|v| v != r.record_id)
            || self.push_id.is_some_and(|v| v != r.push)
            || self.status.is_some_and(|v| v != r.status)
            || self.table_hash.is_some_and(|v| Some(v) != r.table_hash)
            || self.region_id.is_some_and(|v| Some(v) != r.region_id)
        {
            return Ok(false);
        }
        if !self.table.is_empty() && !equal_fold(&self.table, &r.table_name) {
            return Ok(false);
        }
        if !r.locale.is_empty() && !equal_fold(&self.locale, &r.locale) {
            return Ok(false);
        }
        if r.build.is_empty()
            || !(self.full_build == r.build
                || self.full_build.ends_with(&format!(".{}", r.build))
                || r.build.ends_with(&format!(".{}", self.full_build)))
        {
            return Ok(false);
        }
        if self.from.is_none() && self.to.is_none() {
            return Ok(true);
        }
        let created = parse_time(&r.created_at)
            .ok()
            .map(|v| v.with_timezone(&Utc));
        // Go's zero timestamp is year 1 UTC, not the Unix epoch.
        let created = created.unwrap_or_else(|| Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap());
        if self
            .from
            .as_ref()
            .map(|v| parse_time(v))
            .transpose()?
            .is_some_and(|v| created < v)
        {
            return Ok(false);
        }
        if self
            .to
            .as_ref()
            .map(|v| parse_time(v))
            .transpose()?
            .is_some_and(|v| created > v)
        {
            return Ok(false);
        }
        Ok(true)
    }
}
// Go strings.ToLower uses simple Unicode mapping rather than full lowercase expansions.
pub(crate) fn lower(s: &str) -> String {
    s.chars()
        .map(|c| crate::unicode_simple::map(c, false))
        .collect()
}
pub(crate) fn equal_fold(a: &str, b: &str) -> bool {
    // ASCII identifiers need no Unicode table lookups. Both operands must be
    // ASCII: non-ASCII characters such as Kelvin sign and long s can fold to
    // ASCII, so a one-sided shortcut would change identifier resolution.
    // Bound the preliminary scans: long early-mismatch strings retain the
    // original short-circuit traversal. This is not an identifier length limit.
    if a.len() <= 64 && b.len() <= 64 && a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    {
        let fold = |c| {
            if matches!(c, '\u{0130}' | '\u{0131}') {
                c
            } else {
                crate::unicode_simple::map(crate::unicode_simple::map(c, true), false)
            }
        };
        a.chars().map(fold).eq(b.chars().map(fold))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub provider: String,
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub id: u64,
    pub push: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_hash: Option<u32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub table_name: String,
    pub record_id: u32,
    pub status: u8,
    pub build: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub locale: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub payload_length: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub payload_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "fields_empty")]
    pub fields: Option<std::collections::BTreeMap<String, Value>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub decode_state: String,
}
fn fields_empty(fields: &Option<std::collections::BTreeMap<String, Value>>) -> bool {
    fields.as_ref().is_none_or(|f| f.is_empty())
}
impl Record {
    pub fn key(&self) -> String {
        if self.id != 0 {
            format!("id:{}", self.id)
        } else {
            format!(
                "push:{}:table:{}:record:{}:status:{}",
                self.push,
                self.table_hash.unwrap_or(0),
                self.record_id,
                self.status
            )
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchBudget {
    pub max_requests: i64,
    pub max_bytes: i64,
    pub requests: i64,
    pub bytes: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Coverage {
    pub provider: String,
    pub product: String,
    pub full_build: String,
    pub region: String,
    pub locale: String,
    pub complete: bool,
    pub full_query_complete: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub truncation_reason: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub start_page: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub end_page: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub last_page: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub total: i64,
    pub scanned: usize,
    pub matched: usize,
    pub returned: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_push: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_optional_time")]
    pub captured_at: Option<String>,
    #[serde(rename = "ref", skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch: Option<FetchBudget>,
}
impl Coverage {
    pub fn new(provider: &str, q: &Query) -> Self {
        Self {
            provider: provider.into(),
            product: q.product.clone(),
            full_build: q.full_build.clone(),
            region: q.region.clone(),
            locale: q.locale.clone(),
            ..Default::default()
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResult {
    pub schema: &'static str,
    pub query: Query,
    pub records: Vec<Record>,
    pub coverage: Coverage,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub change: selection::ChangePin,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snapshot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<Value>,
    pub complete: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_cursor: String,
}
impl RemoteResult {
    pub fn new(
        provider: &str,
        query: Query,
        records: Vec<Record>,
        coverage: Coverage,
        next_cursor: String,
    ) -> Result<Self> {
        let change = selection::ChangePin {
            provider: provider.into(),
            product: query.product.clone(),
            full_build: query.full_build.clone(),
            region: query.region.clone(),
            scope: format!("{provider}:{}", query.scope()?),
            fetched_at: coverage.captured_at.clone().unwrap_or_default(),
            sha256: coverage.reference.clone(),
        };
        Ok(Self {
            schema: "lycheedev.hotfix-result.v1",
            query,
            records,
            complete: coverage.complete,
            truncated: coverage.truncated,
            coverage,
            warnings: vec![],
            change,
            snapshot: String::new(),
            capture: None,
            next_cursor,
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CursorPage {
    pub page: i64,
    pub key: String,
    #[serde(rename = "sha256")]
    pub sha256: String,
    pub total: i64,
    pub last_page: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next_page_url: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Cursor {
    pub schema: String,
    pub source: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub scope: String,
    pub resume_page: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<CursorPage>,
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    pub current: Option<CursorPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_index: Option<u32>,
}
impl Cursor {
    pub fn encode(mut self, budget: &Budget) -> Result<String> {
        self.schema = CURSOR_SCHEMA.into();
        Ok(URL_SAFE_NO_PAD.encode(wire_bytes(&self, budget, 1 << 20)?))
    }
    pub fn decode(encoded: &str, q: &Query, source: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| error("cursor", 2, e.to_string()))?;
        let cursor: Self =
            serde_json::from_slice(&bytes).map_err(|e| error("cursor", 2, e.to_string()))?;
        if cursor.schema != CURSOR_SCHEMA
            || cursor.source.is_empty()
            || cursor.reference.len() != 64
        {
            return Err(error("cursor", 2, "malformed cursor"));
        }
        if cursor.source != source {
            return Err(error(
                "cursor",
                2,
                format!("cursor was cut from {:?}, not {source}", cursor.source),
            ));
        }
        if cursor.scope != q.scope_digest()? {
            return Err(error(
                "cursor",
                2,
                "query scope differs from the cursor scope",
            ));
        }
        Ok(cursor)
    }
}

#[cfg(test)]
mod equal_fold_tests {
    use super::equal_fold;

    // Frozen pre-fast-path implementation, independent of the ASCII branch.
    fn reference(a: &str, b: &str) -> bool {
        let fold = |c| {
            if matches!(c, '\u{0130}' | '\u{0131}') {
                c
            } else {
                crate::unicode_simple::map(crate::unicode_simple::map(c, true), false)
            }
        };
        a.chars().map(fold).eq(b.chars().map(fold))
    }

    #[test]
    fn ascii_fold_matches_reference_for_every_ascii_pair() {
        for a in 0u8..=127 {
            for b in 0u8..=127 {
                let a = String::from(a as char);
                let b = String::from(b as char);
                assert_eq!(equal_fold(&a, &b), reference(&a, &b), "{a:?} {b:?}");
                let (a, b) = (format!("ID_{a}_z"), format!("id_{b}_Z"));
                assert_eq!(equal_fold(&a, &b), reference(&a, &b));
            }
        }
    }

    #[test]
    fn ascii_fold_preserves_lengths_controls_and_long_identifiers() {
        let samples = [
            "",
            "ID",
            "id",
            "iD",
            "NumericSample",
            "numericsample",
            "ID\0",
            "ID\n",
            "ID ",
            "_01",
            "[",
            "{",
            "@",
            "`",
            "\\",
            "|",
            "\u{7f}",
        ];
        for a in samples {
            for b in samples {
                assert_eq!(equal_fold(a, b), reference(a, b));
            }
        }
        for len in [0, 1, 63, 64, 65, 4096] {
            let a = "I".repeat(len);
            let b = "i".repeat(len);
            assert_eq!(equal_fold(&a, &b), reference(&a, &b));
            if len > 0 {
                let early = format!("X{}", &b[1..]);
                assert_eq!(equal_fold(&a, &early), reference(&a, &early));
                assert_eq!(equal_fold(&early, &a), reference(&early, &a));
            }
            let early_mismatch = format!("X{b}");
            assert_eq!(
                equal_fold(&a, &early_mismatch),
                reference(&a, &early_mismatch)
            );
            assert_eq!(
                equal_fold(&early_mismatch, &a),
                reference(&early_mismatch, &a)
            );
        }
        for prefix_len in [0, 60, 61, 62, 63, 64, 65] {
            let prefix = "i".repeat(prefix_len);
            let a = format!("{prefix}K");
            let b = format!("{prefix}K");
            assert!(reference(&a, &b));
            assert!(equal_fold(&a, &b));
            assert!(equal_fold(&b, &a));
        }
        let a = "Wide_ID_0123".repeat(4096);
        let mut b = a.to_ascii_lowercase();
        assert!(equal_fold(&a, &b));
        b.push('x');
        assert!(!equal_fold(&a, &b));
        assert!(!equal_fold(&b, &a));
    }

    #[test]
    fn ascii_fold_preserves_unicode_cross_ascii_and_nonexpansion() {
        for (a, b, expected) in [
            ("K", "K", true),
            ("s", "ſ", true),
            ("I", "ı", false),
            ("i", "İ", false),
            ("ß", "SS", false),
            ("ß", "ẞ", true),
            ("Σ", "ς", true),
            ("σ", "Σ", true),
            ("é", "É", true),
            ("é", "e\u{301}", false),
            ("𐐀", "𐐨", true),
            ("ID_K_ſ", "id_k_s", true),
            ("中文🙂", "中文🙂", true),
        ] {
            assert_eq!(reference(a, b), expected, "reference {a:?} {b:?}");
            assert_eq!(equal_fold(a, b), expected, "{a:?} {b:?}");
            assert_eq!(equal_fold(b, a), expected, "reverse {a:?} {b:?}");
        }
    }

    #[test]
    fn ascii_fold_unicode_scalar_sweep_matches_original_mapping() {
        let mut encoded = [0; 4];
        let mut upper = [0; 4];
        let mut lower = [0; 4];
        for code in 0..=0x10ffff {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let a = c.encode_utf8(&mut encoded);
            for b in [
                crate::unicode_simple::map(c, true).encode_utf8(&mut upper),
                crate::unicode_simple::map(c, false).encode_utf8(&mut lower),
            ] {
                assert_eq!(equal_fold(a, b), reference(a, b), "U+{code:04X}");
                assert_eq!(equal_fold(b, a), reference(b, a), "reverse U+{code:04X}");
            }
        }
    }
}
