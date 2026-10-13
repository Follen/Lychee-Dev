use super::{
    syntax::{self, Expr, ExprKind, Stmt, StmtKind},
    types::*,
};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default)]
struct Value {
    state: String,
    source: String,
    steps: Vec<Step>,
    truncated: bool,
}
impl Value {
    fn new(state: &str) -> Self {
        Self {
            state: state.into(),
            ..Default::default()
        }
    }
}
#[derive(Clone, Default)]
struct Frame {
    vars: BTreeMap<String, Value>,
    returned: bool,
    returns: Vec<Value>,
}
#[derive(Clone)]
struct Function {
    id: String,
    name: String,
    file: String,
    owner: String,
    line: usize,
    end: usize,
    params: Vec<String>,
    body: Vec<Stmt>,
    local: bool,
    conditional: bool,
}
struct DefinitionScope<'a> {
    owner: &'a str,
    file: &'a str,
    local: bool,
    conditional: bool,
}
struct Analyzer<'a> {
    control: &'a OperationControl,
    end: Instant,
    budget: Budget,
    rules: &'a RuleSet,
    result: Analysis,
    functions: Vec<Function>,
    entries: BTreeMap<String, Function>,
    visiting: BTreeSet<String>,
    paths: usize,
    shadowed_guard: bool,
}
fn fault(code: &str, detail: &str) -> ToolkitError {
    ToolkitError::new(
        code,
        format!("{code}: {detail}"),
        if code == "flow.invalid_input" { 4 } else { 3 },
    )
}
/// Files must be an authenticated frozen closure supplied by the source provider.
/// Findings are static candidates. This does not execute Lua or establish taint.
pub fn analyze(
    files: &BTreeMap<String, Vec<u8>>,
    target: &Target,
    rules: &RuleSet,
    mut budget: Budget,
    control: &OperationControl,
) -> Result<Analysis> {
    control.check()?;
    let defaults = Budget::default();
    if budget.max_files == 0 {
        budget.max_files = defaults.max_files;
    }
    if budget.max_functions == 0 {
        budget.max_functions = defaults.max_functions;
    }
    if budget.max_depth == 0 {
        budget.max_depth = defaults.max_depth;
    }
    if budget.max_paths == 0 {
        budget.max_paths = defaults.max_paths;
    }
    if budget.max_millis == 0 {
        budget.max_millis = defaults.max_millis;
    }
    if budget.max_output_bytes == 0 {
        budget.max_output_bytes = defaults.max_output_bytes;
    }
    if budget.max_output_bytes < 2048 {
        return Err(fault("flow.budget", "output budget too small"));
    }
    for part in [
        &rules.identity.client,
        &rules.identity.api_commit,
        &rules.identity.metadata_digest,
        &rules.identity.version,
        &target.symbol,
    ] {
        if part.len() > 256 {
            return Err(fault("flow.budget", "identity or target exceeds 256 bytes"));
        }
    }
    if files.is_empty() {
        return Err(fault("flow.invalid_input", "empty input or invalid target"));
    }
    if files.len() > budget.max_files {
        return Err(fault("flow.budget", "file count"));
    }
    if !target.path.is_empty()
        && (!crate::git::source_path(&target.path) || !files.contains_key(&target.path))
    {
        return Err(fault(
            "flow.invalid_input",
            "target file missing or invalid path",
        ));
    }
    let mut total = 0usize;
    for (file, raw) in files {
        if !crate::git::source_path(file)
            || !file.to_ascii_lowercase().ends_with(".lua")
            || std::str::from_utf8(raw).is_err()
            || raw.contains(&0)
        {
            return Err(fault("flow.invalid_input", "invalid source file"));
        }
        if file.len() > 512 || raw.len() > 1 << 20 {
            return Err(fault("flow.budget", "source file bytes"));
        }
        total += raw.len();
        if total > 32 << 20 {
            return Err(fault("flow.budget", "total source bytes"));
        }
    }
    let end = Instant::now()
        .checked_add(Duration::from_millis(budget.max_millis))
        .ok_or_else(|| fault("flow.budget", "time bound"))?;
    let mut a = Analyzer {
        control,
        end,
        budget,
        rules,
        result: Analysis {
            rule_identity: rules.identity.clone(),
            boundaries: rules.issues.clone(),
            ..Default::default()
        },
        functions: Vec::new(),
        entries: BTreeMap::new(),
        visiting: BTreeSet::new(),
        paths: 0,
        shadowed_guard: false,
    };
    for (file, raw) in files {
        control.check()?;
        if Instant::now() >= a.end {
            a.boundary(file, 0, "time_budget", "AST preparation stopped");
            break;
        }
        let Some(body) = syntax::parse(raw, control)? else {
            a.boundary(
                file,
                0,
                "parse_error",
                "tree-sitter syntax contains an error",
            );
            continue;
        };
        let entry = Function {
            id: format!("{file}:<file>"),
            name: "<file>".into(),
            file: file.clone(),
            owner: String::new(),
            line: 1,
            end: raw.iter().filter(|b| **b == b'\n').count() + 1,
            params: Vec::new(),
            body,
            local: false,
            conditional: false,
        };
        a.mark_shadow(&entry.body);
        a.collect(&entry.body, &entry.id, file, false);
        a.entries.insert(file.clone(), entry);
        if a.functions.len() > a.budget.max_functions {
            a.boundary(
                file,
                0,
                "function_budget",
                "function discovery exceeded budget",
            );
            break;
        }
    }
    let chosen = a.select(target);
    if chosen.is_empty() && !a.entries.is_empty() {
        a.boundary(
            &target.path,
            target.line,
            "target_unresolved",
            "no unique selected function or file",
        );
    }
    for function in chosen {
        let mut initial = Frame::default();
        for parameter in &function.params {
            initial
                .vars
                .insert(parameter.clone(), Value::new("unknown"));
        }
        a.exec(&function, vec![initial], 0)?;
    }
    control.check()?;
    a.finish()
}
fn trace(mut value: Value, step: Step) -> Value {
    value.steps.push(step);
    if value.steps.len() > 64 {
        value.steps.drain(..value.steps.len() - 64);
        value.truncated = true;
    }
    value
}
fn step(file: &str, line: usize, kind: &str, name: &str, state: &str) -> Step {
    Step {
        path: file.into(),
        line,
        kind: kind.into(),
        name: name.into(),
        state: state.into(),
        ..Default::default()
    }
}
fn join(left: Value, right: Value, file: &str, line: usize, kind: &str) -> Value {
    let (item, state) = if left.state == "clean" {
        let state = right.state.clone();
        (right, state)
    } else if right.state == "clean" {
        let state = left.state.clone();
        (left, state)
    } else if ["secret", "possible"].contains(&left.state.as_str()) {
        (left, "possible".into())
    } else {
        (right, "unknown".into())
    };
    trace(item, step(file, line, kind, "", &state))
}
fn assign(frame: &mut Frame, name: String, value: Value) {
    frame
        .vars
        .retain(|key, _| !key.starts_with(&(name.clone() + ".")));
    frame.vars.insert(name, value);
}
fn restore(outer: &Frame, states: &mut [Frame], body: &[Stmt], extra: &[String]) {
    let mut names = extra.to_vec();
    for statement in body {
        if let StmtKind::Assign(true, lhs, _) = &statement.kind {
            names.extend(lhs.iter().map(syntax::name));
        }
        if let StmtKind::Define(name, _, _, true) = &statement.kind {
            names.push(name.clone());
        }
    }
    for state in states {
        for name in &names {
            let prefix = name.clone() + ".";
            state
                .vars
                .retain(|key, _| key != name && !key.starts_with(&prefix));
            for (key, value) in &outer.vars {
                if key == name || key.starts_with(&prefix) {
                    state.vars.insert(key.clone(), value.clone());
                }
            }
        }
    }
}
impl Analyzer<'_> {
    fn boundary(&mut self, file: &str, line: usize, code: &str, detail: &str) {
        let mut end = detail.len().min(256);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        if self.result.boundaries.len() < 512 {
            self.result.boundaries.push(Boundary {
                path: file.into(),
                line,
                code: code.into(),
                detail: detail[..end].into(),
            });
        }
        if code.contains("budget") {
            self.result.truncated = true;
        }
    }
    fn finish(mut self) -> Result<Analysis> {
        self.result.coverage = Coverage {
            state: "partial".into(),
            files: self.entries.len(),
            functions: self.functions.len(),
            paths: self.paths,
            complete: self.result.boundaries.is_empty() && !self.result.truncated,
        };
        if self.result.coverage.complete {
            self.result.coverage.state = "bounded_complete".into();
        }
        loop {
            if storage::bounded_json(&self.result, self.budget.max_output_bytes).is_ok() {
                return Ok(self.result);
            }
            self.result.truncated = true;
            self.result.coverage.complete = false;
            self.result.coverage.state = "partial".into();
            if self.result.findings.pop().is_some() {
                continue;
            }
            if self.result.boundaries.len() > 1 {
                self.result.boundaries.pop();
                continue;
            }
            if self
                .result
                .boundaries
                .first()
                .is_some_and(|b| !b.detail.is_empty())
            {
                self.result.boundaries[0].detail.clear();
                continue;
            }
            return Err(fault(
                "flow.budget",
                "minimum result exceeds output byte limit",
            ));
        }
    }
    fn register(
        &mut self,
        name: &str,
        params: &[String],
        body: &[Stmt],
        stmt: &Stmt,
        scope: DefinitionScope<'_>,
    ) {
        let DefinitionScope {
            owner,
            file,
            local,
            conditional,
        } = scope;
        if name.is_empty() {
            self.boundary(
                file,
                stmt.line,
                "dynamic_function",
                "function name is not static",
            );
            return;
        }
        let function = Function {
            id: format!("{file}:{}:{name}", stmt.line),
            name: name.into(),
            file: file.into(),
            owner: owner.into(),
            line: stmt.line,
            end: stmt.end,
            params: params.to_vec(),
            body: body.to_vec(),
            local,
            conditional,
        };
        self.shadowed_guard |= params.iter().any(|p| p == "issecretvalue");
        self.functions.push(function.clone());
        self.collect(body, &function.id, file, false);
    }
    fn collect(&mut self, body: &[Stmt], owner: &str, file: &str, block: bool) {
        for stmt in body {
            if self.functions.len() > self.budget.max_functions || Instant::now() >= self.end {
                return;
            }
            match &stmt.kind {
                StmtKind::Define(name, params, body, local) => self.register(
                    name,
                    params,
                    body,
                    stmt,
                    DefinitionScope {
                        owner,
                        file,
                        local: *local,
                        conditional: block,
                    },
                ),
                StmtKind::Assign(local, lhs, rhs) => {
                    for (lhs, rhs) in lhs.iter().zip(rhs) {
                        if let ExprKind::Function(params, body) = &rhs.kind {
                            let local_owner = if *local && block {
                                format!("<block:{owner}:{}>", stmt.line)
                            } else {
                                owner.into()
                            };
                            self.register(
                                &syntax::name(lhs),
                                params,
                                body,
                                stmt,
                                DefinitionScope {
                                    owner: &local_owner,
                                    file,
                                    local: *local,
                                    conditional: block,
                                },
                            );
                        }
                    }
                }
                StmtKind::If(_, yes, no) => {
                    self.collect(yes, owner, file, true);
                    self.collect(no, owner, file, true);
                }
                StmtKind::Block(body) | StmtKind::Loop(_, _, body) => {
                    self.collect(body, owner, file, true)
                }
                _ => {}
            }
        }
    }
    fn mark_shadow(&mut self, body: &[Stmt]) {
        for stmt in body {
            match &stmt.kind {
                StmtKind::Assign(_, lhs, rhs) => {
                    self.shadowed_guard |= lhs.iter().any(|e| syntax::name(e) == "issecretvalue");
                    for rhs in rhs {
                        if let ExprKind::Function(_, body) = &rhs.kind {
                            self.mark_shadow(body);
                        }
                    }
                }
                StmtKind::Define(name, _, body, _) => {
                    self.shadowed_guard |= name == "issecretvalue";
                    self.mark_shadow(body);
                }
                StmtKind::If(_, yes, no) => {
                    self.mark_shadow(yes);
                    self.mark_shadow(no);
                }
                StmtKind::Block(body) => self.mark_shadow(body),
                _ => {}
            }
        }
    }
    fn select(&mut self, target: &Target) -> Vec<Function> {
        if !target.symbol.is_empty() {
            let candidates = self
                .functions
                .iter()
                .filter(|f| {
                    f.name == target.symbol && (target.path.is_empty() || f.file == target.path)
                })
                .cloned()
                .collect::<Vec<_>>();
            if candidates.len() > 1 {
                self.boundary(
                    &target.path,
                    target.line,
                    "ambiguous_symbol",
                    "multiple definitions require an exact location",
                );
                return Vec::new();
            }
            return candidates;
        }
        if !target.path.is_empty() {
            if target.line > 0 {
                let mut candidates = self
                    .functions
                    .iter()
                    .filter(|f| {
                        f.file == target.path && f.line <= target.line && target.line <= f.end
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                candidates.sort_by_key(|f| f.end - f.line);
                if candidates.len() > 1
                    && candidates[0].end - candidates[0].line
                        == candidates[1].end - candidates[1].line
                {
                    self.boundary(
                        &target.path,
                        target.line,
                        "ambiguous_location",
                        "multiple function scopes share the selected line",
                    );
                    return Vec::new();
                }
                if let Some(f) = candidates.first() {
                    return vec![f.clone()];
                }
            }
            return self
                .entries
                .get(&target.path)
                .cloned()
                .into_iter()
                .collect();
        }
        self.entries.values().cloned().collect()
    }
    fn exec(
        &mut self,
        function: &Function,
        states: Vec<Frame>,
        depth: usize,
    ) -> Result<Vec<Frame>> {
        if depth > self.budget.max_depth {
            self.boundary(
                &function.file,
                function.line,
                "depth_budget",
                "call depth exceeded",
            );
            return Ok(states);
        }
        if !self.visiting.insert(function.id.clone()) {
            self.boundary(
                &function.file,
                function.line,
                "recursive_call",
                "recursive summary is unsupported",
            );
            return Ok(states);
        }
        let result = self.exec_body(function, states, &function.body, depth);
        self.visiting.remove(&function.id);
        result
    }
    fn exec_body(
        &mut self,
        function: &Function,
        mut states: Vec<Frame>,
        body: &[Stmt],
        depth: usize,
    ) -> Result<Vec<Frame>> {
        for stmt in body {
            self.control.check()?;
            if Instant::now() >= self.end {
                self.boundary(
                    &function.file,
                    stmt.line,
                    "time_budget",
                    "analysis deadline reached",
                );
                return Ok(states);
            }
            let mut next = Vec::new();
            for state in &states {
                if state.returned {
                    next.push(state.clone());
                    continue;
                }
                self.paths += 1;
                if self.paths > self.budget.max_paths {
                    self.boundary(
                        &function.file,
                        stmt.line,
                        "path_budget",
                        "path count exceeded",
                    );
                    return Ok(states);
                }
                next.extend(self.exec_stmt(function, state.clone(), stmt, depth)?);
                if next.len() > self.budget.max_paths {
                    self.boundary(
                        &function.file,
                        stmt.line,
                        "path_budget",
                        "branch count exceeded",
                    );
                    next.truncate(self.budget.max_paths);
                    return Ok(next);
                }
            }
            states = next;
        }
        Ok(states)
    }
    fn exec_stmt(
        &mut self,
        function: &Function,
        mut state: Frame,
        stmt: &Stmt,
        depth: usize,
    ) -> Result<Vec<Frame>> {
        let file = &function.file;
        match &stmt.kind {
            StmtKind::Assign(_, lhs, rhs) => {
                let values = self.assignments(function, &mut state, rhs, depth)?;
                for (i, lhs) in lhs.iter().enumerate() {
                    let name = syntax::name(lhs);
                    if name.is_empty() {
                        self.boundary(
                            file,
                            stmt.line,
                            "dynamic_write",
                            "dynamic key or alias write",
                        );
                        continue;
                    }
                    let value = values
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| Value::new("unknown"));
                    let state_name = value.state.clone();
                    assign(
                        &mut state,
                        name.clone(),
                        trace(
                            value,
                            step(file, stmt.line, "assignment", &name, &state_name),
                        ),
                    );
                }
            }
            StmtKind::Call(expr) => {
                self.eval(function, &mut state, expr, depth)?;
            }
            StmtKind::Return(exprs) => {
                state.returns = self
                    .assignments(function, &mut state, exprs, depth)?
                    .into_iter()
                    .map(|v| {
                        let name = v.state.clone();
                        trace(v, step(file, stmt.line, "return", &function.name, &name))
                    })
                    .collect();
                state.returned = true;
            }
            StmtKind::If(condition, yes, no) => {
                let guard = self.guard(function, condition);
                if guard.is_none() {
                    let value = self.eval(function, &mut state, condition, depth)?.remove(0);
                    if ["secret", "possible"].contains(&value.state.as_str()) {
                        self.boundary(
                            file,
                            stmt.line,
                            "unsupported_condition",
                            "secret-dependent branch condition needs a version rule",
                        );
                    }
                }
                let (mut left, mut right) = (state.clone(), state.clone());
                if let Some((name, negated)) = guard {
                    Self::set_guard(&mut left, &name, !negated, file, stmt.line);
                    Self::set_guard(&mut right, &name, negated, file, stmt.line);
                }
                let mut then_states = self.exec_body(function, vec![left.clone()], yes, depth)?;
                restore(&left, &mut then_states, yes, &[]);
                let mut else_states = self.exec_body(function, vec![right.clone()], no, depth)?;
                restore(&right, &mut else_states, no, &[]);
                then_states.extend(else_states);
                return Ok(then_states);
            }
            StmtKind::Block(body) => {
                let mut result = self.exec_body(function, vec![state.clone()], body, depth)?;
                restore(&state, &mut result, body, &[]);
                return Ok(result);
            }
            StmtKind::Loop(kind, names, body) => {
                let detail = match kind.as_str() {
                    "repeat_statement" => "repeat loop analyzed for one iteration only",
                    "numeric" => "numeric loop summarized as zero or one iteration",
                    "generic" => "generic loop summarized as zero or one iteration",
                    _ => "loop summarized as zero or one iteration",
                };
                self.boundary(file, stmt.line, "unsupported_loop", detail);
                let mut inner = state.clone();
                for name in names {
                    inner.vars.insert(
                        name.clone(),
                        Value::new(if kind == "numeric" {
                            "clean"
                        } else {
                            "unknown"
                        }),
                    );
                }
                let mut result = self.exec_body(function, vec![inner], body, depth)?;
                restore(&state, &mut result, body, names);
                if kind != "repeat_statement" {
                    result.insert(0, state);
                }
                return Ok(result);
            }
            StmtKind::Define(..) => {}
            StmtKind::Control => self.boundary(
                file,
                stmt.line,
                "unsupported_control_flow",
                "break/goto control flow not modeled",
            ),
        }
        Ok(vec![state])
    }
    fn assignments(
        &mut self,
        function: &Function,
        state: &mut Frame,
        exprs: &[Expr],
        depth: usize,
    ) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        for (i, expr) in exprs.iter().enumerate() {
            let values = self.eval(function, state, expr, depth)?;
            if i + 1 < exprs.len() {
                result.push(
                    values
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Value::new("unknown")),
                );
            } else {
                result.extend(values);
            }
        }
        Ok(result)
    }
    fn eval(
        &mut self,
        function: &Function,
        state: &mut Frame,
        expr: &Expr,
        depth: usize,
    ) -> Result<Vec<Value>> {
        self.control.check()?;
        let file = &function.file;
        let value = match &expr.kind {
            ExprKind::Name(_) | ExprKind::Index(..) => {
                if let ExprKind::Index(base, key) = &expr.kind {
                    let left = self.eval(function, state, base, depth)?.remove(0);
                    let right = self.eval(function, state, key, depth)?.remove(0);
                    self.operation(file, expr.line, "index", &[left, right]);
                }
                let name = syntax::name(expr);
                if let Some(value) = state.vars.get(&name) {
                    value.clone()
                } else if !name.is_empty() {
                    Value {
                        state: "unknown".into(),
                        source: name,
                        ..Default::default()
                    }
                } else {
                    self.boundary(file, expr.line, "dynamic_read", "dynamic field read");
                    Value::new("unknown")
                }
            }
            ExprKind::Call(target, args, receiver) => {
                return self.call(
                    function,
                    state,
                    expr.line,
                    target,
                    args,
                    receiver.as_deref(),
                    depth,
                );
            }
            ExprKind::Literal | ExprKind::String(_) => Value::new("clean"),
            ExprKind::Function(..) => Value::new("function"),
            ExprKind::Table(fields) => {
                let mut value = Value::new("clean");
                for (key, field) in fields {
                    let part = self.eval(function, state, field, depth)?.remove(0);
                    if let Some(key) = key {
                        let key = self.eval(function, state, key, depth)?.remove(0);
                        self.operation(file, expr.line, "table-key", &[key]);
                    }
                    value = join(value, part, file, expr.line, "table-constructor");
                }
                value
            }
            ExprKind::Operation(kind, args) => {
                let mut values = Vec::new();
                for expr in args {
                    values.push(self.eval(function, state, expr, depth)?.remove(0));
                }
                self.operation(file, expr.line, kind, &values);
                if values.len() == 1 {
                    let value = values.remove(0);
                    let name = value.state.clone();
                    trace(value, step(file, expr.line, kind, "", &name))
                } else {
                    join(values.remove(0), values.remove(0), file, expr.line, kind)
                }
            }
            ExprKind::Unknown(kind) => {
                self.boundary(file, expr.line, "unsupported_expression", kind);
                Value::new("unknown")
            }
        };
        Ok(vec![value])
    }
    fn operation(&mut self, file: &str, line: usize, kind: &str, values: &[Value]) {
        if values
            .iter()
            .any(|v| ["secret", "possible"].contains(&v.state.as_str()))
        {
            self.boundary(
                file,
                line,
                "unknown_operation_constraint",
                &format!("{kind} of a potentially secret value has no fixed client operation rule"),
            );
        }
    }
    fn guard(&mut self, function: &Function, expr: &Expr) -> Option<(String, bool)> {
        if self.shadowed_guard {
            self.boundary(
                &function.file,
                expr.line,
                "shadowed_guard",
                "issecretvalue is locally or globally rebound",
            );
            return None;
        }
        let (expr, negated) = if let ExprKind::Operation(kind, args) = &expr.kind {
            if kind == "not" {
                (&args[0], true)
            } else {
                (expr, false)
            }
        } else {
            (expr, false)
        };
        let ExprKind::Call(target, args, receiver) = &expr.kind else {
            return None;
        };
        if args.len() != 1 || syntax::name(target) != "issecretvalue" || receiver.is_some() {
            return None;
        }
        let name = syntax::name(&args[0]);
        if name.is_empty() {
            self.boundary(
                &function.file,
                expr.line,
                "dynamic_guard",
                "guard target is dynamic",
            );
            return None;
        }
        Some((name, negated))
    }
    fn set_guard(frame: &mut Frame, name: &str, secret: bool, file: &str, line: usize) {
        let mut value = frame.vars.get(name).cloned().unwrap_or_else(|| Value {
            state: "unknown".into(),
            source: name.into(),
            ..Default::default()
        });
        if secret {
            if ["possible", "unknown"].contains(&value.state.as_str()) {
                value.state = "secret".into();
            }
        } else {
            value.state = "clean".into();
        }
        let state = value.state.clone();
        let mut evidence = step(file, line, "guard", name, &state);
        evidence.condition = format!("issecretvalue({name})={secret}");
        frame.vars.insert(name.into(), trace(value, evidence));
    }
    fn resolve(&self, caller: &Function, name: &str, line: usize) -> (Option<Function>, bool) {
        let mut local = Vec::new();
        let mut global = Vec::new();
        let mut unresolved = false;
        for f in self.functions.iter().filter(|f| f.name == name) {
            if f.local {
                if f.file == caller.file
                    && f.line <= line
                    && (f.owner == caller.id
                        || f.owner == caller.owner
                        || f.owner == format!("{}:<file>", caller.file))
                {
                    local.push(f);
                } else if f.file == caller.file {
                    unresolved = true;
                }
            } else {
                global.push(f);
            }
        }
        if local.len() == 1 {
            return (
                if local[0].conditional {
                    None
                } else {
                    Some(local[0].clone())
                },
                local[0].conditional,
            );
        }
        if local.len() > 1 || unresolved {
            return (None, true);
        }
        if global.len() == 1 {
            return (
                if global[0].conditional {
                    None
                } else {
                    Some(global[0].clone())
                },
                global[0].conditional,
            );
        }
        (None, global.len() > 1)
    }
    fn invalidate(&mut self, state: &mut Frame, file: &str, line: usize, name: &str) {
        let mut changed = false;
        for (key, value) in &mut state.vars {
            if key.contains('.')
                || value
                    .steps
                    .iter()
                    .any(|s| s.kind == "guard" && s.state == "clean")
            {
                value.state = "unknown".into();
                *value = trace(
                    value.clone(),
                    step(file, line, "unknown-side-effect", name, "unknown"),
                );
                changed = true;
            }
        }
        if changed {
            self.boundary(
                file,
                line,
                "unknown_side_effect",
                "call may change guarded values or static fields",
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        function: &Function,
        state: &mut Frame,
        line: usize,
        target: &Expr,
        expressions: &[Expr],
        receiver: Option<&Expr>,
        depth: usize,
    ) -> Result<Vec<Value>> {
        let name = syntax::name(target);
        let mut args = self.assignments(function, state, expressions, depth)?;
        let file = &function.file;
        if name.is_empty() {
            self.boundary(
                file,
                line,
                "dynamic_call",
                "call target is not statically named",
            );
            self.invalidate(state, file, line, "<dynamic>");
            return Ok(vec![Value::new("unknown")]);
        }
        if state.vars.get(&name).is_some_and(|v| v.state != "function") {
            self.boundary(
                file,
                line,
                "shadowed_call",
                &format!("local/parameter value shadows {name}"),
            );
            self.invalidate(state, file, line, &name);
            return Ok(vec![Value::new("unknown")]);
        }
        let (callee, ambiguous) = self.resolve(function, &name, line);
        if ambiguous {
            self.boundary(
                file,
                line,
                "ambiguous_call",
                &format!("multiple definitions without proven load order: {name}"),
            );
            self.invalidate(state, file, line, &name);
            return Ok(vec![Value::new("unknown")]);
        }
        let base = name.split(['.', ':']).next().unwrap();
        if base != name && state.vars.contains_key(base) && callee.is_none() {
            self.boundary(
                file,
                line,
                "shadowed_call",
                &format!("local table shadows API root {base}"),
            );
            self.invalidate(state, file, line, &name);
            return Ok(vec![Value::new("unknown")]);
        }
        if let Some(callee) = callee {
            if depth >= self.budget.max_depth {
                self.boundary(
                    file,
                    line,
                    "depth_budget",
                    &format!("call depth exceeded at {name}"),
                );
                return Ok(vec![Value::new("unknown")]);
            }
            if self.visiting.contains(&callee.id) {
                self.boundary(
                    file,
                    line,
                    "recursive_call",
                    &format!("recursive call to {name}"),
                );
                return Ok(vec![Value::new("unknown")]);
            }
            if let Some(receiver) = receiver {
                args.insert(0, self.eval(function, state, receiver, depth)?.remove(0));
            }
            let mut inner = Frame::default();
            for (i, param) in callee.params.iter().enumerate() {
                let value = args
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| Value::new("unknown"));
                let state_name = value.state.clone();
                inner.vars.insert(
                    param.clone(),
                    trace(value, step(file, line, "argument", param, &state_name)),
                );
            }
            let states = self.exec(&callee, vec![inner], depth + 1)?;
            let mut result: Option<Vec<Value>> = None;
            for returned in states {
                if !returned.returned {
                    self.boundary(
                        &callee.file,
                        callee.line,
                        "return_unknown",
                        "function may finish without an explicit return",
                    );
                    continue;
                }
                if let Some(result) = &mut result {
                    for (i, value) in returned.returns.into_iter().enumerate() {
                        if i < result.len() {
                            result[i] = join(result[i].clone(), value, file, line, "branch-merge");
                        } else {
                            result.push(value);
                        }
                    }
                } else {
                    result = Some(returned.returns);
                }
            }
            let result = result.unwrap_or_default();
            if result.is_empty() {
                return Ok(vec![Value::new("unknown")]);
            }
            return Ok(result
                .into_iter()
                .map(|value| {
                    let state = value.state.clone();
                    trace(value, step(file, line, "call-return", &name, &state))
                })
                .collect());
        }
        let Some(rule) = self.rules.calls.get(&name).cloned() else {
            if args
                .iter()
                .any(|v| ["secret", "possible"].contains(&v.state.as_str()))
            {
                self.boundary(
                    file,
                    line,
                    "unknown_call_constraint",
                    &format!("no fixed rule or unique definition for {name}"),
                );
            }
            self.boundary(
                file,
                line,
                "unresolved_call",
                &format!("no fixed rule or unique definition for {name}"),
            );
            self.invalidate(state, file, line, &name);
            return Ok(vec![Value::new("unknown")]);
        };
        for (index, argument) in &rule.arguments {
            if *index == 0 || *index > args.len() {
                continue;
            }
            let value = &args[index - 1];
            let mut finding = if argument.secret == "allowed"
                || ["clean", "never"].contains(&value.state.as_str())
            {
                continue;
            } else if argument.secret == "unknown" || value.state == "unknown" {
                self.boundary(
                    file,
                    line,
                    "unknown_argument_constraint",
                    &format!("{name} argument {index} has unresolved rule/value"),
                );
                continue;
            } else if argument.secret == "forbidden"
                && value.state == "secret"
                && argument.condition.is_empty()
            {
                "confirmed_candidate"
            } else {
                "possible"
            };
            if value.truncated {
                self.boundary(
                    file,
                    line,
                    "trace_budget",
                    "source path exceeded 64 retained steps",
                );
                finding = "possible";
            }
            let mut steps = value.steps.clone();
            let mut use_step = step(file, line, "use", &name, finding);
            use_step.condition = argument.condition.clone();
            steps.push(use_step);
            if self.result.findings.len() < 256 {
                self.result.findings.push(Finding {
                    state: finding.into(),
                    source: value.source.clone(),
                    use_name: name.clone(),
                    argument: *index,
                    rule_path: rule.path.clone(),
                    rule_line: rule.line,
                    condition: argument.condition.clone(),
                    steps,
                });
            } else {
                self.boundary(file, line, "output_budget", "finding count exceeded");
            }
        }
        let mut max = rule.returns.keys().copied().max().unwrap_or(1).max(1);
        if max > 32 {
            self.boundary(
                file,
                line,
                "return_slot_budget",
                "more than 32 return slots",
            );
            max = 32;
        }
        let mut result = vec![Value::new("unknown"); max];
        for (index, value) in result.iter_mut().enumerate() {
            if let Some(slot) = rule.returns.get(&(index + 1)) {
                value.state = slot.state.clone();
                value.source = name.clone();
                let mut rule_step = step(&rule.path, rule.line, "rule", &name, &slot.state);
                rule_step.condition = slot.condition.clone();
                let mut source_step = step(file, line, "source", &name, &slot.state);
                source_step.condition = slot.condition.clone();
                value.steps = vec![rule_step, source_step];
            }
        }
        self.invalidate(state, file, line, &name);
        Ok(result)
    }
}
