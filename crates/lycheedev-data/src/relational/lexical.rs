//! Fixed Go lexical admission mapped directly to sqlparser's public tokens.
//! No query interpretation or provider access happens here. Original-source
//! byte, token and nested-comment bounds precede all normal grammar parsing.
use super::*;
use sqlparser::{
    keywords::{ALL_KEYWORDS, ALL_KEYWORDS_INDEX, Keyword},
    tokenizer::{Location, Token, TokenWithSpan, Word},
};

fn name(ch: char, continuation: bool) -> bool {
    ch == '_'
        || identifier_categories::letter(ch)
        || continuation && (identifier_categories::digit(ch) || ch == '$')
}
fn space(ch: char) -> bool {
    matches!(ch, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}'
        | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}'
        | '\u{202f}' | '\u{205f}' | '\u{3000}')
}
fn word(value: String, quote_style: Option<char>, after_union: bool) -> Token {
    let upper = scalar::upper(&value);
    let keyword = if quote_style.is_none()
        && syntax::reserved(&value)
        && (equal_fold(&value, &upper)
            || matches!(
                upper.as_str(),
                "OR" | "AND" | "IS" | "IN" | "BETWEEN" | "LIKE"
            ))
        && (upper != "ALL" || after_union)
    {
        ALL_KEYWORDS
            .binary_search(&upper.as_str())
            .map(|index| ALL_KEYWORDS_INDEX[index])
            .unwrap_or(Keyword::NoKeyword)
    } else {
        Keyword::NoKeyword
    };
    Token::Word(Word {
        value,
        quote_style,
        keyword,
    })
}
fn advance(location: &mut Location, value: &str) {
    for ch in value.chars() {
        if ch == '\n' {
            location.line += 1;
            location.column = 1;
        } else {
            location.column += 1;
        }
    }
}
fn invalid(sql: &str, at: usize, detail: &str) -> lycheedev_core::ToolkitError {
    let mut location = Location { line: 1, column: 1 };
    advance(&mut location, &sql[..at]);
    let mut error = problem(
        "relational.invalid_syntax",
        &format!(
            "relational.invalid_syntax at {}:{}: {detail}",
            location.line, location.column
        ),
    );
    error.details = serde_json::json!({"location":{
        "offset":at,"line":location.line,"column":location.column}});
    error
}

