//! Real public ingestion, provider shapes and OwnedQuery consumers. Destination
//! source ids are deliberately different from the selected callable's source.
use super::super::{ExpressionValue, OwnedQuery, OwnedQueryFailure};
use crate::session::{
    BindingContext, BindingCoverage, BindingDecision, BindingSite, BoundValue, ExternalSourceSpan,
    FieldPresence, FieldWriteOrigin, FunctionRef, ObjectField, ObjectOrigin, ObjectRef,
    ObjectShape, ObjectViewKey, OwnedBindingSession, OwnedValue, SelectedValue, ShapeEnumeration,
    SourceSpan,
};
use crate::{ByteRange, IngestedBatch, ingest_public_batch};
use emmylua_code_analysis::{
    EmmyLuaAnalysis, LuaArrayType, LuaIntersectionType, LuaMemberKey, LuaObjectType, LuaType,
    SemanticId,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaClosureExpr, LuaExpr, LuaIndexExpr};
use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc};

const CONTEXT: &str = "selected-callable-members";
const MAIN: &str = "local target = {}\n---@overload fun(value: boolean): integer\n---@param value number\n---@return boolean\nfunction target.run(value) return false end\nlocal indexed = target.run\nreturn target.run('actual')\n";
const ACTUAL: &str = "---@param value string\n---@return string\nlocal function actual(value) return value end\nreturn actual\n";

