//! Isolated P0 prototype: real Lua public replay and the candidate Rust gate.
//! No production protocol, input API, WoW process, or deployment is used.
#[path = "../../../protocol/native-v1-prototype/gate.rs"]
mod gate;

use std::{collections::BTreeMap,fs::{self,OpenOptions},io::{BufRead,BufReader,Read,Write},path::PathBuf,
    process::{Command,Stdio},sync::mpsc,time::{Duration,Instant}};
use serde_json::json;
use sha2::{Digest,Sha256};

#[test]
fn real_lua_public_replay_is_rejected_only_by_current_attempt_gate() {
    let canonical=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    // Lua 5.1's legacy Windows C runtime does not accept Rust's verbatim prefix.
    let root=PathBuf::from(canonical.to_str().unwrap().strip_prefix(r"\\?\").unwrap_or(canonical.to_str().unwrap()));
    let lua=std::env::var_os("LYCHEEDEV_LUA51").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Program Files (x86)/Lua/5.1/lua.exe"));
    let evidence=root.join(".tmp/rust-supervision").join(format!("native-v1-p0-{}",lycheedev_windows::random::token().unwrap()));
    fs::create_dir_all(&evidence).unwrap();
    let mut transcript=OpenOptions::new().write(true).create_new(true).open(evidence.join("lua.tsv")).unwrap();
    let files=[root.join("protocol/native-v1-prototype/crypto.lua"),root.join("protocol/native-v1-prototype/observation.lua"),
        root.join("protocol/native-v1-prototype/p0_fixture.lua"),root.join("protocol/native-v1-prototype/gate.rs"),
        root.join("crates/lycheedev-live/tests/native_v1_prototype.rs"),lua.clone(),std::env::current_exe().unwrap()];
    let inputs:Vec<_>=files.iter().map(|p| {
        let path=if p.is_absolute(){p.clone()}else{root.join(p)};
        json!({"path":path,"sha256":hex::encode(Sha256::digest(fs::read(&path).unwrap()))})
    }).collect();
    fs::write(evidence.join("inputs.json"),serde_json::to_vec_pretty(&json!({"schema":"lycheedev.protocol-prototype.inputs.v1","inputs":inputs})).unwrap()).unwrap();
    let mut child=Command::new(&lua).arg(root.join("protocol/native-v1-prototype/p0_fixture.lua")).arg(&root)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
    let job=lycheedev_windows::JobGuard::attach(&child).unwrap();
    let mut input=child.stdin.take().unwrap();
    let output=child.stdout.take().unwrap();
    let (tx,rx)=mpsc::sync_channel(16);
    let reader=std::thread::spawn(move || {
        let mut reader=BufReader::new(output);
        for _ in 0..16 {
            let mut line=Vec::new();
            match reader.by_ref().take(8193).read_until(b'\n',&mut line) {
                Ok(0)=>break,
                Ok(_) if line.len()<=8192 && line.last()==Some(&b'\n')=>{
                    if tx.send(String::from_utf8(line).map_err(|_| "utf8")).is_err(){break;}
                }
                _=>{let _=tx.send(Err("line_budget"));break;}
            }
        }
    });
    let deadline=Instant::now()+Duration::from_secs(30);
    let mut frames=BTreeMap::new();
    let mut attempt:Option<gate::Attempt>=None;
    let mut blocked_time=None;
    let mut ready_time=None;
    loop {
        let line=rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("Lua deadline").expect("bounded Lua line");
        let observed=Instant::now();
        transcript.write_all(line.as_bytes()).unwrap();transcript.flush().unwrap();
        let (label,value)=line.trim_end().split_once('\t').unwrap();
        assert!(frames.insert(label.to_owned(),value.to_owned()).is_none());
        if label=="PRIVATE_BLOCKED" {
            let started=Instant::now();
            let nonce=format!("{}{}",lycheedev_windows::random::token().unwrap(),lycheedev_windows::random::token().unwrap());
            attempt=Some(gate::Attempt::new("1".repeat(32),"2".repeat(32),"3".repeat(32),2,nonce.clone(),frames["DIGEST"].clone(),started).unwrap());
            writeln!(input,"{nonce}").unwrap();input.flush().unwrap();
        }
        if label=="CURRENT_BLOCKED" { blocked_time=Some(observed); }
        if label=="CURRENT_READY" { ready_time=Some(observed); }
        if label=="SCOPE" { break; }
    }
    drop(input);
    while child.try_wait().unwrap().is_none() {
        if Instant::now()>=deadline {job.terminate().unwrap();child.wait().unwrap();panic!("Lua terminal deadline");}
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();drop(job);
    let key=[0x19u8;32];
    assert_eq!(frames["PRIVATE_BLOCKED"],"true");
    assert_eq!(gate::hmac(b"Jefe",b"what do ya want for nothing?"),"5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    assert!(gate::v0_admits(&frames["OLD1"],&frames["OLD2"],Duration::from_millis(250),&key));
    let base=attempt.unwrap();
    let at=base.started+Duration::from_millis(100);
    let mut failures=BTreeMap::new();
    let mut candidate=base.clone();
    for label in ["OLD1","OLD2"] {
        let error=candidate.qualify(&frames[label],&key,at).unwrap_err();
        assert_eq!(error,"wrong_attempt");failures.insert(label.to_owned(),error.to_owned());
    }
    assert_eq!(candidate.qualify(&frames["CURRENT_BLOCKED"],&key,blocked_time.unwrap()),Err("input_blocked"));
    assert!(candidate.qualify(&frames["CURRENT_READY"],&key,ready_time.unwrap()).is_ok());
    assert_eq!(candidate.qualify(&frames["CURRENT_READY"],&key,ready_time.unwrap()),Err("attempt_consumed"));
    let ready=&frames["CURRENT_READY"];
    assert_eq!(base.clone().qualify(ready,&key,base.started+gate::DELTA+Duration::from_millis(1)),Err("observation_expired"));
    // Windows Instant's underlying counter need not represent a 1ns decrement.
    assert_eq!(base.clone().qualify(ready,&key,base.started-Duration::from_millis(1)),Err("clock_rollback"));
    let mut wrong=base.clone();wrong.binding="4".repeat(32);
    assert_eq!(wrong.qualify(ready,&key,at),Err("wrong_binding"));
    let mut wrong=base.clone();wrong.digest="5".repeat(64);
    assert_eq!(wrong.qualify(ready,&key,at),Err("wrong_attempt"));
    let corrupt=format!("{}0",&ready[..ready.len()-1]);
    let corrupt=if corrupt==*ready {format!("{}1",&ready[..ready.len()-1])}else{corrupt};
    assert_eq!(base.clone().qualify(&corrupt,&key,at),Err("state_mac"));
    assert_eq!(base.clone().qualify(&"s".repeat(gate::MAX_STATE+1),&key,at),Err("state_size_or_encoding"));
    let mut bounded=base.clone();
    for _ in 0..gate::MAX_READS {assert_eq!(bounded.qualify(&frames["OLD1"],&key,at),Err("wrong_attempt"));}
    assert_eq!(bounded.qualify(ready,&key,at),Err("observation_budget"));
    let report=json!({"schema":"lycheedev.protocol-prototype.p0.v1","state":"passed_scoped_prototype",
        "scope":"real Lua5.1 public historical replay plus actual isolated Rust gate; fixed trusted binding; no WoW or OS input",
        "v0CounterexampleReproduced":true,"candidateOldSamplesRejected":failures,"privateBlockedRejected":true,
        "currentReadyAccepted":true,"actualNonceToReadyMillis":ready_time.unwrap().duration_since(base.started).as_secs_f64()*1000.0,
        "cases":["two_historical_authentic_samples","private_combat_blocked","current_nonce_ready","expired","rollback","wrong_binding","wrong_digest","bad_mac","oversize","read_budget","single_use"],
        "lua8KiBHmac10CpuSeconds":frames["LUA_8K_HMAC_10_CPU_SECONDS"],"runtimeReplayProven":false,"physicalApplyBindingProven":false,
        "authenticationIsolationInWoWProven":false,"finalProtocolPassed":false});
    fs::write(evidence.join("result.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("P0_EVIDENCE={}",evidence.display());
}
