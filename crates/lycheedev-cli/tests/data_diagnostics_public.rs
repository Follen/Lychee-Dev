//! Actual public CLI and typed Agent diagnostics, including protocol rejection.
use serde_json::{Value,json};
use std::{path::PathBuf,process::Command};
fn unused_home()->PathBuf{std::env::temp_dir().join(format!("agent-discovery-unused-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))}
#[test]
fn actual_discovery_is_structured_and_never_creates_home(){
    let home=unused_home();assert!(!home.exists());
    for words in [vec!["--help"],vec!["data","--help"],vec!["data","agent","serve","--help"],
        vec!["cli","search","spell effects"],vec!["schema","data","agent","serve"],vec!["data","agent","schema"]]{
        let out=Command::new(env!("CARGO_BIN_EXE_lycheedev")).current_dir(std::env::temp_dir()).env("LYCHEEDEV_HOME",&home)
            .args(&words).args(["--format","json"]).output().unwrap();
        assert_eq!(out.status.code(),Some(0),"{words:?}: {:?}",out.stderr);assert!(out.stderr.is_empty());
        let value:Value=serde_json::from_slice(&out.stdout).unwrap();assert_eq!(value["schema"],"lycheedev.result.v1");assert_eq!(value["ok"],true);
        assert_eq!(value["error"],Value::Null);assert_eq!(value["captures"],json!([]));
        if words.first()==Some(&"schema"){assert_eq!(value["result"]["schema"],"lycheedev.cli-command.v1");assert_eq!(value["result"]["protocol"]["protocol"],"lycheedev.data-agent.v1");}
        else if words.get(1)==Some(&"search"){assert_eq!(value["result"]["schema"],"lycheedev.cli-search.v1");assert!(value["result"]["matches"].as_array().unwrap().len()<=5);}
        else if words.last()==Some(&"schema"){assert_eq!(value["result"]["protocol"],"lycheedev.data-agent.v1");}
        else{assert_eq!(value["result"]["schema"],"lycheedev.cli-help.v1");}
        assert!(!home.exists(),"metadata must not create a home: {words:?}");
    }
    for words in [vec!["data","sql"],vec!["schema","data","sql"],vec!["version","--invented"],vec!["version","--format","yaml"]]{
        let out=Command::new(env!("CARGO_BIN_EXE_lycheedev")).env("LYCHEEDEV_HOME",&home).args(["--format","json"]).args(&words).output().unwrap();
        assert_eq!(out.status.code(),Some(2));let value:Value=serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["ok"],false);assert_eq!(value["error"]["code"],"command.invalid_arguments");assert_eq!(value["result"],Value::Null);assert_eq!(value["captures"],json!([]));assert!(!home.exists());
    }
}
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod fixture;
#[cfg(windows)]
mod worker {
    use super::*;
    use lycheedev_windows::{JobGuard,guarded_spawn};
    use std::{fs,io::{Read,Write},process::{Child,Stdio},thread,time::{Duration,Instant}};
    use super::fixture::{Fixture,decors,decor_rows};
    struct Owned{child:Option<Child>,job:Option<JobGuard>}
    impl Drop for Owned{fn drop(&mut self){if let Some(job)=&self.job{let _=job.terminate();}if let Some(child)=&mut self.child{let _=child.kill();let until=Instant::now()+Duration::from_secs(5);while Instant::now()<until{if child.try_wait().ok().flatten().is_some(){break;}thread::sleep(Duration::from_millis(10));}}}}
    fn run(fx:&Fixture,name:&str,input:&str)->(i32,Vec<u8>,Vec<u8>){
        let mut cmd=Command::new(env!("CARGO_BIN_EXE_lycheedev"));cmd.current_dir(&fx.root).args(["data","agent","serve","--format","jsonl","--timeout-seconds","30","--snapshot",&fx.pin,"--offline","--installation"])
            .arg(&fx.client).arg("--home").arg(&fx.home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let now=Instant::now();let(child,job)=guarded_spawn::spawn_suspended_guarded_until(cmd,2usize<<30,now+Duration::from_secs(10),now+Duration::from_secs(15),||false).unwrap();
        let mut owned=Owned{child:Some(child),job:Some(job)};let child=owned.child.as_mut().unwrap();
        let mut stdout=child.stdout.take().unwrap();let mut stderr=child.stderr.take().unwrap();
        let out=thread::spawn(move||{let mut b=Vec::new();stdout.read_to_end(&mut b).unwrap();b});
        let err=thread::spawn(move||{let mut b=Vec::new();stderr.read_to_end(&mut b).unwrap();b});
        fs::write(fx.root.join(format!("{name}.stdin.jsonl")),input).unwrap();let mut stdin=child.stdin.take().unwrap();stdin.write_all(input.as_bytes()).unwrap();drop(stdin);
        let deadline=Instant::now()+Duration::from_secs(45);
        let mut status=None;
        let status=loop {
            if status.is_none() {
                status=owned.child.as_mut().unwrap().try_wait().unwrap();
            }
            // Process exit and the outer Job's accounting are separate native
            // observations. Require both within the same original deadline.
            let active=owned.job.as_ref().unwrap().memory_accounting().unwrap().active_processes;
            if active==0 {
                if let Some(status)=status { break status; }
            }
            assert!(Instant::now()<deadline,
                "{name}: independent subprocess deadline; rootExited={}, activeProcesses={active}",
                status.is_some());
            thread::sleep(Duration::from_millis(10));
        };
        let out=out.join().unwrap();let err=err.join().unwrap();fs::write(fx.root.join(format!("{name}.stdout.jsonl")),&out).unwrap();fs::write(fx.root.join(format!("{name}.stderr.jsonl")),&err).unwrap();
        assert_eq!(owned.job.as_ref().unwrap().memory_accounting().unwrap().active_processes,0);
        guarded_spawn::close_child(owned.child.take().unwrap()).unwrap();owned.job.take().unwrap().close().unwrap();(status.code().unwrap(),out,err)
    }
    #[test]
    fn actual_supervisor_rejects_invalid_typed_frames_without_replay(){
        let fx=Fixture::new(&[decors(&decor_rows())]);
        let valid=json!({"protocol":"lycheedev.data-agent.v1","id":1,"task":"query","query":{"kind":"plan","plan":{"op":"lookup","table":{"name":"HouseDecor","mode":"static"},"ids":[10]}}});
        let mut cases=Vec::new();
        for (key,value) in [("invented",json!(true)),("task",json!("sql")),("id",json!(0)),("id",json!("1")),("protocol",json!("wrong")),("timeout_ms",json!(0)),("timeout_ms",json!(3600001))]{let mut v=valid.clone();v[key]=value;cases.push(v.to_string()+"\n");}
        for query in [json!({"kind":"sql","sql":"SELECT 1"}),json!({"kind":"plan","plan":{"op":"unknown"}}),json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"HouseDecor","mode":"static"},"ids":[-1]}}),json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"HouseDecor","mode":"static","invented":true},"ids":[10]}})]{let mut v=valid.clone();v["query"]=query;cases.push(v.to_string()+"\n");}
        cases.extend(["{invalid}\n".into(),valid.to_string()]);
        let before=lycheedev_core::metadata::Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap();
        for (index,input) in cases.iter().enumerate(){let(code,out,err)=run(&fx,&format!("invalid-{index}"),input);assert_eq!(code,3);assert!(out.is_empty(),"invalid frame must not publish a result");
            let value:Value=serde_json::from_slice(&err).unwrap();assert_eq!(value["protocol"],"lycheedev.data-agent-supervisor.v1");assert_eq!(value["ok"],false);
            assert_eq!(value["error"]["code"],"data.session.worker");assert_eq!(value["error"]["exit_code"],3);assert!(!value["error"]["message"].as_str().unwrap().is_empty());
            assert_eq!(lycheedev_core::metadata::Metadata::open(&fx.home,false).unwrap().list::<Value>("capture/",100).unwrap(),before,"invalid frames must not publish CAP");
        }
    }
    #[test]
    fn actual_valid_frames_report_typed_task_errors_and_drain_to_eof(){
        let fx=Fixture::new(&[decors(&decor_rows())]);let input=[
            json!({"protocol":"lycheedev.data-agent.v1","id":1,"task":"page","handle":"RES-missing","offset":0,"limit":0}),
            json!({"protocol":"lycheedev.data-agent.v1","id":2,"task":"page","handle":"RES-missing","offset":0,"limit":1}),
            json!({"protocol":"lycheedev.data-agent.v1","id":3,"task":"export","handle":"RES-missing","offset":0,"limit":1001,"format":"json"})]
            .iter().map(|v|v.to_string()+"\n").collect::<String>();
        let(code,out,err)=run(&fx,"task-errors",&input);assert_eq!(code,0);assert!(err.is_empty());
        let lines:Vec<Value>=String::from_utf8(out).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();assert_eq!(lines.len(),3);
        for (index,(value,(code,exit))) in lines.iter().zip([("data.session.page_limit",2),("data.session.result_missing",4),("data.session.page_limit",2)]).enumerate(){
            assert_eq!(value["protocol"],"lycheedev.data-agent.v1");assert_eq!(value["id"],index+1);assert_eq!(value["ok"],false);assert!(value.get("result").is_none());
            assert_eq!(value["error"]["code"],code);assert_eq!(value["error"]["exit_code"],exit);assert_eq!(value.as_object().unwrap().len(),4);
        }
    }
}
