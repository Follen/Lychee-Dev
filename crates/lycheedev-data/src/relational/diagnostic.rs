//! Keep typed token locations before sqlparser's closed error enum flattens them.
//! The owned grammar retains its original ParserError for public parser callers;
//! the CLI receives the source diagnostic without parsing an error's display text.
use super::{problem, syntax::QueryDialect};
use sqlparser::{
    ast::Ident,
    keywords::Keyword,
    parser::{Parser as SqlParser, ParserError},
    tokenizer::{Location, Token, TokenWithSpan},
};
use std::ops::{Deref, DerefMut};

pub(super) struct Diagnostic {
    pub location: Location,
    pub detail: String,
}

pub(super) fn at(sql: &str, location: Location, detail: &str) -> lycheedev_core::ToolkitError {
    // Locations are produced by our original-source scanner. Its columns count
    // Unicode scalar values; only LF starts a line, including within CRLF.
    // Walk char boundaries rather than treating a column as a UTF-8 byte count.
    let mut current = Location { line: 1, column: 1 };
    let mut offset = None;
    for (at, ch) in sql.char_indices() {
        if current == location {
            offset = Some(at);
            break;
        }
        if ch == '\n' {
            current.line += 1;
            current.column = 1;
        } else {
            current.column += 1;
        }
    }
    let Some(offset) = offset.or_else(|| (current == location).then_some(sql.len())) else {
        return super::fault(
            "records.internal",
            8,
            "SQL diagnostic source location differs",
        );
    };
    problem(
        "relational.invalid_syntax",
        &format!(
            "relational.invalid_syntax at {}:{}: {detail}",
            location.line, location.column
        ),
    )
    .with_details(serde_json::json!({"location":{
        "offset":offset,"line":location.line,"column":location.column}}))
}

pub(super) struct Parser<'p, 'd, 'sql> {
    inner: &'p mut SqlParser<'sql>,
    dialect: &'d QueryDialect,
}
impl<'p, 'd, 'sql> Parser<'p, 'd, 'sql> {
    pub(super) fn new(inner: &'p mut SqlParser<'sql>, dialect: &'d QueryDialect) -> Self {
        Self { inner, dialect }
    }
    fn record(&self, location: Location, detail: String) {
        *self.dialect.diagnostic.borrow_mut() = Some(Diagnostic { location, detail });
    }
    pub(super) fn expected<T>(
        &self,
        expected: &str,
        found: TokenWithSpan,
    ) -> std::result::Result<T, ParserError> {
        self.record(found.span.start, format!("expected {expected}"));
        self.inner.expected(expected, found)
    }
    pub(super) fn expect_token(
        &mut self,
        expected: &Token,
    ) -> std::result::Result<TokenWithSpan, ParserError> {
        let result = self.inner.expect_token(expected);
        if result.is_err() {
            self.record(
                self.inner.peek_token_ref().span.start,
                format!("expected {expected}"),
            );
        }
        result
    }
    pub(super) fn expect_keyword(
        &mut self,
        expected: Keyword,
    ) -> std::result::Result<TokenWithSpan, ParserError> {
        let result = self.inner.expect_keyword(expected);
        if result.is_err() {
            self.record(
                self.inner.peek_token_ref().span.start,
                format!("expected {expected:?}"),
            );
        }
        result
    }
    pub(super) fn parse_identifier(&mut self) -> std::result::Result<Ident, ParserError> {
        // sqlparser consumes the failed token here. Capture it before the call,
        // rather than guessing between the parser's current and next token.
        let location = self.inner.peek_token_ref().span.start;
        let result = self.inner.parse_identifier();
        if result.is_err() {
            self.record(location, "expected identifier".into());
        }
        result
    }
    pub(super) fn invalid_at(
        &self,
        location: Location,
        message: String,
        detail: &str,
    ) -> ParserError {
        self.record(location, detail.into());
        ParserError::ParserError(message)
    }
}
impl<'sql> Deref for Parser<'_, '_, 'sql> {
    type Target = SqlParser<'sql>;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}
impl DerefMut for Parser<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}
