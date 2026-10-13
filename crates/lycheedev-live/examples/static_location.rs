//! Offline disk finalization/reuse evidence helper. Never opens a game process.
use lycheedev_live::locator::*;
use serde_json::json;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: static_location <source.exe> <method.json> <independent-evidence> [target.exe]"
        );
        std::process::exit(2)
    }
    let mut competition = None;
    let run = (|| -> std::result::Result<serde_json::Value, String> {
        let disk = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
        let pe = Pe::parse(&disk)?;
        let method: Method =
            serde_json::from_slice(&std::fs::read(&args[2]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let evidence = std::fs::read(&args[3]).map_err(|e| e.to_string())?;
        competition = Some(inspect_disk(&pe, &method)?);
        let (f, source) = finalize(&pe, method, &evidence)?;
        let target = if let Some(p) = args.get(4) {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            Some(locate(&Pe::parse(&bytes)?, &f)?)
        } else {
            None
        };
        Ok(json!({"source":source,"target":target,"realClientAcceptance":"not_run"}))
    })();
    match run {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            println!("{}", serde_json::to_string_pretty(&json!({"state":"failed","issue":e,"competition":competition,"executionAdmission":false})).unwrap());
            std::process::exit(4)
        }
    }
}
