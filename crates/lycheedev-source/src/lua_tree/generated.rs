//! Literal generated metadata, after the complete ordinary Lua observation pass.
//! Views borrow the original tree. No metadata table is evaluated or mirrored.
use super::*;
use tree_sitter::TreeCursor;

pub(super) const DOC_KEYS: [&str; 14] = [
    "Type",
    "Name",
    "Namespace",
    "LiteralName",
    "Functions",
    "Events",
    "Tables",
    "Structures",
    "Enumerations",
    "ScriptObjects",
    "Callbacks",
    "Arguments",
    "Returns",
    "Payload",
];
const PARAM_KEYS: [&str; 3] = ["Name", "Type", "Nilable"];

/// One direct-child scan replaces a separate child Vec and field scan for each
/// requested key. A matched invalid/nonliteral first value still masks later
/// duplicates. In particular this is not XML's last-value-wins attribute rule.
pub(super) struct LiteralFields<'tree, const N: usize> {
    pub(super) values: [Option<Node<'tree>>; N],
    #[cfg(test)]
    pub(super) visited: usize,
}
impl<'tree, const N: usize> LiteralFields<'tree, N> {
    pub(super) fn read(
        raw: &[u8],
        table: Node<'tree>,
        keys: [&str; N],
        guard: Option<&WorkerGuard<'_>>,
    ) -> Option<Self> {
        let mut fields = Self {
            values: [None; N],
            #[cfg(test)]
            visited: 0,
        };
        let mut seen = [false; N];
        let mut cursor = table.walk();
        for field in table.named_children(&mut cursor) {
            if !worker::check(guard) {
                return None;
            }
            #[cfg(test)]
            {
                fields.visited += 1;
            }
            if field.kind() != "field" {
                continue;
            }
            let Some(name) = field.child_by_field_name("name") else {
                continue;
            };
            // Keep the old raw-name comparison before literal decoding. Decode
            // a quoted/parenthesized key once, rather than once per sought key.
            let key = keys
                .iter()
                .position(|key| text(raw, name) == *key)
                .or_else(|| {
                    literal(raw, name).and_then(|name| keys.iter().position(|key| name == *key))
                });
            if let Some(key) = key.filter(|key| !seen[*key]) {
                seen[key] = true;
                fields.values[key] = field.child_by_field_name("value").map(unparenthesized);
            }
        }
        worker::check(guard).then_some(fields)
    }
    fn text(&self, raw: &[u8], key: usize) -> String {
        self.values[key]
            .and_then(|value| literal(raw, value))
            .unwrap_or_default()
    }
}

// Generated temporaries use explicit capacities: their reservation proof does
// not depend on std's unspecified amortized-growth policy. Decoded bytes never
// outgrow the original literal; per-byte JSON replacement needs at most 3x.
fn literal(raw: &[u8], node: Node<'_>) -> Option<String> {
    let node = unparenthesized(node);
    if node.kind() != "string" {
        return None;
    }
    let value = &raw[node.byte_range()];
    let decoded = string_value_bytes_into(value, Vec::with_capacity(value.len()))?;
    let mut output = String::with_capacity(decoded.len().checked_mul(3)?);
    let mut rest = decoded.as_slice();
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(value) => {
                output.push_str(value);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                output.push_str(std::str::from_utf8(&rest[..valid]).expect("validated prefix"));
                output.push('\u{fffd}');
                rest = &rest[valid + 1..];
            }
        }
    }
    Some(output)
}
fn copy_text(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    result.push_str(value);
    result
}
struct SignatureBuffer {
    value: String,
    requested: usize,
}
impl SignatureBuffer {
    fn append(&mut self, value: &str, guard: Option<&WorkerGuard<'_>>) -> Option<()> {
        if !worker::check(guard) {
            return None;
        }
        let Some(required) = self.value.len().checked_add(value.len()) else {
            if let Some(guard) = guard {
                guard.abandon();
            }
            return None;
        };
        if required > self.requested {
            // Own the requested doubling bound explicitly, then ask the Vec for
            // exactly that capacity. Allocator excess is not a requested quota.
            let capacity = self
                .requested
                .checked_mul(2)
                .unwrap_or(required)
                .max(required);
            if capacity > self.value.capacity() {
                self.value.reserve_exact(capacity - self.value.len());
            }
            self.requested = capacity;
        }
        self.value.push_str(value);
        Some(())
    }
}

