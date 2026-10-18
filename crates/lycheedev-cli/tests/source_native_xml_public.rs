//! Default refs/context must carry original XML through persisted and live caches.
#![cfg(windows)]
use lycheedev_core::{evidence,selection::{self,SelectionSpec,SourcePin},workspace};
use lycheedev_source::git;
use serde_json::{Value,json};
use std::{fs,path::{Path,PathBuf},process::Command,time::{SystemTime,UNIX_EPOCH}};
fn git_run(root:&Path,args:&[&str])->String{
    let result=Command::new("git").current_dir(root).args(args).env("GIT_AUTHOR_NAME","XML research fixture").env("GIT_AUTHOR_EMAIL","fixture@example.invalid").env("GIT_COMMITTER_NAME","XML research fixture").env("GIT_COMMITTER_EMAIL","fixture@example.invalid").output().unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));String::from_utf8(result.stdout).unwrap().trim().into()
}
fn cli(home:&Path,args:&[&str])->Value{
    let output=Command::new(env!("CARGO_BIN_EXE_lycheedev")).args(args).args(["--home",home.to_str().unwrap(),"--format","json"]).output().unwrap();
    let value:Value=serde_json::from_slice(&output.stdout).unwrap_or_else(|error|panic!("{error}: {} / {}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr)));
    assert!(output.status.success(),"{value}");assert_eq!(value["ok"],true,"{value}");
    assert_captured_result(home,&value);
    value
}
fn assert_captured_result(home: &Path, value: &Value) {
    let captures = value["captures"]
        .as_array()
        .expect("captures must be an array");
    assert_eq!(captures.len(), 1, "one capture per refs/context result");
    let capture = &captures[0];
    let id = capture["id"].as_str().expect("capture must have an id");
    assert!(
        id.strip_prefix("CAP-")
            .is_some_and(|digest| selection::valid_digest(digest, 64)),
        "capture must have a canonical CAP id"
    );
    assert_eq!(evidence::read_capture(home, id).unwrap(), *capture);
    let raw = evidence::read_blob(home, &capture["blob"], 32 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&raw).unwrap(),
        value["result"]
    );
}
#[test]
fn public_xml_capture_checker_requires_one_persisted_capture_matching_full_result() {
    struct CaptureFixture(PathBuf);
    impl Drop for CaptureFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    // This checker regression uses no CLI process, native session or release.
    let fixture = CaptureFixture(std::env::temp_dir().join(format!(
        "lycheedev-public-xml-capture-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let home = &fixture.0;
    workspace::initialize(home).unwrap();
    let result = json!({"symbol":{"name":"Fixture"},"relations":[{"path":"Frames.xml","line":3}],"complete":true});
    let capture = evidence::commit_capture(
        home,
        &serde_json::to_vec(&result).unwrap(),
        "application/json",
        json!({"kind":"source-refs","locator":"Fixture"}),
        true,
        false,
    )
    .unwrap();
    let valid = json!({"ok":true,"result":result,"captures":[capture]});
    assert_captured_result(home, &valid);
    let reject = |case: &str, value: Value| {
        assert!(
            std::panic::catch_unwind(|| assert_captured_result(home, &value)).is_err(),
            "{case} must be rejected"
        );
    };

    let mut invalid = valid.clone();
    invalid.as_object_mut().unwrap().remove("captures");
    reject("missing captures", invalid);
    let mut invalid = valid.clone();
    invalid["captures"] = json!({});
    reject("non-array captures", invalid);
    let mut invalid = valid.clone();
    invalid["captures"] = json!([]);
    reject("empty captures", invalid);
    let mut invalid = valid.clone();
    invalid["captures"] = json!([valid["captures"][0], valid["captures"][0]]);
    reject("multiple captures", invalid);
    let mut invalid = valid.clone();
    invalid["captures"][0]["id"] = json!("CAP-invalid");
    reject("malformed capture id", invalid);
    let forged_id = format!("CAP-{}", "0".repeat(64));
    assert_ne!(valid["captures"][0]["id"], forged_id);
    assert_eq!(
        evidence::read_capture(home, &forged_id).unwrap_err().code,
        "vault.record_missing"
    );
    let mut invalid = valid.clone();
    invalid["captures"][0]["id"] = json!(forged_id);
    reject("forged capture id with the real blob", invalid);
    let mut invalid = valid.clone();
    invalid["captures"][0]["provenance"]["locator"] = json!("forged");
    reject("changed descriptor with a real capture id", invalid);
    let mut invalid = valid.clone();
    invalid["result"]["relations"][0]["line"] = json!(4);
    reject("result differs from the captured body", invalid);

    let other_result = json!({"symbol":{"name":"Other"},"relations":[],"complete":true});
    let other_capture = evidence::commit_capture(
        home,
        &serde_json::to_vec(&other_result).unwrap(),
        "application/json",
        json!({"kind":"source-refs","locator":"Other"}),
        true,
        false,
    )
    .unwrap();
    let other = json!({"ok":true,"result":other_result,"captures":[other_capture]});
    assert_captured_result(home, &other);
    let mut invalid = valid.clone();
    invalid["captures"] = other["captures"].clone();
    reject("genuine other capture with the original result", invalid);
    assert_captured_result(home, &valid);
}
fn pin(home:&Path,commit:String)->String{selection::pin_selection(home,SelectionSpec{source:Some(SourcePin{repository:"elvui".into(),product:"main".into(),requested_ref:"fixture".into(),exact_commit:commit,parser_revision:"source-v1".into()}),..Default::default()}).unwrap().id}
struct Fixture{root:PathBuf,home:PathBuf}
impl Drop for Fixture{fn drop(&mut self){let _=Command::new(env!("CARGO_BIN_EXE_lycheedev")).args(["source","session","close","--home",self.home.to_str().unwrap(),"--format","json"]).output();if std::env::var_os("LYCHEEDEV_SOURCE_NATIVE_KEEP_FIXTURE").is_none(){let _=fs::remove_dir_all(&self.root);}}}
fn native_rows(value:&Value)->Vec<&Value>{value["result"]["relations"].as_array().unwrap().iter().filter(|row|row["basis"].as_array().is_some_and(|basis|basis.contains(&json!("native_common_source_facts")))).collect()}
#[test]
#[ignore="requires verified release and owned runner; real default CLI XML refs/context and cache reuse"]
fn default_xml_research_keeps_manifest_binding_original_ranges_and_cache_identity(){
    let release=PathBuf::from(std::env::var_os("LYCHEEDEV_SOURCE_NATIVE_RELEASE").or_else(||std::env::var_os("LYCHEEDEV_SOURCE_FLOW_RELEASE")).expect("verified release"));
    let root=std::env::temp_dir().join(format!("lycheedev-public-native-xml-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));let home=root.join("home");workspace::initialize(&home).unwrap();let _fixture=Fixture{root:root.clone(),home:home.clone()};
    let repository=root.join("repository");fs::create_dir_all(&repository).unwrap();fs::write(repository.join(".gitattributes"),"* -text\n").unwrap();
    let xml="\u{feff}<Ui>\r\n<Frame name=\"DemoFrame\" mixin=\"Mixin\"><Scripts>\r\n<OnLoad>self:R&#117;n(1)</OnLoad>\r\n<OnShow method=\"Run\"/>\r\n<OnHide function=\"GlobalHandler\"/>\r\n</Scripts></Frame>\r\n</Ui>\r\n";
    for(path,text)in [("a.lua","Mixin = {}\nfunction Mixin:Run(value) return value end\nfunction Other() end\nfunction GlobalHandler(self) return self end\n"),("Override.lua","Mixin.Run = Other\n"),("Frames.xml",xml),("Inline.xml","<Ui>\r\n<Script>&#10;function InlineEntity()&#10; return GlobalHandler()&#10;end&#10;InlineEntity()</Script>\r\n</Ui>\r\n"),("Main.toc","a.lua\nFrames.xml\nFrames.xml\nInline.xml\n"),("Alt.toc","a.lua\nOverride.lua\nFrames.xml\nInline.xml\n")]{fs::write(repository.join(path),text).unwrap();}
    git_run(&repository,&["init"]);git_run(&repository,&["add","."]);git_run(&repository,&["commit","-m","XML originals"]);let first_commit=git_run(&repository,&["rev-parse","HEAD"]);
    fs::write(repository.join("Frames.xml"),xml.replace("R&#117;n(1)","R&#117;n(2)")).unwrap();git_run(&repository,&["add","Frames.xml"]);git_run(&repository,&["commit","-m","XML-only change"]);let changed_commit=git_run(&repository,&["rev-parse","HEAD"]);
    let mirror=git::mirror(&home,"elvui");fs::create_dir_all(mirror.parent().unwrap()).unwrap();git_run(&repository,&["clone","--bare",".",mirror.to_str().unwrap()]);let first=pin(&home,first_commit);let changed=pin(&home,changed_commit);
    let query_number=std::cell::Cell::new(0usize);
    let query=|command:&str,pin:&str,symbol:&str,direction:&str,reuse:bool|{let mut args=vec!["source",command,"--snapshot",pin,"--symbol",symbol,"--limit","200","--release",release.to_str().unwrap()];if command=="refs"{args.extend(["--direction",direction]);}if reuse{args.push("--session-reuse");}let result=cli(&home,&args);let number=query_number.get();query_number.set(number+1);fs::write(root.join(format!("query-{number}.json")),serde_json::to_vec_pretty(&result).unwrap()).unwrap();result};
    let cold=query("refs",&first,"Mixin:Run","incoming",false);
    let rows=native_rows(&cold);assert!(rows.iter().any(|row|row["kind"]=="handler_method"&&row["state"]=="resolved"&&row["loadScope"]["manifest"]=="Main.toc"),"{cold}");
    assert!(!rows.iter().any(|row|row["kind"]=="handler_method"&&row["state"]=="resolved"&&row["loadScope"]["manifest"]=="Alt.toc"),"{cold}");
    assert!(rows.iter().filter(|row|row["kind"]=="xml_mixin_copy").count()>=2,"{cold}");
    let warm=query("refs",&first,"Mixin:Run","incoming",false);assert_eq!(cold["result"]["relations"],warm["result"]["relations"]);assert_eq!(cold["result"]["nativeResearch"]["inputIdentity"],warm["result"]["nativeResearch"]["inputIdentity"]);
    let outgoing=query("refs",&first,"DemoFrame","outgoing",true);let rows=native_rows(&outgoing);
    assert_eq!(outgoing["result"]["symbol"]["line"],2);
    for scope in outgoing["result"]["nativeResearch"]["loadScopes"].as_array().unwrap(){for object in scope["xmlFacts"]["objects"].as_array().unwrap(){assert_eq!(object["origin"]["bytes"]["start"],9);}}

    let inline=rows.iter().find(|row|row["kind"]=="call"&&row["path"]=="Frames.xml"&&row["position"]["sourceMap"]["accuracy"]=="covering"&&row["state"]=="resolved").expect("decoded inline call projected to original XML");
    assert_eq!(inline["line"],3);assert!(inline["position"]["sourceMap"]["segments"].as_array().unwrap().iter().any(|segment|segment["origin"]=="Entity"));assert!(inline["position"]["generatedSite"]["file"].as_str().unwrap().contains("__inline_"));
    let context=query("context",&first,"DemoFrame","outgoing",true);assert_eq!(context["result"]["nativeResearch"]["analysis"]["cacheHit"],true);assert_eq!(context["result"]["nativeResearch"]["nativeSession"]["snapshotTransfer"],"identity_only");
    assert!(context["result"]["snippets"].as_array().unwrap().iter().any(|snippet|snippet["path"]=="Frames.xml"),"{context}");
    let changed=query("refs",&changed,"DemoFrame","outgoing",true);assert_ne!(changed["result"]["nativeResearch"]["inputIdentity"],outgoing["result"]["nativeResearch"]["inputIdentity"]);assert_eq!(changed["result"]["nativeResearch"]["analysis"]["cacheHit"],false);
    let entity=query("context",&first,"InlineEntity","outgoing",true);assert_eq!(entity["result"]["symbol"]["line"],2,"{entity}");
    assert!(native_rows(&entity).iter().any(|row|row["kind"]=="call"&&row["target"]=="GlobalHandler"&&row["path"]=="Inline.xml"&&row["line"]==2&&row["state"]=="resolved"),"{entity}");
    assert!(entity["result"]["snippets"].as_array().unwrap().iter().any(|snippet|snippet["path"]=="Inline.xml"&&snippet["text"].as_str().unwrap().contains("function InlineEntity()&#10;")),"{entity}");
    let global=query("refs",&first,"GlobalHandler","incoming",true);assert!(native_rows(&global).iter().any(|row|row["kind"]=="handler_function"&&row["state"]=="resolved"));
    fs::write(root.join("report.json"),serde_json::to_vec_pretty(&json!({"cold":cold,"warm":warm,"outgoing":outgoing,"context":context,"changed":changed,"entity":entity,"global":global})).unwrap()).unwrap();println!("{}",root.display());
}
