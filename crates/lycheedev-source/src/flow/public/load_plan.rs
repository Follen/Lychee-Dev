//! Bounded literal load traversal. Repeated loads remain occurrences; only
//! active ancestors are cycles. Partial plans never prove a unique load order.
use super::{gap, index};
use crate::xml_load::{LoadContext, LoadDecision};
use lycheedev_core::{Result, control::OperationControl};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    rc::Rc,
};

const WORK: usize = 65_536;
const OCCURRENCES: usize = 256;
const PATH_BYTES: usize = 8 << 20;
const ISSUES: usize = 256;
const ISSUE_BYTES: usize = 4 << 20;
const PATH_LIMIT: usize = 4096;

#[derive(Clone, Copy)]
struct Limits {
    work: usize,
    occurrences: usize,
    paths: usize,
    issues: usize,
    issue_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            work: WORK,
            occurrences: OCCURRENCES,
            paths: PATH_BYTES,
            issues: ISSUES,
            issue_bytes: ISSUE_BYTES,
        }
    }
}

pub(super) struct Selection {
    pub ordered: Option<Vec<String>>,
    pub input_gaps: Vec<Value>,
    pub input_bytes: usize,
    pub load_evidence: Value,
}

struct Budget<'a, F> {
    control: &'a OperationControl,
    observer: F,
    limits: Limits,
    work: usize,
    paths: usize,
    input_bytes: usize,
    issues: usize,
    issue_bytes: usize,
    exhausted: bool,
}
impl<F: FnMut(usize)> Budget<'_, F> {
    fn step(&mut self) -> Result<bool> {
        (self.observer)(self.work);
        self.control.check()?;
        if self.exhausted || self.work >= self.limits.work {
            self.exhausted = true;
            return Ok(false);
        }
        self.work += 1;
        Ok(true)
    }
    fn path(&mut self, path: &String) -> bool {
        if path.capacity() > self.limits.paths.saturating_sub(self.paths) {
            self.exhausted = true;
            return false;
        }
        self.paths += path.capacity();
        true
    }
    fn issue(&mut self, output: &mut Vec<Value>, code: &str, detail: String) {
        let code = code.to_owned();
        let bytes = code.capacity().saturating_add(detail.capacity());
        if self.issues == self.limits.issues
            || bytes > self.limits.issue_bytes.saturating_sub(self.issue_bytes)
        {
            self.exhausted = true;
            return;
        }
        self.issues += 1;
        self.issue_bytes += bytes;
        output.push(json!({"code":code,"detail":detail}));
    }
}

