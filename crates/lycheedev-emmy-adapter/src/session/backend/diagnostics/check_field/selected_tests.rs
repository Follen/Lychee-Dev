//! Real public ingestion and owned diagnostic-registry regressions. No engine
//! patch, backend double, or synthetic completion receipt participates.
use crate::session::{
    BindingContext, BindingCoverage, BindingDecision, BindingSite, BoundValue, CallPermit,
    Diagnostic, DiagnosticRun, ExternalSourceSpan, FieldPresence, FieldWriteOrigin, ObjectField,
    ObjectOrigin, ObjectRef, ObjectShape, ObjectViewKey, OwnedBindingSession, OwnedQuery,
    SelectedValue, ShapeEnumeration,
};
use crate::{ByteRange, IngestedBatch, ingest_public_batch};
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, EmmyLuaAnalysis, Emmyrc};
use emmylua_parser::{LuaAstNode, LuaExpr};
use std::{collections::HashMap, sync::Arc};

const IDENTITY: &str = "owned-field-checks";
const CLASS: &str = "---@class Wanted\n---@field required number\nlocal Wanted={}\n";

#[derive(Default)]
struct Context {
    values: HashMap<BindingSite, BindingDecision<BoundValue>>,
    shapes: HashMap<ObjectViewKey, ObjectShape>,
    calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        IDENTITY
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.values
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        self.calls
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn object_shape(&self, key: &ObjectViewKey) -> BindingDecision<ObjectShape> {
        self.shapes
            .get(key)
            .cloned()
            .map(BindingDecision::Bound)
            .unwrap_or(BindingDecision::NotManaged)
    }
}
struct Fixture {
    analysis: EmmyLuaAnalysis,
    batch: IngestedBatch,
}
impl Fixture {
    fn new(sources: &[(&str, &str)]) -> Self {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("owned-selected-field-input");
        analysis.add_main_workspace(root.clone());
        let batch = ingest_public_batch(
            &mut analysis,
            sources
                .iter()
                .map(|(name, source)| (root.join(name), Some(Arc::from(*source))))
                .collect(),
            32 << 20,
            || Ok::<_, ()>(()),
        )
        .unwrap();
        Self { analysis, batch }
    }
    fn site(&self, file: usize, text: &str) -> BindingSite {
        let file_id = self.batch.file_ids[file];
        let expr = self
            .analysis
            .semantic_model(file_id)
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaExpr::cast)
            .find(|expr| expr.syntax().text().to_string() == text)
            .unwrap_or_else(|| panic!("missing expression {text}"));
        BindingSite {
            file_id,
            syntax: expr.get_syntax_id(),
        }
    }
    fn diagnose(&self, file: usize, provider: Context) -> (DiagnosticRun, Vec<BindingCoverage>) {
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider), || Ok::<_, ()>(())).unwrap();
        let mut check = || Ok::<_, ()>(());
        let mut query = OwnedQuery::new_with_contribution_order(
            &self.analysis,
            &session,
            self.batch.file_ids[file],
            &mut check,
            Arc::clone(&self.batch.contribution_order),
        );
        let mut config = Emmyrc::default();
        config
            .diagnostics
            .enables
            .extend([DiagnosticCode::MissingFields, DiagnosticCode::InjectField]);
        let mut run = DiagnosticRun::default();
        query
            .diagnose_into(Arc::new(CheckConfig::new(&config)), &mut run)
            .unwrap();
        assert!(run.complete);
        let rows = session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
        (run, rows)
    }
}
fn fields(run: &DiagnosticRun) -> Vec<&Diagnostic> {
    run.diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code,
                DiagnosticCode::MissingFields | DiagnosticCode::InjectField
            )
        })
        .collect()
}
fn expression(site: BindingSite) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: "actual-expression".into(),
        proof: "expression-proof".into(),
        value: SelectedValue::Expression(site),
    })
}
fn absent() -> BindingDecision<BoundValue> {
    BindingDecision::KnownAbsent {
        proof: "absent-proof".into(),
    }
}
fn unavailable() -> BindingDecision<BoundValue> {
    BindingDecision::Unavailable {
        proof: "unavailable-proof".into(),
        reason: "unknown-field".into(),
    }
}
fn field(name: &str, presence: FieldPresence, value: BindingDecision<BoundValue>) -> ObjectField {
    ObjectField {
        key: name.into(),
        presence,
        value,
        destination_write: FieldWriteOrigin::Xml(ExternalSourceSpan {
            path: "selected.xml".into(),
            content_sha256: "a".repeat(64),
            range: ByteRange { start: 10, end: 20 },
        }),
        source_write: None,
        copy_origin: None,
    }
}
fn select_object(
    provider: &mut Context,
    site: BindingSite,
    fields: Vec<ObjectField>,
    enumeration: ShapeEnumeration,
) {
    let object = ObjectRef {
        identity: "selected-object".into(),
        epoch: "epoch-1".into(),
        origin: ObjectOrigin::Lua(site),
        view: ObjectViewKey {
            context: IDENTITY.into(),
            observation: 1,
            object_id: 1,
        },
    };
    provider.values.insert(
        site,
        BindingDecision::Bound(BoundValue {
            identity: object.identity.clone(),
            proof: "selected-proof".into(),
            value: SelectedValue::Object(object.clone()),
        }),
    );
    provider.shapes.insert(
        object.view.clone(),
        ObjectShape {
            key: object.view,
            fields,
            enumeration,
            proof: "shape-proof".into(),
        },
    );
}

