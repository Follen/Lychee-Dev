//! Public source facts preserve the fixed Go scanner contract. Native semantic
//! analysis uses the syntax tree independently of these literal observations.
use crate::facts::{Declaration, Facts, Relationship, SyntaxNote};
use lycheedev_core::{Result, ToolkitError};
use std::borrow::Cow;
use tree_sitter::{InputEdit, Node, Parser, Tree};

pub struct SyntaxDocument {
    bytes: Vec<u8>,
    tree: Tree,
    parser: Parser,
    lua51_identifiers: bool,
}
impl SyntaxDocument {
    pub(crate) fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }
    pub fn new(bytes: &[u8]) -> Result<Self> {
        Self::new_with_identifiers(bytes, true)
    }
    /// The fixed Go source scanner reserves goto, whereas native WoW Lua 5.1
    /// permits that identifier. Both consumers retain the original source bytes.
    pub(crate) fn new_public(bytes: &[u8]) -> Result<Self> {
        Self::new_with_identifiers(bytes, false)
    }
    fn new_with_identifiers(bytes: &[u8], lua51_identifiers: bool) -> Result<Self> {
        if std::str::from_utf8(bytes).is_err() || (lua51_identifiers && bytes.contains(&0)) {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Lua structural input must be UTF-8 source without NUL",
                4,
            ));
        }
        if bytes.len() > 32 << 20 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Lua syntax byte budget",
                3,
            ));
        }
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_lua::LANGUAGE.into())
            .map_err(|e| ToolkitError::new("codebase.parser_unavailable", e.to_string(), 3))?;
        let grammar = lua51_grammar_input(bytes, lua51_identifiers);
        let tree = parser.parse(grammar.as_ref(), None).ok_or_else(|| {
            ToolkitError::new("codebase.source_budget", "Lua parser cancelled", 3)
        })?;
        Ok(Self {
            bytes: bytes.to_vec(),
            tree,
            parser,
            lua51_identifiers,
        })
    }
    /// Caller supplies one exact edit in byte/point coordinates of old/new content.
    pub fn edit(&mut self, edit: InputEdit, new_bytes: &[u8]) -> Result<()> {
        if new_bytes.len() > 32 << 20
            || edit.start_byte > edit.old_end_byte
            || edit.old_end_byte > self.bytes.len()
            || edit.new_end_byte > new_bytes.len()
            || edit.start_byte > edit.new_end_byte
            || std::str::from_utf8(new_bytes).is_err()
            || (self.lua51_identifiers && new_bytes.contains(&0))
        {
            return Err(ToolkitError::new(
                "codebase.invalid_span",
                "Invalid incremental edit",
                2,
            ));
        }
        if self.bytes[..edit.start_byte] != new_bytes[..edit.start_byte]
            || self.bytes[edit.old_end_byte..] != new_bytes[edit.new_end_byte..]
            || point_at(&self.bytes, edit.start_byte) != edit.start_position
            || point_at(&self.bytes, edit.old_end_byte) != edit.old_end_position
            || point_at(new_bytes, edit.new_end_byte) != edit.new_end_position
        {
            return Err(ToolkitError::new(
                "codebase.invalid_span",
                "Incremental edit does not describe the supplied byte change",
                2,
            ));
        }
        self.tree.edit(&edit);
        let old_grammar = lua51_grammar_input(&self.bytes, self.lua51_identifiers);
        let new_grammar = lua51_grammar_input(new_bytes, self.lua51_identifiers);
        // A comment/string delimiter edit can change token classification
        // outside the edited extent. Such an edit requires a full reparse.
        let reusable = old_grammar[..edit.start_byte] == new_grammar[..edit.start_byte]
            && old_grammar[edit.old_end_byte..] == new_grammar[edit.new_end_byte..];
        self.tree = self
            .parser
            .parse(new_grammar.as_ref(), reusable.then_some(&self.tree))
            .ok_or_else(|| {
                ToolkitError::new("codebase.source_budget", "Lua reparse cancelled", 3)
            })?;
        self.bytes = new_bytes.to_vec();
        Ok(())
    }
    /// Reject newer grammar forms absent from the fixed gopher-lua parser.
    /// This is shared by public facts and matrix usages, never by native flow.
    pub(crate) fn public_parse_note(&self) -> Option<SyntaxNote> {
        if let Some(note) = public_lexical_note(&self.bytes) {
            return Some(note);
        }
        let mut missing_at_eof = false;
        let end = self
            .bytes
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map_or(0, |at| at + 1);
        let mut cursor = self.tree.walk();
        loop {
            let node = cursor.node();
            missing_at_eof |= node.is_missing() && node.start_byte() >= end;
            missing_at_eof |= node.is_error()
                && node.end_byte() >= end
                && incomplete_function_header(text(&self.bytes, node));
            if node.kind() == "arguments"
                && text(&self.bytes, node).starts_with('(')
                && text(&self.bytes, node).ends_with(')')
            {
                let tail = &self.bytes[node.end_byte()..];
                let next = tail
                    .iter()
                    .position(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
                    .unwrap_or(tail.len());
                if tail.get(next) == Some(&b'(')
                    && tail[..next]
                        .iter()
                        .any(|byte| matches!(*byte, b'\r' | b'\n'))
                {
                    // gopher-lua's yacc reduction observes PNewLine after
                    // looking ahead to the following '(', and TokenError
                    // receives the opening token of these completed args.
                    return Some(public_lua_error(
                        &self.bytes,
                        node.start_byte(),
                        b"(",
                        "ambiguous syntax (function call x new statement)",
                    ));
                }
            }
            if node.kind() == "identifier" && text(&self.bytes, node) == "goto" {
                return Some(public_lua_error(
                    &self.bytes,
                    node.end_byte() - 1,
                    b"goto",
                    "syntax error",
                ));
            }
            let unsupported = public_unsupported(&self.bytes, node);
            if unsupported {
                return Some(SyntaxNote {
                    message: "Syntax unavailable in fixed Go Lua parser".into(),
                    line: lua_line_starts(&self.bytes)
                        .partition_point(|start| *start <= node.start_byte()),
                });
            }
            if !matches!(node.kind(), "comment" | "string") && cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return missing_at_eof.then(|| public_lua_eof("syntax error"));
                }
            }
        }
    }
    pub fn facts(&self, generated: bool) -> Facts {
        if !self.lua51_identifiers
            && let Some(note) = self.public_parse_note()
        {
            let mut facts = Facts::fresh();
            facts.diagnostics.push(note);
            return facts;
        }
        let mut w = Walker {
            raw: &self.bytes,
            facts: Facts::fresh(),
            scopes: vec!["<file>".into()],
            api_roots: Vec::new(),
            rejected_syntax: false,
            generated,
            work: 0,
            depth: 0,
            source_lines: lua_line_starts(&self.bytes),
            lines: std::str::from_utf8(&self.bytes)
                .unwrap_or("")
                .split('\n')
                .collect(),
        };
        w.walk(self.tree.root_node());
        // Go rejects a malformed program as a whole, rather than publishing
        // declarations recovered by tree-sitter. The native tree stays intact.
        if self.tree.root_node().has_error() || w.rejected_syntax {
            w.facts.declarations.clear();
            w.facts.relationships.clear();
            if let Some(targets) = &mut w.facts.raw_relationship_targets {
                targets.clear();
            }
        } else {
            for root in std::mem::take(&mut w.api_roots) {
                w.documentation(root, "");
            }
        }
        w.facts
    }
}
/// Lua 5.1's `goto` is an identifier. The pinned modern grammar reserves it.
/// Token-only, same-width masking makes the grammar see an identifier while
/// every observation/name/hash/span reads unchanged original bytes. Comments
/// and longer identifiers are untouched. Unsupported modern string escapes
/// receive a same-width grammar-only mask; the literal decoder reads originals.
/// Bundled luac remains the separate native WoW syntax authority.
fn lua51_grammar_input(raw: &[u8], mask_goto: bool) -> Cow<'_, [u8]> {
    fn long_end(raw: &[u8], at: usize) -> Option<usize> {
        if raw.get(at) != Some(&b'[') {
            return None;
        }
        let mut second = at + 1;
        while raw.get(second) == Some(&b'=') {
            second += 1;
        }
        if raw.get(second) != Some(&b'[') {
            return None;
        }
        let width = second - at;
        let mut cursor = second + 1;
        while cursor < raw.len() {
            if raw[cursor] == b']'
                && raw.get(cursor + width) == Some(&b']')
                && raw
                    .get(cursor + 1..cursor + width)
                    .is_some_and(|v| v.iter().all(|b| *b == b'='))
            {
                return Some(cursor + width + 1);
            }
            cursor += 1;
        }
        Some(raw.len())
    }
    fn identifier(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_' || c >= 128
    }
    let mut changed: Option<Vec<u8>> = None;
    let mut at = 0;
    while at < raw.len() {
        if raw.get(at..at + 2) == Some(b"--") {
            if let Some(end) = long_end(raw, at + 2) {
                if !mask_goto {
                    mask_literal_nul(raw, at, end, &mut changed);
                }
                at = end;
            } else {
                at += 2;
                while at < raw.len() && raw[at] != b'\n' && (mask_goto || raw[at] != b'\r') {
                    if !mask_goto && raw[at] == 0 {
                        changed.get_or_insert_with(|| raw.to_vec())[at] = b' ';
                    }
                    at += 1;
                }
            }
            continue;
        }
        if matches!(raw[at], b'\'' | b'"') {
            let quote = raw[at];
            at += 1;
            while at < raw.len() {
                if !mask_goto && raw[at] == 0 {
                    changed.get_or_insert_with(|| raw.to_vec())[at] = b' ';
                }
                if raw[at] == b'\\' {
                    if !mask_goto && raw.get(at + 1) == Some(&0) {
                        changed.get_or_insert_with(|| raw.to_vec())[at + 1] = b' ';
                    }
                    if let Some(next) = raw.get(at + 1)
                        && !b"abfnrtv\\'\"0123456789\r\n".contains(next)
                    {
                        // Lua 5.1/gopher-lua accept arbitrary escaped bytes.
                        // The modern grammar rejects some and gives others
                        // newer meanings. Only the grammar view is masked;
                        // literal decoding always reads the original bytes.
                        changed.get_or_insert_with(|| raw.to_vec())[at] = b' ';
                    }
                    at = (at + 2).min(raw.len());
                } else if raw[at] == quote {
                    at += 1;
                    break;
                } else {
                    at += 1;
                }
            }
            continue;
        }
        if let Some(end) = long_end(raw, at) {
            if !mask_goto {
                mask_literal_nul(raw, at, end, &mut changed);
            }
            at = end;
            continue;
        }
        if raw[at].is_ascii_alphabetic() || raw[at] == b'_' || raw[at] >= 128 {
            let start = at;
            at += 1;
            while at < raw.len() && identifier(raw[at]) {
                at += 1;
            }
            if mask_goto && &raw[start..at] == b"goto" {
                changed.get_or_insert_with(|| raw.to_vec())[start + 1] = b'0';
            }
        } else {
            at += 1;
        }
    }
    changed.map(Cow::Owned).unwrap_or(Cow::Borrowed(raw))
}
// Same-width grammar adaptation only. Public literal bytes remain untouched.
fn mask_literal_nul(raw: &[u8], start: usize, end: usize, changed: &mut Option<Vec<u8>>) {
    for at in start..end {
        if raw[at] == 0 {
            changed.get_or_insert_with(|| raw.to_vec())[at] = b' ';
        }
    }
}
// Only the unfinished declaration header that tree-sitter recovers as an
// ERROR rather than a missing `end`; other grammar errors retain their path.
fn incomplete_function_header(source: &str) -> bool {
    let mut rest = source.trim();
    if let Some(value) = rest.strip_prefix("local ") {
        rest = value.trim_start();
    }
    let Some(value) = rest.strip_prefix("function") else {
        return false;
    };
    if !value.is_empty() && !value.starts_with(char::is_whitespace) && !value.starts_with('(') {
        return false;
    }
    rest = value.trim_start();
    if rest.is_empty() {
        return true;
    }
    if !rest.starts_with('(') {
        let name_end = rest
            .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '.' | ':'))
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() && !name.starts_with('_') {
            return false;
        }
        rest = rest[name_end..].trim_start();
        if rest.is_empty() {
            return true;
        }
    }
    let Some(parameters) = rest.strip_prefix('(') else {
        return false;
    };
    let parameters = parameters.trim();
    let parameters = parameters.strip_suffix(')').unwrap_or(parameters).trim();
    parameters.split(',').all(|name| {
        let name = name.trim();
        name.is_empty()
            || name == "..."
            || name.bytes().enumerate().all(|(at, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || at > 0 && byte.is_ascii_digit()
            })
    })
}
fn public_lua_eof(message: &str) -> SyntaxNote {
    SyntaxNote {
        message: format!("source at EOF:   {message}\n"),
        line: 0,
    }
}
fn public_lua_error(raw: &[u8], at: usize, token: &[u8], message: &str) -> SyntaxNote {
    let lines = lua_line_starts(raw);
    let line = lines.partition_point(|start| *start <= at);
    let bom = usize::from(line == 1 && raw.starts_with(b"\xef\xbb\xbf")) * 3;
    let column = at + 1 - lines[line - 1] - bom;
    SyntaxNote {
        message: format!(
            "source line:{line}(column:{column}) near '{}':   {message}\n",
            lua_json_string(token)
        ),
        line,
    }
}
// This is only the fixed scanner's string/comment/NUL boundary, not a second
// Lua parser. Statement/expression structure remains tree-sitter's job. The
// fixed scanProgram wrapper normalizes lexer EOF (-1) to public note line 0.
fn public_lexical_note(raw: &[u8]) -> Option<SyntaxNote> {
    let mut at = if raw.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    if raw[at..].starts_with(b"#!") {
        if let Some(end) = raw[at..].iter().position(|byte| *byte == b'\n') {
            at += end;
        }
    }
    while at < raw.len() {
        let comment = raw[at..].starts_with(b"--");
        if comment {
            at += 2;
        }
        if raw.get(at) == Some(&b'[')
            && raw
                .get(at + 1)
                .is_some_and(|byte| matches!(*byte, b'[' | b'='))
        {
            let start = at;
            at += 1;
            while raw.get(at) == Some(&b'=') {
                at += 1;
            }
            if at == raw.len() {
                return Some(public_lua_eof(if comment {
                    "invalid multiline comment"
                } else {
                    "invalid multiline string"
                }));
            }
            if raw[at] != b'[' {
                return Some(public_lua_error(
                    raw,
                    at,
                    if comment { b"" } else { &raw[at..at + 1] },
                    if comment {
                        "invalid multiline comment"
                    } else {
                        "invalid multiline string"
                    },
                ));
            }
            let width = at - start;
            at += 1;
            loop {
                if at == raw.len() {
                    return Some(public_lua_eof(if comment {
                        "invalid multiline comment"
                    } else {
                        "unterminated multiline string"
                    }));
                }
                if raw[at] == b']'
                    && raw.get(at + width) == Some(&b']')
                    && raw
                        .get(at + 1..at + width)
                        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == b'='))
                {
                    at += width + 1;
                    break;
                }
                at += 1;
            }
            continue;
        }
        if comment {
            while at < raw.len() && !matches!(raw[at], b'\r' | b'\n') {
                at += 1;
            }
            continue;
        }
        if matches!(raw[at], b'\'' | b'"') {
            let quote = raw[at];
            let start = at;
            at += 1;
            loop {
                let Some(&byte) = raw.get(at) else {
                    return Some(public_lua_eof("unterminated string"));
                };
                if byte == quote {
                    at += 1;
                    break;
                }
                if matches!(byte, b'\r' | b'\n') {
                    let line = lua_line_starts(raw).partition_point(|start| *start <= at) + 1;
                    // Decode only the malformed token for its diagnostic.
                    // Ordinary strings do not allocate during this scan.
                    let mut literal = raw[start..at].to_vec();
                    literal.push(quote);
                    let token = string_value_bytes(&literal).unwrap_or_default();
                    return Some(SyntaxNote {
                        message: format!(
                            "source line:{line}(column:0) near '{}':   unterminated string\n",
                            lua_json_string(&token)
                        ),
                        line,
                    });
                }
                at += 1;
                if byte == b'\\' {
                    let Some(&escaped) = raw.get(at) else {
                        return Some(public_lua_eof("unterminated string"));
                    };
                    at += 1;
                    if matches!(escaped, b'\r' | b'\n')
                        && raw
                            .get(at)
                            .is_some_and(|next| matches!(*next, b'\r' | b'\n') && *next != escaped)
                    {
                        at += 1;
                    }
                }
            }
            continue;
        }
        if raw[at] == 0 {
            return Some(public_lua_error(raw, at, b"\0", "Invalid token"));
        }
        at += 1;
    }
    None
}
fn point_at(raw: &[u8], at: usize) -> tree_sitter::Point {
    let prefix = &raw[..at];
    let row = prefix.iter().filter(|b| **b == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|n| at - n - 1)
        .unwrap_or(at);
    tree_sitter::Point::new(row, column)
}
fn public_unsupported(raw: &[u8], node: Node<'_>) -> bool {
    match node.kind() {
        "attribute" => true,
        "binary_expression" | "unary_expression" => node
            .child_by_field_name("operator")
            .is_some_and(|operator| {
                matches!(operator.kind(), "//" | "&" | "|" | "~" | "<<" | ">>")
            }),
        "identifier" => !text(raw, node).is_ascii(),
        "number" => {
            let token = text(raw, node).to_ascii_lowercase();
            token.starts_with("0x") && token.contains(['.', 'p'])
        }
        "hash_bang_line" => !raw.contains(&b'\n'),
        _ => false,
    }
}
fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor)
        .filter(|node| node.kind() != "comment")
        .collect()
}
fn text<'a>(raw: &'a [u8], n: Node<'_>) -> &'a str {
    std::str::from_utf8(&raw[n.byte_range()]).unwrap_or("")
}
fn unparenthesized(mut node: Node<'_>) -> Node<'_> {
    while node.kind() == "parenthesized_expression" {
        let Some(child) = children(node).into_iter().next() else {
            break;
        };
        node = child;
    }
    node
}
// gopher-lua folds parentheses into their expression and assigns the opening
// line, except that an adjusted parenthesized call retains its callee line.
pub(crate) fn expression_start(mut node: Node<'_>) -> usize {
    for _ in 0..256 {
        let next = match node.kind() {
            "parenthesized_expression" => {
                let inner = unparenthesized(node);
                if inner.kind() != "function_call" {
                    return node.start_byte();
                }
                Some(inner)
            }
            "function_call" => node.child_by_field_name("name"),
            "dot_index_expression" | "method_index_expression" | "bracket_index_expression" => {
                node.child_by_field_name("table")
            }
            _ => None,
        };
        let Some(next) = next else { break };
        node = next;
    }
    node.start_byte()
}
pub(crate) fn lua_line_starts(raw: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    let mut at = 0;
    while at < raw.len() {
        if matches!(raw[at], b'\r' | b'\n') {
            let first = raw[at];
            at += 1;
            if raw
                .get(at)
                .is_some_and(|byte| matches!(*byte, b'\r' | b'\n') && *byte != first)
            {
                at += 1;
            }
            starts.push(at);
        } else {
            at += 1;
        }
    }
    starts
}

fn name(raw: &[u8], node: Node<'_>) -> String {
    String::from_utf8(name_bytes(raw, node, 0))
        .unwrap_or_else(|error| lua_json_string(error.as_bytes()))
}
fn name_bytes(raw: &[u8], node: Node<'_>, depth: usize) -> Vec<u8> {
    if depth > 256 {
        return Vec::new();
    }
    let node = unparenthesized(node);
    match node.kind() {
        "identifier" => raw[node.byte_range()].to_vec(),
        "dot_index_expression" | "method_index_expression" | "bracket_index_expression" => {
            let Some(table) = node.child_by_field_name("table") else {
                return Vec::new();
            };
            let Some(field) = node
                .child_by_field_name("field")
                .or_else(|| node.child_by_field_name("method"))
            else {
                return Vec::new();
            };
            let mut base = name_bytes(raw, table, depth + 1);
            if base.is_empty() {
                return base;
            }
            let value = if node.kind() == "bracket_index_expression" {
                let Some(value) = string_bytes(raw, field) else {
                    return Vec::new();
                };
                value
            } else {
                raw[field.byte_range()].to_vec()
            };
            base.push(if node.kind() == "method_index_expression" {
                b':'
            } else {
                b'.'
            });
            base.extend(value);
            base
        }
        _ => Vec::new(),
    }
}
// gopher-lua's lexer emits bytes for escapes (including wrapping decimal
// escapes). JSON replaces each invalid UTF-8 byte, rather than Latin-1 widening.
pub(crate) fn lua_json_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(value) => {
                out.push_str(value);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                out.push_str(std::str::from_utf8(&rest[..valid]).expect("validated prefix"));
                out.push('\u{fffd}');
                rest = &rest[valid + 1..];
            }
        }
    }
    out
}
pub(crate) fn string(raw: &[u8], node: Node<'_>) -> Option<String> {
    string_bytes(raw, node).map(|bytes| lua_json_string(&bytes))
}
/// Internal literal identity before Go's JSON replacement of invalid UTF-8.
/// Public fact strings keep the encoding/json-compatible wrapper above.
pub(crate) fn string_bytes(raw: &[u8], node: Node<'_>) -> Option<Vec<u8>> {
    let node = unparenthesized(node);
    if node.kind() != "string" {
        return None;
    }
    string_value_bytes(&raw[node.byte_range()])
}
fn string_value_bytes(value: &[u8]) -> Option<Vec<u8>> {
    fn newline(bytes: &[u8], at: &mut usize) -> u8 {
        let value = bytes[*at];
        *at += 1;
        if matches!(value, b'\r' | b'\n') {
            if bytes
                .get(*at)
                .is_some_and(|next| matches!(*next, b'\r' | b'\n') && *next != value)
            {
                *at += 1;
            }
            b'\n'
        } else {
            value
        }
    }
    let first = *value.first()?;
    let mut out = Vec::new();
    if first == b'[' {
        let second = value[1..].iter().position(|byte| *byte == b'[')? + 1;
        let width = second + 1;
        let content = value.get(width..value.len().checked_sub(width)?)?;
        let mut at = 0;
        if content
            .first()
            .is_some_and(|byte| matches!(*byte, b'\r' | b'\n'))
        {
            newline(content, &mut at);
        }
        while at < content.len() {
            out.push(newline(content, &mut at));
        }
        return Some(out);
    }
    if !b"'\"".contains(&first) || value.len() < 2 {
        return None;
    }
    let content = &value[1..value.len() - 1];
    let mut at = 0;
    while at < content.len() {
        let byte = newline(content, &mut at);
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        if at == content.len() {
            return None;
        }
        let escaped = newline(content, &mut at);
        out.push(match escaped {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'0'..=b'9' => {
                let mut number = u16::from(escaped - b'0');
                for _ in 0..2 {
                    let Some(digit) = content.get(at).filter(|digit| digit.is_ascii_digit()) else {
                        break;
                    };
                    number = number * 10 + u16::from(*digit - b'0');
                    at += 1;
                }
                (number & 255) as u8
            }
            other => other,
        });
    }
    Some(out)
}

