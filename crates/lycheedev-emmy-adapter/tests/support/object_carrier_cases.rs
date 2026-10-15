//! Object evidence fixtures for the concrete query; no semantic backend double.
#![allow(dead_code)]
pub use super::query_support::*;
use emmylua_code_analysis::EmmyLuaAnalysis;
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaClosureExpr};
use lycheedev_emmy_adapter::{ByteRange, session::*};
use std::sync::Arc;
pub fn object(site: BindingSite, id: usize) -> ObjectRef {
    ObjectRef {
        identity: format!("object-{id}"),
        epoch: "epoch-1".into(),
        origin: ObjectOrigin::Lua(site),
        view: ObjectViewKey {
            context: "owned-query-backend-oracle".into(),
            observation: 7,
            object_id: id,
        },
    }
}
pub fn bound_object(object: &ObjectRef) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: object.identity.clone(),
        proof: format!("selected:{}", object.identity),
        value: SelectedValue::Object(object.clone()),
    })
}
pub fn xml() -> ExternalSourceSpan {
    ExternalSourceSpan {
        path: "fixture.xml".into(),
        content_sha256: "q".repeat(64),
        range: ByteRange { start: 50, end: 50 },
    }
}
pub fn field(
    name: &str,
    presence: FieldPresence,
    value: BindingDecision<BoundValue>,
) -> ObjectField {
    ObjectField {
        key: name.into(),
        presence,
        value,
        destination_write: FieldWriteOrigin::Xml(xml()),
        source_write: None,
        copy_origin: None,
    }
}
pub fn shape(object: &ObjectRef, fields: Vec<ObjectField>) -> ObjectShape {
    ObjectShape {
        key: object.view.clone(),
        fields,
        enumeration: ShapeEnumeration::Complete,
        proof: format!("shape:{}", object.identity),
    }
}
pub fn select_shape(
    provider: &mut Provider,
    site: BindingSite,
    object: &ObjectRef,
    shape: ObjectShape,
) {
    provider.values.insert(site, bound_object(object));
    provider
        .shapes
        .insert(object.view.clone(), BindingDecision::Bound(shape));
}
pub fn environment_any() -> BindingDecision<BoundValue> {
    let callable = Arc::new(EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "x".repeat(64),
            pointer: "/functions/0".into(),
            identity: "any-callable".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "Any".into(),
            returns: vec![EnvironmentSlot {
                index: 1,
                name: "value".into(),
                type_name: "any".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog {
            identity: "types".into(),
            ..Default::default()
        }),
    });
    BindingDecision::Bound(BoundValue {
        identity: "any-return".into(),
        proof: "any-proof".into(),
        value: SelectedValue::EnvironmentReturn(EnvironmentReturn {
            callable,
            slot: 1,
            invocation_identity: "invocation".into(),
        }),
    })
}
pub fn basic_case() -> (EmmyLuaAnalysis, BindingSite, Provider, ObjectRef) {
    let (analysis, site, mut provider) = load("return selected", "selected");
    let object = object(site, 1);
    let value = shape(
        &object,
        vec![field("nil", FieldPresence::Present, absent("field-nil"))],
    );
    select_shape(&mut provider, site, &object, value);
    (analysis, site, provider, object)
}
pub fn observed_call(
    analysis: &EmmyLuaAnalysis,
    provider: &mut Provider,
    call: BindingSite,
    returns: Vec<BindingDecision<BoundValue>>,
) {
    let model = analysis.semantic_model(call.file_id);
    let root = model.syntax_tree().unwrap().get_red_root();
    let expr = LuaCallExpr::cast(call.syntax.to_node_from_root(&root).unwrap()).unwrap();
    let callee = BindingSite {
        file_id: call.file_id,
        syntax: expr.get_prefix_expr().unwrap().get_syntax_id(),
    };
    let closure = root.descendants().find_map(LuaClosureExpr::cast).unwrap();
    let function = FunctionRef {
        file_id: call.file_id,
        closure: closure.get_syntax_id(),
        declaration: None,
    };
    provider.values.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "callee".into(),
            proof: "callee-proof".into(),
            value: SelectedValue::Function(function.clone()),
        }),
    );
    provider.calls.insert(
        call,
        BindingDecision::Bound(CallPermit {
            callee,
            proof: "permit".into(),
            capability_profile: "typed-call-v1".into(),
        }),
    );
    provider.observations.insert(
        call,
        BindingDecision::Bound(CallObservation {
            callee: ObservedCallee::Source(function),
            selected_callee_identity: "callee".into(),
            closure_identity: "closure".into(),
            invocation_identity: "call".into(),
            returns,
            proof: "observation".into(),
        }),
    );
}
