//! Fixed Go 3.2.0 physical-record contract, independent of local scan state.
use super::*;
use base64::{
    Engine as _,
    alphabet::URL_SAFE,
    engine::{
        DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::URL_SAFE_NO_PAD,
    },
};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike, Utc};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, MapAccess},
};
use serde_json::value::RawValue;

pub(super) const CURSOR_SCHEMA: &str = "lycheedev.hotfix-cursor.v1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Query {
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
fn arg_number<T: std::str::FromStr>(inv: &Invocation, flag: &str) -> Result<Option<T>> {
    inv.flag(flag)
        .map(|v| {
            v.parse()
                .map_err(|_| error("query", 2, format!("invalid {flag}")))
        })
        .transpose()
}
fn uint(inv: &Invocation, flag: &str) -> Result<Option<u32>> {
    inv.flag(flag)
        .map(|v| {
            validate_option(flag, v)?;
            if flag == "--table-hash" {
                u32::from_str_radix(v, 16)
            } else {
                v.parse()
            }
            .map_err(|_| ToolkitError::arguments(format!("invalid {flag}")))
        })
        .transpose()
}
/// Go entry.go validates each admitted option before provider/workspace I/O.
/// The CLI calls this in argv order; direct provider users validate the same bounds.
pub(super) fn validate_option(key: &str, value: &str) -> Result<()> {
    let signed = || value.parse::<i64>().ok();
    let unsigned = || {
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && value.parse::<u32>().is_ok()
    };
    let message = match key {
        "--source" if !["wago", "dbcache", "raidbots"].contains(&value) => {
            Some("--source must be wago, dbcache, or raidbots".into())
        }
        "--encoding" if !["csv", "raw", "png", "webp"].contains(&value) => {
            Some("--encoding must be raw, png, or webp".into())
        }
        "--table-hash" if value.len() != 8 => {
            Some("--table-hash requires eight hexadecimal digits".into())
        }
        "--table-hash" if !value.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some("invalid --table-hash".into())
        }
        "--record" | "--region-id" | "--after-index" if !unsigned() => {
            Some(format!("invalid {key}"))
        }
        "--push" if value.parse::<i32>().is_err() => Some("invalid --push".into()),
        "--status" if signed().is_none_or(|n| !(0..=255).contains(&n)) => {
            Some("--status must be between 0 and 255".into())
        }
        "--page" if signed().is_none_or(|n| n < 0) => Some("invalid --page".into()),
        "--limit" if signed().is_none_or(|n| !(1..=200).contains(&n)) => {
            Some("invalid --limit".into())
        }
        "--max-pages" if signed().is_none_or(|n| !(1..=2048).contains(&n)) => {
            Some("--max-pages must be between 1 and 2048".into())
        }
        "--max-requests" if signed().is_none_or(|n| n < 1) => {
            Some("--max-requests must be at least 1".into())
        }
        "--max-bytes" if signed().is_none_or(|n| !(1..=536870912).contains(&n)) => {
            Some("--max-bytes must be between 1 and 536870912".into())
        }
        "--timeout-seconds" if signed().is_none_or(|n| !(1..=3600).contains(&n)) => {
            Some("--timeout-seconds must be between 1 and 3600".into())
        }
        _ => None,
    };
    message.map_or(Ok(()), |m| Err(ToolkitError::arguments(m)))
}
fn parsed_time(value: &str) -> Result<evidence::GoTimestamp> {
    if let Some(time) = evidence::parse_go_timestamp(value) {
        return Ok(time);
    }
    // The only fallback is Go's source-native layout. It has no zone. Reuse
    // the same exact date/hour/fraction parser instead of chrono's wider grammar.
    if value.as_bytes().get(10) == Some(&b' ') {
        let rfc = format!("{}T{}Z", &value[..10], value[10..].trim_start_matches(' '));
        if let Some(time) = evidence::parse_go_timestamp(&rfc) {
            return Ok(time);
        }
    }
    Err(error(
        "query",
        2,
        "time must be RFC3339 or 2006-01-02 15:04:05",
    ))
}
fn time_wire(value: &str) -> Result<String> {
    let parsed = parsed_time(value)?;
    Ok(parsed.canonical().unwrap_or(value).to_owned())
}
pub(super) fn parse_time(value: &str) -> Result<DateTime<FixedOffset>> {
    let parsed = parsed_time(value)?;
    if let Some(canonical) = parsed.canonical() {
        return DateTime::parse_from_rfc3339(canonical)
            .map_err(|_| fault("records.internal", 8, "Go time canonical conversion failed"));
    }
    let (seconds, nanos) = parsed.instant();
    // A >=24h zone cannot be represented by chrono::FixedOffset, but its exact
    // UTC instant can. Callers use this result only for comparisons/UTC scope;
    // time_wire retains the original zone for the later JSON marshal boundary.
    DateTime::<Utc>::from_timestamp(seconds, nanos)
        .map(|time| time.fixed_offset())
        .ok_or_else(|| fault("records.internal", 8, "Go time instant conversion failed"))
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
fn time_marshal_failure() -> ToolkitError {
    fault(
        "command.failed",
        5,
        "json: error calling MarshalJSON for type time.Time: Time.MarshalJSON: timezone hour outside of range [0,23]",
    )
}
fn serialize_optional_time<S: serde::Serializer>(
    value: &Option<String>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        None => serializer.serialize_none(),
        Some(raw) => {
            let parsed = parsed_time(raw).map_err(serde::ser::Error::custom)?;
            let canonical = parsed
                .canonical()
                .ok_or_else(|| serde::ser::Error::custom(time_marshal_failure().message))?;
            serializer.serialize_some(canonical)
        }
    }
}

