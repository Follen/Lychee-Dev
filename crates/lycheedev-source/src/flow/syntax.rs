//! Owned flow IR from the shared tree-sitter Lua syntax tree. No Lua is run.
use crate::lua_tree::SyntaxDocument;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use tree_sitter::Node;

#[derive(Clone, Debug)]
pub(super) struct Expr {
    pub line: usize,
    pub kind: ExprKind,
}
#[derive(Clone, Debug)]
pub(super) enum ExprKind {
    Name(String),
    Literal,
    String(String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, Option<Box<Expr>>),
    Function(Vec<String>, Vec<Stmt>),
    Table(Vec<(Option<Expr>, Expr)>),
    Operation(String, Vec<Expr>),
    Unknown(String),
}
#[derive(Clone, Debug)]
pub(super) struct Stmt {
    pub line: usize,
    pub end: usize,
    pub kind: StmtKind,
}
#[derive(Clone, Debug)]
pub(super) enum StmtKind {
    Assign(bool, Vec<Expr>, Vec<Expr>),
    Call(Expr),
    Return(Vec<Expr>),
    Define(String, Vec<String>, Vec<Stmt>, bool),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Block(Vec<Stmt>),
    Loop(String, Vec<String>, Vec<Stmt>),
    Control,
}
pub(super) fn name(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Name(n) => n.clone(),
        ExprKind::Index(base, key) => match &key.kind {
            ExprKind::String(key) if !key.is_empty() && !name(base).is_empty() => {
                format!("{}.{key}", name(base))
            }
            _ => String::new(),
        },
        _ => String::new(),
    }
}
pub(super) fn parse(raw: &[u8], control: &OperationControl) -> Result<Option<Vec<Stmt>>> {
    control.check()?;
    let syntax = SyntaxDocument::new(raw)?;
    control.check()?;
    let root = syntax.root();
    if root.has_error() {
        return Ok(None);
    }
    let mut reader = Reader {
        raw,
        control,
        work: 0,
    };
    if !reader.lua51(root, 0)? {
        return Ok(None);
    }
    Ok(Some(reader.block(Some(root), 0)?))
}
struct Reader<'a> {
    raw: &'a [u8],
    control: &'a OperationControl,
    work: usize,
}
impl Reader<'_> {
    fn lua51(&mut self, n: Node<'_>, depth: usize) -> Result<bool> {
        self.check(depth)?;
        if matches!(n.kind(), "goto_statement" | "label_statement" | "attribute") {
            return Ok(false);
        }
        if matches!(n.kind(), "binary_expression" | "unary_expression")
            && let Some(operator) = n.child_by_field_name("operator")
            && matches!(
                self.text(operator).as_str(),
                "//" | "&" | "|" | "~" | "<<" | ">>"
            )
        {
            return Ok(false);
        }
        for child in Self::children(n) {
            if !self.lua51(child, depth + 1)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn check(&mut self, depth: usize) -> Result<()> {
        self.work += 1;
        if depth > 128 || self.work > 200000 {
            return Err(ToolkitError::new(
                "flow.budget",
                "flow.budget: syntax work",
                3,
            ));
        }
        self.control.check()
    }
    fn text(&self, n: Node<'_>) -> String {
        String::from_utf8_lossy(&self.raw[n.byte_range()]).into_owned()
    }
    fn children<'a>(n: Node<'a>) -> Vec<Node<'a>> {
        let mut c = n.walk();
        n.named_children(&mut c).filter(|n| !n.is_extra()).collect()
    }
    fn block(&mut self, node: Option<Node<'_>>, depth: usize) -> Result<Vec<Stmt>> {
        let mut result = Vec::new();
        if let Some(n) = node {
            for child in Self::children(n) {
                if let Some(stmt) = self.statement(child, depth + 1)? {
                    result.push(stmt);
                }
            }
        }
        Ok(result)
    }
    fn expressions(&mut self, node: Option<Node<'_>>, depth: usize) -> Result<Vec<Expr>> {
        match node {
            Some(n) => Self::children(n)
                .into_iter()
                .map(|n| self.expr(n, depth + 1))
                .collect(),
            None => Ok(Vec::new()),
        }
    }
    fn params(&self, node: Option<Node<'_>>) -> Vec<String> {
        node.map(|n| {
            Self::children(n)
                .into_iter()
                .filter(|n| n.kind() == "identifier")
                .map(|n| self.text(n))
                .collect()
        })
        .unwrap_or_default()
    }
    fn statement(&mut self, n: Node<'_>, depth: usize) -> Result<Option<Stmt>> {
        self.check(depth)?;
        let line = n.start_position().row + 1;
        let end = n.end_position().row + 1;
        let kind = match n.kind() {
            "variable_declaration" => {
                let Some(child) = Self::children(n).first().copied() else {
                    return Ok(None);
                };
                let (lhs, rhs) = if child.kind() == "assignment_statement" {
                    self.assignment(child, depth)?
                } else {
                    (self.expressions(Some(child), depth)?, Vec::new())
                };
                StmtKind::Assign(true, lhs, rhs)
            }
            "assignment_statement" => {
                let (lhs, rhs) = self.assignment(n, depth)?;
                StmtKind::Assign(false, lhs, rhs)
            }
            "function_call" => StmtKind::Call(self.expr(n, depth)?),
            "return_statement" => {
                let children = Self::children(n);
                StmtKind::Return(self.expressions(children.first().copied(), depth)?)
            }
            "function_declaration" => {
                let target = n.child_by_field_name("name").unwrap();
                let mut params = self.params(n.child_by_field_name("parameters"));
                let function_name = if target.kind() == "method_index_expression" {
                    params.insert(0, "self".into());
                    self.method_name(target, depth)?
                } else {
                    name(&self.expr(target, depth)?)
                };
                let body = self.block(n.child_by_field_name("body"), depth)?;
                StmtKind::Define(
                    function_name,
                    params,
                    body,
                    self.text(n).trim_start().starts_with("local "),
                )
            }
            "if_statement" | "elseif_statement" => {
                let condition = self.expr(n.child_by_field_name("condition").unwrap(), depth)?;
                let yes = self.block(n.child_by_field_name("consequence"), depth)?;
                let alternatives = Self::children(n)
                    .into_iter()
                    .filter(|n| matches!(n.kind(), "else_statement" | "elseif_statement"))
                    .collect::<Vec<_>>();
                let mut no = Vec::new();
                for alternative in alternatives.into_iter().rev() {
                    if alternative.kind() == "else_statement" {
                        no = self.block(alternative.child_by_field_name("body"), depth)?;
                    } else if let Some(mut stmt) = self.statement(alternative, depth + 1)? {
                        if let StmtKind::If(_, _, tail) = &mut stmt.kind
                            && tail.is_empty()
                        {
                            *tail = no;
                        }
                        no = vec![stmt];
                    }
                }
                StmtKind::If(condition, yes, no)
            }
            "do_statement" => StmtKind::Block(self.block(n.child_by_field_name("body"), depth)?),
            "while_statement" | "repeat_statement" => StmtKind::Loop(
                n.kind().into(),
                Vec::new(),
                self.block(n.child_by_field_name("body"), depth)?,
            ),
            "for_statement" => {
                let clause = n.child_by_field_name("clause").unwrap();
                let numeric = clause.kind() == "for_numeric_clause";
                let names = if numeric {
                    vec![self.text(clause.child_by_field_name("name").unwrap())]
                } else {
                    let vars = Self::children(clause)
                        .into_iter()
                        .find(|n| n.kind() == "variable_list");
                    self.expressions(vars, depth)?.iter().map(name).collect()
                };
                StmtKind::Loop(
                    if numeric { "numeric" } else { "generic" }.into(),
                    names,
                    self.block(n.child_by_field_name("body"), depth)?,
                )
            }
            "break_statement" | "goto_statement" | "label_statement" => StmtKind::Control,
            _ => return Ok(None),
        };
        Ok(Some(Stmt { line, end, kind }))
    }
    fn assignment(&mut self, n: Node<'_>, depth: usize) -> Result<(Vec<Expr>, Vec<Expr>)> {
        let children = Self::children(n);
        let lhs = children
            .iter()
            .find(|n| n.kind() == "variable_list")
            .copied();
        let rhs = children
            .iter()
            .find(|n| n.kind() == "expression_list")
            .copied();
        Ok((self.expressions(lhs, depth)?, self.expressions(rhs, depth)?))
    }
    fn method_name(&mut self, n: Node<'_>, depth: usize) -> Result<String> {
        let base = self.expr(n.child_by_field_name("table").unwrap(), depth)?;
        Ok(format!(
            "{}:{}",
            name(&base),
            self.text(
                n.child_by_field_name("method")
                    .or_else(|| n.child_by_field_name("field"))
                    .unwrap()
            )
        ))
    }
    fn expr(&mut self, n: Node<'_>, depth: usize) -> Result<Expr> {
        self.check(depth)?;
        let line = n.start_position().row + 1;
        let kind = match n.kind() {
            "identifier" => ExprKind::Name(self.text(n)),
            "number" | "nil" | "true" | "false" => ExprKind::Literal,
            "string" => {
                let text = self.text(n);
                ExprKind::String(
                    if (text.starts_with('\'') && text.ends_with('\''))
                        || (text.starts_with('"') && text.ends_with('"'))
                    {
                        text[1..text.len() - 1].to_owned()
                    } else {
                        String::new()
                    },
                )
            }
            "parenthesized_expression" => {
                let child = Self::children(n).into_iter().next().unwrap();
                return self.expr(child, depth + 1);
            }
            "dot_index_expression" | "bracket_index_expression" => {
                let base = self.expr(n.child_by_field_name("table").unwrap(), depth + 1)?;
                let field = n.child_by_field_name("field").unwrap();
                let key = if n.kind() == "dot_index_expression" {
                    Expr {
                        line: field.start_position().row + 1,
                        kind: ExprKind::String(self.text(field)),
                    }
                } else {
                    self.expr(field, depth + 1)?
                };
                ExprKind::Index(Box::new(base), Box::new(key))
            }
            "function_call" => {
                let target = n.child_by_field_name("name").unwrap();
                let (target, receiver) = if target.kind() == "method_index_expression" {
                    let receiver =
                        self.expr(target.child_by_field_name("table").unwrap(), depth + 1)?;
                    let method = self.method_name(target, depth + 1)?;
                    (
                        Expr {
                            line,
                            kind: ExprKind::Name(method),
                        },
                        Some(Box::new(receiver)),
                    )
                } else {
                    (self.expr(target, depth + 1)?, None)
                };
                ExprKind::Call(
                    Box::new(target),
                    self.expressions(n.child_by_field_name("arguments"), depth + 1)?,
                    receiver,
                )
            }
            "function_definition" => ExprKind::Function(
                self.params(n.child_by_field_name("parameters")),
                self.block(n.child_by_field_name("body"), depth + 1)?,
            ),
            "table_constructor" => {
                let mut fields = Vec::new();
                for field in Self::children(n)
                    .into_iter()
                    .filter(|n| n.kind() == "field")
                {
                    let key = field
                        .child_by_field_name("name")
                        .map(|n| self.expr(n, depth + 1))
                        .transpose()?;
                    let value =
                        self.expr(field.child_by_field_name("value").unwrap(), depth + 1)?;
                    fields.push((key, value));
                }
                ExprKind::Table(fields)
            }
            "binary_expression" => {
                let operator = self.text(n.child_by_field_name("operator").unwrap());
                let operation = match operator.as_str() {
                    "and" | "or" => "logical",
                    "==" | "~=" | ">" | "<" | ">=" | "<=" => "comparison",
                    ".." => "concatenation",
                    _ => "arithmetic",
                };
                ExprKind::Operation(
                    operation.into(),
                    vec![
                        self.expr(n.child_by_field_name("left").unwrap(), depth + 1)?,
                        self.expr(n.child_by_field_name("right").unwrap(), depth + 1)?,
                    ],
                )
            }
            "unary_expression" => {
                let operation = match self
                    .text(n.child_by_field_name("operator").unwrap())
                    .as_str()
                {
                    "#" => "length",
                    "not" => "not",
                    _ => "negation",
                };
                ExprKind::Operation(
                    operation.into(),
                    vec![self.expr(n.child_by_field_name("operand").unwrap(), depth + 1)?],
                )
            }
            _ => ExprKind::Unknown(n.kind().into()),
        };
        Ok(Expr { line, kind })
    }
}
