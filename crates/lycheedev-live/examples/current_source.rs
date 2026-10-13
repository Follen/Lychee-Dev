//! Research-only current semantic-site corroboration. This does not locate a root on disk.
use lycheedev_live::locator::Pe;
use lycheedev_windows::Process;
use serde_json::json;
use sha2::{Digest, Sha256};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "usage: current_source <pid> <semantic-anchor-rva-hex> <retained-semantic-evidence.json>"
        );
        std::process::exit(2)
    }
    let run = (|| -> std::result::Result<serde_json::Value, String> {
        let pid = args[1].parse::<u32>().map_err(|e| e.to_string())?;
        let anchor =
            u64::from_str_radix(args[2].trim_start_matches("0x"), 16).map_err(|e| e.to_string())?;
        let p = Process::open(pid)?;
        let module = p.main_module()?;
        let evidence = std::fs::read(&args[3]).map_err(|e| e.to_string())?;
        let prior: serde_json::Value =
            serde_json::from_slice(&evidence).map_err(|e| e.to_string())?;
        if prior["source"]["executableSha256"] != module.hash()
            || prior["source"]["rva"]
                .as_str()
                .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                != Some(anchor)
        {
            return Err("source.semantic_site_identity_mismatch".into());
        }
        let pe = Pe::parse(module.disk())?;
        if !pe.sections.iter().any(|s| {
            s.flags & 0x20000000 != 0 && anchor >= s.rva && anchor + 58 <= s.rva + s.virtual_size
        }) {
            return Err("source.semantic_range".into());
        }
        let mut code = [0u8; 58];
        p.read_module(&module, module.base() + anchor, &mut code)?;
        let pattern = prior["pattern"]
            .as_str()
            .ok_or("source.pattern_missing")?
            .split_whitespace()
            .collect::<Vec<_>>();
        if pattern.len() != code.len()
            || pattern
                .iter()
                .zip(&code)
                .any(|(v, b)| *v != "??" && u8::from_str_radix(v, 16).ok() != Some(*b))
        {
            return Err("source.semantic_site_changed".into());
        }
        let displacement = i32::from_le_bytes(code[3..7].try_into().unwrap()) as i64;
        let root = (anchor + 7)
            .checked_add_signed(displacement)
            .ok_or("source.rip_overflow")?;
        if !pe.sections.iter().any(|s| {
            s.flags & 0x80000000 != 0
                && s.flags & 0x20000000 == 0
                && root >= s.rva
                && root + 8 <= s.rva + s.virtual_size
        }) {
            return Err("source.root_section".into());
        }
        let mut pointer = [0; 8];
        p.read_module(&module, module.base() + root, &mut pointer)?;
        let state = u64::from_le_bytes(pointer);
        let mut tag = [0u8; 1];
        p.read_private(state + 16, &mut tag)?;
        let mut globals = [0u8; 24];
        p.read_private(state + 0x90, &mut globals)?;
        if tag != [8] || globals[8] != 5 || globals[9] != 0 {
            return Err("source.lua_layout_guards".into());
        }
        let mut pointer_after = [0; 8];
        p.read_module(&module, module.base() + root, &mut pointer_after)?;
        let mut code_after = [0; 58];
        p.read_module(&module, module.base() + anchor, &mut code_after)?;
        let mut tag_after = [0; 1];
        p.read_private(state + 16, &mut tag_after)?;
        let mut globals_after = [0; 24];
        p.read_private(state + 0x90, &mut globals_after)?;
        p.verify()?;
        if code_after != code
            || pointer_after != pointer
            || tag_after != tag
            || globals_after != globals
        {
            return Err("source.guard_changed".into());
        }
        Ok(
            json!({"schema":"lycheedev.current-source-witness.v1","process":p.identity(),"module":{"sha256":module.hash(),"base":module.base(),"size":module.size()},"source":{"value":root,"output":"pointer_slot","semanticEvidenceSha256":hex::encode(Sha256::digest(&evidence)),"anchorRva":anchor,"runtimeAnchorBytes":hex::encode(code)},"guards":{"stable":true,"luaState":state,"threadTag":tag[0],"globalsTag":globals[8],"globalsSecret":globals[9]},"scope":"Current Lua root, thread and globals TValue only","diskFinalization":false,"executionAdmission":false,"readCalls":8,"readBytes":182,"realClientFunctionalAcceptance":"not_run"}),
        )
    })();
    match run {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            println!("{}", json!({"state":"failed","issue":e}));
            std::process::exit(4)
        }
    }
}