/// Parentheses are transparent to metadata semantics, but their outer opening
/// byte is the declaration/link origin. The actual table remains the end span;
/// a closing parenthesis on a later line must not extend that table's endLine.
#[derive(Clone, Copy)]
struct DocumentationNode<'tree> {
    table: Node<'tree>,
    origin_start: usize,
}
impl<'tree> DocumentationNode<'tree> {
    fn from_expression(mut node: Node<'tree>, guard: Option<&WorkerGuard<'_>>) -> Option<Self> {
        let origin_start = node.start_byte();
        while node.kind() == "parenthesized_expression" {
            if !worker::check(guard) {
                return None;
            }
            let mut cursor = node.walk();
            let mut inner = None;
            for child in node.named_children(&mut cursor) {
                if !worker::check(guard) {
                    return None;
                }
                if child.kind() != "comment" {
                    inner = Some(child);
                    break;
                }
            }
            node = inner?;
        }
        (node.kind() == "table_constructor").then_some(Self {
            table: node,
            origin_start,
        })
    }
    fn from_root(mut node: Node<'tree>, guard: Option<&WorkerGuard<'_>>) -> Option<Self> {
        // api_roots records semantic table nodes during the ordinary pass.
        // Recover only their transparent outer wrappers, without another AST.
        while let Some(parent) = node
            .parent()
            .filter(|parent| parent.kind() == "parenthesized_expression")
        {
            if !worker::check(guard) {
                return None;
            }
            node = parent;
        }
        Self::from_expression(node, guard)
    }
}
struct DocumentationFrame<'tree> {
    node: DocumentationNode<'tree>,
    fields: Option<LiteralFields<'tree, 14>>,
    // Only a System owns a new namespace. Descendants borrow that live ancestor
    // by stack index, so wide collections do not clone it into pending tasks.
    namespace: Option<String>,
    namespace_owner: Option<usize>,
    next_section: usize,
    cursor: Option<TreeCursor<'tree>>,
}
impl<'tree> DocumentationFrame<'tree> {
    fn new(node: DocumentationNode<'tree>, namespace_owner: Option<usize>) -> Self {
        Self {
            node,
            fields: None,
            namespace: None,
            namespace_owner,
            next_section: 4,
            cursor: None,
        }
    }
    fn next_child(&mut self, guard: Option<&WorkerGuard<'_>>) -> Option<DocumentationNode<'tree>> {
        loop {
            if !worker::check(guard) {
                return None;
            }
            if let Some(cursor) = &mut self.cursor {
                let field = cursor.node();
                let next = cursor.goto_next_sibling();
                if !next {
                    self.cursor = None;
                }
                if field.is_named()
                    && field.kind() != "comment"
                    && let Some(value) = field.child_by_field_name("value")
                    && let Some(value) = DocumentationNode::from_expression(value, guard)
                {
                    return Some(value);
                }
            } else {
                if self.next_section == 11 {
                    return None;
                }
                let value =
                    self.fields.as_ref().expect("entered descriptor").values[self.next_section];
                self.next_section += 1;
                if let Some(value) = value {
                    let mut cursor = value.walk();
                    if cursor.goto_first_child() {
                        self.cursor = Some(cursor);
                    }
                }
            }
        }
    }
}

impl<'tree> Walker<'_, 'tree> {
    pub(super) fn documentation(&mut self, root: Node<'tree>) {
        let Some(root) = DocumentationNode::from_root(root, self.guard) else {
            return;
        };
        let mut stack = Vec::new();
        if !worker::prepare_scratch_push(self.guard, &mut stack) {
            return;
        }
        stack.push(DocumentationFrame::new(root, None));
        while !stack.is_empty() {
            if !worker::check(self.guard) {
                return;
            }
            let index = stack.len() - 1;
            if stack[index].fields.is_none() {
                // One descriptor's decoding, qualification and streaming
                // signature temporaries live at a time. Literal decoding is at
                // most one byte per source byte, JSON replacement at most three;
                // this reserves all simultaneous decoded names, growth overlap,
                // qualification/signature copies and small buffer minima before
                // allocation. Explicit signature requests overlap by <=3 times
                // (14N+7); other simultaneous buckets total <=22N+2. See the
                // generated fixture README for the per-buffer/phase proof.
                // 64*64KiB+4KiB stays inside the existing remaining 8MiB scratch
                // ledger. It is not a new allowance or an RSS claim.
                let _temporary = if let Some(guard) = self.guard {
                    let Some(bytes) = self
                        .raw
                        .len()
                        .checked_mul(64)
                        .and_then(|n| n.checked_add(4096))
                    else {
                        guard.abandon();
                        return;
                    };
                    let Some(temporary) = guard.reserve_scratch(bytes) else {
                        return;
                    };
                    Some(temporary)
                } else {
                    None
                };
                let node = stack[index].node;
                let Some(fields) = LiteralFields::read(self.raw, node.table, DOC_KEYS, self.guard)
                else {
                    return;
                };
                let mut kind = fields.text(self.raw, 0);
                kind.make_ascii_lowercase();
                let display = fields.text(self.raw, 1);
                if kind == "system" {
                    let mut namespace = fields.text(self.raw, 2);
                    if namespace.is_empty() && display.starts_with("C_") {
                        namespace = copy_text(&display);
                    }
                    // Transfer this live buffer from temporary admission to the
                    // document scratch ledger before the temporary frame ends.
                    // Like vector growth, reservations are conservative for the
                    // document lifetime and never exceed the original limit.
                    if self
                        .guard
                        .is_some_and(|guard| !guard.charge_scratch(namespace.capacity()))
                    {
                        return;
                    }
                    stack[index].namespace = Some(namespace);
                    stack[index].namespace_owner = Some(index);
                }
                let namespace = stack[index]
                    .namespace_owner
                    .and_then(|owner| stack[owner].namespace.as_deref())
                    .unwrap_or("");
                self.emit_documentation(node, &fields, display, &kind, namespace);
                stack[index].fields = Some(fields);
            }
            if !worker::check(self.guard) {
                return;
            }
            if let Some(child) = stack[index].next_child(self.guard) {
                let namespace_owner = stack[index].namespace_owner;
                if !worker::prepare_scratch_push(self.guard, &mut stack) {
                    return;
                }
                stack.push(DocumentationFrame::new(child, namespace_owner));
            } else {
                stack.pop();
            }
        }
    }

