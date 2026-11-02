//! Fixtures exercise the actual owned backend and fixed-engine member paths.
#![allow(dead_code)]
pub use super::object_support::*;
use emmylua_code_analysis::EmmyLuaAnalysis;
use lycheedev_emmy_adapter::{ByteRange, session::*};
pub fn member_case(
    source: &str,
    query: &str,
) -> (EmmyLuaAnalysis, BindingSite, Provider, ObjectRef) {
    let (analysis, query, mut provider) = load(source, query);
    let selected = provider.named("selected");
    let object = object(selected, 1);
    select_shape(
        &mut provider,
        selected,
        &object,
        shape(
            &object,
            vec![
                field("z", FieldPresence::Present, absent("z")),
                field("omitted", FieldPresence::Absent, absent("omitted")),
                field("a", FieldPresence::Present, environment_any()),
                field("uncertain", FieldPresence::Unavailable, absent("ignored")),
            ],
        ),
    );
    (analysis, query, provider, object)
}
pub fn lua_destination(
    analysis: &EmmyLuaAnalysis,
    site: BindingSite,
    key: &str,
) -> FieldWriteOrigin {
    let model = analysis.semantic_model(site.file_id);
    let member = model
        .file_facts()
        .unwrap()
        .members
        .iter()
        .find(|m| m.key.name() == Some(key))
        .unwrap();
    let range = member.id.member_key_range().unwrap();
    FieldWriteOrigin::Lua(SourceSpan {
        file_id: site.file_id,
        range: ByteRange {
            start: range.start().into(),
            end: range.end().into(),
        },
    })
}
pub fn replace_fields(provider: &mut Provider, object: &ObjectRef, fields: Vec<ObjectField>) {
    provider.shapes.insert(
        object.view.clone(),
        BindingDecision::Bound(shape(object, fields)),
    );
}
