//! Explicit Go 3.2.3 scalar conversion and Unicode LIKE, using the same query
//! work/control and independent allocation safety ledger as the row executor.
use super::*;
use std::borrow::Cow;

pub(super) fn decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    let before = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let mut digits = at - before;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let before = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        digits += at - before;
    }
    if digits == 0 {
        return false;
    }
    if bytes.get(at).is_some_and(|b| matches!(b, b'e' | b'E')) {
        at += 1;
        if bytes.get(at).is_some_and(|b| matches!(b, b'+' | b'-')) {
            at += 1;
        }
        let before = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == before {
            return false;
        }
    }
    at == bytes.len()
}
pub(super) fn upper(name: &str) -> String {
    name.chars()
        .map(|c| crate::unicode_simple::map(c, true))
        .collect()
}
pub(super) fn name(name: &ObjectName) -> Result<String> {
    let [ObjectNamePart::Identifier(name)] = name.0.as_slice() else {
        return Err(unsupported());
    };
    Ok(upper(&name.value))
}
pub(super) fn cast_name(kind: &DataType) -> Result<String> {
    match kind {
        DataType::Custom(object, modifiers) if modifiers.is_empty() => name(object),
        _ => Ok(upper(&kind.to_string())),
    }
}
pub(super) fn cast_kind(kind: &str) -> Result<()> {
    if matches!(
        kind,
        "STRING"
            | "TEXT"
            | "VARCHAR"
            | "INT"
            | "INTEGER"
            | "BIGINT"
            | "FLOAT"
            | "DOUBLE"
            | "REAL"
            | "BOOL"
            | "BOOLEAN"
    ) {
        Ok(())
    } else {
        Err(unsupported())
    }
}
/// Shared with CSV: Go fmt.Sprint's shortest binary64 spelling and exponent
/// thresholds, while preserving complete signed/unsigned integer spelling.
pub(crate) fn text(value: &Scalar) -> Result<Cow<'_, str>> {
    Ok(match value {
        Scalar::Null => Cow::Borrowed(""),
        Scalar::String(s) => Cow::Borrowed(s),
        Scalar::Bool(true) => Cow::Borrowed("true"),
        Scalar::Bool(false) => Cow::Borrowed("false"),
        Scalar::Int(n) => Cow::Owned(n.to_string()),
        Scalar::UInt(n) => Cow::Owned(n.to_string()),
        Scalar::Float(n) => {
            if !n.is_finite() {
                return Err(numeric());
            }
            let absolute = n.abs();
            let text = if absolute != 0.0 && !(1e-4..1e6).contains(&absolute) {
                let scientific = format!("{n:e}");
                let (mantissa, exponent) = scientific.split_once('e').ok_or_else(numeric)?;
                let exponent: i32 = exponent.parse().map_err(|_| numeric())?;
                format!("{mantissa}e{exponent:+03}")
            } else {
                n.to_string()
            };
            Cow::Owned(text)
        }
    })
}
pub(super) fn cast(value: Scalar, kind: &str, work: &mut Work<'_>) -> Result<Scalar> {
    cast_kind(kind)?;
    if let Scalar::Float(value) = &value
        && !value.is_finite()
    {
        return Err(numeric());
    }
    if value == Scalar::Null {
        return Ok(Scalar::Null);
    }
    if let Scalar::String(text) = &value
        && text.len() > 1 << 20
    {
        return Err(budget());
    }
    if matches!(kind, "STRING" | "TEXT" | "VARCHAR") {
        // Existing text is already owned by this evaluation; don't clone it.
        if let Scalar::String(_) = &value {
            return Ok(value);
        }
        work.temporary(64)?;
        return Ok(Scalar::String(text(&value)?.into_owned()));
    }
    if matches!(kind, "BOOL" | "BOOLEAN") {
        return match value {
            Scalar::Bool(_) => Ok(value),
            Scalar::String(s) => match s.as_str() {
                "1" | "t" | "T" | "TRUE" | "true" | "True" => Ok(Scalar::Bool(true)),
                "0" | "f" | "F" | "FALSE" | "false" | "False" => Ok(Scalar::Bool(false)),
                _ => Err(type_error()),
            },
            _ => {
                let number = rational(&value)?;
                if number.is_zero() {
                    Ok(Scalar::Bool(false))
                } else if number == BigRational::from_integer(1.into()) {
                    Ok(Scalar::Bool(true))
                } else {
                    Err(type_error())
                }
            }
        };
    }
    let value = match value {
        Scalar::String(text) => literal(&text)?,
        value => value,
    };
    let exact = rational(&value)?;
    if matches!(kind, "INT" | "INTEGER" | "BIGINT") {
        return exact
            .to_integer()
            .to_i64()
            .map(Scalar::Int)
            .ok_or_else(numeric);
    }
    // Conversion through the exact rational also matches Go's +0 result when
    // converting a negative binary64 zero to REAL/FLOAT/DOUBLE.
    exact
        .to_f64()
        .filter(|f| f.is_finite())
        .map(Scalar::Float)
        .ok_or_else(numeric)
}
pub(super) fn like(
    value: &Scalar,
    pattern: &Scalar,
    negated: bool,
    work: &mut Work<'_>,
) -> Result<Scalar> {
    if value == &Scalar::Null || pattern == &Scalar::Null {
        return Ok(Scalar::Null);
    }
    let (Scalar::String(text), Scalar::String(mask)) = (value, pattern) else {
        return Err(type_error());
    };
    if text.len() > 1 << 20 || mask.len() > 1 << 20 {
        return Err(budget());
    }
    // Go charges each DP cell as work, but no cumulative retained bytes for
    // these temporary arrays. Only actual Rust ownership is safety-reserved.
    let shared = work.shared;
    let count = mask.chars().count();
    let _allocation = shared
        .map(|budget| budget.reserve_retained(count * 6 + 2))
        .transpose()?;
    let mut letters = Vec::with_capacity(count);
    letters.extend(mask.chars());
    let mut previous = vec![false; letters.len() + 1];
    let mut next = vec![false; letters.len() + 1];
    previous[0] = true;
    for (i, letter) in letters.iter().enumerate() {
        work.spend()?;
        previous[i + 1] = previous[i] && *letter == '%';
    }
    for letter in text.chars() {
        work.spend()?;
        next[0] = false;
        for (i, token) in letters.iter().enumerate() {
            work.spend()?;
            next[i + 1] = match token {
                '%' => next[i] || previous[i + 1],
                '_' => previous[i],
                token => previous[i] && *token == letter,
            };
        }
        std::mem::swap(&mut previous, &mut next);
    }
    Ok(Scalar::Bool(previous[letters.len()] != negated))
}

#[cfg(test)]
#[path = "scalar_tests.rs"]
mod tests;
