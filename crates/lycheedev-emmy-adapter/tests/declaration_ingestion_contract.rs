//! Constructor outcome parity plus exhaustive owned-checkpoint interruption.
//! Run unchanged on the old shared constructor and this public-input candidate.
//! Callback counts/stage placement intentionally differ; compare the complete
//! outcome rows, and retain each side's checkpoint rows separately.
use lycheedev_emmy_adapter::{
    ByteRange, CallableInput, DeclarationFailure, DeclarationModel, OriginInput,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug)]
struct Stop { ordinal: usize, token: Arc<()> }

fn outcome(source: &str) -> Value {
    let model = match DeclarationModel::new(Arc::from(source), || Ok::<_, Stop>(())) {
        Ok(model) => model,
        Err(DeclarationFailure::Parse(issue)) => return json!({"parse":{"count":issue.count,"first":{"kind":issue.first.kind,"range":{"start":issue.first.range.start,"end":issue.first.range.end}}}}),
        Err(DeclarationFailure::InvalidInput(reason)) => return json!({"invalid":reason}),
        Err(DeclarationFailure::Interrupted(_)) => panic!("non-failing check interrupted"),
    };
    let Some(start) = source.find("C_Test.Test") else { return json!({"created":true}); };
    let record = json!({"kind":"function","name":"C_Test.Test","parameters":[
        {"index":1,"name":"value","type":"string","nilable":false}],"returns":[],"raw":{}});
    let aliases = BTreeMap::new();
    let projected = model.type_at(
        ByteRange { start: start as u32, end: (start + "C_Test.Test".len()) as u32 },
        "ingestion/原始证据",
        CallableInput {
            origin: OriginInput { material:"api", material_sha256:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                pointer:"/bindings/0", identity:"verified" },
            declaration:&record, catalog_identity:"same-catalog", aliases:&aliases,
        },
        Arc::new(|| Ok::<_, Stop>(())), |_| "original-stop".into(), || Ok(()),
    ).unwrap().unwrap();
    json!({"known":projected.known,"rendered":projected.rendered,
        "coverage":projected.coverage,"declarations":projected.declarations})
}

fn fixtures() -> Vec<(&'static str, String)> {
    let source = "---@param value number\nfunction C_Test.Test(value) end\n";
    // Make multi-byte characters straddle the proposed 16-KiB copy boundary.
    let crossing = format!("--{}界🙂\r\n{}", "x".repeat(16_381), source);
    vec![
        ("empty", String::new()),
        ("selected-string", source.into()),
        ("unicode-crlf", format!("-- 界🙂\r\n{}", source.replace('\n', "\r\n"))),
        ("chunk-boundary", crossing),
        ("malformed", "function Broken(\n".into()),
        ("malformed-after-chunk", format!("--{}\nfunction Broken(\n", "x".repeat(16_385))),
    ]
}

#[test]
fn constructor_preserves_full_outputs_parse_ranges_and_unicode_boundaries() {
    for (name, source) in fixtures() {
        let row = outcome(&source);
        if name.contains("malformed") { assert!(row.get("parse").is_some()); }
        if name == "selected-string" || name == "chunk-boundary" || name == "unicode-crlf" {
            assert_eq!(row["known"], true);
            assert!(row["rendered"].as_str().unwrap().contains("string"));
            assert_eq!(row["coverage"][0]["proof"], "ingestion/原始证据");
        }
        println!("\nDECLARATION_INGESTION_OUTCOME {}", json!({"case":name,"outcome":row}));
    }
}

#[test]
fn every_constructor_checkpoint_returns_the_exact_original_error_once() {
    for (name, source) in fixtures() {
        let mut total = 0;
        let _ = DeclarationModel::new(Arc::from(source.as_str()), || { total += 1; Ok::<_, Stop>(()) });
        assert!(total > 1);
        for persistent in [false, true] {
            for stop in 1..=total {
                let token = Arc::new(());
                let mut seen = 0;
                let result = DeclarationModel::new(Arc::from(source.as_str()), || {
                    seen += 1;
                    if seen == stop || persistent && seen > stop {
                        Err(Stop { ordinal:seen, token:token.clone() })
                    } else { Ok(()) }
                });
                match result {
                    Err(DeclarationFailure::Interrupted(error)) => {
                        assert_eq!(error.ordinal, stop);
                        assert!(Arc::ptr_eq(&error.token, &token));
                    }
                    _ => panic!("{name} stop {stop} returned a model or non-interruption"),
                }
                assert_eq!(seen, stop, "must not call checks again after their first error");
            }
        }
        println!("\nDECLARATION_INGESTION_CHECKPOINTS {}", json!({"case":name,"checkpoints":total,"singleShotAndPersistent":true}));
    }
}

#[test]
fn input_limit_and_stop_priority_are_unchanged() {
    let oversized: Arc<str> = Arc::from("x".repeat((8 << 20) + 1));
    let mut calls = 0;
    let result = DeclarationModel::new(oversized.clone(), || { calls += 1; Ok::<_, Stop>(()) });
    assert!(matches!(result, Err(DeclarationFailure::InvalidInput(_))));
    assert_eq!(calls, 1, "oversized input is rejected before conversion or engine work");
    let token = Arc::new(());
    let result = DeclarationModel::new(oversized, || Err(Stop { ordinal:1, token:token.clone() }));
    assert!(matches!(result, Err(DeclarationFailure::Interrupted(Stop { ordinal:1, .. }))));
}

#[test]
fn caller_arc_and_subsequent_models_remain_independent_after_stop() {
    let source: Arc<str> = Arc::from("---@param value number\nfunction C_Test.Test(value) end\n");
    let original = source.clone();
    let mut calls = 0;
    let _ = DeclarationModel::new(source.clone(), || {
        calls += 1;
        if calls == 2 { Err(Stop { ordinal:2, token:Arc::new(()) }) } else { Ok(()) }
    });
    assert_eq!(&*source, &*original);
    assert_eq!(Arc::strong_count(&source), 2);
    let first = outcome(&source);
    let second = outcome(&source);
    assert_eq!(first, second);
}