fn field<'a>(raw: &[u8], table: Node<'a>, key: &str) -> Option<Node<'a>> {
    for f in children(table).into_iter().filter(|f| f.kind() == "field") {
        let n = f.child_by_field_name("name");
        if n.is_some_and(|n| text(raw, n) == key || string(raw, n).as_deref() == Some(key)) {
            return f.child_by_field_name("value").map(unparenthesized);
        }
    }
    None
}
fn table_text(raw: &[u8], table: Node<'_>, key: &str) -> String {
    field(raw, table, key)
        .and_then(|n| string(raw, n))
        .unwrap_or_default()
}
struct Walker<'a, 'tree> {
    raw: &'a [u8],
    facts: Facts,
    scopes: Vec<String>,
    api_roots: Vec<Node<'tree>>,
    rejected_syntax: bool,
    generated: bool,
    work: usize,
    depth: usize,
    lines: Vec<&'a str>,
    source_lines: Vec<usize>,
}
impl<'tree> Walker<'_, 'tree> {
    fn line(&self, node: Node<'_>) -> usize {
        self.source_lines
            .partition_point(|start| *start <= expression_start(node))
    }
    fn end_line(&self, node: Node<'_>) -> usize {
        self.source_lines
            .partition_point(|start| *start <= node.end_byte().saturating_sub(1))
    }
    fn scope(&self) -> String {
        self.scopes
            .last()
            .cloned()
            .unwrap_or_else(|| "<file>".into())
    }
    fn declare(
        &mut self,
        n: Node<'_>,
        name: String,
        category: &str,
        scope: bool,
        signature: Option<String>,
    ) {
        if name.is_empty() {
            return;
        }
        let line = self.line(n);
        let signature = signature.unwrap_or_else(|| {
            self.lines
                .get(line - 1)
                .copied()
                .unwrap_or("")
                .trim_start_matches('\u{feff}')
                .trim()
                .into()
        });
        self.facts.declarations.push(Declaration {
            name,
            category: category.into(),
            scope: if scope { self.scope() } else { String::new() },
            line,
            end_line: if matches!(n.kind(), "assignment_statement" | "variable_declaration") {
                0
            } else {
                self.end_line(n)
            },
            signature,
        });
    }
    fn link(&mut self, n: Node<'_>, from: String, to: String, category: &str, confidence: &str) {
        self.facts.relationships.push(Relationship {
            from,
            to,
            category: category.into(),
            confidence: confidence.into(),
            line: self.line(n),
        });
    }
    fn link_raw(
        &mut self,
        node: Node<'_>,
        from: String,
        target: Vec<u8>,
        category: &str,
        confidence: &str,
    ) {
        let ordinal = self.facts.relationships.len();
        let display = match String::from_utf8(target) {
            Ok(value) => value,
            Err(error) => {
                let target = error.into_bytes();
                let display = lua_json_string(&target);
                self.facts
                    .raw_relationship_targets
                    .as_mut()
                    .expect("fresh parser target identity")
                    .insert(ordinal, target);
                display
            }
        };
        self.link(node, from, display, category, confidence);
    }
    fn function(
        &mut self,
        n: Node<'tree>,
        declared: Option<(String, &str, Node<'tree>)>,
        line: usize,
    ) {
        let scope = if let Some((name, category, source)) = declared {
            self.declare(source, name.clone(), category, true, None);
            name
        } else {
            format!("{}/<anonymous:{line}>", self.scope())
        };
        self.scopes.push(scope);
        if let Some(parameters) = n.child_by_field_name("parameters") {
            self.walk(parameters);
        }
        if let Some(body) = n.child_by_field_name("body") {
            self.walk(body);
        }
        self.scopes.pop();
    }
    fn walk(&mut self, n: Node<'tree>) {
        if self.depth >= 256 {
            if !self
                .facts
                .diagnostics
                .iter()
                .any(|n| n.message == "source_ast_depth_limit")
            {
                self.facts.diagnostics.push(SyntaxNote {
                    message: "source_ast_depth_limit".into(),
                    line: n.start_position().row + 1,
                });
            }
            return;
        }
        self.depth += 1;
        self.walk_inner(n);
        self.depth -= 1;
    }
    fn walk_inner(&mut self, n: Node<'tree>) {
        if public_unsupported(self.raw, n) {
            if !self.rejected_syntax {
                self.facts.diagnostics.push(SyntaxNote {
                    message: "Syntax unavailable in fixed Go Lua parser".into(),
                    line: self.line(n),
                });
                self.rejected_syntax = true;
            }
            return;
        }
        self.work += 1;
        if self.work > 2_000_000 {
            if self.work == 2_000_001 {
                self.facts.diagnostics.push(SyntaxNote {
                    message: "source_ast_work_limit".into(),
                    line: n.start_position().row + 1,
                });
            }
            return;
        }
        if n.is_error() || n.is_missing() {
            self.facts.diagnostics.push(SyntaxNote {
                message: if n.is_missing() {
                    format!("Missing Lua token: {}", n.kind())
                } else {
                    "Lua syntax recovery region".into()
                },
                line: n.start_position().row + 1,
            });
        }
        if n.kind() == "comment" || n.kind() == "string" {
            return;
        }
        match n.kind() {
            "function_declaration" => {
                if !n.has_error() {
                    if let Some(target) = n.child_by_field_name("name") {
                        self.walk(target);
                    }
                    let target = n
                        .child_by_field_name("name")
                        .map(|n| name(self.raw, n))
                        .unwrap_or_default();
                    let local = text(self.raw, n).split_whitespace().next() == Some("local");
                    self.function(
                        n,
                        Some((target, if local { "local-function" } else { "function" }, n)),
                        self.line(n),
                    );
                    return;
                }
            }
            "function_definition" => {
                if !n.has_error() {
                    self.function(n, None, self.line(n));
                    return;
                }
            }
            "parenthesized_expression" => {
                let inner = unparenthesized(n);
                if inner.kind() == "function_definition" && !inner.has_error() {
                    self.function(inner, None, self.line(n));
                    return;
                }
            }
            "variable_declaration" => {
                if let Some(a) = children(n)
                    .into_iter()
                    .find(|a| a.kind() == "assignment_statement")
                {
                    self.assignment(a, true, n);
                } else {
                    for child in children(n) {
                        self.walk(child);
                    }
                }
                return;
            }
            "assignment_statement" => {
                self.assignment(n, false, n);
                return;
            }
            "function_call" if !n.has_error() => {
                if let Some(f) = n.child_by_field_name("name") {
                    let target_bytes = name_bytes(self.raw, f, 0);
                    let event_target = target_bytes.ends_with(b":RegisterEvent")
                        || target_bytes.ends_with(b":RegisterUnitEvent");
                    let confidence = if target_bytes.is_empty() {
                        "dynamic-unresolved"
                    } else {
                        "inferred"
                    };
                    self.link_raw(
                        n,
                        self.scope(),
                        if target_bytes.is_empty() {
                            b"<dynamic>".to_vec()
                        } else {
                            target_bytes
                        },
                        "call",
                        confidence,
                    );
                    if event_target
                        && n.child_by_field_name("arguments")
                            .is_some_and(|a| !children(a).is_empty())
                    {
                        let value = n
                            .child_by_field_name("arguments")
                            .and_then(|a| children(a).into_iter().next())
                            .and_then(|v| string_bytes(self.raw, v));
                        let confidence = if value.is_some() {
                            "exact"
                        } else {
                            "dynamic-unresolved"
                        };
                        self.link_raw(
                            n,
                            self.scope(),
                            value.unwrap_or_else(|| b"<dynamic>".to_vec()),
                            "event-registration",
                            confidence,
                        );
                    }
                }
            }
            "table_constructor"
                if self.generated && table_text(self.raw, n, "Type") == "System" =>
            {
                self.api_roots.push(n)
            }
            "attribute" => self.facts.diagnostics.push(SyntaxNote {
                message: "Syntax unavailable in WoW Lua 5.1".into(),
                line: n.start_position().row + 1,
            }),
            _ => {}
        }
        for c in children(n) {
            self.walk(c);
        }
    }
    fn assignment(&mut self, n: Node<'tree>, local: bool, source: Node<'tree>) {
        let nodes = children(n);
        let vars = nodes
            .iter()
            .find(|v| v.kind() == "variable_list")
            .map(|v| children(*v))
            .unwrap_or_default();
        let values = nodes
            .iter()
            .find(|v| v.kind() == "expression_list")
            .map(|v| children(*v))
            .unwrap_or_default();
        // Assignment targets may contain calls. Go visits all targets before
        // any RHS, including targets that cannot produce a declaration name.
        for var in &vars {
            self.walk(*var);
        }
        for (i, expression) in values.into_iter().enumerate() {
            let value = unparenthesized(expression);
            let target = vars.get(i).map(|v| name(self.raw, *v)).unwrap_or_default();
            if value.kind() == "function_definition" && !target.is_empty() && !value.has_error() {
                self.function(
                    value,
                    Some((
                        target,
                        if local { "local-function" } else { "function" },
                        source,
                    )),
                    self.line(expression),
                );
            } else {
                if !local
                    && !target.is_empty()
                    && value.kind() == "function_call"
                    && value
                        .child_by_field_name("name")
                        .is_some_and(|f| name(self.raw, f) == "CreateFromMixins")
                {
                    self.declare(source, target.clone(), "mixin", true, None);
                    if let Some(arguments) = value.child_by_field_name("arguments") {
                        for base in children(arguments) {
                            let parent = name(self.raw, base);
                            if !parent.is_empty() {
                                self.link(source, target.clone(), parent, "inherits", "inferred");
                            }
                        }
                    }
                }
                self.walk(expression);
            }
        }
    }
    fn documentation(&mut self, n: Node<'_>, namespace: &str) {
        let kind = table_text(self.raw, n, "Type").to_ascii_lowercase();
        let display = table_text(self.raw, n, "Name");
        let mut namespace = namespace.to_owned();
        if kind == "system" {
            namespace = table_text(self.raw, n, "Namespace");
            if namespace.is_empty() && display.starts_with("C_") {
                namespace = display.clone();
            }
            self.declare(n, display.clone(), "api-system", false, Some(display));
        } else if !display.is_empty()
            && [
                "function",
                "event",
                "structure",
                "enumeration",
                "scriptobject",
                "callback",
            ]
            .contains(&kind.as_str())
        {
            let qualified = if namespace.is_empty() {
                display
            } else {
                format!("{namespace}.{display}")
            };
            self.declare(
                n,
                qualified.clone(),
                &format!("api-{kind}"),
                false,
                Some(self.signature(n, &qualified, &kind)),
            );
            if !namespace.is_empty() {
                self.link(n, namespace.clone(), qualified.clone(), "contains", "exact");
            }
            let literal_name = table_text(self.raw, n, "LiteralName");
            if kind == "event" && !literal_name.is_empty() && literal_name != qualified {
                self.declare(
                    n,
                    literal_name.clone(),
                    "api-event",
                    false,
                    Some(self.signature(n, &literal_name, &kind)),
                );
                self.link(n, literal_name, qualified, "event-name", "exact");
            }
        }
        for key in [
            "Functions",
            "Events",
            "Tables",
            "Structures",
            "Enumerations",
            "ScriptObjects",
            "Callbacks",
        ] {
            if let Some(list) = field(self.raw, n, key) {
                for f in children(list) {
                    if let Some(t) = f.child_by_field_name("value")
                        && t.kind() == "table_constructor"
                    {
                        self.documentation(t, &namespace);
                    }
                }
            }
        }
    }
    fn signature(&self, n: Node<'_>, name: &str, kind: &str) -> String {
        let params = |key: &str| -> String {
            let Some(list) = field(self.raw, n, key) else {
                return String::new();
            };
            children(list)
                .into_iter()
                .filter_map(|f| f.child_by_field_name("value"))
                .filter(|n| n.kind() == "table_constructor")
                .map(|p| {
                    let mut name = table_text(self.raw, p, "Name");
                    let mut kind = table_text(self.raw, p, "Type");
                    if name.is_empty() {
                        name = "<dynamic>".into();
                    }
                    if kind.is_empty() {
                        kind = "<dynamic>".into();
                    }
                    if field(self.raw, p, "Nilable").is_some_and(|n| n.kind() == "true") {
                        name.push('?');
                    }
                    format!("{name}: {kind}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        match kind {
            "function" | "scriptobject" | "callback" => {
                let mut s = format!("{name}({})", params("Arguments"));
                let out = params("Returns");
                if !out.is_empty() {
                    s.push_str(&format!(" -> {out}"));
                }
                s
            }
            "event" => format!("{name}({})", params("Payload")),
            _ => name.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lua51_goto_identifiers_preserve_trivia_ranges_and_incremental_facts() {
        let raw=b"-- goto\nlocal text = 'goto'; local long = [==[goto]==]\n--[=[ goto ]=]\nfunction goto() return text, long end\nlocal obj={}; obj.goto(); goto()\nlocal gotox = 1\n";
        let masked = lua51_grammar_input(raw, true);
        assert_eq!(masked.len(), raw.len());
        let changed: Vec<_> = raw
            .iter()
            .zip(masked.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(changed.len(), 3);
        assert!(
            changed
                .iter()
                .all(|i| &raw[i - 1..i + 3] == b"goto" && masked[*i] == b'0')
        );
        let mut document = SyntaxDocument::new(raw).unwrap();
        let facts = document.facts(false);
        assert!(facts.diagnostics.is_empty());
        assert!(
            facts
                .declarations
                .iter()
                .any(|d| d.name == "goto" && d.line == 4)
        );
        assert!(
            facts
                .relationships
                .iter()
                .any(|r| r.to == "obj.goto" && r.line == 5)
        );
        let start = raw.windows(5).position(|v| v == b"gotox").unwrap();
        let mut new = raw.to_vec();
        new.splice(start..start + 5, b"goto".iter().copied());
        document
            .edit(
                InputEdit {
                    start_byte: start,
                    old_end_byte: start + 5,
                    new_end_byte: start + 4,
                    start_position: point_at(raw, start),
                    old_end_position: point_at(raw, start + 5),
                    new_end_position: point_at(&new, start + 4),
                },
                &new,
            )
            .unwrap();
        let incremental = serde_json::to_value(document.facts(false)).unwrap();
        let full = serde_json::to_value(SyntaxDocument::new(&new).unwrap().facts(false)).unwrap();
        assert_eq!(incremental, full);
        assert_eq!(document.bytes, new);
    }
    #[test]
    fn public_calls_preserve_lexical_targets_without_alias_rows() {
        let d=SyntaxDocument::new(b"local api = C_Test\nlocal function run(api) api.Do() end\nlocal function ok() api.Do() end\napi = Other\napi.Do()\n").unwrap();
        let f = d.facts(false);
        assert_eq!(f.declarations[0].line, 2);
        assert_eq!(f.declarations[0].end_line, 2);
        assert!(
            !f.relationships
                .iter()
                .any(|r| r.from == "run" && r.to == "C_Test.Do")
        );
        assert_eq!(
            f.relationships
                .iter()
                .map(|r| (r.from.as_str(), r.to.as_str(), r.category.as_str()))
                .collect::<Vec<_>>(),
            [
                ("run", "api.Do", "call"),
                ("ok", "api.Do", "call"),
                ("<file>", "api.Do", "call")
            ]
        );
    }
    #[test]
    fn incremental_reparse_and_recovery() {
        let old = b"function Good() return 1 end\nfunction Broken(\n";
        let mut d = SyntaxDocument::new(old).unwrap();
        let f = d.facts(false);
        assert!(!f.diagnostics.is_empty());
        assert!(f.declarations.is_empty());
        assert!(d.root().has_error());
        let new = b"function Better() return 1 end\nfunction Broken(\n";
        d.edit(
            InputEdit {
                start_byte: 9,
                old_end_byte: 13,
                new_end_byte: 15,
                start_position: tree_sitter::Point::new(0, 9),
                old_end_position: tree_sitter::Point::new(0, 13),
                new_end_position: tree_sitter::Point::new(0, 15),
            },
            new,
        )
        .unwrap();
        assert!(d.facts(false).declarations.is_empty());
        assert!(d.root().has_error());
        assert_eq!(d.bytes, new);
    }
    #[test]
    fn go323_assignments_keep_exact_declarations_relations_and_order() {
        let raw = b"local api = C_Test\nlocal Item = CreateFromMixins(LocalBase)\nGlobal = CreateFromMixins(Base, C.Base)\nGetHolder()[GetKey()] = function() Ping() end\nNamed = (function() Run() end)\nlocal Local = function() Stop() end\nfunction Declared() Tail() end\n";
        let facts = SyntaxDocument::new(raw).unwrap().facts(false);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(
            serde_json::to_value(&facts.declarations).unwrap(),
            serde_json::json!([
                {"name":"Global","category":"mixin","scope":"<file>","line":3,"endLine":0,"signature":"Global = CreateFromMixins(Base, C.Base)"},
                {"name":"Named","category":"function","scope":"<file>","line":5,"endLine":0,"signature":"Named = (function() Run() end)"},
                {"name":"Local","category":"local-function","scope":"<file>","line":6,"endLine":0,"signature":"local Local = function() Stop() end"},
                {"name":"Declared","category":"function","scope":"<file>","line":7,"endLine":7,"signature":"function Declared() Tail() end"}
            ])
        );
        assert_eq!(
            serde_json::to_value(&facts.relationships).unwrap(),
            serde_json::json!([
                {"from":"<file>","to":"CreateFromMixins","category":"call","confidence":"inferred","line":2},
                {"from":"Global","to":"Base","category":"inherits","confidence":"inferred","line":3},
                {"from":"Global","to":"C.Base","category":"inherits","confidence":"inferred","line":3},
                {"from":"<file>","to":"CreateFromMixins","category":"call","confidence":"inferred","line":3},
                {"from":"<file>","to":"GetHolder","category":"call","confidence":"inferred","line":4},
                {"from":"<file>","to":"GetKey","category":"call","confidence":"inferred","line":4},
                {"from":"<file>/<anonymous:4>","to":"Ping","category":"call","confidence":"inferred","line":4},
                {"from":"Named","to":"Run","category":"call","confidence":"inferred","line":5},
                {"from":"Local","to":"Stop","category":"call","confidence":"inferred","line":6},
                {"from":"Declared","to":"Tail","category":"call","confidence":"inferred","line":7}
            ])
        );
    }
    #[test]
    fn generated_metadata_follows_all_ordinary_ast_observations() {
        let raw = br#"local api = {Name="Demo", Type="System", Namespace="C_Demo", Functions={{Name="Run", Type="Function"}}}
APIDocumentation:AddDocumentationTable(api)"#;
        let facts = SyntaxDocument::new(raw).unwrap().facts(true);
        assert!(facts.diagnostics.is_empty());
        assert_eq!(
            serde_json::to_value(&facts.relationships).unwrap(),
            serde_json::json!([
                {"from":"<file>","to":"APIDocumentation:AddDocumentationTable","category":"call","confidence":"inferred","line":2},
                {"from":"C_Demo","to":"C_Demo.Run","category":"contains","confidence":"exact","line":1}
            ])
        );
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|d| (d.name.as_str(), d.line, d.end_line))
                .collect::<Vec<_>>(),
            [("Demo", 1, 1), ("C_Demo.Run", 1, 1)]
        );
    }
    #[test]
    fn comments_do_not_shift_assignments_or_event_arguments() {
        let raw = br#"local -- first
 a, -- second
 b = function() A() end, -- between
 function() B() end
frame:RegisterEvent()
frame:RegisterEvent(-- note
 ("READY"))
;(C_Test)[("Run")]()"#;
        let facts = SyntaxDocument::new(raw).unwrap().facts(false);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|d| (d.name.as_str(), d.line, d.end_line))
                .collect::<Vec<_>>(),
            [("a", 1, 0), ("b", 1, 0)]
        );
        assert_eq!(
            facts
                .relationships
                .iter()
                .filter(|r| r.category == "event-registration")
                .map(|r| r.to.as_str())
                .collect::<Vec<_>>(),
            ["READY"]
        );
        assert_eq!(facts.relationships.last().unwrap().to, "C_Test.Run");
    }

    #[test]
    fn go323_literal_decoder_keeps_byte_escapes_parentheses_and_newlines() {
        let raw = br#"frame:RegisterEvent(("\195\169"))
frame:RegisterEvent("\255\300\q\x41")
frame:RegisterEvent([=[
READY
]=])"#;
        let facts = SyntaxDocument::new(raw).unwrap().facts(false);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert_eq!(
            facts
                .relationships
                .iter()
                .filter(|r| r.category == "event-registration")
                .map(|r| r.to.as_str())
                .collect::<Vec<_>>(),
            ["é", "�,qx41", "READY\n"]
        );
        let cr = SyntaxDocument::new(b"function A() end\rfunction B() end\n\rfunction C() end")
            .unwrap()
            .facts(false);
        assert_eq!(
            cr.declarations
                .iter()
                .map(|d| (d.line, d.end_line))
                .collect::<Vec<_>>(),
            [(1, 1), (2, 2), (3, 3)]
        );
    }

    #[test]
    fn internal_literal_bytes_keep_identity_before_public_json_replacement() {
        let raw = r#"Call("\255", "\226\130", "\195\169", "�")"#.as_bytes();
        let document = SyntaxDocument::new_public(raw).unwrap();
        let call = children(document.root()).into_iter().next().unwrap();
        let arguments = children(call.child_by_field_name("arguments").unwrap());
        assert_eq!(string_bytes(raw, arguments[0]).unwrap(), vec![255]);
        assert_eq!(string_bytes(raw, arguments[1]).unwrap(), vec![226, 130]);
        assert_eq!(string_bytes(raw, arguments[2]).unwrap(), "é".as_bytes());
        assert_eq!(string_bytes(raw, arguments[3]).unwrap(), "�".as_bytes());
        assert_eq!(string(raw, arguments[0]).unwrap(), "�");
        assert_eq!(string(raw, arguments[1]).unwrap(), "��");
        assert_eq!(string(raw, arguments[2]).unwrap(), "é");
        assert_eq!(string(raw, arguments[3]).unwrap(), "�");
    }
    #[test]
    fn public_eof_diagnostics_use_fixed_go_messages_and_zero_line() {
        for (raw, message) in [
            ("function Missing()", "syntax error"),
            ("function Missing(", "syntax error"),
            ("function", "syntax error"),
            ("Run(); local s=\"missing", "unterminated string"),
            ("local s=[=[missing", "unterminated multiline string"),
            ("Run() --[=[missing", "invalid multiline comment"),
        ] {
            let facts = SyntaxDocument::new_public(raw.as_bytes())
                .unwrap()
                .facts(false);
            assert!(facts.declarations.is_empty(), "{raw}");
            assert!(facts.relationships.is_empty(), "{raw}");
            assert_eq!(
                serde_json::to_value(facts.diagnostics).unwrap(),
                serde_json::json!([{"message":format!("source at EOF:   {message}\n"), "line":0}]),
                "{raw}"
            );
        }
    }
    #[test]
    fn public_nul_is_a_lexical_error_only_outside_literals_and_comments() {
        let facts = SyntaxDocument::new_public(b"Run()\n\0After()")
            .unwrap()
            .facts(false);
        assert!(facts.relationships.is_empty());
        assert_eq!(
            serde_json::to_value(facts.diagnostics).unwrap(),
            serde_json::json!([{"message":"source line:2(column:1) near '\0':   Invalid token\n", "line":2}])
        );
        for raw in [
            b"frame:RegisterEvent(\"A\0B\")".as_slice(),
            b"frame:RegisterEvent([[A\0B]])",
        ] {
            assert!(SyntaxDocument::new(raw).is_err());
            let facts = SyntaxDocument::new_public(raw).unwrap().facts(false);
            assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
            assert_eq!(facts.relationships[1].to, "A\0B");
        }
        for raw in [b"-- A\0B\nRun()".as_slice(), b"--[[A\0B]] Run()"] {
            let facts = SyntaxDocument::new_public(raw).unwrap().facts(false);
            assert!(facts.diagnostics.is_empty());
            assert_eq!(facts.relationships[0].to, "Run");
        }
    }
    #[test]
    fn public_newline_call_rejects_recovered_rows_without_changing_native_tree() {
        for raw in [b"f()\n()".as_slice(), b"f(1)\r\n(2)"] {
            let document = SyntaxDocument::new_public(raw).unwrap();
            let facts = document.facts(false);
            assert!(facts.relationships.is_empty());
            assert_eq!(facts.diagnostics.len(), 1);
            assert!(
                facts.diagnostics[0]
                    .message
                    .contains("ambiguous syntax (function call x new statement)")
            );
            assert_eq!(
                document.root().to_sexp(),
                SyntaxDocument::new(raw).unwrap().root().to_sexp()
            );
        }
        assert!(
            SyntaxDocument::new_public(b"f() ()")
                .unwrap()
                .facts(false)
                .diagnostics
                .is_empty()
        );
    }
    #[test]
    fn public_goto_keywords_and_native_lua51_identifiers_have_separate_consumers() {
        let identifier = b"function goto() end";
        assert!(!SyntaxDocument::new(identifier).unwrap().root().has_error());
        assert!(
            SyntaxDocument::new_public(identifier)
                .unwrap()
                .public_parse_note()
                .is_some()
        );
        let keyword = SyntaxDocument::new_public(b"goto done; ::done:: Run()")
            .unwrap()
            .facts(false);
        assert!(keyword.diagnostics.is_empty());
        assert_eq!(keyword.relationships.len(), 1);
        assert_eq!(keyword.relationships[0].to, "Run");
    }

    #[test]
    fn public_newer_syntax_never_leaks_partial_observations() {
        for raw in [
            "function Good() end; local x=1//2",
            "Run(); local x <const> = 1",
            "Run(); local x=0x1p2",
            "Run(); local 界=1",
        ] {
            let document = SyntaxDocument::new_public(raw.as_bytes()).unwrap();
            assert!(document.public_parse_note().is_some());
            let facts = document.facts(false);
            assert!(facts.declarations.is_empty());
            assert!(facts.relationships.is_empty());
            assert!(!facts.diagnostics.is_empty());
        }
    }
}