fn bounded(value: &str) -> &str {
    let mut end = value.len().min(PATH_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn resolve(from: &str, target: &str) -> Option<String> {
    // Limits precede replacement/copying. The borrowed input index is separate
    // from this traversal's owned-string budget, so it need not constrain length.
    if from.len() > PATH_LIMIT || target.len() > PATH_LIMIT {
        return None;
    }
    let target = target.replace('\\', "/");
    if target.starts_with('/') || target.contains([':', '\0', '\r', '\n']) {
        return None;
    }
    let mut parts = from
        .rsplit_once('/')
        .map(|(parent, _)| parent.split('/').collect::<Vec<_>>())
        .unwrap_or_default();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    let bytes = parts
        .iter()
        .try_fold(0usize, |sum, part| sum.checked_add(part.len()))?
        .checked_add(parts.len().saturating_sub(1))?;
    if bytes == 0 || bytes > PATH_LIMIT {
        return None;
    }
    Some(parts.join("/"))
}

fn lua(path: &str) -> bool {
    path.get(path.len().saturating_sub(4)..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(".lua"))
}

struct Plan {
    manifest: Rc<str>,
    order: Vec<String>,
    issues: Vec<Value>,
}
struct Frame {
    path: Rc<str>,
    next: usize,
}

fn walk<F: FnMut(usize)>(
    plan: &mut Plan,
    loads: &BTreeMap<&str, Vec<&index::LoadFact>>,
    context: &LoadContext,
    budget: &mut Budget<'_, F>,
) -> Result<()> {
    let mut active = BTreeSet::new();
    active.insert(Rc::clone(&plan.manifest));
    let mut stack = vec![Frame {
        path: Rc::clone(&plan.manifest),
        next: 0,
    }];
    while !stack.is_empty() {
        if !budget.step()? {
            break;
        }
        let frame = stack.last_mut().expect("nonempty stack");
        let Some(edge) = loads
            .get(frame.path.as_ref())
            .and_then(|edges| edges.get(frame.next))
            .copied()
        else {
            active.remove(frame.path.as_ref());
            stack.pop();
            continue;
        };
        frame.next += 1;
        // Bound the original line before conditional parsing can allocate.
        // Facts remain unchanged; only the derived selection is decoded.
        let decoded = if edge.target.len() > PATH_LIMIT {
            None
        } else {
            match context.target(&edge.target) {
                LoadDecision::Include(target) => Some(target),
                LoadDecision::Exclude => continue,
                LoadDecision::Unavailable(reason) => {
                    let from = bounded(&frame.path);
                    let target = bounded(&edge.target);
                    let reason = bounded(&reason);
                    let mut detail =
                        String::with_capacity(from.len() + target.len() + reason.len() + 24);
                    write!(&mut detail, "{from}:{} {target}: {reason}", edge.line)
                        .expect("String writing is infallible");
                    budget.issue(&mut plan.issues, "load_condition_unavailable", detail);
                    continue;
                }
            }
        };
        let Some(target) = decoded
            .as_deref()
            .and_then(|target| resolve(&frame.path, target))
        else {
            let from = bounded(&frame.path);
            let target = bounded(&edge.target);
            let mut detail = String::with_capacity(from.len() + target.len() + 22);
            write!(&mut detail, "{from}:{} {target}", edge.line)
                .expect("String writing is infallible");
            budget.issue(&mut plan.issues, "load_path_unavailable", detail);
            continue;
        };
        if !budget.path(&target) {
            break;
        }
        if lua(&target) {
            if plan.order.len() >= budget.limits.occurrences {
                budget.exhausted = true;
                break;
            }
            plan.order.push(target);
        } else if active.contains(target.as_str()) {
            budget.issue(&mut plan.issues, "load_cycle", target);
        } else {
            let path: Rc<str> = Rc::from(target);
            active.insert(Rc::clone(&path));
            stack.push(Frame { path, next: 0 });
        }
    }
    Ok(())
}

/// context_flow receives the full index from the current fingerprinted v6
/// cache/build gate. Header symbols are mandatory persisted parser observations;
/// unlike Document.facts, an empty current projection really means no headers.
pub(super) fn collect(
    selected: &str,
    index: &index::Index,
    facts: &[index::LoadFact],
    source: &Value,
    control: &OperationControl,
) -> Result<Selection> {
    let complete_projection = index.summary["schema"] == "lycheedev.source-map.v2"
        && index.summary["documents"].as_u64() == Some(index.documents.len() as u64)
        && ["repository", "product", "commit"]
            .iter()
            .all(|key| source[*key].is_string() && index.summary[*key] == source[*key]);
    collect_with(
        selected,
        index.documents.iter(),
        facts,
        &index.symbols,
        complete_projection,
        Some(source),
        control,
        Limits::default(),
        |_| {},
    )
}

fn collect_with<'a>(
    selected: &str,
    roots: impl IntoIterator<Item = &'a index::Document>,
    facts: &[index::LoadFact],
    symbols: &[index::Symbol],
    complete_projection: bool,
    source: Option<&Value>,
    control: &OperationControl,
    limits: Limits,
    observer: impl FnMut(usize),
) -> Result<Selection> {
    let mut budget = Budget {
        control,
        observer,
        limits,
        work: 0,
        paths: 0,
        input_bytes: 0,
        issues: 0,
        issue_bytes: 0,
        exhausted: false,
    };
    control.check()?;
    let mut loads = BTreeMap::<&str, Vec<&index::LoadFact>>::new();
    let mut invalid_facts = Vec::new();
    for fact in facts {
        if !budget.step()? {
            break;
        }
        if !crate::git::source_path(&fact.path) {
            budget.issue(
                &mut invalid_facts,
                "load_path_unavailable",
                bounded(&fact.path).to_owned(),
            );
            continue;
        }
        loads.entry(&fact.path).or_default().push(fact);
    }
    // The already authenticated index bounds the scan. Only header references
    // are retained; each consumes the existing work budget before insertion.
    let mut headers = BTreeMap::<&str, Vec<&index::Symbol>>::new();
    if !complete_projection {
        budget.issue(
            &mut invalid_facts,
            "load_header_evidence_unavailable",
            "current complete indexed TOC header projection is unavailable".into(),
        );
    } else if !budget.exhausted {
        for header in symbols {
            control.check()?;
            if header.kind != "header" {
                if header.category == "toc-field" {
                    budget.issue(
                        &mut invalid_facts,
                        "load_header_evidence_unavailable",
                        bounded(&header.path).to_owned(),
                    );
                }
                continue;
            }
            if !budget.step()? {
                break;
            }
            if header.category != "toc-field"
                || header.line == 0
                || header.end_line != header.line
                || !crate::git::source_path(&header.path)
                || !header
                    .path
                    .get(header.path.len().saturating_sub(4)..)
                    .is_some_and(|tail| tail.eq_ignore_ascii_case(".toc"))
            {
                budget.issue(
                    &mut invalid_facts,
                    "load_header_evidence_unavailable",
                    bounded(&header.path).to_owned(),
                );
                continue;
            }
            let bytes = header.name.len().saturating_add(header.target.len());
            if bytes > super::INPUT_BYTES.saturating_sub(budget.input_bytes) {
                budget.exhausted = true;
                break;
            }
            // Metadata and later Lua bytes share the one public input allowance.
            // Borrowed metadata is not a second I/O or retained-original budget.
            budget.input_bytes += bytes;
            headers.entry(&header.path).or_default().push(header);
        }
    }
    let mut plans = Vec::new();
    if !budget.exhausted && complete_projection {
        for document in roots {
            let root = document.path.as_str();
            if !budget.step()? {
                break;
            }
            if !root
                .get(root.len().saturating_sub(4)..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(".toc"))
            {
                continue;
            }
            if root.len() > PATH_LIMIT || !crate::git::source_path(root) {
                // Never duplicate an unbounded caller-controlled path in evidence.
                let mut plan = Plan {
                    manifest: Rc::from(bounded(root)),
                    order: vec![],
                    issues: vec![],
                };
                let charged = bounded(root).to_owned();
                if !budget.path(&charged) {
                    break;
                }
                budget.issue(
                    &mut plan.issues,
                    "load_path_unavailable",
                    bounded(root).to_owned(),
                );
                plans.push(plan);
                if budget.exhausted {
                    break;
                }
                continue;
            }
            let root = root.to_owned();
            if !budget.path(&root) {
                break;
            }
            let mut plan = Plan {
                manifest: Rc::from(root),
                order: vec![],
                issues: vec![],
            };
            let manifest_headers = headers.remove(document.path.as_str()).unwrap_or_default();
            let fields = || {
                manifest_headers
                    .iter()
                    .map(|header| (header.name.as_str(), header.target.as_str()))
            };
            control.check()?;
            let context = LoadContext::for_manifest_headers(&document.path, fields(), source);
            let admission = context.manifest_admission_headers(fields());
            control.check()?;
            match admission {
                LoadDecision::Include(_) => walk(&mut plan, &loads, &context, &mut budget)?,
                LoadDecision::Exclude => {}
                LoadDecision::Unavailable(reason) => budget.issue(
                    &mut plan.issues,
                    "toc_manifest_condition_unavailable",
                    format!("{}: {}", bounded(&document.path), bounded(&reason)),
                ),
            }
            plans.push(plan);
            if budget.exhausted {
                break;
            }
        }
    }
    if !budget.exhausted && complete_projection && !headers.is_empty() {
        let path = *headers.keys().next().expect("nonempty headers");
        budget.issue(
            &mut invalid_facts,
            "load_header_evidence_unavailable",
            bounded(path).to_owned(),
        );
    }
    control.check()?;
    let incomplete = budget.exhausted
        || !invalid_facts.is_empty()
        || plans.iter().any(|plan| !plan.issues.is_empty());
    let candidates = plans
        .iter()
        .filter(|plan| plan.order.iter().any(|path| path == selected))
        .collect::<Vec<_>>();
    let ordered = if candidates.len() == 1 && !incomplete {
        Some(candidates[0].order.clone())
    } else {
        None
    };
    let mut input_gaps = Vec::new();
    if budget.exhausted {
        let terminal = gap(
            "load_budget",
            "TOC/XML load traversal exceeded its work, occurrence, path, input-byte, or issue budget; unseen loads remain unknown",
        );
        if let Some(last) = plans.last_mut() {
            last.issues.push(terminal.clone());
        }
        input_gaps.push(terminal);
    }
    if ordered.is_none() {
        input_gaps.push(gap(
            "load_order_unknown",
            "no unique complete TOC/XML load plan proves the selected file order",
        ));
    }
    let mut candidate_evidence = Vec::new();
    let mut incomplete_evidence = Vec::new();
    for plan in &plans {
        control.check()?;
        if plan.order.iter().any(|path| path == selected) {
            candidate_evidence.push(json!({"manifest":plan.manifest.as_ref(),"ordered":plan.order,"issues":plan.issues}));
        }
        if !plan.issues.is_empty() {
            incomplete_evidence.push(json!({"manifest":plan.manifest.as_ref(),"ordered":plan.order,"issues":plan.issues}));
        }
    }
    let mut load_evidence = json!({
        "basis":"pinned_toc_xml_literals", "runtimeExecution":false,
        "budget":{"work":budget.work,"workLimit":limits.work,"pathBytes":budget.paths,"pathBytesLimit":limits.paths,"metadataBytes":budget.input_bytes,"inputBytesLimit":super::INPUT_BYTES,"issues":budget.issues,"issueBytes":budget.issue_bytes,"exhausted":budget.exhausted}
    });
    let evidence = load_evidence.as_object_mut().expect("load evidence object");
    evidence.insert("candidates".into(), Value::Array(candidate_evidence));
    evidence.insert("incompletePlans".into(), Value::Array(incomplete_evidence));
    evidence.insert("unavailableFacts".into(), Value::Array(invalid_facts));
    control.check()?;
    Ok(Selection {
        ordered,
        input_gaps,
        input_bytes: budget.input_bytes,
        load_evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    fn document(path: &str, raw: &[u8]) -> index::Document {
        index::Document {
            path: path.into(),
            text: Some(raw.into()),
            bytes: raw.len(),
            sha256: lycheedev_core::storage::sha256(raw),
            facts: Default::default(),
            facts_locator: None,
        }
    }
    fn test_collect<'a>(
        selected: &str,
        roots: impl IntoIterator<Item = &'a str>,
        facts: &[index::LoadFact],
        control: &OperationControl,
    ) -> Result<Selection> {
        test_collect_with(selected, roots, facts, control, Limits::default(), |_| {})
    }
    fn test_collect_with<'a>(
        selected: &str,
        roots: impl IntoIterator<Item = &'a str>,
        facts: &[index::LoadFact],
        control: &OperationControl,
        limits: Limits,
        observer: impl FnMut(usize),
    ) -> Result<Selection> {
        let roots = roots
            .into_iter()
            .map(|path| document(path, b""))
            .collect::<Vec<_>>();
        collect_with(
            selected,
            &roots,
            facts,
            &[],
            true,
            None,
            control,
            limits,
            observer,
        )
    }
    fn fact(path: &str, target: &str) -> index::LoadFact {
        index::LoadFact {
            path: path.into(),
            target: target.into(),
            line: 1,
            kind: "load".into(),
            source_blob: Value::Null,
        }
    }
    fn run(selected: &str, roots: &[&str], facts: &[index::LoadFact]) -> Selection {
        test_collect(
            selected,
            roots.iter().copied(),
            facts,
            &OperationControl::default(),
        )
        .unwrap()
    }
    fn dag(depth: usize) -> Vec<index::LoadFact> {
        let mut facts = vec![fact("Main.toc", "n0.xml")];
        for n in 0..depth {
            for _ in 0..2 {
                facts.push(fact(&format!("n{n}.xml"), &format!("n{}.xml", n + 1)));
            }
        }
        facts
    }
    #[test]
    fn flow_load_dag_preserves_exact_repeated_depth_first_sequence() {
        let facts = vec![
            fact("Main.toc", "a.xml"),
            fact("Main.toc", "b.xml"),
            fact("a.xml", "shared.xml"),
            fact("a.xml", "a.lua"),
            fact("b.xml", "shared.xml"),
            fact("b.xml", "b.lua"),
            fact("shared.xml", "selected.lua"),
        ];
        let result = run("selected.lua", &["Main.toc"], &facts);
        assert_eq!(
            result.ordered.unwrap(),
            ["selected.lua", "a.lua", "selected.lua", "b.lua"]
        );
        assert!(result.input_gaps.is_empty());
    }
    #[test]
    fn flow_load_cycle_marks_only_active_ancestors_and_keeps_later_occurrences() {
        let facts = vec![
            fact("Main.toc", "a.xml"),
            fact("Main.toc", "a.xml"),
            fact("a.xml", "a.xml"),
            fact("a.xml", "selected.lua"),
        ];
        let result = run("selected.lua", &["Main.toc"], &facts);
        assert!(result.ordered.is_none());
        assert_eq!(
            result.load_evidence["candidates"][0]["ordered"],
            json!(["selected.lua", "selected.lua"])
        );
        assert_eq!(
            result.load_evidence["candidates"][0]["issues"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn flow_load_empty_leaf_dag_stops_at_total_work_budget() {
        let result = run("never.lua", &["Main.toc"], &dag(30));
        assert!(result.ordered.is_none());
        assert_eq!(result.load_evidence["budget"]["work"], WORK);
        assert_eq!(result.load_evidence["budget"]["exhausted"], true);
        assert!(
            result
                .input_gaps
                .iter()
                .any(|gap| gap["code"] == "load_budget")
        );
    }
    #[test]
    fn flow_load_total_work_budget_is_shared_across_manifests() {
        let roots = (0..WORK).map(|n| format!("{n}.toc")).collect::<Vec<_>>();
        let result = test_collect(
            "never.lua",
            roots.iter().map(String::as_str),
            &[],
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(result.load_evidence["budget"]["work"], WORK);
        assert_eq!(result.load_evidence["budget"]["exhausted"], true);
    }
    #[test]
    fn flow_load_deep_chain_uses_default_thread_stack_and_completes_exactly() {
        std::thread::spawn(|| {
            let mut facts = vec![fact("Main.toc", "0.xml")];
            for n in 0..10_000 {
                facts.push(fact(&format!("{n}.xml"), &format!("{}.xml", n + 1)));
            }
            facts.push(fact("10000.xml", "selected.lua"));
            let result = run("selected.lua", &["Main.toc"], &facts);
            assert_eq!(result.ordered.unwrap(), ["selected.lua"]);
        })
        .join()
        .unwrap();
    }
    #[test]
    fn flow_load_original_cancellation_interrupts_non_lua_traversal() {
        let token = Arc::new(AtomicBool::new(false));
        let control =
            OperationControl::new(Instant::now() + Duration::from_secs(60), token.clone());
        let facts = dag(30);
        let error = test_collect_with(
            "never.lua",
            ["Main.toc"],
            &facts,
            &control,
            Limits::default(),
            |work| {
                if work > facts.len() + 100 {
                    token.store(true, Ordering::Release);
                }
            },
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "command.cancelled");
    }
    #[test]
    fn flow_load_expired_original_deadline_precedes_budget_or_partial_success() {
        let error = test_collect(
            "never.lua",
            ["Main.toc"],
            &dag(30),
            &OperationControl::uncancelable(Instant::now()),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "command.deadline");
    }
    #[test]
    fn flow_load_occurrence_budget_cannot_hide_an_ambiguous_unseen_candidate() {
        let mut facts = vec![fact("First.toc", "selected.lua")];
        for _ in 0..257 {
            facts.push(fact("Second.toc", "other.lua"));
        }
        facts.push(fact("Second.toc", "selected.lua"));
        let result = run("selected.lua", &["First.toc", "Second.toc"], &facts);
        assert!(result.ordered.is_none());
        assert_eq!(
            result.load_evidence["candidates"].as_array().unwrap().len(),
            1
        );
        assert!(
            !result.load_evidence["incompletePlans"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn flow_load_path_bytes_are_bounded_across_all_manifests() {
        let limits = Limits {
            paths: 32,
            ..Limits::default()
        };
        let result = test_collect_with(
            "x.lua",
            ["One.toc", "Two.toc"],
            &[
                fact("One.toc", "x.lua"),
                fact("Two.toc", "longer-than-remaining.lua"),
            ],
            &OperationControl::default(),
            limits,
            |_| {},
        )
        .unwrap();
        assert!(result.ordered.is_none());
        assert_eq!(result.load_evidence["budget"]["exhausted"], true);
        assert!(
            result.load_evidence["budget"]["pathBytes"]
                .as_u64()
                .unwrap()
                <= 32
        );
    }
    #[test]
    fn flow_load_issue_count_and_oversized_path_details_are_bounded() {
        let facts = (0..300)
            .map(|_| fact("Main.toc", &"x".repeat(8193)))
            .collect::<Vec<_>>();
        let result = run("x.lua", &["Main.toc"], &facts);
        let issues = result.load_evidence["incompletePlans"][0]["issues"]
            .as_array()
            .unwrap();
        assert_eq!(issues.len(), ISSUES + 1);
        assert!(
            issues
                .iter()
                .all(|issue| issue["detail"].as_str().unwrap().len() <= PATH_LIMIT + 64)
        );
        assert!(
            result.load_evidence["budget"]["issueBytes"]
                .as_u64()
                .unwrap()
                <= ISSUE_BYTES as u64
        );
    }
    #[test]
    fn flow_load_invalid_and_overlong_relative_paths_are_explicit_gaps() {
        for target in [
            "../outside.lua",
            "/absolute.lua",
            "C:\\x.lua",
            "x\0.lua",
            &"x".repeat(4097),
        ] {
            let result = run("x.lua", &["Main.toc"], &[fact("Main.toc", target)]);
            assert!(result.ordered.is_none());
            assert_eq!(
                result.load_evidence["incompletePlans"][0]["issues"][0]["code"],
                "load_path_unavailable"
            );
        }
        assert_eq!(
            resolve("dir/Main.toc", ".\\x.lua"),
            Some("dir/x.lua".into())
        );
        assert!(resolve(&format!("{}/Main.toc", "x".repeat(4090)), "x.lua").is_none());
    }
    #[test]
    fn flow_load_default_occurrence_boundary_keeps_256_and_reports_the_257th() {
        for count in [256, 257] {
            let facts = (0..count)
                .map(|_| fact("Main.toc", "selected.lua"))
                .collect::<Vec<_>>();
            let result = run("selected.lua", &["Main.toc"], &facts);
            assert_eq!(
                result.load_evidence["candidates"][0]["ordered"]
                    .as_array()
                    .unwrap()
                    .len(),
                256
            );
            assert_eq!(result.ordered.is_some(), count == 256);
        }
    }
}

#[cfg(test)]
#[path = "conditional_tests.rs"]
mod conditional_tests;
