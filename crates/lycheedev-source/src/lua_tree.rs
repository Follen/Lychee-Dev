//! Structural Lua observations from tree-sitter. Alias links are local static
//! evidence; semantic definitions/references are provided by the LuaLS adapter.
use crate::facts::{Declaration, Facts, Relationship, SyntaxNote};
use lycheedev_core::{Result, ToolkitError};
use std::{borrow::Cow, collections::BTreeMap};
use tree_sitter::{InputEdit, Node, Parser, Tree};

pub struct SyntaxDocument {
    bytes: Vec<u8>,
    tree: Tree,
    parser: Parser,
}
impl SyntaxDocument {
    pub(crate) fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if std::str::from_utf8(bytes).is_err() || bytes.contains(&0) {
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
        let grammar = lua51_grammar_input(bytes);
        let tree = parser.parse(grammar.as_ref(), None).ok_or_else(|| {
            ToolkitError::new("codebase.source_budget", "Lua parser cancelled", 3)
        })?;
        Ok(Self {
            bytes: bytes.to_vec(),
            tree,
            parser,
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
            || new_bytes.contains(&0)
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
        let old_grammar = lua51_grammar_input(&self.bytes);
        let new_grammar = lua51_grammar_input(new_bytes);
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
    pub fn facts(&self, generated: bool) -> Facts {
        let mut w = Walker {
            raw: &self.bytes,
            facts: Facts::default(),
            scopes: vec!["<file>".into()],
            aliases: vec![BTreeMap::new()],
            generated,
            work: 0,
            depth: 0,
            lines: std::str::from_utf8(&self.bytes)
                .unwrap_or("")
                .split('\n')
                .collect(),
        };
        w.walk(self.tree.root_node());
        w.facts
    }
}
/// Lua 5.1's `goto` is an identifier. The pinned modern grammar reserves it.
/// Token-only, same-width masking makes the grammar see an identifier while
/// every observation/name/hash/span reads unchanged original bytes. Strings,
/// comments and longer identifiers are never rewritten. This is structural
/// compatibility; bundled luac remains the syntax authority.
fn lua51_grammar_input(raw: &[u8]) -> Cow<'_, [u8]> {
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
                at = end;
            } else {
                at += 2;
                while at < raw.len() && raw[at] != b'\n' {
                    at += 1;
                }
            }
            continue;
        }
        if matches!(raw[at], b'\'' | b'"') {
            let quote = raw[at];
            at += 1;
            while at < raw.len() {
                if raw[at] == b'\\' {
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
            at = end;
            continue;
        }
        if raw[at].is_ascii_alphabetic() || raw[at] == b'_' || raw[at] >= 128 {
            let start = at;
            at += 1;
            while at < raw.len() && identifier(raw[at]) {
                at += 1;
            }
            if &raw[start..at] == b"goto" {
                changed.get_or_insert_with(|| raw.to_vec())[start + 1] = b'0';
            }
        } else {
            at += 1;
        }
    }
    changed.map(Cow::Owned).unwrap_or(Cow::Borrowed(raw))
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
fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor).collect()
}
fn text<'a>(raw: &'a [u8], n: Node<'_>) -> &'a str {
    std::str::from_utf8(&raw[n.byte_range()]).unwrap_or("")
}
fn name(raw: &[u8], n: Node<'_>) -> String {
    name_limited(raw, n, 0)
}
fn name_limited(raw: &[u8], n: Node<'_>, depth: usize) -> String {
    if depth > 256 {
        return String::new();
    }
    match n.kind() {
        "identifier" => text(raw, n).into(),
        "dot_index_expression" | "method_index_expression" => {
            let Some(t) = n.child_by_field_name("table") else {
                return String::new();
            };
            let Some(f) = n
                .child_by_field_name("field")
                .or_else(|| n.child_by_field_name("method"))
            else {
                return String::new();
            };
            let base = name_limited(raw, t, depth + 1);
            if base.is_empty() {
                return base;
            }
            format!(
                "{base}{}{}",
                if n.kind() == "method_index_expression" {
                    ":"
                } else {
                    "."
                },
                text(raw, f)
            )
        }
        "bracket_index_expression" => {
            let Some(t) = n.child_by_field_name("table") else {
                return String::new();
            };
            let Some(f) = n.child_by_field_name("field") else {
                return String::new();
            };
            let base = name_limited(raw, t, depth + 1);
            let Some(field) = string(raw, f) else {
                return String::new();
            };
            if base.is_empty() {
                return base;
            }
            format!("{base}.{field}")
        }
        _ => String::new(),
    }
}
fn string(raw: &[u8], node: Node<'_>) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let s = text(raw, node);
    let first = s.as_bytes().first().copied()?;
    if first == b'[' {
        let open = s.find('[')?;
        let second = s[open + 1..].find('[')? + open + 1;
        let width = second + 1;
        let mut content = s.get(width..s.len().checked_sub(width)?)?;
        if let Some(rest) = content
            .strip_prefix("\r\n")
            .or_else(|| content.strip_prefix('\n'))
        {
            content = rest;
        }
        return Some(content.into());
    }
    if !b"'\"".contains(&first) || s.len() < 2 {
        return None;
    }
    let mut out = String::new();
    let mut chars = s[1..s.len() - 1].chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let escaped = chars.next()?;
        match escaped {
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\u{b}'),
            '\\' | '\'' | '"' => out.push(escaped),
            '\n' => out.push('\n'),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '0'..='9' => {
                let mut digits = escaped.to_string();
                for _ in 0..2 {
                    if chars.peek().is_some_and(char::is_ascii_digit) {
                        digits.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let value: u8 = digits.parse().ok()?;
                out.push(char::from(value));
            }
            _ => return None,
        }
    }
    Some(out)
}
fn field<'a>(raw: &[u8], table: Node<'a>, key: &str) -> Option<Node<'a>> {
    for f in children(table).into_iter().filter(|f| f.kind() == "field") {
        let n = f.child_by_field_name("name");
        if n.is_some_and(|n| text(raw, n) == key || string(raw, n).as_deref() == Some(key)) {
            return f.child_by_field_name("value");
        }
    }
    None
}
fn table_text(raw: &[u8], table: Node<'_>, key: &str) -> String {
    field(raw, table, key)
        .and_then(|n| string(raw, n))
        .unwrap_or_default()
}
struct Walker<'a> {
    raw: &'a [u8],
    facts: Facts,
    scopes: Vec<String>,
    aliases: Vec<BTreeMap<String, Option<String>>>,
    generated: bool,
    work: usize,
    depth: usize,
    lines: Vec<&'a str>,
}
impl Walker<'_> {
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
        let line = n.start_position().row + 1;
        let signature = signature.unwrap_or_else(|| {
            self.lines
                .get(line - 1)
                .copied()
                .unwrap_or("")
                .trim()
                .into()
        });
        self.facts.declarations.push(Declaration {
            name,
            category: category.into(),
            scope: if scope { self.scope() } else { String::new() },
            line,
            end_line: n.end_position().row + 1,
            signature,
        });
    }
    fn link(&mut self, n: Node<'_>, from: String, to: String, category: &str, confidence: &str) {
        self.facts.relationships.push(Relationship {
            from,
            to,
            category: category.into(),
            confidence: confidence.into(),
            line: n.start_position().row + 1,
        });
    }
    fn resolve_alias(&self, target: &str) -> Option<String> {
        let split = target.find(['.', ':']).unwrap_or(target.len());
        let base = &target[..split];
        for scope in self.aliases.iter().rev() {
            if let Some(v) = scope.get(base) {
                return v.as_ref().map(|v| format!("{v}{}", &target[split..]));
            }
        }
        Some(target.into())
    }
    fn function(&mut self, n: Node<'_>, declared: Option<(String, &str, Node<'_>)>) {
        let scope = if let Some((name, category, source)) = declared {
            self.declare(source, name.clone(), category, true, None);
            name
        } else {
            format!(
                "{}/<anonymous:{}>",
                self.scope(),
                n.start_position().row + 1
            )
        };
        self.scopes.push(scope);
        self.aliases.push(BTreeMap::new());
        if let Some(params) = n.child_by_field_name("parameters") {
            for p in children(params)
                .into_iter()
                .filter(|p| p.kind() == "identifier")
            {
                self.aliases
                    .last_mut()
                    .unwrap()
                    .insert(text(self.raw, p).into(), None);
            }
        }
        if let Some(body) = n.child_by_field_name("body") {
            self.walk(body);
        }
        self.aliases.pop();
        self.scopes.pop();
    }
    fn walk(&mut self, n: Node<'_>) {
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
    fn walk_inner(&mut self, n: Node<'_>) {
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
                    let target = n
                        .child_by_field_name("name")
                        .map(|n| name(self.raw, n))
                        .unwrap_or_default();
                    let local = text(self.raw, n).trim_start().starts_with("local ");
                    if local {
                        self.aliases
                            .last_mut()
                            .unwrap()
                            .insert(target.clone(), Some(target.clone()));
                    }
                    self.function(
                        n,
                        Some((target, if local { "local-function" } else { "function" }, n)),
                    );
                    return;
                }
            }
            "function_definition" => {
                if !n.has_error() {
                    self.function(n, None);
                    return;
                }
            }
            "variable_declaration" => {
                let nodes = children(n);
                if let Some(a) = nodes.iter().find(|a| a.kind() == "assignment_statement") {
                    self.assignment(*a, true);
                    return;
                }
                for v in nodes
                    .into_iter()
                    .flat_map(children)
                    .filter(|n| n.kind() == "identifier")
                {
                    self.aliases
                        .last_mut()
                        .unwrap()
                        .insert(text(self.raw, v).into(), None);
                }
                return;
            }
            "assignment_statement" => {
                self.assignment(n, false);
                return;
            }
            "function_call" if !n.has_error() => {
                if let Some(f) = n.child_by_field_name("name") {
                    let target = name(self.raw, f);
                    let confidence = if target.is_empty() {
                        "dynamic-unresolved"
                    } else {
                        "inferred"
                    };
                    self.link(
                        n,
                        self.scope(),
                        if target.is_empty() {
                            "<dynamic>".into()
                        } else {
                            target.clone()
                        },
                        "call",
                        confidence,
                    );
                    if let Some(resolved) = self.resolve_alias(&target)
                        && resolved != target
                    {
                        self.link(n, self.scope(), resolved, "alias-call", "static-resolved");
                    }
                    if target.ends_with(":RegisterEvent") || target.ends_with(":RegisterUnitEvent")
                    {
                        let value = n
                            .child_by_field_name("arguments")
                            .and_then(|a| children(a).into_iter().next())
                            .and_then(|v| string(self.raw, v));
                        let confidence = if value.is_some() {
                            "exact"
                        } else {
                            "dynamic-unresolved"
                        };
                        self.link(
                            n,
                            self.scope(),
                            value.unwrap_or_else(|| "<dynamic>".into()),
                            "event-registration",
                            confidence,
                        );
                    }
                }
            }
            "table_constructor"
                if self.generated && table_text(self.raw, n, "Type") == "System" =>
            {
                self.documentation(n, "")
            }
            "block" => {
                self.aliases.push(BTreeMap::new());
                for c in children(n) {
                    self.walk(c);
                }
                self.aliases.pop();
                return;
            }
            "goto_statement" | "label_statement" | "attribute" => {
                self.facts.diagnostics.push(SyntaxNote {
                    message: "Syntax unavailable in WoW Lua 5.1".into(),
                    line: n.start_position().row + 1,
                })
            }
            _ => {}
        }
        for c in children(n) {
            self.walk(c);
        }
    }
    fn assignment(&mut self, n: Node<'_>, local: bool) {
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
        let mapped: Vec<_> = values
            .iter()
            .map(|v| self.resolve_alias(&name(self.raw, *v)))
            .collect();
        for (i, var) in vars.iter().enumerate() {
            let target = name(self.raw, *var);
            if local {
                self.aliases.last_mut().unwrap().insert(
                    target.clone(),
                    mapped.get(i).cloned().flatten().filter(|s| !s.is_empty()),
                );
            } else {
                for scope in self.aliases.iter_mut().rev() {
                    if scope.contains_key(&target) {
                        scope.insert(target.clone(), None);
                        break;
                    }
                }
            }
            if let Some(value) = values.get(i) {
                if value.kind() == "function_definition" && !value.has_error() {
                    if local {
                        self.aliases
                            .last_mut()
                            .unwrap()
                            .insert(target.clone(), Some(target.clone()));
                    }
                    self.function(
                        *value,
                        Some((target, if local { "local-function" } else { "function" }, n)),
                    );
                } else {
                    if value.kind() == "function_call"
                        && value
                            .child_by_field_name("name")
                            .is_some_and(|f| name(self.raw, f) == "CreateFromMixins")
                        && !target.is_empty()
                    {
                        self.declare(n, target, "mixin", true, None);
                    }
                    self.walk(*value);
                }
            }
        }
        for value in values.iter().skip(vars.len()) {
            self.walk(*value);
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
        let masked = lua51_grammar_input(raw);
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
    fn exact_ranges_scope_alias_and_reassignment() {
        let d=SyntaxDocument::new(b"local api = C_Test\nlocal function run(api) api.Do() end\nlocal function ok() api.Do() end\napi = Other\napi.Do()\n").unwrap();
        let f = d.facts(false);
        assert_eq!(f.declarations[0].line, 2);
        assert_eq!(f.declarations[0].end_line, 2);
        assert!(
            !f.relationships
                .iter()
                .any(|r| r.from == "run" && r.to == "C_Test.Do")
        );
        assert!(
            f.relationships
                .iter()
                .any(|r| r.from == "ok" && r.to == "C_Test.Do" && r.category == "alias-call")
        );
        assert!(
            !f.relationships
                .iter()
                .any(|r| r.line == 5 && r.category == "alias-call")
        );
    }
    #[test]
    fn incremental_reparse_and_recovery() {
        let old = b"function Good() return 1 end\nfunction Broken(\n";
        let mut d = SyntaxDocument::new(old).unwrap();
        let f = d.facts(false);
        assert!(!f.diagnostics.is_empty());
        assert!(f.declarations.iter().any(|d| d.name == "Good"));
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
        assert!(
            d.facts(false)
                .declarations
                .iter()
                .any(|d| d.name == "Better")
        );
    }
}