#[test]
fn selected_table_fields_replace_raw_initializer_assignment_and_call_syntax() {
    for body in [
        "---@type Wanted\nlocal target={raw=9}\n",
        "---@type Wanted\nlocal target\ntarget={raw=9}\n",
        "---@param target Wanted\nlocal function F(target) end\nF({raw=9})\n",
    ] {
        let source = format!("{CLASS}local actual=7\n{body}");
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let site = fixture.site(0, "{raw=9}");
        let mut provider = Context::default();
        select_object(
            &mut provider,
            site,
            vec![field(
                "required",
                FieldPresence::Present,
                expression(fixture.site(0, "7")),
            )],
            ShapeEnumeration::Complete,
        );
        let (run, rows) = fixture.diagnose(0, provider);
        assert!(fields(&run).is_empty(), "{body}: {:?}", fields(&run));
        assert!(rows.iter().any(|row| row.site == site
            && row.operation == "member_lookup"
            && row.state == "bound"));
        assert!(rows.iter().any(|row| row.site == site
            && row.operation == "structure_check"
            && row.state == "native_checked"));
    }
}

#[test]
fn selected_complete_shape_reports_actual_keys_in_provider_order_at_use_site() {
    let source = format!("{CLASS}local actual=7\n---@type Wanted\nlocal target={{required=1}}\n");
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let site = fixture.site(0, "{required=1}");
    let mut provider = Context::default();
    select_object(
        &mut provider,
        site,
        vec![
            field(
                "second",
                FieldPresence::Present,
                expression(fixture.site(0, "7")),
            ),
            field(
                "first",
                FieldPresence::Present,
                expression(fixture.site(0, "7")),
            ),
            field("required", FieldPresence::Absent, absent()),
        ],
        ShapeEnumeration::Complete,
    );
    let (run, _) = fixture.diagnose(0, provider);
    let found = fields(&run);
    assert_eq!(
        found
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::InjectField,
            DiagnosticCode::InjectField,
            DiagnosticCode::MissingFields
        ]
    );
    assert_eq!(
        found[0].message,
        "Field `second` cannot be injected into the target type."
    );
    assert_eq!(
        found[1].message,
        "Field `first` cannot be injected into the target type."
    );
    assert_eq!(found[2].message, "Missing required fields: `required`");
    assert!(
        found
            .iter()
            .all(|diagnostic| diagnostic.range == site.syntax.get_range())
    );
}

#[test]
fn selected_nil_unknown_and_scalar_never_reconstruct_raw_table_fields() {
    let source = format!("{CLASS}local actual=7\n---@type Wanted\nlocal target={{raw=9}}\n");
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let site = fixture.site(0, "{raw=9}");
    for decision in [absent(), unavailable(), expression(fixture.site(0, "7"))] {
        let mut provider = Context::default();
        provider.values.insert(site, decision);
        let (run, rows) = fixture.diagnose(0, provider);
        assert!(fields(&run).is_empty(), "{:?}", fields(&run));
        assert!(
            rows.iter()
                .any(|row| row.site == site && row.operation == "resolve")
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.site == site && row.operation == "member_lookup")
        );
    }
}

#[test]
fn selected_incomplete_shapes_keep_real_unavailable_structure_evidence() {
    let source = format!("{CLASS}local actual=7\n---@type Wanted\nlocal target={{raw=9}}\n");
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let site = fixture.site(0, "{raw=9}");
    for (presence, value, enumeration) in [
        (
            FieldPresence::Present,
            expression(fixture.site(0, "7")),
            ShapeEnumeration::Unavailable("partial".into()),
        ),
        (
            FieldPresence::Unavailable,
            unavailable(),
            ShapeEnumeration::Complete,
        ),
        (
            FieldPresence::Present,
            unavailable(),
            ShapeEnumeration::Complete,
        ),
    ] {
        let mut provider = Context::default();
        select_object(
            &mut provider,
            site,
            vec![field("extra", presence, value)],
            enumeration,
        );
        let (run, rows) = fixture.diagnose(0, provider);
        assert!(fields(&run).is_empty(), "{:?}", fields(&run));
        assert!(rows.iter().any(|row| row.site == site
            && row.operation == "structure_check"
            && row.state == "unavailable"
            && row.reason.as_deref() == Some("object_context_or_shape_unavailable")));
        assert!(!rows.iter().any(|row| row.site == site
            && row.operation == "structure_check"
            && row.state == "native_checked"));
    }
}

