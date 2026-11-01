//! Read the two named objects in the fixed CPython 3.7.17 GC graph. Values and
//! object addresses remain private to the channel reader; no Python is executed.
use super::fault;
use lycheedev_core::{Result, control::OperationControl};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
pub(super) const RUNTIME_RVA: u64 = 3_639_528;
const LIMIT: u64 = 0x0000_8000_0000_0000;
pub(super) trait Memory {
    fn read(&mut self, address: u64, size: usize) -> Result<Vec<u8>>;
}
pub(super) struct Reader<'a, M> {
    memory: &'a mut M,
    control: &'a OperationControl,
    types: BTreeMap<u64, String>,
}
// No Debug/Serialize: all credentials and managed addresses belong only to the
// process completing this one explicit channel operation.
pub(super) struct Snapshot {
    pub(super) cached: String,
    pub(super) login_token: String,
    pub(super) uid: String,
    pub(super) eid: String,
    pub(super) ccid: String,
    pub(super) delta: f64,
    controller: u64,
    helper: u64,
    center: u64,
    session: u64,
    session_value: String,
    token: u64,
    data_center: u64,
    user: u64,
    login: u64,
    delta_object: u64,
}
enum Scalar {
    Text(String),
    Integer(i128),
    Float(f64),
}
fn bad<T>() -> Result<T> {
    Err(fault(
        "the existing DD managed runtime does not match its supported session layout",
    ))
}
impl<'a, M: Memory> Reader<'a, M> {
    pub(super) fn new(memory: &'a mut M, control: &'a OperationControl) -> Self {
        Self {
            memory,
            control,
            types: BTreeMap::new(),
        }
    }
    fn bytes(&mut self, address: u64, size: usize) -> Result<Vec<u8>> {
        self.control.check()?;
        if address < 0x10000
            || size == 0
            || size > 1 << 20
            || address
                .checked_add(size as u64)
                .is_none_or(|end| end > LIMIT)
        {
            return bad();
        }
        let value = self.memory.read(address, size)?;
        if value.len() != size {
            return bad();
        }
        self.control.check()?;
        Ok(value)
    }
    fn pointer(&mut self, address: u64) -> Result<u64> {
        let value = u64::from_le_bytes(self.bytes(address, 8)?.try_into().unwrap());
        if value != 0 && !(0x10000..LIMIT).contains(&value) {
            return bad();
        }
        Ok(value)
    }
    fn integer(&mut self, address: u64) -> Result<i64> {
        Ok(i64::from_le_bytes(
            self.bytes(address, 8)?.try_into().unwrap(),
        ))
    }
    fn type_name(&mut self, object: u64) -> Result<String> {
        let typ = self.pointer(object + 8)?;
        if let Some(value) = self.types.get(&typ) {
            return Ok(value.clone());
        }
        let name = self.pointer(typ + 24)?;
        let bytes = self.bytes(name, 128)?;
        let size = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let value = String::from_utf8_lossy(&bytes[..size]).into_owned();
        self.types.insert(typ, value.clone());
        Ok(value)
    }
    fn text(&mut self, object: u64) -> Result<String> {
        if self.type_name(object)? != "str" {
            return bad();
        }
        let header = self.bytes(object, 48)?;
        let length = i64::from_le_bytes(header[16..24].try_into().unwrap());
        let state = u32::from_le_bytes(header[32..36].try_into().unwrap());
        let kind = (state >> 2) & 7;
        let compact = state & (1 << 5) != 0;
        let ascii = state & (1 << 6) != 0;
        if !(0..=16384).contains(&length)
            || ![1, 2, 4].contains(&kind)
            || (ascii && (kind != 1 || !compact))
        {
            return bad();
        }
        if length == 0 {
            return Ok(String::new());
        }
        let data = if compact {
            object + if ascii { 48 } else { 72 }
        } else {
            self.pointer(object + 72)?
        };
        let bytes = self.bytes(data, length as usize * kind as usize)?;
        match kind {
            1 => Ok(bytes.into_iter().map(char::from).collect()),
            2 => String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| fault("DD managed text is not valid Unicode")),
            4 => bytes
                .chunks_exact(4)
                .map(|b| {
                    char::from_u32(u32::from_le_bytes(b.try_into().unwrap()))
                        .ok_or_else(|| fault("DD managed text is not valid Unicode"))
                })
                .collect(),
            _ => unreachable!("validated Unicode width"),
        }
    }
    fn dict(&mut self, object: u64, wanted: &str) -> Result<u64> {
        if self.type_name(object)? != "dict" {
            return bad();
        }
        let keys = self.pointer(object + 32)?;
        let values = self.pointer(object + 40)?;
        let size = self.integer(keys + 8)?;
        let count = self.integer(keys + 32)?;
        if size <= 0
            || size > 1_048_576
            || (size as u64).count_ones() != 1
            || !(0..=16384).contains(&count)
        {
            return bad();
        }
        let width = if size <= 255 {
            1
        } else if size <= 65535 {
            2
        } else {
            4
        };
        let entries = keys + 40 + size as u64 * width;
        for index in 0..count as u64 {
            let key = self.pointer(entries + index * 24 + 8)?;
            if key != 0 && self.text(key)? == wanted {
                return self.pointer(if values == 0 {
                    entries + index * 24 + 16
                } else {
                    values + index * 8
                });
            }
        }
        Ok(0)
    }
    fn attribute(&mut self, object: u64, name: &str) -> Result<u64> {
        let typ = self.pointer(object + 8)?;
        let mut offset = i128::from(self.integer(typ + 288)?);
        if offset < 0 {
            let basic = self.integer(typ + 32)?;
            let item = self.integer(typ + 40)?;
            if basic < 0 || item < 0 {
                return bad();
            }
            let count = if item == 0 {
                0
            } else {
                self.integer(object + 16)?.unsigned_abs()
            };
            let Some(total) = count
                .checked_mul(item as u64)
                .and_then(|v| v.checked_add(basic as u64))
                .and_then(|v| v.checked_add(7))
            else {
                return bad();
            };
            offset += i128::from(total & !7);
        }
        if !(1..65536).contains(&offset) {
            return bad();
        }
        let dictionary = self.pointer(object + offset as u64)?;
        if dictionary == 0 {
            Ok(0)
        } else {
            self.dict(dictionary, name)
        }
    }
    fn scalar(&mut self, object: u64) -> Result<Scalar> {
        match self.type_name(object)?.as_str() {
            "str" => self.text(object).map(Scalar::Text),
            "float" => {
                let value = f64::from_le_bytes(self.bytes(object + 16, 8)?.try_into().unwrap());
                if !value.is_finite() {
                    return bad();
                }
                Ok(Scalar::Float(value))
            }
            "int" => {
                let size = self.integer(object + 16)?;
                if !(-4..=4).contains(&size) {
                    return bad();
                }
                let mut value = 0u128;
                for index in 0..size.unsigned_abs() {
                    let digit = u32::from_le_bytes(
                        self.bytes(object + 24 + index * 4, 4)?.try_into().unwrap(),
                    );
                    if digit >= 1 << 30 {
                        return bad();
                    }
                    value |= u128::from(digit) << (30 * index);
                }
                let value = value as i128;
                Ok(Scalar::Integer(if size < 0 { -value } else { value }))
            }
            _ => bad(),
        }
    }
    fn id(&mut self, object: u64) -> Result<String> {
        let value = match self.scalar(object)? {
            Scalar::Text(v) => v,
            Scalar::Integer(v) => v.to_string(),
            Scalar::Float(_) => return bad(),
        };
        if value.is_empty() || value.len() > 30 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return bad();
        }
        Ok(value)
    }
    fn delta(&mut self, object: u64) -> Result<f64> {
        let value = match self.scalar(object)? {
            Scalar::Integer(v) => v as f64,
            Scalar::Float(v) => v,
            Scalar::Text(_) => return bad(),
        };
        if !(-86400.0..=86400.0).contains(&value) {
            return bad();
        }
        Ok(value)
    }
    fn class_module(&mut self, object: u64) -> Result<String> {
        let typ = self.pointer(object + 8)?;
        let dictionary = self.pointer(typ + 264)?;
        let value = self.dict(dictionary, "__module__")?;
        if value == 0 {
            Ok(String::new())
        } else {
            self.text(value)
        }
    }
    fn logged_in(&mut self, center: u64) -> Result<()> {
        if self.type_name(center)? != "MessageCenter"
            || self.class_module(center)? != "components.net_server.messagecenter"
        {
            return bad();
        }
        let state = self.attribute(center, "state")?;
        if state == 0 || self.class_module(state)? != "components.net_server.messagecenter" {
            return bad();
        }
        let name = self.attribute(state, "_name_")?;
        let value = self.attribute(state, "_value_")?;
        if self.text(name)? != "LoggedIn" || !matches!(self.scalar(value)?, Scalar::Integer(3)) {
            return bad();
        }
        Ok(())
    }
    fn objects(&mut self, base: u64) -> Result<(u64, u64)> {
        let runtime = base
            .checked_add(RUNTIME_RVA)
            .ok_or_else(|| fault("DD managed runtime address is invalid"))?;
        if self.bytes(runtime, 4)? != 1i32.to_le_bytes()
            || self.pointer(runtime + 432)? != runtime + 336
        {
            return bad();
        }
        let mut controller = None;
        let mut helper = None;
        let mut seen = BTreeSet::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        for generation in 0..3 {
            let head = runtime + 336 + generation * 32;
            let mut node = self.pointer(head)?;
            while node != head {
                self.control.check()?;
                if node == 0
                    || seen.len() >= 300000
                    || !seen.insert(node)
                    || Instant::now() >= deadline
                {
                    return bad();
                }
                let next = self.pointer(node)?;
                let object = node + 24;
                let name = self.type_name(object)?;
                let selected = match name.as_str() {
                    "LoginController" => {
                        Some(("components.login.login_controller", &mut controller))
                    }
                    "JwtHelper" => Some(("util.cgi.jwt_helper", &mut helper)),
                    _ => None,
                };
                if let Some((module, target)) = selected {
                    if self.class_module(object)? == module && target.replace(object).is_some() {
                        return bad();
                    }
                }
                node = next;
            }
        }
        match (controller, helper) {
            (Some(controller), Some(helper)) => Ok((controller, helper)),
            _ => bad(),
        }
    }
    fn cached(&mut self, object: u64) -> Result<String> {
        if object == 0 || self.type_name(object)? == "NoneType" {
            return Ok(String::new());
        }
        let value = self.text(object)?;
        if value.chars().any(char::is_control) {
            return bad();
        }
        Ok(value)
    }
    pub(super) fn snapshot(&mut self, base: u64) -> Result<Snapshot> {
        let (controller, helper) = self.objects(base)?;
        let center = self.attribute(controller, "_messageCenter")?;
        if center == 0 || self.attribute(helper, "_msgCenter")? != center {
            return bad();
        }
        self.logged_in(center)?;
        let session = self.attribute(controller, "_session")?;
        let session_value = self.text(session)?;
        if session_value.is_empty() {
            return bad();
        }
        let token = self.attribute(helper, "_jwt")?;
        let cached = self.cached(token)?;
        let data_center = self.attribute(helper, "_dataCenter")?;
        let user = self.attribute(data_center, "_myUserInfo")?;
        let login = self.attribute(helper, "_loginToken")?;
        let login_token = self.text(login)?;
        let delta_object = self.attribute(helper, "_server_time_delta")?;
        let delta = self.delta(delta_object)?;
        if login_token.is_empty() || login_token.chars().any(char::is_control) {
            return bad();
        }
        let uid = self.attribute(user, "uid")?;
        let uid = self.id(uid)?;
        let eid = self.attribute(user, "eid")?;
        let eid = self.id(eid)?;
        let ccid = self.attribute(user, "ccid")?;
        let ccid = self.id(ccid)?;
        let snapshot = Snapshot {
            cached,
            login_token,
            uid,
            eid,
            ccid,
            delta,
            controller,
            helper,
            center,
            session,
            session_value,
            token,
            data_center,
            user,
            login,
            delta_object,
        };
        self.verify(&snapshot)?;
        Ok(snapshot)
    }
    pub(super) fn verify(&mut self, s: &Snapshot) -> Result<()> {
        if self.attribute(s.controller, "_messageCenter")? != s.center
            || self.attribute(s.helper, "_msgCenter")? != s.center
            || self.attribute(s.controller, "_session")? != s.session
            || self.text(s.session)? != s.session_value
            || self.attribute(s.helper, "_jwt")? != s.token
            || self.cached(s.token)? != s.cached
            || self.attribute(s.helper, "_dataCenter")? != s.data_center
            || self.attribute(s.data_center, "_myUserInfo")? != s.user
            || self.attribute(s.helper, "_loginToken")? != s.login
            || self.text(s.login)? != s.login_token
            || self.attribute(s.helper, "_server_time_delta")? != s.delta_object
            || self.delta(s.delta_object)? != s.delta
        {
            return Err(fault(
                "the existing DD GUI session changed during the channel read",
            ));
        }
        self.logged_in(s.center)?;
        for (key, expected) in [("uid", &s.uid), ("eid", &s.eid), ("ccid", &s.ccid)] {
            let object = self.attribute(s.user, key)?;
            if self.id(object)? != *expected {
                return Err(fault(
                    "the existing DD GUI account changed during the channel read",
                ));
            }
        }
        self.control.check()
    }
}

#[cfg(test)]
pub(super) mod tests;
