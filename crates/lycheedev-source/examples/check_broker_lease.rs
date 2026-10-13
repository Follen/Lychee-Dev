//! Test-only post-prune lease acquisition; no backend shortcut or game input.
use lycheedev_core::{Result, selection, storage::ExclusiveLock, workspace};
use std::path::PathBuf;
fn run() -> Result<()> {
    let home = PathBuf::from(std::env::args_os().nth(1).expect("fixed task home"));
    let id = std::env::args().nth(2).expect("fixed Source selection");
    workspace::open(&home)?;
    let pin = selection::load_pinned(&home, &id)?;
    let source = pin.source.as_ref().expect("SourcePin");
    let key = format!("worktree:{}:{}", source.repository, source.exact_commit);
    let lease = ExclusiveLock::resource(&home.join("locks"), &key)?;
    println!(
        "{}",
        serde_json::json!({"state":"acquired","source":source,"key":key})
    );
    drop(lease);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}", serde_json::to_string(&error).unwrap());
        std::process::exit(error.exit_code);
    }
}
