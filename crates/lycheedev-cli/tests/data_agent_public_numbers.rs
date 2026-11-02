//! Actual Windows supervisor/worker JSONL, authenticated fixture, explicit CAP.
//! No declaration here is evidence of execution; non-Windows is not acceptance.
#![cfg(windows)]
#[allow(dead_code)]
#[path = "../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Fixture, decor_rows, decors};
use lycheedev_core::{evidence, metadata::Metadata};
use lycheedev_windows::{JobGuard, guarded_spawn};
use serde_json::{Value, json, value::RawValue};
use std::{collections::BTreeMap, fs, io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio}, sync::mpsc, thread,
    time::{Duration, Instant}};
const PROTOCOL: &str = "lycheedev.data-agent.v1";

struct Running {
    child: Option<Child>, job: Option<JobGuard>, input: Option<ChildStdin>,
    lines: mpsc::Receiver<String>, stdout: Option<thread::JoinHandle<()>>, stderr: Option<thread::JoinHandle<Vec<u8>>>,
    root: std::path::PathBuf, case: String, next: u64,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.input.take();
        if let Some(job) = &self.job { let _ = job.terminate(); }
        if let Some(child) = &mut self.child { let _ = child.kill();
            let until=Instant::now()+Duration::from_secs(5);
            while Instant::now()<until { if child.try_wait().ok().flatten().is_some(){break;} thread::sleep(Duration::from_millis(10)); } }
    }
}
fn start(fx: &Fixture, case: &str) -> Running {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lycheedev"));
    command.current_dir(&fx.root).args(["data","agent","serve","--format","jsonl","--timeout-seconds","180",
        "--snapshot",&fx.pin,"--offline","--installation"]).arg(&fx.client).arg("--home").arg(&fx.home)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let now=Instant::now();
    let (mut child,job)=guarded_spawn::spawn_suspended_guarded_until(command,2usize<<30,
        now+Duration::from_secs(10),now+Duration::from_secs(15),||false).unwrap();
    let input=child.stdin.take();let stdout=child.stdout.take().unwrap();let mut stderr=child.stderr.take().unwrap();
    let root=fx.root.clone();let out_path=root.join(format!("{case}.stdout.jsonl"));
    let (tx,lines)=mpsc::channel();
    let stdout=thread::spawn(move || {
        let mut saved=fs::File::create(out_path).unwrap();let mut reader=BufReader::new(stdout);
        loop {
            let mut bytes=Vec::new();if reader.read_until(b'\n',&mut bytes).unwrap()==0 {break;}
            // Preserve original bytes, including malformed/truncated framing,
            // before admission. Never manufacture a terminating newline.
            saved.write_all(&bytes).unwrap();saved.flush().unwrap();
            assert_eq!(bytes.last(),Some(&b'\n'),"incomplete public JSONL frame");
            bytes.pop();let line=String::from_utf8(bytes).expect("public JSONL is UTF-8");
            if tx.send(line).is_err(){break;}
        }
    });
    let err_path=root.join(format!("{case}.stderr.bin"));
    let stderr=thread::spawn(move || {let mut bytes=Vec::new();stderr.read_to_end(&mut bytes).unwrap();fs::write(err_path,&bytes).unwrap();bytes});
    Running{child:Some(child),job:Some(job),input,lines,stdout:Some(stdout),stderr:Some(stderr),root,case:case.into(),next:0}
}
fn request(run: &mut Running, mut task: Value) -> (Value,String) {
    run.next+=1;task["protocol"]=json!(PROTOCOL);task["id"]=json!(run.next);task["timeout_ms"]=json!(30000);
    let mut bytes=serde_json::to_vec(&task).unwrap();bytes.push(b'\n');
    let mut log=fs::OpenOptions::new().create(true).append(true).open(run.root.join(format!("{}.stdin.jsonl",run.case))).unwrap();
    log.write_all(&bytes).unwrap();log.flush().unwrap();
    run.input.as_mut().unwrap().write_all(&bytes).unwrap();run.input.as_mut().unwrap().flush().unwrap();
    let raw=run.lines.recv_timeout(Duration::from_secs(40)).expect("independent response deadline");
    let value:Value=serde_json::from_str(&raw).unwrap();
    assert_eq!(value["protocol"],PROTOCOL);assert_eq!(value["id"],run.next);assert_eq!(value["ok"],true,"{raw}");
    assert!(value.get("error").is_none());assert_eq!(value.as_object().unwrap().len(),4);
    (value["result"].clone(),raw_field(&raw,"result"))
}
fn finish(mut run:Running) {
    // Cancelling already completed work is a real successful control response;
    // it must not replay it or terminate this still usable session.
    let (cancelled,_)=request(&mut run,json!({"task":"cancel","request_id":1}));
    assert_eq!(cancelled,json!({"requestId":1,"cancellationRequested":false,"replayed":false}));
    let (closed,_)=request(&mut run,json!({"task":"close"}));assert_eq!(closed,json!({"closed":true,"handlesReleased":true}));
    let descriptor=lycheedev_data::agent_session::schema();
    for (task,payload) in [("cancel",&cancelled),("close",&closed)] {
        let schema=&descriptor["resultSchemas"][task];
        for required in schema["required"].as_array().unwrap() {
            let name=required.as_str().unwrap();assert!(payload.get(name).is_some(),"{task} requires {name}");
            if let Some(expected)=schema["properties"][name].get("const") {assert_eq!(&payload[name],expected,"{task}.{name}");}
        }
    }
    run.input.take();let deadline=Instant::now()+Duration::from_secs(15);
    let status=loop {if let Some(status)=run.child.as_mut().unwrap().try_wait().unwrap(){break status;}
        assert!(Instant::now()<deadline,"supervisor cleanup deadline");thread::sleep(Duration::from_millis(10));};
    assert!(status.success());assert!(matches!(run.lines.recv_timeout(Duration::from_secs(5)),Err(mpsc::RecvTimeoutError::Disconnected)));
    assert_eq!(run.job.as_ref().unwrap().memory_accounting().unwrap().active_processes,0);
    run.stdout.take().unwrap().join().unwrap();
    assert!(run.stderr.take().unwrap().join().unwrap().is_empty());
    guarded_spawn::close_child(run.child.take().unwrap()).unwrap();run.job.take().unwrap().close().unwrap();
}
fn raw_field(raw:&str,field:&str)->String {let object:BTreeMap<String,Box<RawValue>>=serde_json::from_str(raw).unwrap();object[field].get().to_owned()}
fn captures(fx:&Fixture)->Vec<Value>{Metadata::open(&fx.home,false).unwrap().list("capture/",100).unwrap()}
fn lookup(ids:Value)->Value{json!({"op":"lookup","table":{"name":"HouseDecor","mode":"static"},"ids":ids})}
fn catalog_query(ids:Value)->Value{json!({"kind":"plan","plan":{"op":"sort","input":{"op":"project","input":lookup(ids),
    "columns":[{"name":"ID","expression":{"kind":"column","name":"ID"}},{"name":"Name_lang","expression":{"kind":"column","name":"Name_lang"}},
    {"name":"ModelFileDataID","expression":{"kind":"column","name":"ModelFileDataID"}}]},
    "keys":[{"expression":{"kind":"column","name":"ID"},"descending":false,"nulls_first":false}]}})}
