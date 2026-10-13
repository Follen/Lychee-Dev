//! Offline independent archive locator prototype; cannot open a game or issue input admission.
use lycheedev_live::{
    locator::{Layout, Method, Parameter, Pe},
    runtime_archive::Archive,
};
use std::{io::Read, os::windows::fs::OpenOptionsExt};
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result = (|| -> Result<serde_json::Value, String> {
        if args.len() != 6 {
            return Err("usage: runtime_archive_location <original.exe> <source-manifest> <method.json> <independent-evidence.json> <target-manifest>".into());
        }
        let mut original = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&args[1])
            .map_err(|e| e.to_string())?;
        if original.metadata().map_err(|e| e.to_string())?.len() > 512 << 20 {
            return Err("pe.file_budget".into());
        }
        let mut bytes = Vec::new();
        original
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let pe = Pe::parse(&bytes)?;
        let mut method: Method =
            serde_json::from_slice(&std::fs::read(&args[3]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        method.id = "retail-lua-state-root-runtime-archive-rip-v1".into();
        let evidence = std::fs::read(&args[4]).map_err(|e| e.to_string())?;
        let source = Archive::open(std::path::Path::new(&args[2]), &pe, &method.section)?;
        let (archived, source_report) = source.finalize_source(&pe, method.clone(), &evidence)?;
        drop(source); // Target replay uses method/provenance, never source archive files.
        let target = Archive::open(std::path::Path::new(&args[5]), &pe, &method.section)?;
        let target_report = target.locate(&pe, &archived)?;
        let parameter = Parameter {
            name: "luaStateRoot".into(),
            method_id: method.id,
            expected_type: method.output,
            reader_pointer: "/root".into(),
        };
        let layout = Layout {
            id: "retail-current-thread-and-globals-only".into(),
            applicable_hashes: vec![pe.sha256.clone()],
            state_globals: 0x90,
            tag_offset: 16,
            string_length: 24,
            string_data: 32,
            table_nodes: 40,
            table_log2: 19,
            node_bytes: 56,
        };
        let value =
            lycheedev_live::runtime_archive::bind(&parameter, &archived, &target_report, &layout)?;
        Ok(
            serde_json::json!({"schema":"lycheedev.runtime-archive-prototype.v1","originalFile":args[1],"originalSha256":pe.sha256,"sourceReport":source_report,"targetReport":target_report,"parameter":parameter,"binding":{"type":"PointerSlot","value":value},"layout":layout,"executionAdmission":false,"scope":"offline archived domain search and typed binding; current OS instance revalidation/live admission not performed"}),
        )
    })();
    match result {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
