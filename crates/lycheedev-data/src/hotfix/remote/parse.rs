//! Fixed Go Inertia envelope and HTML entity rules. Never treat bad HTML as an empty page.
use super::*;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;
#[derive(Default)]
pub(super) struct Page<'a> {
    pub current_page: i64,
    pub last_page: i64,
    pub per_page: i64,
    pub total: i64,
    pub next_page_url: String,
    pub records: Vec<Record>,
    _retained: Option<crate::resource::RetainedReservation<'a>>,
}
fn protocol(m: impl Into<String>) -> ToolkitError {
    error("wago_protocol", 4, m)
}
fn named(name: &str) -> Option<&'static str> {
    super::entities::ENTITIES
        .binary_search_by_key(&name, |(k, _)| *k)
        .ok()
        .map(|n| super::entities::ENTITIES[n].1)
}
fn unescape(raw: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    const WINDOWS: [u32; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        if at % 4096 == 0 {
            control.check()?;
        }
        if raw[at] != b'&' {
            out.push(raw[at]);
            at += 1;
            continue;
        }
        let s = &raw[at..];
        let mut i = 1;
        if s.len() > 3 && s.get(i) == Some(&b'#') {
            i += 1;
            let hex = matches!(s.get(i), Some(b'x' | b'X'));
            if hex {
                i += 1;
            }
            let mut x = 0i32;
            while i < s.len() {
                let c = s[i];
                i += 1;
                let digit = (c as char).to_digit(if hex { 16 } else { 10 });
                if let Some(n) = digit {
                    x = x
                        .wrapping_mul(if hex { 16 } else { 10 })
                        .wrapping_add(n as i32);
                    continue;
                }
                if c != b';' {
                    i -= 1;
                }
                break;
            }
            if i > 3 {
                let x = if (0x80..=0x9f).contains(&x) {
                    WINDOWS[(x - 0x80) as usize]
                } else {
                    x as u32
                };
                let c = char::from_u32(x)
                    .filter(|c| *c != '\0')
                    .unwrap_or('\u{fffd}');
                let mut b = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                at += i;
                continue;
            }
        } else {
            while i < s.len() {
                let c = s[i];
                i += 1;
                if c.is_ascii_alphanumeric() {
                    continue;
                }
                if c != b';' {
                    i -= 1;
                }
                break;
            }
            if let Ok(name) = std::str::from_utf8(&s[1..i]) {
                if let Some(v) = named(name) {
                    out.extend_from_slice(v.as_bytes());
                    at += i;
                    continue;
                }
                let mut found = None;
                for n in (2..name.len().min(7)).rev() {
                    if let Some(v) = named(&name[..n]) {
                        found = Some((n, v));
                        break;
                    }
                }
                if let Some((n, v)) = found {
                    out.extend_from_slice(v.as_bytes());
                    at += n + 1;
                    continue;
                }
            }
        }
        out.extend_from_slice(&s[..i]);
        at += i;
    }
    control.check()?;
    Ok(out)
}
fn space(c: u8) -> bool {
    b" \t\n\r\x0c".contains(&c)
}
fn tag<'a>(raw: &'a [u8], start: usize) -> Option<&'a [u8]> {
    let mut i = start + 1;
    let ns = i;
    while raw
        .get(i)
        .is_some_and(|c| c.is_ascii_alphanumeric() || b":-".contains(c))
    {
        i += 1;
    }
    if !raw.get(ns..i)?.eq_ignore_ascii_case(b"div") {
        return None;
    }
    let mut id = &b""[..];
    let mut payload = &b""[..];
    while i < raw.len() {
        while raw.get(i).is_some_and(|c| space(*c)) {
            i += 1;
        }
        let c = *raw.get(i)?;
        if c == b'>' {
            break;
        }
        if c == b'/' {
            i += 1;
            continue;
        }
        let ks = i;
        while raw
            .get(i)
            .is_some_and(|c| *c != b'=' && !space(*c) && !b">/".contains(c))
        {
            i += 1;
        }
        let key = raw.get(ks..i)?;
        if key.is_empty() {
            return None;
        }
        while raw.get(i).is_some_and(|c| space(*c)) {
            i += 1;
        }
        let mut value = &b""[..];
        if raw.get(i) == Some(&b'=') {
            i += 1;
            while raw.get(i).is_some_and(|c| space(*c)) {
                i += 1;
            }
            let c = *raw.get(i)?;
            if c == b'\'' || c == b'"' {
                i += 1;
                let vs = i;
                while raw.get(i).is_some_and(|b| *b != c) {
                    i += 1;
                }
                raw.get(i)?;
                value = &raw[vs..i];
                i += 1;
            } else {
                let vs = i;
                while raw.get(i).is_some_and(|b| !space(*b) && *b != b'>') {
                    i += 1;
                }
                value = &raw[vs..i];
            }
        }
        if key.eq_ignore_ascii_case(b"id") {
            id = value;
        } else if key.eq_ignore_ascii_case(b"data-page") {
            payload = value;
        }
    }
    (id == b"app" && !payload.is_empty()).then_some(payload)
}
fn data_page(raw: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    if let Some(marker) = raw.windows(8).position(|v| v == b"id=\"app\"") {
        if let Some(div) = raw[..marker].windows(4).rposition(|v| v == b"<div") {
            if marker - div <= 4096 {
                if let Some(p) = tag(raw, div) {
                    return unescape(p, control);
                }
            }
        }
    }
    for (n, c) in raw.iter().enumerate() {
        if n % 4096 == 0 {
            control.check()?;
        }
        if *c == b'<' {
            if let Some(p) = tag(raw, n) {
                return unescape(p, control);
            }
        }
    }
    Err(protocol("div#app data-page not found"))
}
// Encoding/json structs accept case-folded keys, ordered duplicates and scalar null no-ops.
pub(super) trait Object: Default {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error>;
}
pub(super) struct Seed<'a, T>(pub &'a mut T);
impl<'de, T: Object> DeserializeSeed<'de> for Seed<'_, T> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct V<'a, T>(&'a mut T);
        impl<'de, T: Object> Visitor<'de> for V<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> std::result::Result<(), A::Error> {
                while let Some(k) = m.next_key::<String>()? {
                    let k: String = k
                        .chars()
                        .map(|c| match c {
                            '\u{017f}' => 's',
                            '\u{212a}' => 'k',
                            _ => c.to_ascii_lowercase(),
                        })
                        .collect();
                    self.0.field(&k, &mut m)?;
                }
                Ok(())
            }
        }
        d.deserialize_any(V(self.0))
    }
}
pub(super) fn scalar<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    m: &mut A,
    out: &mut T,
) -> std::result::Result<(), A::Error> {
    if let Some(v) = m.next_value::<Option<T>>()? {
        *out = v;
    }
    Ok(())
}
pub(super) fn ignored<'de, A: MapAccess<'de>>(m: &mut A) -> std::result::Result<(), A::Error> {
    m.next_value::<serde::de::IgnoredAny>().map(|_| ())
}
pub(super) fn pointer<'de, A: MapAccess<'de>, T: Object>(
    m: &mut A,
    out: &mut Option<T>,
) -> std::result::Result<(), A::Error> {
    let raw = m.next_value::<Option<Box<RawValue>>>()?;
    match raw {
        None => {
            *out = None;
            Ok(())
        }
        Some(raw) => {
            let mut d = serde_json::Deserializer::from_str(raw.get());
            d.disable_recursion_limit();
            Seed(out.get_or_insert_with(T::default))
                .deserialize(&mut d)
                .map_err(serde::de::Error::custom)
        }
    }
}
#[derive(Default)]
struct RowBuffer {
    slots: Vec<Row>,
    len: usize,
}
struct Rows<'a>(&'a mut RowBuffer);
impl<'de> DeserializeSeed<'de> for Rows<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct V<'a>(&'a mut RowBuffer);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an array or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                self.0.slots.clear();
                self.0.len = 0;
                Ok(())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
                let mut n = 0;
                loop {
                    if n == self.0.slots.len() {
                        self.0.slots.push(Row::default());
                    }
                    if seq.next_element_seed(Seed(&mut self.0.slots[n]))?.is_none() {
                        break;
                    }
                    n += 1;
                }
                self.0.len = n;
                // encoding/json resets empty arrays; nonempty shrink retains backing slots.
                if n == 0 {
                    self.0.slots.clear();
                }
                Ok(())
            }
        }
        d.deserialize_any(V(self.0))
    }
}
macro_rules! object_decode {
    ($ty:ty) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let mut v = Self::default();
                Seed(&mut v).deserialize(d)?;
                Ok(v)
            }
        }
    };
}
#[derive(Default)]
struct Root {
    component: String,
    props: Props,
}
#[derive(Default)]
struct Props {
    hotfixes: Option<Pagination>,
}
#[derive(Default)]
struct Pagination {
    current_page: i64,
    last_page: i64,
    per_page: i64,
    total: FlexInt,
    data: RowBuffer,
    next_page_url: String,
}
#[derive(Default)]
struct Row {
    id: FlexInt,
    push: FlexInt,
    record_id: FlexInt,
    status: FlexInt,
    build: FlexText,
    table_name: String,
    data: Option<Box<RawValue>>,
    created_at: String,
    region_id: FlexInt,
    locale: String,
}
#[derive(Default)]
struct FlexInt(i64);
impl<'de> Deserialize<'de> for FlexInt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        let t = raw.get();
        if t == "null" {
            return Ok(Self(0));
        }
        let text = if t.starts_with('"') {
            serde_json::from_str::<String>(t).map_err(serde::de::Error::custom)?
        } else {
            t.into()
        };
        text.parse()
            .map(Self)
            .map_err(|_| serde::de::Error::custom(format!("numeric field {text:?}")))
    }
}
#[derive(Default)]
struct FlexText(String);
impl<'de> Deserialize<'de> for FlexText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        let t = raw.get();
        Ok(Self(if t == "null" {
            String::new()
        } else if t.starts_with('"') {
            serde_json::from_str(t).map_err(serde::de::Error::custom)?
        } else {
            t.into()
        }))
    }
}
impl Object for Root {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "component" => scalar(m, &mut self.component),
            "props" => m.next_value_seed(Seed(&mut self.props)),
            _ => ignored(m),
        }
    }
}
impl Object for Props {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "hotfixes" => pointer(m, &mut self.hotfixes),
            _ => ignored(m),
        }
    }
}
impl Object for Pagination {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "current_page" => scalar(m, &mut self.current_page),
            "last_page" => scalar(m, &mut self.last_page),
            "per_page" => scalar(m, &mut self.per_page),
            "total" => {
                self.total = m.next_value()?;
                Ok(())
            }
            "data" => m.next_value_seed(Rows(&mut self.data)),
            "next_page_url" => scalar(m, &mut self.next_page_url),
            _ => ignored(m),
        }
    }
}
impl Object for Row {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        k: &str,
        m: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match k {
            "id" => {
                self.id = m.next_value()?;
                Ok(())
            }
            "push_id" => {
                self.push = m.next_value()?;
                Ok(())
            }
            "record_id" => {
                self.record_id = m.next_value()?;
                Ok(())
            }
            "status" => {
                self.status = m.next_value()?;
                Ok(())
            }
            "build" => {
                self.build = m.next_value()?;
                Ok(())
            }
            "table_name" => scalar(m, &mut self.table_name),
            "data" => {
                self.data = m.next_value()?;
                Ok(())
            }
            "created_at" => scalar(m, &mut self.created_at),
            "region_id" => {
                self.region_id = m.next_value()?;
                Ok(())
            }
            "locale" => scalar(m, &mut self.locale),
            _ => ignored(m),
        }
    }
}
object_decode!(Root);
object_decode!(Props);
object_decode!(Pagination);
object_decode!(Row);
pub(super) fn parse<'a>(
    raw: &[u8],
    budget: &'a Budget,
    control: &OperationControl,
) -> Result<Page<'a>> {
    control.check()?;
    let allocation = budget.reserve_retained(raw.len().saturating_mul(8))?;
    let embedded = data_page(raw, control)?;
    // Go Unmarshal validates the entire document, including ignored members and raw payload.
    if !lycheedev_core::raw_json::is_valid(&embedded) {
        return Err(protocol("data-page is invalid JSON"));
    }
    let mut decoder = serde_json::Deserializer::from_slice(&embedded);
    decoder.disable_recursion_limit();
    let root = Root::deserialize(&mut decoder)
        .map_err(|e| protocol(format!("data-page is invalid JSON: {e}")))?;
    control.check()?;
    if root.component != "Hotfixes" {
        return Err(protocol(format!(
            "component {:?} is not Hotfixes",
            root.component
        )));
    }
    let p = root
        .props
        .hotfixes
        .ok_or_else(|| protocol("props.hotfixes is missing"))?;
    if p.current_page < 1 || p.last_page < p.current_page || p.per_page < 1 || p.total.0 < 0 {
        return Err(protocol(format!(
            "invalid pagination {}/{}/{}/{}",
            p.current_page, p.last_page, p.per_page, p.total.0
        )));
    }
    let mut page = Page {
        current_page: p.current_page,
        last_page: p.last_page,
        per_page: p.per_page,
        total: p.total.0,
        next_page_url: p.next_page_url,
        records: Vec::with_capacity(p.data.len),
        _retained: Some(allocation),
    };
    for row in p.data.slots.into_iter().take(p.data.len) {
        budget.check()?;
        if row.id.0 < 0
            || !(0..=u32::MAX as i64).contains(&row.record_id.0)
            || !(0..=u32::MAX as i64).contains(&row.region_id.0)
            || !(0..=255).contains(&row.status.0)
        {
            return Err(protocol(format!(
                "record id {} has out of range fields",
                row.id.0
            )));
        }
        let push = i32::try_from(row.push.0)
            .map_err(|_| protocol(format!("push {} does not fit int32", row.push.0)))?;
        if row.table_name.trim().is_empty() || row.build.0.trim().is_empty() {
            return Err(protocol(format!(
                "record id {} has no table name or build",
                row.id.0
            )));
        }
        let payload = row.data.filter(|v| v.get() != "null");
        page.records.push(Record {
            provider: "wago".into(),
            id: row.id.0 as u64,
            push,
            record_id: row.record_id.0 as u32,
            status: row.status.0 as u8,
            build: row.build.0,
            table_name: row.table_name,
            payload_length: payload.as_ref().map_or(0, |v| v.get().len()),
            payload,
            created_at: row.created_at,
            region_id: Some(row.region_id.0 as u32),
            locale: row.locale,
            ..Default::default()
        });
    }
    Ok(page)
}
fn unquote_query(raw: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    let bytes = raw.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let a = (*bytes.get(i + 1)? as char).to_digit(16)?;
                let b = (*bytes.get(i + 2)? as char).to_digit(16)?;
                out.push((a * 16 + b) as u8);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(out)
}
fn valid_escapes(raw: &str) -> bool {
    unquote_query(raw).is_some()
}
fn pointed_page(raw: &str) -> std::result::Result<Option<i64>, ()> {
    if raw.bytes().any(|b| b < 32 || b == 127) {
        return Err(());
    }
    let (before_fragment, fragment) = raw
        .split_once('#')
        .map_or((raw, None), |(a, b)| (a, Some(b)));
    if fragment.is_some_and(|v| !valid_escapes(v)) {
        return Err(());
    }
    let (mut path, query) = before_fragment
        .split_once('?')
        .unwrap_or((before_fragment, ""));
    let first = path.split('/').next().unwrap_or_default();
    if let Some((scheme, _)) = first.split_once(':') {
        if scheme.is_empty()
            || !scheme.as_bytes()[0].is_ascii_alphabetic()
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        {
            return Err(());
        }
        path = &path[scheme.len() + 1..];
    }
    if let Some(rest) = path.strip_prefix("//") {
        let host = rest.split('/').next().unwrap_or_default();
        if host.bytes().any(|b| b == b' ' || b == b'\\') {
            return Err(());
        }
        let host = host.rsplit('@').next().unwrap_or_default();
        if let Some(host) = host.strip_prefix('[') {
            let Some((_, port)) = host.split_once(']') else {
                return Err(());
            };
            if !port.is_empty()
                && (!port.starts_with(':') || !port[1..].bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(());
            }
        } else if let Some((_, port)) = host.rsplit_once(':') {
            if !port.bytes().all(|b| b.is_ascii_digit()) {
                return Err(());
            }
        }
    }
    if !valid_escapes(path) {
        return Err(());
    }
    // net/url.Query ignores each malformed/semicolon pair independently and Get uses the first value.
    for pair in query.split('&') {
        if pair.contains(';') {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (Some(key), Some(value)) = (unquote_query(key), unquote_query(value)) else {
            continue;
        };
        if key == b"page" {
            return Ok(std::str::from_utf8(&value)
                .ok()
                .and_then(|v| v.parse().ok()));
        }
    }
    Ok(None)
}
pub(super) fn sequence(previous: &CursorPage, next: &Page) -> Result<()> {
    if next.total != previous.total || next.last_page != previous.last_page {
        return Err(error(
            "page_drift",
            4,
            format!(
                "total/last_page drift {}/{} -> {}/{}",
                previous.total, previous.last_page, next.total, next.last_page
            ),
        ));
    }
    if next.current_page <= previous.page {
        return Err(error(
            "page_drift",
            4,
            format!(
                "duplicate or reversed page {} after {}",
                next.current_page, previous.page
            ),
        ));
    }
    if next.current_page != previous.page + 1 {
        return Err(error(
            "page_drift",
            4,
            format!(
                "missing page between {} and {}",
                previous.page, next.current_page
            ),
        ));
    }
    if previous.page < previous.last_page {
        if previous.next_page_url.trim().is_empty() {
            return Err(error(
                "page_drift",
                4,
                format!("missing next_page_url after page {}", previous.page),
            ));
        }
        let pointed = pointed_page(&previous.next_page_url).map_err(|_| {
            error(
                "page_drift",
                4,
                format!("invalid next_page_url {:?}", previous.next_page_url),
            )
        })?;
        if pointed != Some(next.current_page) {
            return Err(error(
                "page_drift",
                4,
                format!(
                    "next_page_url of page {} points to {:?}, expected page {}",
                    previous.page, previous.next_page_url, next.current_page
                ),
            ));
        }
    }
    Ok(())
}

impl fmt::Debug for Page<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WagoPage")
            .field("current_page", &self.current_page)
            .field("last_page", &self.last_page)
            .field("records", &self.records)
            .finish()
    }
}
