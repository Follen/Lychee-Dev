//! Current installed bytes through the production Session and DataFusion engine.
//! Expected values come only from the independently captured Go 3.2.4 fixture.
#[path = "support/current_local_data.rs"]
mod current_local_data;

use current_local_data::Fixture;
use lycheedev_core::control::OperationControl;
use lycheedev_data::{
    agent_session::{Request, Session, Task},
    engine::{SourceMode, TableRequest},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(540))
}
fn table() -> TableRequest {
    TableRequest {
        name: "Map".into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    }
}
fn scan() -> Value {
    json!({"op":"scan","table":{"name":"Map","mode":"static"}})
}
fn column(name: &str) -> Value {
    json!({"kind":"column","name":name})
}
fn literal(value: Value) -> Value {
    json!({"kind":"literal","value":value})
}
fn binary(left: Value, operator: &str, right: Value) -> Value {
    json!({"kind":"binary","left":left,"operator":operator,"right":right})
}
fn filter(predicate: Value) -> Value {
    json!({"op":"filter","input":scan(),"predicate":predicate})
}
fn project(input: Value, columns: &[(&str, Value)]) -> Value {
    let columns: Vec<_> = columns
        .iter()
        .map(|(name, expression)| json!({"name":name,"expression":expression}))
        .collect();
    json!({"op":"project","input":input,"columns":columns})
}
fn sort(input: Value, name: &str, descending: bool) -> Value {
    json!({"op":"sort","input":input,"keys":[{"expression":column(name),"descending":descending,"nulls_first":false}]})
}
fn query(plan: Value) -> Task {
    let wire = json!({"protocol":lycheedev_data::agent_session::PROTOCOL,"id":1,"task":"query","query":{"kind":"plan","plan":plan}});
    serde_json::from_value::<Request>(wire).unwrap().task
}
async fn execute(session: &mut Session, task: Task) -> lycheedev_core::Result<Value> {
    let response = session.execute(task, control()).await?;
    // Exercise the public response representation, including exact numbers.
    Ok(current_local_data::parse_json(
        &serde_json::to_vec(&response).unwrap(),
    ))
}
async fn rows(session: &mut Session, task: Task) -> (Value, Value) {
    let receipt = execute(session, task).await.unwrap();
    assert_eq!(receipt["complete"], true);
    let handle = receipt["result"]["handle"].as_str().unwrap().to_owned();
    let count = receipt["result"]["rows"].as_u64().unwrap() as usize;
    let mut all = Vec::new();
    let mut columns = Value::Null;
    for offset in (0..count.max(1)).step_by(1000) {
        let page = execute(
            session,
            Task::Page {
                handle: handle.clone(),
                offset,
                limit: 1000,
            },
        )
        .await
        .unwrap();
        assert_eq!(page["offset"], offset);
        assert_eq!(page["totalRows"], count);
        assert_eq!(page["complete"], true);
        if columns.is_null() {
            columns = page["columns"].clone();
        }
        assert_eq!(columns, page["columns"]);
        all.extend(page["rows"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(all.len(), count);
    execute(session, Task::Release { handle }).await.unwrap();
    (json!({"columns":columns,"rows":all}), receipt)
}
fn object_rows(page: &Value) -> Value {
    let columns = page["columns"].as_array().unwrap();
    Value::Array(
        page["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let cells = row.as_array().unwrap();
                assert_eq!(columns.len(), cells.len());
                Value::Object(
                    columns
                        .iter()
                        .zip(cells)
                        .map(|(name, value)| (name.as_str().unwrap().to_owned(), value.clone()))
                        .collect(),
                )
            })
            .collect(),
    )
}

#[test]
#[ignore = "explicit current installed-client Go324 fixture and fresh owned output required"]
fn current_installed_map_uses_production_session_and_datafusion() {
    let fixture = Fixture::load();
    let home = PathBuf::from(
        std::env::var_os("LYCHEEDEV_RUST_DATA_AGENT_HOME").expect("fresh owned Agent home"),
    );
    let pin = fixture.initialize(&home);
    let mut session =
        Session::new(fixture.context(&home, &pin), Default::default()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let expected = fixture.rows();
        let expected_row_count = expected["rows"].as_array().unwrap().len();
        let (all, full_receipt) = rows(&mut session, query(sort(scan(), "ID", false))).await;
        assert_eq!(full_receipt["provenance"]["engine"], "datafusion");
        let identity = &full_receipt["provenance"]["sources"][0]["sourceIdentity"];
        fixture.assert_json_equal(&identity["data"], &fixture.document["pin"]["data"], "actual DataPin");
        assert_eq!(identity["source"]["origin"], "installation");
        assert_eq!(identity["source"]["content"]["sha256"], fixture.document["map"]["raw"]["sha256"]);
        assert_eq!(identity["source"]["content"]["bytes"], fixture.document["map"]["raw"]["bytes"]);
        assert_eq!(identity["complete"], true);
        let actual = object_rows(&all);
        assert_eq!(actual.as_array().unwrap().len(), expected_row_count);
        for (index, (actual, expected)) in actual.as_array().unwrap().iter().zip(expected["rows"].as_array().unwrap()).enumerate() {
            fixture.assert_json_equal(actual, expected, &format!("Map row {index}, ID={}", expected["ID"]));
        }
        let fields = execute(&mut session, Task::Fields { table: table() }).await.unwrap();
        let schema = fixture.oracle("schema");
        assert_eq!(fields["rowCount"], schema["result"]["rowCount"]);
        let names: Vec<_> = fields["columns"].as_array().unwrap().iter().map(|field| field["name"].clone()).collect();
        let expected_names: Vec<_> = schema["result"]["fields"].as_array().unwrap().iter().map(|field| field["name"].clone()).collect();
        assert_eq!(names, expected_names);
        for name in schema["result"]["arrayFields"].as_array().unwrap() {
            let field = fields["columns"].as_array().unwrap().iter().find(|field| field["name"] == *name).unwrap();
            assert!(field["dataType"].as_str().unwrap().starts_with("FixedSizeList"));
        }
        let id = fixture.document["probes"]["recordId"].as_u64().unwrap();
        let (record, _) = rows(&mut session, query(json!({"op":"lookup","table":{"name":"Map","mode":"static"},"ids":[id]}))).await;
        fixture.assert_json_equal(&object_rows(&record)[0], &fixture.oracle("record")["result"]["row"], "typed record");

        let id_zero = binary(column("ID"), "equal", literal(json!(id)));
        let plans = [
            ("allIDs", sort(project(filter(binary(column("ID"), "greater_equal", literal(json!(0)))), &[("ID", column("ID"))]), "ID", false)),
            ("range", json!({"op":"limit","offset":1,"count":5,"input":sort(project(filter(json!({"kind":"between","value":column("ID"),"low":literal(json!(0)),"high":literal(json!(20)),"negated":false})), &[("ID",column("ID")),("Directory",column("Directory"))]),"ID",true)})),
            ("flags", project(filter(id_zero.clone()), &[("ID", column("ID")), ("Flags[0]", json!({"kind":"index","value":column("Flags"),"index":0}))])),
            ("expression", sort(project(filter(json!({"kind":"in","value":column("ID"),"values":[0,1,2]})), &[("NextID", binary(column("ID"), "add", literal(json!(1))))]), "NextID", true)),
            ("distinct", sort(json!({"op":"aggregate","input":scan(),"groups":[{"name":"InstanceType","expression":column("InstanceType")}],"metrics":[]}), "InstanceType", false)),
            ("text", project(filter(binary(id_zero.clone(),"and",binary(json!({"kind":"call","function":"length","arguments":[column("MapName_lang")]}),"greater",literal(json!(0))))), &[("ID",column("ID")),("MapName_lang",column("MapName_lang"))])),
            ("point", project(filter(id_zero), &[("ID",column("ID")),("Directory",column("Directory")),("MapName_lang",column("MapName_lang"))])),
        ];
        let mut queries = Vec::new();
        for (name, plan) in plans {
            let (actual, receipt) = rows(&mut session, query(plan)).await;
            let expected = fixture.oracle(&format!("sql{}{}", name[..1].to_uppercase(), &name[1..]));
            fixture.assert_json_equal(&actual["columns"], &expected["result"]["result"]["columns"], &format!("typed {name} columns"));
            fixture.assert_json_equal(&actual["rows"], &expected["result"]["result"]["rows"], &format!("typed {name} rows"));
            queries.push(json!({"name":name,"result":actual,"receipt":receipt}));
        }
        for (oracle, probe, limit) in [("search", "searchQuery", 1), ("truncatedSearch", "truncatedSearchQuery", 2)] {
            let pattern = format!("%{}%", fixture.document["probes"][probe].as_str().unwrap().to_lowercase());
            let plan = json!({"op":"limit","offset":0,"count":limit,"input":sort(filter(json!({"kind":"like","value":{"kind":"call","function":"lower","arguments":[column("Directory")]},"pattern":literal(json!(pattern)),"negated":false})),"ID",false)});
            let (actual, _) = rows(&mut session, query(plan)).await;
            fixture.assert_json_equal(&object_rows(&actual), &fixture.oracle(oracle)["result"]["rows"], oracle);
        }
        let related: Request = serde_json::from_value(json!({"protocol":lycheedev_data::agent_session::PROTOCOL,"id":2,"task":"business","request":{"kind":"related","table":{"name":"Map","mode":"static"},"field":"ID","ids":[id]}})).unwrap();
        let related = execute(&mut session, related.task).await.unwrap();
        let component = &related["components"][0];
        assert_eq!(component["complete"], true);
        let handle = component["result"]["handle"].as_str().unwrap().to_owned();
        let page = execute(&mut session, Task::Page {handle:handle.clone(),offset:0,limit:1}).await.unwrap();
        fixture.assert_json_equal(&object_rows(&page), &fixture.oracle("foreignKey")["result"]["rows"], "typed related ID");
        execute(&mut session, Task::Release {handle:handle.clone()}).await.unwrap();
        assert_eq!(execute(&mut session, Task::Page {handle,offset:0,limit:1}).await.unwrap_err().code, "data.session.result_missing");

        let cancelled = control();
        cancelled.tracked_cancel_handle().unwrap().cancel();
        let error = session.execute(query(scan()), cancelled).await.err().unwrap();
        assert_eq!(error.code, "command.cancelled");
        assert!(execute(&mut session, query(project(scan(), &[("missing",column("DefinitelyMissing"))]))).await.is_err());
        let (recovered, _) = rows(&mut session, query(json!({"op":"lookup","table":{"name":"Map","mode":"static"},"ids":[id]}))).await;
        fixture.assert_json_equal(&object_rows(&recovered)[0], &fixture.oracle("record")["result"]["row"], "after cancellation and query error");
        session.close().unwrap();
        std::fs::write(home.parent().unwrap().join("agent-current-local-report.json"), serde_json::to_vec_pretty(&json!({
            "schema":"lycheedev.agent-current-local.v1","pin":pin,"build":fixture.pin.full_build,"expectedRows":expected_row_count,
            "fullTable":full_receipt,"fields":fields,"queries":queries,"record":record,"related":related,
            "cancel":"pre_admission_cooperative","errorRecovery":true,"closed":true
        })).unwrap()).unwrap();
    });
}
