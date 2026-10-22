//! Matrix-only syntactic usages, matching fixed Go 3.2.3 usage.go. This is
//! neither a second public Facts model nor binding/alias analysis.
use lycheedev_core::{Result, control::OperationControl};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Clone, Debug)]
pub(super) struct Usage {
    pub kind: String,
    pub name: String,
    pub expression: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    // Go usages retain byte strings until JSON output. Indexed symbols have
    // already crossed JSON's UTF-8 boundary; invalid bytes cannot match them.
    pub invalid_name_bytes: Option<Vec<u8>>,
}
impl Usage {
    pub fn new(kind: &str, name: impl Into<String>, file: &str, line: usize) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
            expression: String::new(),
            file: file.into(),
            line,
            column: 0,
            invalid_name_bytes: None,
        }
    }
    fn from_bytes(kind: &str, name: &[u8], file: &str, line: usize) -> Self {
        let mut usage = Self::new(kind, crate::lua_tree::lua_json_string(name), file, line);
        if std::str::from_utf8(name).is_err() {
            usage.invalid_name_bytes = Some(name.to_vec());
        }
        usage
    }
}
fn unresolved(kind: &str, expression: String, file: &str, line: usize, reason: &str) -> Value {
    json!({"kind":kind,"expression":if expression.is_empty(){"<dynamic>".into()}else{expression},"file":file,"line":line,"column":0,"reason":reason,"evidence":{"confidence":"dynamic-unresolved"}})
}
pub(super) fn dedupe(usages: Vec<Usage>) -> Vec<Usage> {
    let mut seen = BTreeSet::new();
    usages
        .into_iter()
        .filter(|usage| {
            seen.insert((
                usage.kind.clone(),
                usage.name.clone(),
                usage.file.clone(),
                usage.line,
                usage.invalid_name_bytes.clone(),
            ))
        })
        .collect()
}
fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect()
}
fn raw_text<'a>(raw: &'a [u8], node: Node<'_>) -> &'a str {
    std::str::from_utf8(&raw[node.byte_range()]).unwrap_or("")
}