fn submit(run:&mut Running,query:Value,explain:Option<bool>)->(String,Value){
    let task=match explain{None=>json!({"task":"query","query":query}),Some(analyze)=>json!({"task":"explain","query":query,"analyze":analyze,"verbose":true})};
    let (result,_)=request(run,task);assert_eq!(result["complete"],true);assert_eq!(result["provenance"]["engine"],"datafusion");
    assert_eq!(result["provenance"]["complete"],true);assert_eq!(result["provenance"]["sources"].as_array().unwrap().len(),1);
    let source=&result["provenance"]["sources"][0];
    assert_eq!(source["complete"],true);assert_eq!(source["truncated"],false);
    assert_eq!(source["sourceIdentity"]["complete"],true);assert_eq!(source["sourceIdentity"]["table"]["db2FileDataId"],403);
    let reading=&source["sourceReading"];
    assert_eq!(reading["source"],"installation");assert_eq!(reading["entry"]["fileDataID"],403);
    assert_eq!(reading["contentVerified"],true);
    assert_eq!(reading["verification"]["rootWholeDecodedCKey"],true);
    assert_eq!(reading["verification"]["contentWholeDecodedCKey"],true);
    assert_eq!(source["sourceVerification"],reading["verification"]);
    let digest=source["sourceSha256"].as_str().unwrap();assert_eq!(digest.len(),64);assert!(digest.bytes().all(|b|b.is_ascii_hexdigit()));
    (result["result"]["handle"].as_str().unwrap().to_owned(),result)
}
fn page(run:&mut Running,handle:&str,offset:usize,limit:usize)->(Value,String){request(run,json!({"task":"page","handle":handle,"offset":offset,"limit":limit}))}
fn capture_and_verify(fx:&Fixture,run:&mut Running,handle:&str,page:&Value,provenance:&Value)->String {
    let before=captures(fx);let (capture,_)=request(run,json!({"task":"capture","handle":handle}));let after=captures(fx);
    assert_eq!(after.len(),before.len()+1);assert!(after.contains(&capture));
    assert!(!before.iter().any(|old|old["id"]==capture["id"]));
    assert_eq!(capture["provenance"]["kind"],"data-agent-query");assert_eq!(capture["provenance"]["snapshot"],fx.pin);
    assert_eq!(capture["complete"],true);assert_eq!(capture["truncated"],false);
    let id=capture["id"].as_str().unwrap();assert_eq!(evidence::read_capture(&fx.home,id).unwrap(),capture);
    let raw=String::from_utf8(evidence::read_blob(&fx.home,&capture["blob"],16<<20).unwrap()).unwrap();let body:Value=serde_json::from_str(&raw).unwrap();
    assert_eq!(body["schema"],"lycheedev.data-agent-result.v1");assert_eq!(body["columns"],page["columns"]);
    assert_eq!(body["rows"],page["rows"]);assert_eq!(body["rowCount"],page["totalRows"]);assert_eq!(body["complete"],true);assert_eq!(&body["provenance"],provenance);
    let out=Command::new(env!("CARGO_BIN_EXE_lycheedev")).current_dir(&fx.root).args(["evidence","verify",id,"--home"])
        .arg(&fx.home).args(["--format","json"]).output().unwrap();
    fs::write(fx.root.join(format!("{}.{}.verify.stdout",run.case,run.next)),&out.stdout).unwrap();
    fs::write(fx.root.join(format!("{}.{}.verify.stderr",run.case,run.next)),&out.stderr).unwrap();
    assert!(out.status.success());assert!(out.stderr.is_empty());let value:Value=serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema"],"lycheedev.result.v1");assert_eq!(value["ok"],true);assert_eq!(value["error"],Value::Null);
    assert_eq!(value["result"],capture);assert_eq!(value["captures"],json!([]));assert_eq!(captures(fx),after);raw
}
fn exports(run:&mut Running,handle:&str,page:&Value)->String{
    let (json_export,raw)=request(run,json!({"task":"export","handle":handle,"offset":0,"limit":1000,"format":"json"}));
    assert_eq!(json_export,json!({"format":"json","page":page}));
    let (csv,_)=request(run,json!({"task":"export","handle":handle,"offset":0,"limit":1000,"format":"csv"}));
    assert_eq!(csv["format"],"csv");assert_eq!(&csv["page"],page);
    let quote=|s:&str|format!("\"{}\"",s.replace('"',"\"\""));
    let mut expected=page["columns"].as_array().unwrap().iter().map(|v|quote(v.as_str().unwrap())).collect::<Vec<_>>().join(",")+"\r\n";
    for row in page["rows"].as_array().unwrap(){expected+=&row.as_array().unwrap().iter().map(|v|quote(&match v{Value::Null=>String::new(),Value::String(s)=>s.clone(),v=>v.to_string()})).collect::<Vec<_>>().join(",");expected+="\r\n";}
    assert_eq!(csv["text"],expected);raw_field(&raw,"page")
}
fn numeric_query()->Value{
    let literals=["-0","-0.0","1E-07","1e-6","1.0e20","1e21","1.00","9007199254740993","18446744073709551615","-9223372036854775808","null"];
    let columns:Vec<Value>=literals.iter().enumerate().map(|(i,v)|json!({"name":format!("n{i}"),"expression":{"kind":"literal","value":serde_json::from_str::<Value>(v).unwrap()}})).collect();
    json!({"kind":"plan","plan":{"op":"project","input":lookup(json!([10])),"columns":columns}})
}
#[test]
fn actual_agent_numeric_page_exports_and_capture_preserve_exact_scalars(){
    let fx=Fixture::new(&[decors(&decor_rows())]);let mut run=start(&fx,"numeric");
    let query=numeric_query();
    let before=captures(&fx);let(handle,result)=submit(&mut run,query.clone(),None);assert_eq!(captures(&fx),before,"query is not CAP");
    assert_eq!(result["provenance"]["request"],query);let(p,raw)=page(&mut run,&handle,0,1000);
    assert_eq!(p["offset"],0);assert_eq!(p["totalRows"],1);assert_eq!(p["complete"],true);
    let exported=exports(&mut run,&handle,&p);let archived=capture_and_verify(&fx,&mut run,&handle,&p,&result["provenance"]);
    for raw in [&raw,&exported,&archived]{let rows:Vec<Vec<Box<RawValue>>>=serde_json::from_str(&raw_field(raw,"rows")).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0].len(),11);
        for (i,token) in [(0,"0"),(7,"9007199254740993"),(8,"18446744073709551615"),(9,"-9223372036854775808"),(10,"null")]{assert_eq!(rows[0][i].get(),token);}
        for (i,value) in [-0.0_f64,1e-7,1e-6,1e20,1e21,1.0].into_iter().enumerate(){assert_eq!(rows[0][i+1].get().parse::<f64>().unwrap().to_bits(),value.to_bits());}
    }
    let (released,_)=request(&mut run,json!({"task":"release","handle":handle}));assert_eq!(released,json!({"released":handle}));finish(run);
}
#[test]
fn actual_agent_catalog_empty_arrays_paging_and_explicit_capture(){
    let fx=Fixture::new(&[decors(&decor_rows())]);let mut run=start(&fx,"catalog");
    for (ids,rows) in [(json!([30,10,10,20]),json!([[10,"无模型🙂",0],[20,"第二",501],[30,"第三",501]])),(json!([999]),json!([]))]{
        let before=captures(&fx);let(handle,result)=submit(&mut run,catalog_query(ids),None);assert_eq!(captures(&fx),before);
        let(p,_)=page(&mut run,&handle,0,1000);assert_eq!(p["columns"],json!(["ID","Name_lang","ModelFileDataID"]));assert_eq!(p["rows"],rows);
        assert_eq!(p["totalRows"],rows.as_array().unwrap().len());assert_eq!(p["complete"],true);
        if !rows.as_array().unwrap().is_empty(){let(first,_)=page(&mut run,&handle,0,1);let(tail,_)=page(&mut run,&handle,1,2);
            assert_eq!(first["rows"],json!([[10,"无模型🙂",0]]));assert_eq!(tail["rows"],json!([[20,"第二",501],[30,"第三",501]]));assert_eq!(tail["offset"],1);assert_eq!(tail["totalRows"],3);}
        exports(&mut run,&handle,&p);capture_and_verify(&fx,&mut run,&handle,&p,&result["provenance"]);request(&mut run,json!({"task":"release","handle":handle}));
    }finish(run);
}
#[test]
fn actual_agent_explain_and_analyze_are_real_plan_handles_with_caps(){
    let fx=Fixture::new(&[decors(&decor_rows())]);let mut run=start(&fx,"explain");
    for (query,expected_rows) in [(catalog_query(json!([30,10,10,20])),"3"),(numeric_query(),"1")] {
    for analyze in [false,true]{let before=captures(&fx);let(handle,result)=submit(&mut run,query.clone(),Some(analyze));assert_eq!(captures(&fx),before);
        assert_eq!(result["provenance"]["request"],json!({"kind":"explain","query":query,"verbose":true,"analyze":analyze}));
        let analysis=&result["provenance"]["analysis"];assert_eq!(analysis["nativePreflightAndPrerequisites"]["executed"],analyze);
        assert_eq!(analysis["analysisScope"],if analyze{"complete_native_root"}else{"expanded_native_plan_without_execution"});
        for v in [&analysis["requestElapsedMs"],&analysis["nativePreflightAndPrerequisites"]["elapsedMs"],&analysis["rootExecutionAndOutputCompaction"]["elapsedMs"]]{let t=v.as_f64().unwrap();assert!(t.is_finite()&&t>=0.0);}
        let(p,_)=page(&mut run,&handle,0,1000);assert_eq!(p["columns"],json!(["plan_type","plan"]));assert!(!p["rows"].as_array().unwrap().is_empty());
        for row in p["rows"].as_array().unwrap(){assert_eq!(row.as_array().unwrap().len(),2);for v in row.as_array().unwrap(){assert!(!v.as_str().unwrap().is_empty());}}
        let rows=p["rows"].as_array().unwrap();
        if analyze {
            assert_eq!(p["totalRows"],4);assert_eq!(rows.len(),4);
            let labels:Vec<_>=rows.iter().map(|row|row[0].as_str().unwrap()).collect();
            assert_eq!(labels,vec!["Plan with Metrics","Plan with Full Metrics","Output Rows","Duration"]);
            assert_eq!(rows[2][1],expected_rows);
            assert!(!rows[0][1].as_str().unwrap().is_empty());assert!(!rows[1][1].as_str().unwrap().is_empty());
            assert!(!rows[3][1].as_str().unwrap().is_empty());
        } else {
            assert!(rows.iter().any(|row|row[0].as_str().unwrap().starts_with("logical_plan")));
            assert!(rows.iter().any(|row|row[0].as_str().unwrap().starts_with("physical_plan")));
            assert!(!rows.iter().any(|row|matches!(row[0].as_str(),Some("Output Rows"|"Duration"|"Plan with Metrics"|"Plan with Full Metrics"))));
        }
        exports(&mut run,&handle,&p);capture_and_verify(&fx,&mut run,&handle,&p,&result["provenance"]);request(&mut run,json!({"task":"release","handle":handle}));
    }}finish(run);
}