#[derive(Clone, Default)]
struct Context {
    values: HashMap<BindingSite, BindingDecision<BoundValue>>,
    shapes: HashMap<ObjectViewKey, ObjectShape>,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        CONTEXT
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.values
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
    fn new() -> Self {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("owned-selected-callable-input");
        analysis.add_main_workspace(root.clone());
        let batch = ingest_public_batch(
            &mut analysis,
            vec![
                (root.join("main.lua"), Some(Arc::from(MAIN))),
                (root.join("actual.lua"), Some(Arc::from(ACTUAL))),
            ],
            1 << 20,
            || Ok::<_, ()>(()),
        )
        .unwrap();
        Self { analysis, batch }
    }
    fn sites(&self, text: &str) -> Vec<BindingSite> {
        let file_id = self.batch.file_ids[0];
        self.analysis
            .semantic_model(file_id)
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaExpr::cast)
            .filter(|expr| expr.syntax().text().to_string() == text)
            .map(|expr| BindingSite {
                file_id,
                syntax: expr.get_syntax_id(),
            })
            .collect()
    }
    fn site(&self, text: &str) -> BindingSite {
        self.sites(text).into_iter().last().unwrap()
    }
    fn call(&self) -> LuaCallExpr {
        let site = self.site("target.run('actual')");
        let model = self.analysis.semantic_model(site.file_id);
        LuaCallExpr::cast(
            site.syntax
                .to_node_from_root(&model.syntax_tree().unwrap().get_red_root())
                .unwrap(),
        )
        .unwrap()
    }
    fn raw_member(&self) -> (SemanticId, SemanticId, FieldWriteOrigin) {
        let model = self.analysis.semantic_model(self.batch.file_ids[0]);
        let member = model
            .file_facts()
            .unwrap()
            .members
            .iter()
            .find(|member| member.key == key())
            .unwrap();
        let range = member.id.member_key_range().unwrap();
        (
            member.id.clone(),
            member.owner.clone(),
            FieldWriteOrigin::Lua(SourceSpan {
                file_id: model.file_id(),
                range: ByteRange {
                    start: range.start().into(),
                    end: range.end().into(),
                },
            }),
        )
    }
    fn function(&self) -> BindingDecision<BoundValue> {
        let file_id = self.batch.file_ids[1];
        let closure = self
            .analysis
            .semantic_model(file_id)
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .find_map(LuaClosureExpr::cast)
            .unwrap();
        BindingDecision::Bound(BoundValue {
            identity: "actual-function".into(),
            proof: "actual-function-proof".into(),
            value: SelectedValue::Function(FunctionRef {
                file_id,
                closure: closure.get_syntax_id(),
                declaration: None,
            }),
        })
    }
    fn context(
        &self,
        value: BindingDecision<BoundValue>,
        presence: FieldPresence,
        lua: bool,
    ) -> (Context, ObjectRef) {
        let object = ObjectRef {
            identity: "actual-object".into(),
            epoch: "epoch-1".into(),
            origin: ObjectOrigin::Lua(self.site("{}")),
            view: ObjectViewKey {
                context: CONTEXT.into(),
                observation: 1,
                object_id: 1,
            },
        };
        let mut context = Context::default();
        for site in self.sites("target").into_iter().chain(self.sites("{}")) {
            context.values.insert(site, object_decision(&object));
        }
        context.shapes.insert(
            object.view.clone(),
            ObjectShape {
                key: object.view.clone(),
                fields: vec![ObjectField {
                    key: "run".into(),
                    presence,
                    value,
                    destination_write: if lua {
                        self.raw_member().2
                    } else {
                        xml_origin()
                    },
                    source_write: None,
                    copy_origin: None,
                }],
                enumeration: ShapeEnumeration::Complete,
                proof: "actual-shape-proof".into(),
            },
        );
        (context, object)
    }
    fn query<'a, 'c, E>(
        &'a self,
        session: &'a OwnedBindingSession,
        check: &'c mut dyn FnMut() -> Result<(), E>,
    ) -> OwnedQuery<'a, 'c, E> {
        OwnedQuery::new_with_contribution_order(
            &self.analysis,
            session,
            self.batch.file_ids[0],
            check,
            Arc::clone(&self.batch.contribution_order),
        )
    }
}
fn key() -> LuaMemberKey {
    LuaMemberKey::Name("run".into())
}
fn xml_origin() -> FieldWriteOrigin {
    FieldWriteOrigin::Xml(ExternalSourceSpan {
        path: "actual.xml".into(),
        content_sha256: "a".repeat(64),
        range: ByteRange { start: 10, end: 20 },
    })
}
fn object_decision(object: &ObjectRef) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: object.identity.clone(),
        proof: "object-proof".into(),
        value: SelectedValue::Object(object.clone()),
    })
}
fn absent() -> BindingDecision<BoundValue> {
    BindingDecision::KnownAbsent {
        proof: "actual-absent-proof".into(),
    }
}
fn unavailable() -> BindingDecision<BoundValue> {
    BindingDecision::Unavailable {
        proof: "actual-unavailable-proof".into(),
        reason: "not-observed".into(),
    }
}
fn session(context: Context) -> Arc<OwnedBindingSession> {
    OwnedBindingSession::new_checked(Arc::new(context), || Ok::<_, ()>(())).unwrap()
}
fn rows(session: &OwnedBindingSession) -> Vec<BindingCoverage> {
    session.coverage_checked(|| Ok::<_, ()>(())).unwrap()
}
fn assert_actual(function: &OwnedValue) {
    assert_eq!(
        function.function_return().unwrap().structural_type(),
        &LuaType::String
    );
    let params = function.function_parameters().unwrap();
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0].as_ref().unwrap().structural_type(),
        &LuaType::String
    );
}
fn inherited(value: OwnedValue, owner: SemanticId) -> ExpressionValue<OwnedValue> {
    ExpressionValue {
        value,
        owner: Some(owner),
        closure_syntax: None,
        receiver: None,
    }
}