// gopher-lua's AttrGet label accepts both identifier and string keys. Public
// source Facts intentionally have their own access-name contract.
fn label_bytes(raw: &[u8], node: Node<'_>) -> Vec<u8> {
    enum Part<'a> {
        Node(Node<'a>),
        Dot,
    }
    let mut pending = vec![Part::Node(node)];
    let mut result = Vec::new();
    while let Some(part) = pending.pop() {
        let node = match part {
            Part::Dot => {
                result.push(b'.');
                continue;
            }
            Part::Node(node) => node,
        };
        match node.kind() {
            "identifier" => result.extend_from_slice(&raw[node.byte_range()]),
            "string" => {
                let Some(value) = crate::lua_tree::string_bytes(raw, node) else {
                    return Vec::new();
                };
                if value.is_empty() {
                    return Vec::new();
                }
                result.extend_from_slice(&value)
            }
            "parenthesized_expression" => {
                let Some(child) = children(node).into_iter().next() else {
                    return Vec::new();
                };
                pending.push(Part::Node(child));
            }
            "dot_index_expression" | "bracket_index_expression" => {
                let (Some(table), Some(field)) = (
                    node.child_by_field_name("table"),
                    node.child_by_field_name("field"),
                ) else {
                    return Vec::new();
                };
                pending.push(Part::Node(field));
                pending.push(Part::Dot);
                pending.push(Part::Node(table));
            }
            _ => return Vec::new(),
        }
    }
    result
}
fn label(raw: &[u8], node: Node<'_>) -> String {
    crate::lua_tree::lua_json_string(&label_bytes(raw, node))
}
fn call_name(raw: &[u8], node: Node<'_>) -> Vec<u8> {
    if node.kind() == "method_index_expression" {
        if let (Some(receiver), Some(method)) = (
            node.child_by_field_name("table"),
            node.child_by_field_name("method"),
        ) {
            let mut base = label_bytes(raw, receiver);
            if !base.is_empty() {
                base.push(b':');
                base.extend_from_slice(raw_text(raw, method).as_bytes());
                return base;
            }
        }
        Vec::new()
    } else {
        label_bytes(raw, node)
    }
}
fn csv(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
fn first_char(raw: &[u8]) -> Option<char> {
    let width = match *raw.first()? {
        0..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    std::str::from_utf8(raw.get(..width)?).ok()?.chars().next()
}
fn trim_bytes(mut raw: &[u8]) -> &[u8] {
    while let Some(ch) = first_char(raw).filter(|ch| ch.is_whitespace()) {
        raw = &raw[ch.len_utf8()..];
    }
    loop {
        let Some(mut at) = raw.len().checked_sub(1) else {
            break;
        };
        while at > 0 && raw[at] & 0xc0 == 0x80 {
            at -= 1;
        }
        let Some(ch) = first_char(&raw[at..])
            .filter(|ch| at + ch.len_utf8() == raw.len() && ch.is_whitespace())
        else {
            break;
        };
        raw = &raw[..raw.len() - ch.len_utf8()];
    }
    raw
}
fn csv_bytes(value: &[u8]) -> impl Iterator<Item = &[u8]> {
    value
        .split(|byte| *byte == b',')
        .map(trim_bytes)
        .filter(|value| !value.is_empty())
}

pub(super) fn lua(
    file: &str,
    raw: &[u8],
    control: &OperationControl,
) -> Result<(Vec<Usage>, Vec<Value>)> {
    control.check()?;
    // Unlike addoncheck's public scanner, fixed Go usage.go only strips BOM.
    // A shebang therefore fails its separate Lua parser and yields no usages.
    if raw
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(raw)
        .starts_with(b"#!")
    {
        return Ok((vec![], vec![]));
    }
    let document = crate::lua_tree::SyntaxDocument::new_public(raw)?;
    if document.root().has_error() || document.public_parse_note().is_some() {
        return Ok((vec![], vec![]));
    }
    let mut usages = vec![];
    let mut undecided = vec![];
    let lines = crate::lua_tree::lua_line_starts(raw);
    let mut pending = vec![document.root()];
    while let Some(node) = pending.pop() {
        control.check()?;
        match node.kind() {
            "comment" | "string" => continue,
            "function_declaration" | "function_definition" => {
                if let Some(body) = node.child_by_field_name("body") {
                    pending.push(body);
                }
                continue;
            }
            "assignment_statement" => {
                for child in children(node)
                    .into_iter()
                    .rev()
                    .filter(|child| child.kind() == "expression_list")
                {
                    pending.push(child);
                }
                continue;
            }
            "function_call" => {
                let target_bytes = node
                    .child_by_field_name("name")
                    .map(|target| call_name(raw, target))
                    .unwrap_or_default();
                let target = String::from_utf8_lossy(&target_bytes);
                let line = lines
                    .partition_point(|start| *start <= crate::lua_tree::expression_start(node));
                if target.is_empty() {
                    undecided.push(unresolved(
                        "api",
                        "<dynamic call>".into(),
                        file,
                        line,
                        "call target is computed",
                    ));
                } else if target.starts_with("C_") && target.contains('.') {
                    usages.push(Usage::from_bytes("api", &target_bytes, file, line));
                } else if !target.contains(':') {
                    usages.push(Usage::from_bytes(
                        "api-candidate",
                        &target_bytes,
                        file,
                        line,
                    ));
                }
                let args = node
                    .child_by_field_name("arguments")
                    .map(children)
                    .unwrap_or_default();
                if target.ends_with(":RegisterEvent") || target.ends_with(":RegisterUnitEvent") {
                    if let Some(&arg) = args.first() {
                        if let Some(value) = crate::lua_tree::string_bytes(raw, arg) {
                            usages.push(Usage::from_bytes("event", &value, file, line));
                        } else {
                            undecided.push(unresolved(
                                "event",
                                label(raw, arg),
                                file,
                                line,
                                "event name is computed",
                            ));
                        }
                    }
                } else if target == "CreateFrame" {
                    for (position, kind) in [(0, "frame-type"), (3, "template")] {
                        if let Some(&arg) = args.get(position) {
                            if let Some(value) = crate::lua_tree::string_bytes(raw, arg) {
                                if kind == "template" {
                                    for value in csv_bytes(&value) {
                                        usages.push(Usage::from_bytes(kind, value, file, line));
                                    }
                                } else {
                                    usages.push(Usage::from_bytes(kind, &value, file, line));
                                }
                            } else {
                                undecided.push(unresolved(
                                    kind,
                                    label(raw, arg),
                                    file,
                                    line,
                                    &format!("{kind} is computed"),
                                ));
                            }
                        }
                    }
                } else if target == "CreateFromMixins" {
                    for &arg in &args {
                        let value = label_bytes(raw, arg);
                        if value.is_empty() {
                            undecided.push(unresolved(
                                "mixin",
                                "<dynamic>".into(),
                                file,
                                line,
                                "Mixin is computed",
                            ));
                        } else {
                            usages.push(Usage::from_bytes("mixin", &value, file, line));
                        }
                    }
                }
            }
            _ => {}
        }
        // Preorder expression traversal, with assignment LHS and function
        // declaration names excluded exactly as usage.go's statement walker.
        pending.extend(children(node).into_iter().rev());
    }
    Ok((usages, undecided))
}

pub(super) fn xml(
    file: &str,
    raw: &[u8],
    control: &OperationControl,
) -> Result<(Vec<Usage>, Vec<Value>)> {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_reader(raw);
    reader.config_mut().check_comments = true;
    let mut usages = vec![];
    let mut undecided = vec![];
    let prefix = if raw.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let mut cursor = 0usize;
    let mut line = 1usize;
    loop {
        control.check()?;
        let at =
            crate::xml_inline::source_map::raw_cursor(raw.len(), prefix, reader.buffer_position())
                .expect("XML slice cursor")
                .min(raw.len());
        line += raw[cursor..at]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        cursor = at;
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                let mut inherits = Vec::new();
                for attribute in element.attributes().with_checks(false) {
                    let Ok(attribute) = attribute else {
                        return Ok((usages, undecided));
                    };
                    let Ok(value) = crate::facts::xml_attribute(&attribute, reader.decoder())
                    else {
                        return Ok((usages, undecided));
                    };
                    if attribute
                        .key
                        .local_name()
                        .as_ref()
                        .eq_ignore_ascii_case(b"inherits")
                    {
                        inherits.push(value);
                    }
                }
                // Go's decoder validates every attribute before returning the
                // start token. A malformed non-inherits attribute also stops.
                if !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "ui" | "script"
                        | "include"
                        | "scripts"
                        | "layers"
                        | "frames"
                        | "anchors"
                        | "size"
                ) {
                    usages.push(Usage::new("frame-type", name, file, line));
                }
                for value in inherits {
                    for name in csv(&value) {
                        if name.contains(['$', '%', '+']) {
                            undecided.push(unresolved(
                                "template",
                                name.into(),
                                file,
                                line,
                                "XML inheritance is dynamic",
                            ));
                        } else {
                            usages.push(Usage::new("template", name, file, line));
                        }
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                let Ok(name) = reference.decode() else { break };
                if crate::facts::xml_entity(&name).is_err() {
                    break;
                }
            }
            Ok(Event::Text(text)) => {
                let Ok(text) = text.xml10_content() else {
                    break;
                };
                if crate::facts::xml_characters(&text).is_err() || text.contains("]]>") {
                    break;
                }
            }
            Ok(Event::CData(text)) => {
                let Ok(text) = text.xml10_content() else {
                    break;
                };
                if crate::facts::xml_characters(&text).is_err() {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    Ok((usages, undecided))
}