#[test]
fn selected_expression_uses_other_file_table_members_and_original_diagnostic_range() {
    for (actual, expected) in [
        ("{required=7}", vec![]),
        (
            "{foreign=7}",
            vec![
                "Field `foreign` cannot be injected into the target type.",
                "Missing required fields: `required`",
            ],
        ),
    ] {
        let source = format!("{CLASS}---@type Wanted\nlocal target={{raw=9}}\n");
        let origin = format!("return {actual}\n");
        let fixture = Fixture::new(&[("main.lua", &source), ("origin.lua", &origin)]);
        let site = fixture.site(0, "{raw=9}");
        let mut provider = Context::default();
        provider
            .values
            .insert(site, expression(fixture.site(1, actual)));
        let (run, _) = fixture.diagnose(0, provider);
        let found = fields(&run);
        assert_eq!(
            found
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            found
                .iter()
                .all(|diagnostic| diagnostic.range == site.syntax.get_range())
        );
    }
}

#[test]
fn selected_index_assignment_requires_a_complete_shape_and_owned_member_lookup() {
    let fixture = Fixture::new(&[("main.lua", "local actual=7\nselected.required=1\n")]);
    let site = fixture.site(0, "selected");
    for (enumeration, present, expected_injections) in [
        (ShapeEnumeration::Complete, true, 0),
        (ShapeEnumeration::Complete, false, 1),
        (ShapeEnumeration::Unavailable("partial".into()), false, 0),
    ] {
        let mut provider = Context::default();
        let object_fields = if present {
            vec![field(
                "required",
                FieldPresence::Present,
                expression(fixture.site(0, "7")),
            )]
        } else {
            vec![]
        };
        select_object(&mut provider, site, object_fields, enumeration.clone());
        let (run, rows) = fixture.diagnose(0, provider);
        assert_eq!(
            fields(&run).len(),
            expected_injections,
            "{:?}",
            fields(&run)
        );
        assert!(rows.iter().any(|row| row.site == site
            && row.operation == "structure_check"
            && row.state
                == if matches!(enumeration, ShapeEnumeration::Complete) {
                    "native_checked"
                } else {
                    "unavailable"
                }));
    }
}

#[test]
fn ordinary_syntax_field_messages_order_and_ranges_are_unchanged() {
    let source = format!("{CLASS}---@type Wanted\nlocal target={{extra=1}}\n");
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let site = fixture.site(0, "{extra=1}");
    let (run, rows) = fixture.diagnose(0, Context::default());
    let found = fields(&run);
    assert_eq!(
        found
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "Field `extra` cannot be injected into the target type.",
            "Missing required fields: `required`"
        ]
    );
    assert!(
        found
            .iter()
            .all(|diagnostic| diagnostic.range == site.syntax.get_range())
    );
    assert!(rows.is_empty());
}

#[test]
fn absent_or_unavailable_calls_keep_argument_gate_proof_and_skip_raw_field_checks() {
    let source =
        format!("{CLASS}---@param target Wanted\nlocal function F(target) end\nF({{raw=9}})\n");
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let call = fixture.site(0, "F({raw=9})");
    for (decision, proof, reason) in [
        (
            BindingDecision::KnownAbsent {
                proof: "call-absent-proof".into(),
            },
            "call-absent-proof",
            "known_absent_callee",
        ),
        (
            BindingDecision::Unavailable {
                proof: "call-unavailable-proof".into(),
                reason: "selected-call-unavailable".into(),
            },
            "call-unavailable-proof",
            "selected-call-unavailable",
        ),
    ] {
        let mut provider = Context::default();
        provider.calls.insert(call, decision);
        let (run, rows) = fixture.diagnose(0, provider);
        assert!(fields(&run).is_empty(), "{:?}", fields(&run));
        assert!(rows.iter().any(|row| row.site == call
            && row.operation == "argument_check"
            && row.state == "unavailable"
            && row.proof == proof
            && row.reason.as_deref() == Some(reason)));
        assert!(!rows.iter().any(|row| row.site == call
            && row.operation == "argument_check"
            && row.state == "native_checked"));
    }
}

#[test]
fn selected_unresolved_named_types_are_unknown_shapes_not_empty_tables() {
    for name in ["Missing", "Missing<string>"] {
        let source = format!("{CLASS}---@type Wanted\nlocal target={{raw=9}}\n");
        let origin = format!("---@type {name}\nlocal actual\nreturn actual\n");
        let fixture = Fixture::new(&[("main.lua", &source), ("origin.lua", &origin)]);
        let site = fixture.site(0, "{raw=9}");
        let mut provider = Context::default();
        provider
            .values
            .insert(site, expression(fixture.site(1, "actual")));
        let (run, _) = fixture.diagnose(0, provider);
        assert!(fields(&run).is_empty(), "{name}: {:?}", fields(&run));
    }
}