    fn emit_documentation(
        &mut self,
        node: DocumentationNode<'tree>,
        fields: &LiteralFields<'tree, 14>,
        display: String,
        kind: &str,
        namespace: &str,
    ) {
        if kind == "system" {
            self.declare_at(
                node.table,
                node.origin_start,
                copy_text(&display),
                "api-system",
                false,
                Some(display),
            );
        } else if !display.is_empty()
            && [
                "function",
                "event",
                "structure",
                "enumeration",
                "scriptobject",
                "callback",
            ]
            .contains(&kind)
        {
            let qualified = if namespace.is_empty() {
                display
            } else {
                let mut value = String::with_capacity(namespace.len() + display.len() + 1);
                value.push_str(namespace);
                value.push('.');
                value.push_str(&display);
                value
            };
            let Some(signature) = self.signature(fields, &qualified, kind) else {
                return;
            };
            let category = match kind {
                "function" => "api-function",
                "event" => "api-event",
                "structure" => "api-structure",
                "enumeration" => "api-enumeration",
                "scriptobject" => "api-scriptobject",
                "callback" => "api-callback",
                _ => unreachable!("recognized descriptor kind"),
            };
            self.declare_at(
                node.table,
                node.origin_start,
                copy_text(&qualified),
                category,
                false,
                Some(signature),
            );
            if !namespace.is_empty() {
                self.link_at(
                    node.origin_start,
                    copy_text(namespace),
                    copy_text(&qualified),
                    "contains",
                    "exact",
                );
            }
            let literal_name = fields.text(self.raw, 3);
            if kind == "event" && !literal_name.is_empty() && literal_name != qualified {
                let Some(signature) = self.signature(fields, &literal_name, kind) else {
                    return;
                };
                self.declare_at(
                    node.table,
                    node.origin_start,
                    copy_text(&literal_name),
                    "api-event",
                    false,
                    Some(signature),
                );
                self.link_at(
                    node.origin_start,
                    literal_name,
                    qualified,
                    "event-name",
                    "exact",
                );
            }
        }
    }

    fn append_parameters(
        &self,
        output: &mut SignatureBuffer,
        list: Option<Node<'tree>>,
    ) -> Option<bool> {
        let Some(list) = list else { return Some(false) };
        let mut first = true;
        let mut cursor = list.walk();
        for field in list.named_children(&mut cursor) {
            if !worker::check(self.guard) {
                return None;
            }
            if field.kind() == "comment" {
                continue;
            }
            let Some(parameter) = field
                .child_by_field_name("value")
                .and_then(|value| DocumentationNode::from_expression(value, self.guard))
            else {
                continue;
            };
            let fields = LiteralFields::read(self.raw, parameter.table, PARAM_KEYS, self.guard)?;
            let name = fields.text(self.raw, 0);
            let kind = fields.text(self.raw, 1);
            if !first {
                output.append(", ", self.guard)?;
            }
            first = false;
            output.append(
                if name.is_empty() { "<dynamic>" } else { &name },
                self.guard,
            )?;
            if fields.values[2].is_some_and(|value| value.kind() == "true") {
                output.append("?", self.guard)?;
            }
            output.append(": ", self.guard)?;
            output.append(
                if kind.is_empty() { "<dynamic>" } else { &kind },
                self.guard,
            )?;
        }
        worker::check(self.guard).then_some(!first)
    }

    fn signature(
        &self,
        fields: &LiteralFields<'tree, 14>,
        name: &str,
        kind: &str,
    ) -> Option<String> {
        if !worker::check(self.guard) {
            return None;
        }
        let mut signature = SignatureBuffer {
            value: copy_text(name),
            requested: name.len(),
        };
        match kind {
            "function" | "scriptobject" | "callback" => {
                signature.append("(", self.guard)?;
                self.append_parameters(&mut signature, fields.values[11])?;
                signature.append(")", self.guard)?;
                // Append returns directly to the same destination; remove only
                // the provisional separator when there are no parameter rows.
                let end = signature.value.len();
                signature.append(" -> ", self.guard)?;
                if !self.append_parameters(&mut signature, fields.values[12])? {
                    signature.value.truncate(end);
                }
            }
            "event" => {
                signature.append("(", self.guard)?;
                self.append_parameters(&mut signature, fields.values[13])?;
                signature.append(")", self.guard)?;
            }
            _ => {}
        }
        worker::check(self.guard).then_some(signature.value)
    }
}
