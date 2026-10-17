//! Public source facts preserve the fixed Go scanner contract. Native semantic
//! analysis uses the syntax tree independently of these literal observations.
use crate::facts::worker::{self, WorkerGuard};
use crate::facts::{Declaration, Facts, Relationship, SyntaxNote};
#[cfg(test)]
use lycheedev_core::control::OperationControl;
use lycheedev_core::{Result, ToolkitError};
use std::borrow::Cow;
use tree_sitter::{InputEdit, Node, ParseOptions, Parser, Tree};

mod generated;
#[cfg(test)]
#[path = "lua_tree/generated_tests.rs"]
mod generated_tests;

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
        Self::new_with_guard(bytes, lua51_identifiers, None)
    }
    fn new_with_guard(
        bytes: &[u8],
        lua51_identifiers: bool,
        guard: Option<&WorkerGuard<'_>>,
    ) -> Result<Self> {
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
        let grammar = lua51_grammar_input_with_guard(bytes, lua51_identifiers, guard);
        let tree = if let Some(guard) = guard {
            if !guard.check() {
                None
            } else {
                let mut progress = |_: &tree_sitter::ParseState| {
                    if guard.check() {
                        std::ops::ControlFlow::Continue(())
                    } else {
                        std::ops::ControlFlow::Break(())
                    }
                };
                parser.parse_with_options(
                    &mut |offset, _| grammar.get(offset..).unwrap_or_default(),
                    None,
                    Some(ParseOptions::new().progress_callback(&mut progress)),
                )
            }
        } else {
            parser.parse(grammar.as_ref(), None)
        }
        .ok_or_else(|| ToolkitError::new("codebase.source_budget", "Lua parser cancelled", 3))?;
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
        self.public_parse_note_with_guard(None)
    }
    fn public_parse_note_with_guard(&self, guard: Option<&WorkerGuard<'_>>) -> Option<SyntaxNote> {
        if let Some(note) = public_lexical_note(&self.bytes, guard) {
            return Some(note);
        }
        let mut missing_at_eof = false;
        let mut previous_token = "";
        let end = self
            .bytes
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map_or(0, |at| at + 1);
        let mut cursor = self.tree.walk();
        loop {
            if guard.is_some_and(|guard| !guard.observe_node()) {
                return None;
            }
            let node = cursor.node();
            missing_at_eof |= node.is_missing() && node.start_byte() >= end;
            missing_at_eof |= node.is_error()
                && node.end_byte() >= end
                && incomplete_function_header(text(&self.bytes, node));
            missing_at_eof |=
                node.is_error() && node.end_byte() >= end && incomplete_function_body(node);
            // An ERROR subtree can be bypassed by the facts walk (notably an
            // assignment's recovered RHS). Inspect its real token boundary
            // here before publishing any recovered facts. Comments are
            // trivia; strings remain one token, never their contents.
            if node.kind() != "comment"
                && (node.child_count() == 0 || node.kind() == "string")
                && !node.is_missing()
            {
                let token = text(&self.bytes, node);
                let empty_expression = token == ")"
                    && previous_token == "("
                    && !node.parent().is_some_and(|delimiter| {
                        delimiter
                            .parent()
                            .is_some_and(|owner| match delimiter.kind() {
                                "arguments" => owner.kind() == "function_call",
                                "parameters" => {
                                    matches!(
                                        owner.kind(),
                                        "function_declaration" | "function_definition"
                                    ) || owner.is_error() && {
                                        let mut cursor = owner.walk();
                                        let found = owner
                                            .children(&mut cursor)
                                            .any(|child| child.kind() == "function");
                                        found
                                    }
                                }
                                _ => false,
                            })
                    });
                if token == ";" && previous_token == "=" || empty_expression {
                    return Some(public_lua_error(
                        &self.bytes,
                        node.end_byte() - 1,
                        &self.bytes[node.byte_range()],
                        "syntax error",
                    ));
                }
                previous_token = token;
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
                if !worker::check(guard) {
                    return None;
                }
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
        self.facts_with_guard(generated, None)
            .expect("unrestricted facts")
    }
    fn facts_with_guard(&self, generated: bool, guard: Option<&WorkerGuard<'_>>) -> Option<Facts> {
        if !self.lua51_identifiers
            && let Some(note) = self.public_parse_note_with_guard(guard)
        {
            let mut facts = Facts::fresh();
            if !worker::charge(guard, note.message.capacity()) {
                return None;
            }
            worker::push(guard, &mut facts.diagnostics, note);
            return worker::check(guard).then_some(facts);
        }
        if !worker::charge(guard, "<file>".len()) {
            return None;
        }
        let mut scopes = Vec::new();
        if !worker::prepare_push(guard, &mut scopes) {
            return None;
        }
        scopes.push("<file>".into());
        let mut w = Walker {
            raw: &self.bytes,
            facts: Facts::fresh(),
            scopes,
            guard,
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
                w.documentation(root);
            }
        }
        worker::check(guard).then_some(w.facts)
    }
}
/// Nested XML handlers share their document's owned/scratch admission. The
/// caller keeps the original input <=64 KiB and the same observation-only guard.
pub(crate) fn facts_with_worker_guard(raw: &[u8], guard: &WorkerGuard<'_>) -> Option<Facts> {
    generated_facts_with_worker_guard(raw, false, guard)
}
pub(crate) fn generated_facts_with_worker_guard(
    raw: &[u8],
    generated: bool,
    guard: &WorkerGuard<'_>,
) -> Option<Facts> {
    if raw.len() > 64 << 10 || !guard.check() {
        guard.abandon();
        return None;
    }
    let result = SyntaxDocument::new_with_guard(raw, false, Some(guard))
        .ok()
        .and_then(|document| document.facts_with_guard(generated, Some(guard)));
    if result.is_none() {
        guard.abandon();
    }
    result
}
/// Lua 5.1's `goto` is an identifier. The pinned modern grammar reserves it.
/// Token-only, same-width masking makes the grammar see an identifier while
/// every observation/name/hash/span reads unchanged original bytes. Comments
/// and longer identifiers are untouched. Unsupported modern string escapes
/// receive a same-width grammar-only mask; the literal decoder reads originals.
/// Bundled luac remains the separate native WoW syntax authority.
fn lua51_grammar_input(raw: &[u8], mask_goto: bool) -> Cow<'_, [u8]> {
    lua51_grammar_input_with_guard(raw, mask_goto, None)
}
fn lua51_grammar_input_with_guard<'a>(
    raw: &'a [u8],
    mask_goto: bool,
    guard: Option<&WorkerGuard<'_>>,
) -> Cow<'a, [u8]> {
    fn long_end(raw: &[u8], at: usize, scan: &mut worker::ByteScan<'_>) -> Option<usize> {
        if raw.get(at) != Some(&b'[') {
            return None;
        }
        if !scan.byte() {
            return None;
        }
        let mut second = at + 1;
        while raw.get(second) == Some(&b'=') {
            if !scan.byte() {
                return None;
            }
            second += 1;
        }
        if raw.get(second) != Some(&b'[') {
            return None;
        }
        if !scan.byte() {
            return None;
        }
        let width = second - at;
        let mut cursor = second + 1;
        while cursor < raw.len() {
            if !scan.byte() {
                return None;
            }
            if raw[cursor] == b']'
                && raw.get(cursor + width) == Some(&b']')
                && raw
                    .get(cursor + 1..cursor + width)
                    .is_some_and(|v| scan.all_eq(v, b'='))
            {
                if !scan.byte() {
                    return None;
                }
                return Some(cursor + width + 1);
            }
            cursor += 1;
        }
        Some(raw.len())
    }
    fn identifier(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_' || c >= 128
    }
    let mut scan = worker::ByteScan::new(guard);
    let mut changed: Option<Vec<u8>> = None;
    let mut at = 0;
    while at < raw.len() {
        if !scan.byte() {
            break;
        }
        if raw.get(at..at + 2) == Some(b"--") {
            if !scan.byte() {
                break;
            }
            if let Some(end) = long_end(raw, at + 2, &mut scan) {
                if !mask_goto {
                    mask_literal_nul(raw, at, end, &mut changed, &mut scan);
                }
                at = end;
            } else {
                at += 2;
                while at < raw.len() && raw[at] != b'\n' && (mask_goto || raw[at] != b'\r') {
                    if !scan.byte() {
                        break;
                    }
                    if !mask_goto && raw[at] == 0 {
                        if !set_grammar_byte(raw, at, b' ', &mut changed, &mut scan) {
                            break;
                        }
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
                if !scan.byte() {
                    break;
                }
                if !mask_goto && raw[at] == 0 {
                    if !set_grammar_byte(raw, at, b' ', &mut changed, &mut scan) {
                        break;
                    }
                }
                if raw[at] == b'\\' {
                    if !mask_goto && raw.get(at + 1) == Some(&0) {
                        if !set_grammar_byte(raw, at + 1, b' ', &mut changed, &mut scan) {
                            break;
                        }
                    }
                    if let Some(next) = raw.get(at + 1)
                        && !b"abfnrtv\\'\"0123456789\r\n".contains(next)
                    {
                        // Lua 5.1/gopher-lua accept arbitrary escaped bytes.
                        // The modern grammar rejects some and gives others
                        // newer meanings. Only the grammar view is masked;
                        // literal decoding always reads the original bytes.
                        if !set_grammar_byte(raw, at, b' ', &mut changed, &mut scan) {
                            break;
                        }
                    }
                    if !scan.bytes(usize::from(at + 1 < raw.len())) {
                        break;
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
        if let Some(end) = long_end(raw, at, &mut scan) {
            if !mask_goto {
                mask_literal_nul(raw, at, end, &mut changed, &mut scan);
            }
            at = end;
            continue;
        }
        if raw[at].is_ascii_alphabetic() || raw[at] == b'_' || raw[at] >= 128 {
            let start = at;
            at += 1;
            while at < raw.len() && identifier(raw[at]) {
                if !scan.byte() {
                    break;
                }
                at += 1;
            }
            if mask_goto && &raw[start..at] == b"goto" {
                if !set_grammar_byte(raw, start + 1, b'0', &mut changed, &mut scan) {
                    break;
                }
            }
        } else {
            at += 1;
        }
    }
    changed.map(Cow::Owned).unwrap_or(Cow::Borrowed(raw))
}
// Same-width grammar adaptation only. Public literal bytes remain untouched.
fn mask_literal_nul(
    raw: &[u8],
    start: usize,
    end: usize,
    changed: &mut Option<Vec<u8>>,
    scan: &mut worker::ByteScan<'_>,
) {
    for at in start..end {
        if !scan.byte() {
            return;
        }
        if raw[at] == 0 && !set_grammar_byte(raw, at, b' ', changed, scan) {
            return;
        }
    }
}
fn set_grammar_byte(
    raw: &[u8],
    at: usize,
    value: u8,
    changed: &mut Option<Vec<u8>>,
    scan: &mut worker::ByteScan<'_>,
) -> bool {
    if changed.is_none() && !scan.check() {
        return false;
    }
    changed.get_or_insert_with(|| raw.to_vec())[at] = value;
    true
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
// Tree-sitter may retain an unfinished function as one ERROR containing its
// parsed parameters/body rather than inserting a missing `end`. A parsed
// body boundary proves that this is beyond the header. Keep EOF pending so a
// real bad body token visited later wins over this missing terminator.
fn incomplete_function_body(node: Node<'_>) -> bool {
    if !node.is_error() {
        return false;
    }
    let mut cursor = node.walk();
    let mut function = false;
    let mut parameters = false;
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_missing() {
            return false;
        }
        match child.kind() {
            "comment" => {}
            "function" if !parameters => function = true,
            "parameters" if !parameters => parameters = true,
            "end" => return false,
            // These are already parsed statement/body productions, not
            // arbitrary text after a function token. An earlier malformed
            // body child must not be reclassified as a terminator-at-EOF.
            "block"
            | "function_call"
            | "assignment_statement"
            | "variable_declaration"
            | "function_declaration"
            | "do_statement"
            | "while_statement"
            | "repeat_statement"
            | "if_statement"
            | "for_statement"
            | "return_statement"
            | "break_statement"
            | "goto_statement"
            | "label_statement"
            | "empty_statement"
                if parameters => {}
            _ if parameters => return false,
            _ => {}
        }
    }
    function && parameters
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
fn public_lexical_note(raw: &[u8], guard: Option<&WorkerGuard<'_>>) -> Option<SyntaxNote> {
    let mut scan = worker::ByteScan::new(guard);
    let mut at = if raw.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    if !scan.bytes(at) {
        return None;
    }
    if raw[at..].starts_with(b"#!") {
        if let Some(end) = scan.find(&raw[at..], b"\n") {
            at += end;
        }
    }
    while at < raw.len() {
        if !scan.byte() {
            return None;
        }
        let comment = raw[at..].starts_with(b"--");
        if comment {
            if !scan.byte() {
                return None;
            }
            at += 2;
        }
        if raw.get(at) == Some(&b'[')
            && raw
                .get(at + 1)
                .is_some_and(|byte| matches!(*byte, b'[' | b'='))
        {
            let start = at;
            if !scan.byte() {
                return None;
            }
            at += 1;
            while raw.get(at) == Some(&b'=') {
                if !scan.byte() {
                    return None;
                }
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
            if !scan.byte() {
                return None;
            }
            at += 1;
            loop {
                if !scan.byte() {
                    return None;
                }
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
                        .is_some_and(|bytes| scan.all_eq(bytes, b'='))
                {
                    if !scan.byte() {
                        return None;
                    }
                    at += width + 1;
                    break;
                }
                at += 1;
            }
            continue;
        }
        if comment {
            while at < raw.len() && !matches!(raw[at], b'\r' | b'\n') {
                if !scan.byte() {
                    return None;
                }
                at += 1;
            }
            continue;
        }
        if matches!(raw[at], b'\'' | b'"') {
            let quote = raw[at];
            let start = at;
            if !scan.byte() {
                return None;
            }
            at += 1;
            loop {
                if !scan.byte() {
                    return None;
                }
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
                    if !scan.check() {
                        return None;
                    }
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
                    if !scan.byte() {
                        return None;
                    }
                    at += 1;
                    if matches!(escaped, b'\r' | b'\n')
                        && raw
                            .get(at)
                            .is_some_and(|next| matches!(*next, b'\r' | b'\n') && *next != escaped)
                    {
                        if !scan.byte() {
                            return None;
                        }
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
    string_value_bytes_into(value, Vec::new())
}
fn string_value_bytes_into(value: &[u8], mut out: Vec<u8>) -> Option<Vec<u8>> {
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

// A root probe needs only the first matching field. Do not allocate all of a
// table's children before testing its usually early Type field.
fn field<'a>(
    raw: &[u8],
    table: Node<'a>,
    key: &str,
    guard: Option<&WorkerGuard<'_>>,
) -> Option<Node<'a>> {
    let mut cursor = table.walk();
    for f in table
        .named_children(&mut cursor)
        .filter(|f| f.kind() == "field")
    {
        if !worker::check(guard) {
            return None;
        }
        let n = f.child_by_field_name("name");
        if n.is_some_and(|n| text(raw, n) == key || string(raw, n).as_deref() == Some(key)) {
            return f.child_by_field_name("value").map(unparenthesized);
        }
    }
    None
}
struct Walker<'a, 'tree> {
    raw: &'a [u8],
    guard: Option<&'a WorkerGuard<'a>>,
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
    fn scope_name(&self) -> &str {
        self.scopes.last().map(String::as_str).unwrap_or("<file>")
    }
    fn scope(&self) -> String {
        let scope = self.scope_name();
        if !worker::charge(self.guard, scope.len()) {
            return String::new();
        }
        scope.into()
    }
    fn note(&mut self, message: &str, line: usize) {
        if !worker::charge(self.guard, message.len())
            || !worker::prepare_push(self.guard, &mut self.facts.diagnostics)
        {
            return;
        }
        self.facts.diagnostics.push(SyntaxNote {
            message: message.into(),
            line,
        });
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
        let source_signature = self
            .lines
            .get(line - 1)
            .copied()
            .unwrap_or("")
            .trim_start_matches('\u{feff}')
            .trim();
        let signature_bytes = signature
            .as_ref()
            .map_or(source_signature.len(), String::capacity);
        let scope_bytes = if scope { self.scope_name().len() } else { 0 };
        if !worker::charge(
            self.guard,
            name.capacity()
                .saturating_add(category.len())
                .saturating_add(scope_bytes)
                .saturating_add(signature_bytes),
        ) || !worker::prepare_push(self.guard, &mut self.facts.declarations)
        {
            return;
        }
        let signature = signature.unwrap_or_else(|| source_signature.into());
        self.facts.declarations.push(Declaration {
            name,
            category: category.into(),
            scope: if scope {
                self.scope_name().into()
            } else {
                String::new()
            },
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
        if !worker::charge(
            self.guard,
            from.capacity()
                .saturating_add(to.capacity())
                .saturating_add(category.len())
                .saturating_add(confidence.len()),
        ) || !worker::prepare_push(self.guard, &mut self.facts.relationships)
        {
            return;
        }
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
        if !worker::check(self.guard) {
            return;
        }
        let ordinal = self.facts.relationships.len();
        let display = match String::from_utf8(target) {
            Ok(value) => value,
            Err(error) => {
                let target = error.into_bytes();
                let display = lua_json_string(&target);
                if self.guard.is_some_and(|guard| {
                    !guard.raw_target(
                        self.facts
                            .raw_relationship_targets
                            .as_ref()
                            .map_or(0, |targets| targets.len()),
                        target.capacity(),
                    )
                }) {
                    return;
                }
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
        if !worker::check(self.guard) {
            return;
        }
        let scope = if let Some((name, category, source)) = declared {
            if !worker::charge(self.guard, name.len()) {
                return;
            }
            self.declare(source, name.clone(), category, true, None);
            name
        } else {
            // Reserve before formatting the owned scope; 32 bytes covers the
            // suffix and any usize line number on the supported targets.
            if !worker::charge(
                self.guard,
                self.scope_name().len().saturating_add(32).saturating_mul(2),
            ) {
                return;
            }
            format!("{}/<anonymous:{line}>", self.scope_name())
        };
        if !worker::charge(self.guard, scope.capacity())
            || !worker::prepare_push(self.guard, &mut self.scopes)
        {
            return;
        }
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
        if !worker::check(self.guard) {
            return;
        }
        let _scratch = if let Some(guard) = self.guard {
            let Some(frame) = guard.frame(self.raw.len()) else {
                return;
            };
            Some(frame)
        } else {
            None
        };
        if self.depth >= 256 {
            if !self
                .facts
                .diagnostics
                .iter()
                .any(|n| n.message == "source_ast_depth_limit")
            {
                self.note("source_ast_depth_limit", n.start_position().row + 1);
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
                self.note("Syntax unavailable in fixed Go Lua parser", self.line(n));
                self.rejected_syntax = true;
            }
            return;
        }
        self.work += 1;
        if self.work > 2_000_000 {
            if self.work == 2_000_001 {
                self.note("source_ast_work_limit", n.start_position().row + 1);
            }
            return;
        }
        if n.is_error() || n.is_missing() {
            let message = if n.is_missing() {
                format!("Missing Lua token: {}", n.kind())
            } else {
                "Lua syntax recovery region".into()
            };
            self.note(&message, n.start_position().row + 1);
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
                        if !worker::check(self.guard) {
                            return;
                        }
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
                if self.generated
                    && field(self.raw, n, "Type", self.guard)
                        .and_then(|value| string(self.raw, value))
                        .as_deref()
                        == Some("System") =>
            {
                if worker::prepare_scratch_push(self.guard, &mut self.api_roots) {
                    self.api_roots.push(n);
                }
            }
            "attribute" => self.note(
                "Syntax unavailable in WoW Lua 5.1",
                n.start_position().row + 1,
            ),
            _ => {}
        }
        if !worker::check(self.guard) {
            return;
        }
        for c in children(n) {
            if !worker::check(self.guard) {
                return;
            }
            self.walk(c);
        }
    }
    fn assignment(&mut self, n: Node<'tree>, local: bool, source: Node<'tree>) {
        if !worker::check(self.guard) {
            return;
        }
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
            if !worker::check(self.guard) {
                return;
            }
            self.walk(*var);
        }
        for (i, expression) in values.into_iter().enumerate() {
            if !worker::check(self.guard) {
                return;
            }
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
                            if !worker::check(self.guard) {
                                return;
                            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    fn on_worker_stack(work: impl FnOnce() + Send) {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("source-facts-regression".into())
                .stack_size(2 << 20)
                .spawn_scoped(scope, work)
                .expect("production-sized parser test worker")
                .join()
                .expect("parser test worker completed");
        });
    }
    fn same_worker_facts(raw: &[u8]) {
        on_worker_stack(|| {
            let control = OperationControl::default();
            let expected = crate::facts::lua(raw, false);
            let actual = crate::facts::lua_for_worker(raw, &control, 4 << 20)
                .expect("ordinary fixture fits speculative allowance");
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
            assert_eq!(
                actual.raw_relationship_targets,
                expected.raw_relationship_targets
            );
            for ordinal in 0..expected.relationships.len() {
                assert_eq!(
                    actual.relationship_target_bytes(ordinal),
                    expected.relationship_target_bytes(ordinal)
                );
            }
        });
    }
    #[test]
    fn worker_facts_preserve_complete_public_fields_and_raw_target_identity() {
        for raw in [
            b"".as_slice(),
            b"local function f(x) frame:RegisterEvent('READY'); return A.B(x) end\nf()",
            b"A, B = CreateFromMixins(Base, Other), function() local x = function() call() end end",
            b"function T:m(a, b) local z = (function() f() end)(); self:RegisterUnitEvent('UNIT', a) end",
            br#"T["\255"](); frame:RegisterEvent("\255\254"); T["\000"]()"#,
            br#"T["\999"](); frame:RegisterEvent("\x41\u{42}\z"); ((A.B))()"#,
            b"-- comment\r\nfunction f()\r call()\n\rend",
            b"local f = function() return function() foo() end end",
            b"local x = ;",
            b"function f(",
            b"function f() local x = ;",
            b"f(())",
            b"function goto() end",
            b"::label:: goto label",
            b"local s = 'unterminated\n",
            b"local s = [==[unterminated",
            b"--[=x",
            b"local x = 1\0",
            b"f('literal\0byte') -- comment\0\n",
            b"local x <const> = 1",
            b"return 1 // 2",
        ] {
            same_worker_facts(raw);
        }
    }
    #[test]
    fn worker_facts_owned_cap_abandons_without_a_new_public_failure() {
        on_worker_stack(|| {
            let control = OperationControl::default();
            let raw = b"function f() frame:RegisterEvent('READY'); A.B() end";
            let expected = crate::facts::lua(raw, false);
            for cap in [0, 1, 64, 128] {
                assert!(crate::facts::lua_for_worker(raw, &control, cap).is_none());
                let replay = crate::facts::lua(raw, false);
                assert_eq!(
                    serde_json::to_vec(&replay).unwrap(),
                    serde_json::to_vec(&expected).unwrap()
                );
            }
            assert!(crate::facts::lua_for_worker(b"local x = ;", &control, 1).is_none());
            assert!(control.check().is_ok());
            same_worker_facts(raw);
        });
    }
    #[test]
    fn worker_facts_scope_signature_and_invalid_target_caps_fall_back() {
        on_worker_stack(|| {
            let control = OperationControl::default();
            // Every declaration retains the whole physical source line as its
            // signature. Admission must happen before those repeated clones.
            let raw = format!("{}{}", "function a() end ".repeat(100), " ".repeat(2000));
            assert!(crate::facts::lua_for_worker(raw.as_bytes(), &control, 32 << 10).is_none());
            let expected = crate::facts::lua(raw.as_bytes(), false);
            assert_eq!(expected.declarations.len(), 100);
            same_worker_facts(raw.as_bytes());
            let invalid = br#"T["\255"]()"#;
            assert!(crate::facts::lua_for_worker(invalid, &control, 1024).is_none());
            same_worker_facts(invalid);
        });
    }
    #[test]
    fn worker_facts_observe_stop_without_latching_or_copying_parent_state() {
        on_worker_stack(|| {
            use lycheedev_core::control::{StopCause, TrackedCancellation};
            use std::time::{Duration, Instant};
            let parent_signal = TrackedCancellation::new();
            let parent = OperationControl::tracked(
                Instant::now() + Duration::from_secs(5),
                parent_signal.clone(),
            );
            let child = parent.ordered_child(parent.deadline()).unwrap();
            let local_signal = child.tracked_cancel_handle().unwrap();
            let document = SyntaxDocument::new_public(b"function f() call() end").unwrap();
            let guard = WorkerGuard::new(&child, 4 << 20);
            assert!(guard.check());
            parent_signal.cancel();
            assert!(document.facts_with_guard(false, Some(&guard)).is_none());
            assert!(crate::facts::lua_for_worker(b"f()", &child, 4 << 20).is_none());
            assert!(!local_signal.is_cancelled());
            assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
            let expired = OperationControl::uncancelable(Instant::now());
            assert!(crate::facts::lua_for_worker(b"f()", &expired, 4 << 20).is_none());
            assert!(!expired.is_cancelled());
            assert_eq!(
                expired.stop_cause().unwrap(),
                Some(StopCause::DeadlineExceeded)
            );
        });
    }
    #[test]
    fn worker_facts_scratch_and_input_admission_leave_original_depth_contract() {
        on_worker_stack(|| {
            let control = OperationControl::default();
            same_worker_facts(&vec![b'\n'; 64 << 10]);
            assert!(
                crate::facts::lua_for_worker(&vec![b' '; (64 << 10) + 1], &control, 4 << 20)
                    .is_none()
            );
            assert!(crate::facts::lua_for_worker(b"\xff", &control, 4 << 20).is_none());
            // The original walk still reaches its depth-256 diagnostic. Optional
            // scratch admission may abandon earlier and must never publish it.
            let raw = format!(
                "{}call(){}{}",
                "do ".repeat(260),
                " end".repeat(260),
                " ".repeat(32 << 10)
            );
            assert!(raw.len() < 64 << 10);
            assert!(crate::facts::lua_for_worker(raw.as_bytes(), &control, 4 << 20).is_none());
            assert!(
                crate::facts::lua(raw.as_bytes(), false)
                    .diagnostics
                    .iter()
                    .any(|note| note.message == "source_ast_depth_limit")
            );
        });
    }
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
            ("function Missing()\n Run()", "syntax error"),
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
    fn public_newline_calls_keep_fixed_go_nested_relationship_order() {
        // Actual fixed-Go1.27.1/gopher-lua1.1.1 observations, report SHA256
        // 849e3b0ab76e2252e10f183bb84c281ba96452cb0f32bc0f430e8ab9680730dc.
        // These calls are accepted despite the lexer/parser PNewLine code.
        for raw in [b"f()\n()".as_slice(), b"f(1)\n(2)", b"f()\r\n()", b"f() ()"] {
            let document = SyntaxDocument::new_public(raw).unwrap();
            let facts = document.facts(false);
            assert_eq!(
                serde_json::to_value(&facts).unwrap(),
                serde_json::json!({
                    "declarations":[], "loads":[], "headers":[], "diagnostics":[],
                    "relationships":[
                        {"from":"<file>","to":"<dynamic>","category":"call","confidence":"dynamic-unresolved","line":1},
                        {"from":"<file>","to":"f","category":"call","confidence":"inferred","line":1}
                    ]
                }),
                "tree: {}",
                document.root().to_sexp()
            );
            assert_eq!(
                document.root().to_sexp(),
                SyntaxDocument::new(raw).unwrap().root().to_sexp()
            );
        }
    }
    #[test]
    fn public_recovery_reports_bad_tokens_before_an_unclosed_function_eof() {
        for (raw, line, column, token, message) in [
            ("f();\n()", 2, 2, ")", "syntax error"),
            ("local value = ;\nC_Good.Do()", 1, 15, ";", "syntax error"),
            (
                "function Missing()\n local value = ;",
                2,
                16,
                ";",
                "syntax error",
            ),
            ("function Missing()\n \0", 2, 2, "\0", "Invalid token"),
            ("local value = -- comment\n ;", 2, 2, ";", "syntax error"),
        ] {
            let document = SyntaxDocument::new_public(raw.as_bytes()).unwrap();
            let facts = document.facts(false);
            assert_eq!(
                serde_json::to_value(&facts).unwrap(),
                serde_json::json!({
                    "declarations":[], "relationships":[], "loads":[], "headers":[],
                    "diagnostics":[{"message":format!("source line:{line}(column:{column}) near '{token}':   {message}\n"),"line":line}]
                }),
                "input: {raw:?}; tree: {}",
                document.root().to_sexp()
            );
        }
        for raw in [
            "function Complete() local value = 1; Run() end",
            "local value = '= ;'; Run()",
            "local value = {key = 1;}; Run()",
            "f();\n g()",
        ] {
            let document = SyntaxDocument::new_public(raw.as_bytes()).unwrap();
            assert!(
                document.facts(false).diagnostics.is_empty(),
                "input: {raw}; tree: {}",
                document.root().to_sexp()
            );
        }
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
