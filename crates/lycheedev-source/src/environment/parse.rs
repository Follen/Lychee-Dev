use super::{ApiRecord, DeclarationOwner, Gap, Slot};
use crate::lua_tree::SyntaxDocument;
use lycheedev_core::{Result, control::OperationControl};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;
fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor)
        .filter(|c| !matches!(c.kind(), "comment" | "hash_bang_line"))
        .collect()
}
fn text<'a>(raw: &'a [u8], n: Node<'_>) -> &'a str {
    std::str::from_utf8(&raw[n.byte_range()]).unwrap_or("")
}
fn unparen(mut n: Node<'_>) -> Node<'_> {
    while n.kind() == "parenthesized_expression" {
        if let Some(c) = children(n).first() {
            n = *c;
        } else {
            break;
        }
    }
    n
}
fn gap(path: &str, n: Node<'_>, code: &str, detail: &str) -> Gap {
    Gap {
        path: path.into(),
        line: n.start_position().row + 1,
        code: code.into(),
        detail: detail.into(),
    }
}
fn call_argument<'a>(raw: &[u8], n: Node<'a>) -> Option<Node<'a>> {
    if n.kind() != "function_call" {
        return None;
    }
    let name = unparen(n.child_by_field_name("name")?);
    if name.kind() != "method_index_expression" {
        return None;
    }
    let receiver = unparen(name.child_by_field_name("table")?);
    if receiver.kind() != "identifier"
        || text(raw, receiver) != "APIDocumentation"
        || text(raw, name.child_by_field_name("method")?) != "AddDocumentationTable"
    {
        return None;
    }
    let args = children(n.child_by_field_name("arguments")?);
    (args.len() == 1).then(|| unparen(args[0]))
}
pub(super) fn document(
    path: &str,
    raw: &[u8],
    control: &OperationControl,
) -> Result<(Vec<ApiRecord>, Vec<Gap>)> {
    let input = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    let doc = match SyntaxDocument::new(input) {
        Ok(doc) => doc,
        Err(e) => {
            return Ok((
                vec![],
                vec![Gap {
                    path: path.into(),
                    line: 0,
                    code: "parse".into(),
                    detail: e.message,
                }],
            ));
        }
    };
    let root = doc.root();
    if root.has_error() {
        return Ok((
            vec![],
            vec![Gap {
                path: path.into(),
                line: 0,
                code: "parse".into(),
                detail: "Lua structural parser reported malformed input".into(),
            }],
        ));
    }
    let nodes = children(root);
    let mut bindings = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for n in &nodes {
        control.check()?;
        match n.kind() {
            "variable_declaration" => {
                let assignment = children(*n)
                    .into_iter()
                    .find(|n| n.kind() == "assignment_statement");
                if let Some(assignment) = assignment {
                    let lists = children(assignment);
                    let lhs = lists
                        .iter()
                        .find(|n| n.kind() == "variable_list")
                        .map(|n| children(*n))
                        .unwrap_or_default();
                    let rhs = lists
                        .iter()
                        .find(|n| n.kind() == "expression_list")
                        .map(|n| children(*n))
                        .unwrap_or_default();
                    for (i, name) in lhs.iter().enumerate() {
                        let name = text(input, *name).to_string();
                        if bindings.contains_key(&name) {
                            ambiguous.insert(name.clone());
                        }
                        if let Some(table) = rhs
                            .get(i)
                            .map(|n| unparen(*n))
                            .filter(|n| n.kind() == "table_constructor")
                        {
                            bindings.insert(name, table);
                        } else {
                            ambiguous.insert(name);
                        }
                    }
                    for rhs in rhs {
                        let rhs = unparen(rhs);
                        match rhs.kind() {
                            "table_constructor" => {}
                            "identifier" => {
                                let name = text(input, rhs);
                                if bindings.contains_key(name) {
                                    ambiguous.insert(name.into());
                                }
                            }
                            _ => ambiguous.extend(bindings.keys().cloned()),
                        }
                    }
                } else {
                    for n in children(*n)
                        .into_iter()
                        .flat_map(children)
                        .filter(|n| n.kind() == "identifier")
                    {
                        ambiguous.insert(text(input, n).into());
                    }
                }
            }
            "function_call" if call_argument(input, *n).is_some() => {}
            "empty_statement" => {}
            _ => ambiguous.extend(bindings.keys().cloned()),
        }
    }
    let mut records = vec![];
    let mut gaps = vec![];
    let content_hash = lycheedev_core::storage::sha256(raw);
    for n in nodes {
        control.check()?;
        let Some(arg) = call_argument(input, n) else {
            continue;
        };
        let table = if arg.kind() == "table_constructor" {
            Some(arg)
        } else if arg.kind() == "identifier" && !ambiguous.contains(text(input, arg)) {
            bindings.get(text(input, arg)).copied()
        } else {
            None
        };
        if let Some(table) = table {
            read_doc(
                path,
                input,
                table,
                (raw.len() - input.len(), &content_hash),
                &mut records,
                &mut gaps,
                control,
            )?;
        } else {
            gaps.push(gap(
                path,
                n,
                "dynamic_metadata",
                "AddDocumentationTable argument is not an unmodified local literal",
            ));
        }
    }
    Ok((records, gaps))
}
fn fields(n: Node<'_>) -> Vec<Node<'_>> {
    children(n)
        .into_iter()
        .filter(|n| n.kind() == "field")
        .collect()
}
fn key(raw: &[u8], field: Node<'_>) -> Option<String> {
    let n = field.child_by_field_name("name")?;
    if n.kind() == "identifier" {
        Some(text(raw, n).into())
    } else {
        string(raw, n)
    }
}
fn field<'a>(raw: &[u8], n: Node<'a>, name: &str) -> Option<Node<'a>> {
    fields(n)
        .into_iter()
        .find(|n| key(raw, *n).as_deref() == Some(name))
        .and_then(|n| n.child_by_field_name("value"))
        .map(unparen)
}
fn field_string(raw: &[u8], n: Node<'_>, name: &str) -> String {
    field(raw, n, name)
        .and_then(|n| string(raw, n))
        .unwrap_or_default()
}
fn string(raw: &[u8], n: Node<'_>) -> Option<String> {
    let n = unparen(n);
    if n.kind() != "string" {
        return None;
    }
    let raw = &raw[n.byte_range()];
    if raw.first() == Some(&b'[') {
        let second = raw.iter().skip(1).position(|b| *b == b'[')? + 1;
        let mut content = raw.get(second + 1..raw.len().checked_sub(second + 1)?)?;
        if content.starts_with(b"\r\n") || content.starts_with(b"\n\r") {
            content = &content[2..];
        } else if content.first().is_some_and(|b| b"\r\n".contains(b)) {
            content = &content[1..];
        }
        return Some(
            String::from_utf8_lossy(content)
                .replace("\r\n", "\n")
                .replace("\n\r", "\n")
                .replace('\r', "\n"),
        );
    }
    let content = raw.get(1..raw.len().checked_sub(1)?)?;
    let mut out = vec![];
    let mut i = 0;
    while i < content.len() {
        let b = content[i];
        i += 1;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let b = *content.get(i)?;
        i += 1;
        let value = match b {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => 10,
            b'r' => 13,
            b't' => 9,
            b'v' => 11,
            b'\r' | b'\n' => {
                if content
                    .get(i)
                    .is_some_and(|next| *next != b && b"\r\n".contains(next))
                {
                    i += 1;
                }
                10
            }
            b'0'..=b'9' => {
                let mut value = u16::from(b - b'0');
                for _ in 0..2 {
                    if let Some(d) = content.get(i).filter(|b| b.is_ascii_digit()) {
                        value = value * 10 + u16::from(*d - b'0');
                        i += 1;
                    } else {
                        break;
                    }
                }
                u8::try_from(value).ok()?
            }
            _ => b,
        };
        out.push(value);
    }
    Some(String::from_utf8_lossy(&out).into())
}
fn literal(raw: &[u8], n: Node<'_>, depth: usize) -> Option<Value> {
    let n = unparen(n);
    if depth > 16 {
        return None;
    }
    match n.kind() {
        "string" => string(raw, n).map(Value::String),
        "number" => {
            let token = text(raw, n);
            token.parse::<f64>().ok().filter(|v| v.is_finite())?;
            serde_json::from_str(token).ok()
        }
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "nil" => Some(Value::Null),
        "table_constructor" => {
            let fields = fields(n);
            if fields.len() > 4096 {
                return None;
            }
            let mut map = Map::new();
            let mut array = vec![];
            for f in fields {
                let value = literal(raw, f.child_by_field_name("value")?, depth + 1)?;
                if f.child_by_field_name("name").is_some() {
                    let key = key(raw, f)?;
                    if !array.is_empty() || map.contains_key(&key) {
                        return None;
                    }
                    map.insert(key, value);
                } else {
                    if !map.is_empty() {
                        return None;
                    }
                    array.push(value);
                }
            }
            Some(if array.is_empty() {
                Value::Object(map)
            } else {
                Value::Array(array)
            })
        }
        _ => None,
    }
}
fn raw_record(
    path: &str,
    raw: &[u8],
    n: Node<'_>,
    exclude: &[&str],
    gaps: &mut Vec<Gap>,
) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for f in fields(n) {
        let Some(value) = f.child_by_field_name("value") else {
            continue;
        };
        let Some(key) = key(raw, f) else {
            gaps.push(gap(path, value, "non_named_metadata_field", ""));
            continue;
        };
        if exclude.contains(&key.as_str()) {
            continue;
        }
        if out.contains_key(&key) {
            gaps.push(gap(path, value, "duplicate_metadata_field", &key));
            continue;
        }
        if let Some(v) = literal(raw, value, 0) {
            out.insert(key, v);
        } else {
            gaps.push(gap(path, value, "dynamic_metadata_field", &key));
        }
    }
    out
}
fn slots(path: &str, raw: &[u8], n: Node<'_>, name: &str, gaps: &mut Vec<Gap>) -> Vec<Slot> {
    let Some(list) = field(raw, n, name) else {
        return vec![];
    };
    if list.kind() != "table_constructor" {
        gaps.push(gap(path, list, "dynamic_slots", name));
        return vec![];
    }
    let mut out = vec![];
    for (i, f) in fields(list).into_iter().enumerate() {
        let Some(value) = f.child_by_field_name("value").map(unparen) else {
            continue;
        };
        if f.child_by_field_name("name").is_some() {
            gaps.push(gap(path, value, "non_array_slots", name));
            continue;
        }
        if value.kind() != "table_constructor" {
            gaps.push(gap(path, value, "dynamic_slot", name));
            continue;
        }
        let r = raw_record(path, raw, value, &[], gaps);
        let s = |name: &str| {
            r.get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        out.push(Slot {
            index: i + 1,
            name: s("Name"),
            type_name: s("Type"),
            inner_type: s("InnerType"),
            nilable: r.get("Nilable") == Some(&Value::Bool(true)),
            line: value.start_position().row + 1,
            raw: r,
        });
    }
    out
}
fn read_doc(
    path: &str,
    raw: &[u8],
    root: Node<'_>,
    extent: (usize, &str),
    out: &mut Vec<ApiRecord>,
    gaps: &mut Vec<Gap>,
    control: &OperationControl,
) -> Result<()> {
    let (byte_offset, content_hash) = extent;
    let group = field_string(raw, root, "Name");
    let script_object = field_string(raw, root, "Type") == "ScriptObject";
    let owner = if script_object {
        let source = serde_json::json!({"path":path,"record":group,"kind":"script-object","firstLine":root.start_position().row+1,"lastLine":root.end_position().row+1,"startByte":root.start_byte()+byte_offset,"endByte":root.end_byte()+byte_offset,"contentHash":content_hash,"coordinate":"utf8-byte","rules":super::DECLARATION_RULES});
        if group.is_empty() { gaps.push(gap(path,root,"missing_script_object_name","ScriptObject")); }
        else { out.push(ApiRecord {
            observed_span: source.clone(), kind: "script-object".into(), name: group.clone(),
            owner: None, declaration_role: "script-object-root".into(),
            display_group: group.clone(), namespace: String::new(), path: path.into(),
            line: root.start_position().row+1, end_line: root.start_position().row+1,
            parameters: vec![], returns: vec![], payload: vec![], fields: vec![], values: vec![],
            raw: raw_record(path,raw,root,&["Functions","Events","Tables","Predicates","Constants"],gaps),
        }); }
        Some(DeclarationOwner { kind: "script-object".into(), name: group.clone(), source })
    } else {
        None
    };
    let mut namespace = field_string(raw, root, "Namespace");
    if namespace.is_empty() && group.starts_with("C_") {
        namespace = group.clone();
    }
    for section in ["Functions", "Events", "Tables", "Predicates", "Constants"] {
        let Some(list) = field(raw, root, section) else {
            continue;
        };
        if list.kind() != "table_constructor" {
            gaps.push(gap(path, list, "dynamic_section", section));
            continue;
        }
        for f in fields(list) {
            control.check()?;
            if out.len() >= 100000 || gaps.len() > 10000 {
                return Err(super::budget());
            }
            let Some(n) = f.child_by_field_name("value").map(unparen) else {
                continue;
            };
            if f.child_by_field_name("name").is_some() {
                gaps.push(gap(path, n, "non_array_section", section));
                continue;
            }
            if n.kind() != "table_constructor" {
                gaps.push(gap(path, n, "dynamic_record", section));
                continue;
            }
            let mut name = field_string(raw, n, "Name");
            if name.is_empty() {
                gaps.push(gap(path, n, "missing_record_name", section));
                continue;
            }
            let mut kind = field_string(raw, n, "Type").to_ascii_lowercase();
            if kind.is_empty() {
                kind = section.trim_end_matches('s').to_ascii_lowercase();
            }
            if kind == "enum" {
                kind = "enumeration".into();
            }
            if kind == "scriptobject" {
                kind = "script-object".into();
            }
            if kind == "event" {
                let literal = field_string(raw, n, "LiteralName");
                if !literal.is_empty() {
                    name = literal;
                }
            }
            if kind == "function" && !namespace.is_empty() {
                name = format!("{namespace}.{name}");
            }
            let r = raw_record(
                path,
                raw,
                n,
                &["Arguments", "Returns", "Payload", "Fields", "Values"],
                gaps,
            );
            let parameters = slots(path, raw, n, "Arguments", gaps);
            let returns = slots(path, raw, n, "Returns", gaps);
            let payload = slots(path, raw, n, "Payload", gaps);
            let mut record_fields = slots(path, raw, n, "Fields", gaps);
            let mut values = slots(path, raw, n, "Values", gaps);
            if kind == "enumeration" && values.is_empty() {
                values = std::mem::take(&mut record_fields);
            }
            out.push(ApiRecord {
                observed_span:serde_json::json!({"path":path,"record":name,"kind":kind,"firstLine":n.start_position().row+1,"lastLine":n.end_position().row+1,"startByte":n.start_byte()+byte_offset,"endByte":n.end_byte()+byte_offset,"contentHash":content_hash,"coordinate":"utf8-byte","legacyEndLineAccuracy":"line"}),
                kind,
                name,
                owner: if section == "Functions" { owner.clone() } else { None },
                declaration_role: String::new(),
                display_group: group.clone(),
                namespace: namespace.clone(),
                path: path.into(),
                line: n.start_position().row + 1,
                // Frozen gopher AST table.LastLine is unset; preserve its public
                // line-only value and expose the precise TS extent separately.
                end_line: n.start_position().row + 1,
                parameters,
                returns,
                payload,
                fields: record_fields,
                values,
                raw: r,
            });
        }
    }
    Ok(())
}
