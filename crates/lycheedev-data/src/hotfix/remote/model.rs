//! Fixed Go 3.2.0 physical-record contract, independent of local scan state.
use super::*;
use base64::{
    Engine as _,
    alphabet::URL_SAFE,
    engine::{
        DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::URL_SAFE_NO_PAD,
    },
};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
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
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
pub(super) fn parse_time(value: &str) -> Result<DateTime<FixedOffset>> {
    static FORM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern=FORM.get_or_init(||regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}(T[0-9]{1,2}:[0-9]{2}:[0-9]{2}([.,][0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})| [0-9]{1,2}:[0-9]{2}:[0-9]{2}([.,][0-9]+)?)$").expect("fixed timestamp pattern"));
    if !pattern.is_match(value) {
        return Err(error(
            "query",
            2,
            "time must be RFC3339 or 2006-01-02 15:04:05",
        ));
    }
    let mut normalized = value.replace(',', ".");
    if normalized.as_bytes().get(12) == Some(&b':') {
        normalized.insert(11, '0');
    }
    if let Some(dot) = normalized.find('.') {
        let end = normalized[dot + 1..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(normalized.len(), |n| dot + 1 + n);
        if end > dot + 10 {
            normalized.replace_range(dot + 10..end, "");
        }
    }
    // chrono models leap seconds as second59+nanos>=1e9; Go time.Parse rejects :60.
    if normalized
        .as_bytes()
        .get(17..19)
        .is_some_and(|seconds| seconds == b"60")
    {
        return Err(error(
            "query",
            2,
            "time must be RFC3339 or 2006-01-02 15:04:05",
        ));
    }
    DateTime::parse_from_rfc3339(&normalized)
        .or_else(|_| {
            NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f")
                .map(|t| Utc.from_utc_datetime(&t).fixed_offset())
        })
        .map_err(|_| error("query", 2, "time must be RFC3339 or 2006-01-02 15:04:05"))
}
impl Query {
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
                .map(|v| parse_time(v).map(storage::timestamp_offset))
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
            p.push(format!(
                "from:{}",
                storage::timestamp(parse_time(v)?.with_timezone(&Utc))
            ));
        }
        if let Some(v) = &self.to {
            p.push(format!(
                "to:{}",
                storage::timestamp(parse_time(v)?.with_timezone(&Utc))
            ));
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
fn lower(s: &str) -> String {
    s.chars()
        .map(|c| crate::unicode_simple::map(c, false))
        .collect()
}
fn equal_fold(a: &str, b: &str) -> bool {
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