pub(super) fn tokens(sql: &str) -> Result<Vec<TokenWithSpan>> {
    if sql.len() > 256 << 10 {
        return Err(budget());
    }
    if sql.contains('\0') {
        return Err(problem("relational.invalid_syntax", "Invalid SQL text"));
    }
    let bytes = sql.as_bytes();
    let mut tokens: Vec<TokenWithSpan> = Vec::new();
    let mut pos = 0;
    let mut location = Location { line: 1, column: 1 };
    while pos < bytes.len() {
        // Exactly 32768 tokens at EOF are admitted. Even trailing trivia
        // performs this check, matching the original scanner loop ordering.
        if tokens.len() >= 32768 {
            return Err(budget());
        }
        let start = pos;
        let begin = location;
        let ch = sql[pos..].chars().next().unwrap();
        if space(ch) {
            pos += ch.len_utf8();
            advance(&mut location, &sql[start..pos]);
            continue;
        }
        if sql[pos..].starts_with("--") {
            pos = sql[pos..].find('\n').map_or(bytes.len(), |n| pos + n + 1);
            advance(&mut location, &sql[start..pos]);
            continue;
        }
        if sql[pos..].starts_with("/*") {
            let mut depth = 1;
            pos += 2;
            while pos < bytes.len() && depth != 0 {
                if bytes[pos..].starts_with(b"/*") {
                    depth += 1;
                    pos += 2;
                    if depth > 16 {
                        return Err(budget());
                    }
                } else if bytes[pos..].starts_with(b"*/") {
                    depth -= 1;
                    pos += 2;
                } else {
                    pos += 1;
                }
            }
            if depth != 0 {
                return Err(invalid(sql, start, "unclosed comment"));
            }
            advance(&mut location, &sql[start..pos]);
            continue;
        }
        let token = if matches!(ch, '\'' | '"' | '`' | '[') {
            let close = if ch == '[' { ']' } else { ch };
            pos += 1;
            let mut value = String::new();
            let mut closed = false;
            while pos < bytes.len() {
                let next = sql[pos..].chars().next().unwrap();
                pos += next.len_utf8();
                if next == close {
                    if bytes.get(pos) == Some(&(close as u8)) {
                        pos += 1;
                        value.push(close);
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    value.push(next);
                }
            }
            if !closed {
                return Err(invalid(sql, start, "unclosed quoted value"));
            }
            if ch != '\'' && value.is_empty() {
                return Err(invalid(sql, start, "empty identifier"));
            }
            if ch == '\'' {
                Token::SingleQuotedString(value)
            } else {
                word(value, Some(ch), false)
            }
        } else if ch == ':' {
            pos += 1;
            let first = pos;
            while pos < bytes.len() {
                let next = sql[pos..].chars().next().unwrap();
                if !name(next, pos > first) {
                    break;
                }
                pos += next.len_utf8();
            }
            if first == pos {
                return Err(invalid(sql, start, "missing parameter name"));
            }
            Token::Placeholder(sql[start..pos].to_owned())
        } else if name(ch, false) {
            pos += ch.len_utf8();
            while pos < bytes.len() {
                let next = sql[pos..].chars().next().unwrap();
                if !name(next, true) {
                    break;
                }
                pos += next.len_utf8();
            }
            let after_union = tokens.last().is_some_and(|last| {
                matches!(&last.token, Token::Word(w) if w.quote_style.is_none()
                    && equal_fold(&w.value, "UNION"))
            });
            word(sql[start..pos].to_owned(), None, after_union)
        } else if ch.is_ascii_digit()
            || ch == '.' && bytes.get(pos + 1).is_some_and(u8::is_ascii_digit)
        {
            while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                pos += 1;
            }
            if bytes.get(pos) == Some(&b'.') {
                pos += 1;
                while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
            }
            if matches!(bytes.get(pos), Some(b'e' | b'E')) {
                pos += 1;
                if matches!(bytes.get(pos), Some(b'+' | b'-')) {
                    pos += 1;
                }
                let exponent = pos;
                while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
                if pos == exponent {
                    return Err(invalid(sql, start, "missing exponent"));
                }
            }
            Token::Number(sql[start..pos].to_owned(), false)
        } else {
            pos += ch.len_utf8();
            let next = bytes.get(pos).copied();
            match (ch, next) {
                ('<', Some(b'=')) => {
                    pos += 1;
                    Token::LtEq
                }
                ('>', Some(b'=')) => {
                    pos += 1;
                    Token::GtEq
                }
                ('<', Some(b'>')) | ('!', Some(b'=')) => {
                    pos += 1;
                    Token::Neq
                }
                ('|', Some(b'|')) => {
                    pos += 1;
                    Token::StringConcat
                }
                (',', _) => Token::Comma,
                ('.', _) => Token::Period,
                ('(', _) => Token::LParen,
                (')', _) => Token::RParen,
                ('*', _) => Token::Mul,
                ('+', _) => Token::Plus,
                ('-', _) => Token::Minus,
                ('/', _) => Token::Div,
                ('%', _) => Token::Mod,
                ('=', _) => Token::Eq,
                ('<', _) => Token::Lt,
                ('>', _) => Token::Gt,
                (';', _) => Token::SemiColon,
                ('!' | '|', _) => return Err(invalid(sql, start, "unsupported operator")),
                _ => return Err(invalid(sql, start, "unexpected character")),
            }
        };
        advance(&mut location, &sql[start..pos]);
        tokens.push(TokenWithSpan::at(token, begin, location));
    }
    tokens.push(TokenWithSpan::at(Token::EOF, location, location));
    Ok(tokens)
}
