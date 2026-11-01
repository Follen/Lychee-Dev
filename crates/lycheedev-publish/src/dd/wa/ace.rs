use super::*;

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    arena: Arena<'a>,
}
pub(super) fn parse(bytes: &[u8], control: &OperationControl) -> Result<WaIdentity> {
    // AceSerializer strips ASCII controls and spaces before tokenization.
    let bytes: Vec<_> = bytes
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_control() && *byte != b' ')
        .collect();
    let mut parser = Parser {
        bytes: &bytes,
        position: 0,
        arena: Arena::new(control),
    };
    if parser.token()?.0 != b'1' {
        return Err(invalid(
            "serialization",
            "unsupported AceSerializer revision",
        ));
    }
    let token = parser.token()?;
    if token.0 == b'^' {
        return Err(invalid("identity", "WA export has no value"));
    }
    let root = parser.value(token, 0)?;
    loop {
        let token = parser.token()?;
        if token.0 == b'^' {
            break;
        }
        parser.value(token, 0)?;
    }
    parser.arena.finish(root)
}
impl<'a> Parser<'a> {
    fn token(&mut self) -> Result<(u8, &'a [u8])> {
        let start = self.bytes[self.position..]
            .iter()
            .position(|byte| *byte == b'^')
            .map(|offset| offset + self.position)
            .ok_or_else(|| invalid("serialization", "AceSerializer terminator is missing"))?;
        let code = *self
            .bytes
            .get(start + 1)
            .ok_or_else(|| invalid("serialization", "AceSerializer control code is truncated"))?;
        let start = start + 2;
        let end = self.bytes[start..]
            .iter()
            .position(|byte| *byte == b'^')
            .map(|offset| start + offset)
            .unwrap_or(self.bytes.len());
        self.position = end;
        Ok((code, &self.bytes[start..end]))
    }
    fn value(&mut self, (code, bytes): (u8, &'a [u8]), depth: usize) -> Result<Value> {
        self.arena.tick(depth)?;
        match code {
            b'S' => Ok(self.arena.string(unescape(bytes)?)),
            b'N' => Ok(Value::Number(number(bytes)?)),
            b'F' => {
                let mantissa = number(bytes)?;
                let (code, bytes) = self.token()?;
                if code != b'f' {
                    return Err(invalid(
                        "serialization",
                        "AceSerializer float exponent is missing",
                    ));
                }
                Ok(Value::Number(mantissa * 2f64.powf(number(bytes)?)))
            }
            b'B' | b'b' => Ok(Value::Bool),
            b'Z' => Ok(Value::Nil),
            b'T' => {
                let table = self.arena.table();
                loop {
                    let token = self.token()?;
                    if token.0 == b't' {
                        break;
                    }
                    let key = self.value(token, depth + 1)?;
                    let token = self.token()?;
                    let value = self.value(token, depth + 1)?;
                    if matches!(key, Value::Nil) || matches!(value, Value::Nil) {
                        return Err(invalid(
                            "serialization",
                            "AceSerializer table has a missing key or value",
                        ));
                    }
                    self.arena.assign(table, key, value)?;
                }
                Ok(Value::Table(table))
            }
            _ => Err(invalid(
                "serialization",
                "invalid AceSerializer control code",
            )),
        }
    }
}
fn unescape(bytes: &[u8]) -> Result<Cow<'_, [u8]>> {
    if !bytes.contains(&b'~') {
        return Ok(Cow::Borrowed(bytes));
    }
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let mut byte = bytes[index];
        index += 1;
        if byte == b'~' && index < bytes.len() {
            byte = match bytes[index] {
                64..=121 => bytes[index] - 64,
                b'z' => 30,
                b'{' => 127,
                b'|' => 126,
                b'}' => 94,
                _ => {
                    return Err(invalid(
                        "serialization",
                        "invalid AceSerializer string escape",
                    ));
                }
            };
            index += 1;
        }
        output.push(byte);
    }
    Ok(Cow::Owned(output))
}
