use super::*;
// API detail prices are yuan; write prices are integer fen. No f64 round trip.
pub(super) fn yuan(text: &str) -> Result<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(invalid(
            "$.price_yuan",
            "provide a nonnegative decimal yuan string without an exponent",
        ));
    }
    decimal(text).ok_or_else(|| {
        invalid(
            "$.price_yuan",
            "decimal yuan amount exceeds the integer-fen range or is malformed",
        )
    })
}
pub(super) fn wire(value: &Value) -> Result<u64> {
    let text = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err(data("official price omitted its decimal yuan amount")),
    };
    let (base, exponent) = match text.split_once(['e', 'E']) {
        Some((base, exp)) => (
            base,
            exp.parse::<i32>()
                .map_err(|_| data("official price exponent is invalid"))?,
        ),
        None => (text.as_str(), 0),
    };
    scaled(base, exponent).ok_or_else(|| {
        data("official decimal yuan price exceeds integer-fen range or is malformed")
    })
}
fn decimal(text: &str) -> Option<u64> {
    scaled(text, 0)
}
fn scaled(text: &str, exponent: i32) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || text.ends_with('.')
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    let shift = 2i64
        .checked_add(exponent as i64)?
        .checked_sub(fraction.len().try_into().ok()?)?;
    if shift >= 0 {
        if digits.len() as i64 + shift > 20 {
            return None;
        }
        let mut value = digits.parse::<u64>().ok()?;
        for _ in 0..shift {
            value = value.checked_mul(10)?;
        }
        Some(value)
    } else {
        let remove = usize::try_from(-shift).ok()?;
        if remove > digits.len() {
            return Some(0);
        }
        let cut = digits.len() - remove;
        let round = digits.as_bytes()[cut] >= b'5';
        let value = if cut == 0 {
            0
        } else {
            digits[..cut].parse::<u64>().ok()?
        };
        value.checked_add(u64::from(round))
    }
}