impl Query {
    pub(super) fn check_time_marshal(&self) -> Result<()> {
        for raw in [&self.from, &self.to].into_iter().flatten() {
            if parsed_time(raw)?.canonical().is_none() {
                return Err(time_marshal_failure());
            }
        }
        Ok(())
    }
    pub fn from_invocation(inv: &Invocation) -> Result<Self> {
        for (key, value) in &inv.flags {
            validate_option(key, value)?;
        }
        let source = inv.required("--source")?;
        if source == "raidbots" && inv.flag("--raidbots").unwrap_or_default().is_empty() {
            return Err(ToolkitError::arguments(
                "data hotfix --source raidbots requires --raidbots",
            ));
        }
        if source == "wago" && inv.flag("--raidbots").is_some_and(|v| !v.is_empty()) {
            return Err(ToolkitError::arguments(
                "--raidbots is not supported by the wago hotfix source",
            ));
        }
        if inv.flag("--dbcache").is_some_and(|v| !v.is_empty()) {
            return Err(ToolkitError::arguments(format!(
                "--dbcache is not supported by the {source} hotfix source"
            )));
        }
        if ["--product", "--build", "--region", "--locale"]
            .iter()
            .any(|f| inv.flag(f).unwrap_or_default().is_empty())
        {
            return Err(ToolkitError::arguments(format!(
                "data hotfix --source {source} requires --product, --build, --region, and --locale"
            )));
        }
        if inv.flag("--after-index").is_some() {
            return Err(ToolkitError::arguments(format!(
                "data hotfix --source {source} pages with --cursor, not --after-index"
            )));
        }
        let time = |flag| {
            inv.flag(flag)
                .filter(|s| !s.is_empty())
                .map(time_wire)
                .transpose()
        };
        let query = Self {
            product: inv.required("--product")?.into(),
            full_build: inv.required("--build")?.into(),
            region: inv.required("--region")?.into(),
            locale: inv.required("--locale")?.into(),
            table: inv.flag("--table").unwrap_or_default().into(),
            table_hash: uint(inv, "--table-hash")?,
            record_id: uint(inv, "--record")?,
            push_id: arg_number(inv, "--push")?,
            region_id: uint(inv, "--region-id")?,
            status: arg_number(inv, "--status")?,
            from: time("--from")?,
            to: time("--to")?,
            search: inv.flag("--search").unwrap_or_default().into(),
            latest: inv.flag_bool("--latest"),
            page: arg_number(inv, "--page")?.unwrap_or(0),
            limit: arg_number(inv, "--limit")?.unwrap_or(50),
            cursor: inv.flag("--cursor").unwrap_or_default().into(),
            definition: None,
        };
        query.validate()?;
        Ok(query)
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
pub(super) struct Record {
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
pub(super) struct FetchBudget {
    pub max_requests: i64,
    pub max_bytes: i64,
    pub requests: i64,
    pub bytes: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Coverage {
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
pub(super) struct RemoteResult {
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
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CursorPage {
    pub page: i64,
    pub key: String,
    #[serde(rename = "sha256")]
    pub sha256: String,
    pub total: i64,
    pub last_page: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_page_url: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Cursor {
    pub schema: String,
    pub source: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub scope: String,
    pub resume_page: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub after: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<CursorPage>,
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    pub current: Option<CursorPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_index: Option<u32>,
}
impl parse::Object for CursorPage {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "page" => parse::scalar(m, &mut self.page),
            "key" => parse::scalar(m, &mut self.key),
            "sha256" => parse::scalar(m, &mut self.sha256),
            "total" => parse::scalar(m, &mut self.total),
            "lastpage" => parse::scalar(m, &mut self.last_page),
            "nextpageurl" => parse::scalar(m, &mut self.next_page_url),
            _ => parse::ignored(m),
        }
    }
}
impl parse::Object for Cursor {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "schema" => parse::scalar(m, &mut self.schema),
            "source" => parse::scalar(m, &mut self.source),
            "ref" => parse::scalar(m, &mut self.reference),
            "scope" => parse::scalar(m, &mut self.scope),
            "resumepage" => parse::scalar(m, &mut self.resume_page),
            "after" => parse::scalar(m, &mut self.after),
            "previous" => parse::pointer(m, &mut self.previous),
            "self" => parse::pointer(m, &mut self.current),
            "afterindex" => {
                self.after_index = m.next_value()?;
                Ok(())
            }
            _ => parse::ignored(m),
        }
    }
}
impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut c = Self::default();
        parse::Seed(&mut c).deserialize(d)?;
        Ok(c)
    }
}
impl Cursor {
    pub fn encode(mut self, budget: &Budget) -> Result<String> {
        self.schema = CURSOR_SCHEMA.into();
        Ok(URL_SAFE_NO_PAD.encode(go_bytes(&self, budget, 1 << 20)?))
    }
    pub fn decode(encoded: &str, q: &Query, source: &str) -> Result<Self> {
        let decoder = GeneralPurpose::new(
            &URL_SAFE,
            GeneralPurposeConfig::new()
                .with_encode_padding(false)
                .with_decode_padding_mode(DecodePaddingMode::RequireNone)
                .with_decode_allow_trailing_bits(true),
        );
        let bytes = decoder
            .decode(encoded.replace(['\r', '\n'], ""))
            .map_err(|e| error("cursor", 2, e.to_string()))?;
        if !lycheedev_core::raw_json::is_valid(&bytes) {
            return Err(error("cursor", 2, "invalid cursor JSON"));
        }
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        decoder.disable_recursion_limit();
        let cursor =
            Self::deserialize(&mut decoder).map_err(|e| error("cursor", 2, e.to_string()))?;
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
