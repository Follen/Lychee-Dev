//! Go token-walk JSON admission, with no floating-point number conversion.
//! Duplicate keys compare decoded Go strings exactly (case-sensitive), while
//! typed signal fields subsequently use the existing Go case-folding decoder.
use super::*;
use std::collections::HashSet;

pub(super) fn validate(
    data: &[u8],
    depth_limit: usize,
    entry_limit: usize,
    control: &OperationControl,
) -> Result<()> {
    let mut cursor = Cursor {
        data,
        at: 0,
        next_check: 0,
        entries: 0,
        depth_limit,
        entry_limit,
        control,
    };
    cursor.value(0)?;
    cursor.space()?;
    if cursor.at != data.len() {
        return Err(error("bridge.json_trailing_data"));
    }
    control.check()
}
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
    next_check: usize,
    entries: usize,
    depth_limit: usize,
    entry_limit: usize,
    control: &'a OperationControl,
}
impl Cursor<'_> {
    fn check(&mut self) -> Result<()> {
        if self.at >= self.next_check {
            self.control.check()?;
            self.next_check = self.at.saturating_add(4096);
        }
        Ok(())
    }
    fn bad(&self) -> ToolkitError {
        syntax_error(format!("invalid JSON at byte {}", self.at))
    }
    fn space(&mut self) -> Result<()> {
        while self
            .data
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.check()?;
            self.at += 1;
        }
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<()> {
        self.control.check()?;
        if depth > self.depth_limit {
            return Err(error("bridge.json_depth_limit"));
        }
        self.space()?;
        match self.data.get(self.at).copied() {
            Some(b'{' | b'[') => self.container(depth),
            Some(b'"') => self.string().map(|_| ()),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.bad()),
        }
    }
    fn literal(&mut self, token: &[u8]) -> Result<()> {
        if !self.data[self.at..].starts_with(token) {
            return Err(self.bad());
        }
        self.at += token.len();
        Ok(())
    }
    fn container(&mut self, depth: usize) -> Result<()> {
        let object = self.data[self.at] == b'{';
        let close = if object { b'}' } else { b']' };
        self.at += 1;
        self.space()?;
        if self.data.get(self.at) == Some(&close) {
            self.at += 1;
            return Ok(());
        }
        let mut keys = HashSet::new();
        loop {
            self.entries += 1;
            if self.entries > self.entry_limit {
                return Err(error("bridge.json_entry_limit"));
            }
            if object {
                let name = self.string()?;
                if !keys.insert(name) {
                    return Err(error("bridge.json_duplicate_or_invalid_key"));
                }
                self.space()?;
                if self.data.get(self.at) != Some(&b':') {
                    return Err(self.bad());
                }
                self.at += 1;
            }
            self.value(depth + 1)?;
            self.space()?;
            match self.data.get(self.at) {
                Some(c) if *c == close => {
                    self.at += 1;
                    return Ok(());
                }
                Some(b',') => {
                    self.at += 1;
                    self.space()?;
                }
                _ => return Err(self.bad()),
            }
        }
    }
    fn string(&mut self) -> Result<String> {
        self.check()?;
        if self.data.get(self.at) != Some(&b'"') {
            return Err(self.bad());
        }
        let start = self.at;
        self.at += 1;
        loop {
            self.check()?;
            match self.data.get(self.at).copied() {
                Some(b'"') => {
                    self.at += 1;
                    // Go replaces unpaired UTF-16 escapes with U+FFFD. Only
                    // this token is normalized; the original report stays raw.
                    let token = lycheedev_core::evidence::normalize_go_json_strings(
                        &self.data[start..self.at],
                        self.control,
                    )?;
                    return serde_json::from_slice(&token).map_err(syntax_error);
                }
                Some(b'\\') => {
                    self.at += 1;
                    match self.data.get(self.at).copied() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.at += 1
                        }
                        Some(b'u') => {
                            self.at += 1;
                            for _ in 0..4 {
                                if !self.data.get(self.at).is_some_and(u8::is_ascii_hexdigit) {
                                    return Err(self.bad());
                                }
                                self.at += 1;
                            }
                        }
                        _ => return Err(self.bad()),
                    }
                }
                Some(0x20..=0xff) => self.at += 1,
                _ => return Err(self.bad()),
            }
        }
    }
    fn digits(&mut self) -> Result<usize> {
        let start = self.at;
        while self.data.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.check()?;
            self.at += 1;
        }
        Ok(self.at - start)
    }
    fn number(&mut self) -> Result<()> {
        if self.data.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        match self.data.get(self.at) {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits()?;
            }
            _ => return Err(self.bad()),
        }
        if self.data.get(self.at) == Some(&b'.') {
            self.at += 1;
            if self.digits()? == 0 {
                return Err(self.bad());
            }
        }
        if matches!(self.data.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.data.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits()? == 0 {
                return Err(self.bad());
            }
        }
        Ok(())
    }
}
