use lycheedev_emmy_adapter::{
    ByteRange, CallableInput, DeclarationLookup, DeclarationModel, OriginInput,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn selected_callback_signature_is_known_and_stays_query_local() {
    let source = "---@param seconds number\n---@param callback number\nfunction C_Timer.After(seconds, callback) end\n";
    let model = DeclarationModel::new(Arc::from(source), || Ok::<(), ()>(())).unwrap();
    let start = source.find("After(seconds").unwrap() as u32;
    let record = json!({"kind":"function","name":"C_Timer.After","parameters":[
        {"index":1,"name":"seconds","type":"number"},{"index":2,"name":"callback","type":"TimerCallback"}],"returns":[]});
    let callback = json!({"kind":"callbacktype","name":"TimerCallback","parameters":[],"returns":[],"path":"UITimerDocumentation.lua","line":10});
    let aliases = BTreeMap::new();
    let run = |unique: bool| {
        model.type_at_with_declarations(ByteRange{start,end:start+5},"query",CallableInput{
        origin:OriginInput{material:"api",material_sha256:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",pointer:"/bindings/0/descriptor",identity:"admitted"},
        declaration:&record,catalog_identity:"admitted-catalog",aliases:&aliases,
    },Arc::new(||Ok::<(),()>(())),|_|"cancelled".into(),||Ok(()),|name|{
        assert_eq!(name,"TimerCallback");
        Ok(if unique{DeclarationLookup::Unique(callback.clone())}else{DeclarationLookup::Ambiguous})
    }).unwrap().unwrap()
    };
    let known = run(true);
    assert!(known.known, "{known:?}");
    assert!(
        known.rendered.contains("seconds: number"),
        "{}",
        known.rendered
    );
    assert!(
        known.rendered.contains("callback: fun"),
        "{}",
        known.rendered
    );
    assert_eq!(known.declarations[0].catalog_identity, "admitted-catalog");
    assert_eq!(known.declarations[0].path, "UITimerDocumentation.lua");
    let ambiguous = run(false);
    assert!(!ambiguous.known);
    assert!(
        ambiguous
            .coverage
            .iter()
            .any(|row| row.reason.as_deref() == Some("environment_declaration_ambiguous"))
    );
    assert!(
        !ambiguous.rendered.contains("number"),
        "raw inference leaked"
    );
    assert_eq!(known.rendered, run(true).rendered);
}

#[test]
fn non_timer_callback_names_are_resolved_by_unique_declarations() {
    let source = "function Use(value) end\n";
    let model = DeclarationModel::new(Arc::from(source), || Ok::<(), ()>(())).unwrap();
    let start = source.find("Use(value)").unwrap() as u32;
    let record = json!({"kind":"function","name":"Use","parameters":[{"index":1,"name":"value","type":"Completion"}],"returns":[]});
    let aliases = BTreeMap::from([("Completion".into(), "ArbitraryCallback".into())]);
    let result=model.type_at_with_declarations(ByteRange{start,end:start+3},"other-query",CallableInput{
        origin:OriginInput{material:"api",material_sha256:"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",pointer:"/binding",identity:"other"},
        declaration:&record,catalog_identity:"other-catalog",aliases:&aliases,
    },Arc::new(||Ok::<(),()>(())),|_|"cancelled".into(),||Ok(()),|name|{
        assert_eq!(name,"ArbitraryCallback");
        Ok(DeclarationLookup::Unique(json!({"kind":"callback","name":name,"parameters":[],"returns":[{"index":1,"name":"done","type":"boolean"}]})))
    }).unwrap().unwrap();
    assert!(result.known, "{result:?}");
    assert_eq!(result.declarations.len(), 1);
    assert_eq!(
        serde_json::to_value(&result.declarations).unwrap()[0]["name"],
        Value::String("ArbitraryCallback".into())
    );
}