#[test]
fn lua_and_xml_destinations_use_the_actual_cross_file_callable() {
    let fixture = Fixture::new();
    for lua in [true, false] {
        let (context, _) = fixture.context(fixture.function(), FieldPresence::Present, lua);
        let session = session(context);
        let mut check = || Ok::<_, ()>(());
        let mut query = fixture.query(&session, &mut check);
        let selected = query.type_of_expr(fixture.site("target").syntax).unwrap();
        let members = query.selected_members(&selected).unwrap().unwrap();
        assert_eq!(members.len(), 1);
        let (raw, _, _) = fixture.raw_member();
        assert_eq!(members[0].id.as_ref(), lua.then_some(&raw));
        assert_actual(&members[0].typ);
        let candidates = query
            .callable_candidates_from_prefix(&selected, &key(), Some(&raw))
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_actual(&candidates[0]);
        let call = fixture.call();
        let callee = call.get_prefix_expr().unwrap();
        let candidates = query.callable_candidates_cached(&callee).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_actual(&candidates[0]);
        let resolved = query
            .resolve_owned_member(&LuaIndexExpr::cast(callee.syntax().clone()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(resolved.member_id.as_ref(), lua.then_some(&raw));
        assert_actual(&query.resolved_member_type(&resolved).unwrap().unwrap());
        let result = query.type_of_expr(call.get_syntax_id()).unwrap();
        assert_eq!(result.structural_type(), &LuaType::String);
        assert!(
            rows(&session)
                .iter()
                .any(|row| row.operation == "member_lookup"
                    && row.state == "bound"
                    && row.proof == "actual-shape-proof")
        );
    }
}

#[test]
fn absent_and_unavailable_fields_never_resurrect_destination_callables() {
    let fixture = Fixture::new();
    for lua in [true, false] {
        for (value, presence, expected) in [
            (absent(), FieldPresence::Present, LuaType::Nil),
            (absent(), FieldPresence::Absent, LuaType::Nil),
            (unavailable(), FieldPresence::Present, LuaType::Unknown),
            (
                fixture.function(),
                FieldPresence::Unavailable,
                LuaType::Unknown,
            ),
        ] {
            let (context, _) = fixture.context(value, presence, lua);
            let session = session(context);
            let mut check = || Ok::<_, ()>(());
            let mut query = fixture.query(&session, &mut check);
            let selected = query.type_of_expr(fixture.site("target").syntax).unwrap();
            let (raw, owner, _) = fixture.raw_member();
            assert!(
                query
                    .callable_candidates_from_prefix(&selected, &key(), Some(&raw))
                    .unwrap()
                    .is_empty()
            );
            let indexed = query
                .index_member_general(inherited(selected, owner), &key())
                .unwrap();
            assert_eq!(indexed.value.structural_type(), &expected);
            assert!(indexed.owner.is_none());
            let call = fixture.call();
            assert!(
                query
                    .callable_candidates_cached(&call.get_prefix_expr().unwrap())
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                query
                    .type_of_expr(call.get_syntax_id())
                    .unwrap()
                    .structural_type(),
                &LuaType::Unknown
            );
        }
    }
}

#[test]
fn inherited_selected_object_does_not_reuse_its_old_execution_owner() {
    let fixture = Fixture::new();
    let (context, _) = fixture.context(fixture.function(), FieldPresence::Present, true);
    let session = session(context);
    let mut check = || Ok::<_, ()>(());
    let mut query = fixture.query(&session, &mut check);
    let selected = query.type_of_expr(fixture.site("{}").syntax).unwrap();
    let (_, owner, _) = fixture.raw_member();
    let indexed = query
        .index_member_general(inherited(selected.clone(), owner), &key())
        .unwrap();
    assert_actual(&indexed.value);
    assert!(indexed.owner.is_none());
    assert_eq!(indexed.receiver, Some(selected));
    let candidates = query.owned_callable_candidates(&indexed).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_actual(&candidates[0]);
}

#[test]
fn selected_composite_members_do_not_merge_destination_signatures() {
    let fixture = Fixture::new();
    let (context, _) = fixture.context(fixture.function(), FieldPresence::Present, true);
    let session = session(context);
    let mut check = || Ok::<_, ()>(());
    let mut query = fixture.query(&session, &mut check);
    let selected = query.type_of_expr(fixture.site("{}").syntax).unwrap();
    let (raw, owner, _) = fixture.raw_member();
    let array = OwnedValue::with_structural_children(
        LuaType::Array(Arc::new(LuaArrayType::from_base_type(
            selected.structural_type().clone(),
        ))),
        vec![selected.clone()],
    );
    let intersection = OwnedValue::with_structural_children(
        LuaType::Intersection(Arc::new(LuaIntersectionType::new(vec![
            selected.structural_type().clone(),
            selected.structural_type().clone(),
        ]))),
        vec![selected.clone(), selected.clone()],
    );
    let union = OwnedValue::raw_union(vec![selected.clone(), OwnedValue::plain(LuaType::Nil)]);
    for value in [array, intersection, union] {
        let candidates = query
            .callable_candidates_from_prefix(&value, &key(), Some(&raw))
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_actual(&candidates[0]);
        // The diagnostic candidate entry must also accept a value-level
        // composite answer when there is no unique source declaration id.
        let callee = fixture.call().get_prefix_expr().unwrap();
        let index = LuaIndexExpr::cast(callee.syntax().clone()).unwrap();
        let prefix = index.get_prefix_expr().unwrap();
        query.expressions.borrow_mut().insert(
            prefix.get_syntax_id(),
            super::super::CacheEntry::Ready(value.clone()),
        );
        query.resolved_members.clear();
        let access = query.callable_candidates_for_expr(&callee).unwrap();
        assert_eq!(access.len(), 1);
        assert_actual(&access[0]);
        let indexed = query
            .index_member_general(inherited(value, owner.clone()), &key())
            .unwrap();
        assert_actual(&indexed.value);
        assert!(indexed.owner.is_none());
    }
    // Preserve the existing conflicting-intersection result even when only one
    // member happens to be callable. A singleton candidate is not a new type.
    let scalar = OwnedValue::plain(LuaType::Object(Arc::new(LuaObjectType::new_with_fields(
        HashMap::from([(key(), LuaType::Number)]),
        Vec::new(),
    ))));
    let conflict = OwnedValue::with_structural_children(
        LuaType::Intersection(Arc::new(LuaIntersectionType::new(vec![
            selected.structural_type().clone(),
            scalar.structural_type().clone(),
        ]))),
        vec![scalar, selected],
    );
    let indexed = query
        .index_member_general(inherited(conflict, owner), &key())
        .unwrap();
    assert_eq!(indexed.value.structural_type(), &LuaType::Never);
}

#[test]
fn selected_member_initializers_are_authoritative_before_raw_overloads() {
    let fixture = Fixture::new();
    let (raw, _, _) = fixture.raw_member();
    let syntax = fixture
        .analysis
        .semantic_model(fixture.batch.file_ids[0])
        .file_facts()
        .unwrap()
        .member_by_id(&raw)
        .unwrap()
        .value_syntax
        .unwrap();
    for (decision, callable) in [
        (fixture.function(), true),
        (absent(), false),
        (unavailable(), false),
    ] {
        let context = Context {
            values: HashMap::from([(
                BindingSite {
                    file_id: fixture.batch.file_ids[0],
                    syntax,
                },
                decision,
            )]),
            shapes: HashMap::new(),
        };
        let session = session(context);
        let mut check = || Ok::<_, ()>(());
        let mut query = fixture.query(&session, &mut check);
        let candidates = query
            .callable_candidates_for_owner_single(&raw)
            .unwrap()
            .unwrap();
        assert_eq!(candidates.len(), usize::from(callable));
        if callable {
            assert_actual(&candidates[0]);
        }
        assert_eq!(
            query.callable_candidates_for_owner(&raw).unwrap(),
            Some(candidates.clone())
        );
        let prefix = query.type_of_expr(fixture.site("target").syntax).unwrap();
        assert_eq!(
            query
                .callable_candidates_from_prefix(&prefix, &key(), Some(&raw))
                .unwrap(),
            candidates
        );
    }
}

#[test]
fn selected_members_reject_foreign_session_and_conflicting_epoch() {
    let fixture = Fixture::new();
    let (context, object) = fixture.context(fixture.function(), FieldPresence::Present, true);
    let first = session(context.clone());
    let second = session(context.clone());
    let mut check = || Ok::<_, ()>(());
    let selected = fixture
        .query(&first, &mut check)
        .type_of_expr(fixture.site("{}").syntax)
        .unwrap();
    let mut check = || Ok::<_, ()>(());
    let mut foreign = fixture.query(&second, &mut check);
    let (raw, owner, _) = fixture.raw_member();
    assert!(
        foreign
            .callable_candidates_from_prefix(&selected, &key(), Some(&raw))
            .unwrap()
            .is_empty()
    );
    let indexed = foreign
        .index_member_general(inherited(selected, owner), &key())
        .unwrap();
    assert_eq!(indexed.value.structural_type(), &LuaType::Unknown);
    assert!(indexed.owner.is_none());
    assert!(
        rows(&second)
            .iter()
            .any(|row| row.operation == "member_lookup"
                && row.state == "unavailable"
                && row.reason.as_deref() == Some("object_context_mismatch"))
    );

    let mut context = context;
    let mut changed = object;
    changed.epoch = "epoch-2".into();
    for site in fixture.sites("target") {
        context.values.insert(site, object_decision(&changed));
    }
    let session = session(context);
    let mut check = || Ok::<_, ()>(());
    let mut query = fixture.query(&session, &mut check);
    assert!(
        query
            .type_of_expr(fixture.site("{}").syntax)
            .unwrap()
            .object_info()
            .is_some()
    );
    let call = fixture.call();
    assert_eq!(
        query
            .type_of_expr(call.get_syntax_id())
            .unwrap()
            .structural_type(),
        &LuaType::Unknown
    );
    assert!(
        query
            .callable_candidates_cached(&call.get_prefix_expr().unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(rows(&session).iter().any(|row| row.operation == "resolve"
        && row.state == "unavailable"
        && row.reason.as_deref() == Some("object_frozen_identity_mismatch")));
}

#[test]
fn missing_members_distinguish_complete_incomplete_and_known_absence() {
    let fixture = Fixture::new();
    for (enumeration, keep_absent, expected) in [
        (ShapeEnumeration::Complete, false, LuaType::Nil),
        (
            ShapeEnumeration::Unavailable("partial".into()),
            false,
            LuaType::Unknown,
        ),
        (
            ShapeEnumeration::Unavailable("partial".into()),
            true,
            LuaType::Nil,
        ),
    ] {
        let (mut context, object) = fixture.context(absent(), FieldPresence::Absent, true);
        let shape = context.shapes.get_mut(&object.view).unwrap();
        shape.enumeration = enumeration;
        if !keep_absent {
            shape.fields.clear();
        }
        let session = session(context);
        let mut check = || Ok::<_, ()>(());
        let mut query = fixture.query(&session, &mut check);
        let selected = query.type_of_expr(fixture.site("{}").syntax).unwrap();
        let (_, owner, _) = fixture.raw_member();
        let indexed = query
            .index_member_general(inherited(selected, owner), &key())
            .unwrap();
        assert_eq!(indexed.value.structural_type(), &expected);
        assert!(indexed.owner.is_none());
    }
}

#[test]
fn unmanaged_destination_overload_priority_is_unchanged() {
    let fixture = Fixture::new();
    let session = session(Context::default());
    let mut check = || Ok::<_, ()>(());
    let mut query = fixture.query(&session, &mut check);
    let (raw, _, _) = fixture.raw_member();
    let expected = query
        .callable_candidates_for_owner_single(&raw)
        .unwrap()
        .unwrap();
    assert_eq!(expected.len(), 2);
    assert_eq!(
        expected[0].function_return().unwrap().structural_type(),
        &LuaType::Integer
    );
    assert_eq!(
        expected[1].function_return().unwrap().structural_type(),
        &LuaType::Boolean
    );
    let prefix = query.type_of_expr(fixture.site("target").syntax).unwrap();
    let actual = query
        .callable_candidates_from_prefix(&prefix, &key(), Some(&raw))
        .unwrap();
    assert_eq!(actual, expected);
    assert!(rows(&session).is_empty());
}

#[test]
fn selected_member_control_failure_preserves_error_and_can_retry() {
    let fixture = Fixture::new();
    let (context, _) = fixture.context(fixture.function(), FieldPresence::Present, true);
    let session = session(context);
    let remaining = Rc::new(Cell::new(None::<usize>));
    let mut check = {
        let remaining = Rc::clone(&remaining);
        move || match remaining.get() {
            None => Ok(()),
            Some(0) => {
                remaining.set(None);
                Err("original-deadline")
            }
            Some(n) => {
                remaining.set(Some(n - 1));
                Ok(())
            }
        }
    };
    let mut query = fixture.query(&session, &mut check);
    let selected = query.type_of_expr(fixture.site("target").syntax).unwrap();
    let (raw, _, _) = fixture.raw_member();
    remaining.set(Some(3));
    assert!(matches!(
        query.callable_candidates_from_prefix(&selected, &key(), Some(&raw)),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
    assert!(
        !rows(&session)
            .iter()
            .any(|row| row.operation == "member_lookup")
    );
    let candidates = query
        .callable_candidates_from_prefix(&selected, &key(), Some(&raw))
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_actual(&candidates[0]);
}
