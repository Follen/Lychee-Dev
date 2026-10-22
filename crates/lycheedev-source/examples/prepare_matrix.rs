//! Explicit read-only source-network fixture preparation. Never called by
//! production: branch observations become exact immutable matrix input pins.
use lycheedev_core::{
    Result,
    control::OperationControl,
    evidence,
    selection::{self, SelectionSpec},
    workspace,
};
use lycheedev_source::{git, index};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<()> {
    let home = std::path::absolute(PathBuf::from(
        std::env::args_os().nth(1).expect("new task-owned home"),
    ))?;
    if home.exists() {
        panic!("fresh explicit fixture home required");
    }
    workspace::initialize(&home)?;
    let control = OperationControl::default();
    let baselines = std::env::args().nth(2).as_deref() == Some("--baselines");
    let mut pins = Vec::new();
    for product in ["retail", "classic", "titan", "forever"] {
        control.check()?;
        let source = git::resolve_source_with_control(
            &home,
            "wow-ui-source",
            product,
            match product {
                "retail" => "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b",
                "classic" if baselines => "1028c1e687f721ba9d3af14d1b12a5745e4227c7",
                "titan" if baselines => "825d29d3662b372f0bead725ee6abd339e4a77b5",
                "forever" if baselines => "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069",
                _ => "",
            },
            &control,
        )?;
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                source: Some(source.clone()),
                ..Default::default()
            },
        )?;
        let indexed = index::build_with_control(&home, &source, &control)?;
        let observation = json!({"pin":pin,"index":indexed.summary,"scope":"read-only source fixture; observed named refs are frozen exact commits; not a live-client acceptance"});
        let capture = evidence::commit_capture(
            &home,
            &serde_json::to_vec(&observation)?,
            "application/json",
            json!({"kind":"source-matrix-input","locator":product,"sourceCommit":source.exact_commit}),
            true,
            false,
        )?;
        println!(
            "{}",
            json!({"product":product,"pin":pin.id,"commit":source.exact_commit,"capture":capture["id"]})
        );
        pins.push(json!({"product":product,"pin":pin,"observation":capture}));
        fs::write(
            home.parent().unwrap().join("prepared-pins.json"),
            serde_json::to_vec_pretty(
                &json!({"schema":"lycheedev.source-matrix-input.v1","home":home,"targets":pins,"scope":"source-only fixed observations; Forever is not promoted to interactive game acceptance"}),
            )?,
        )?;
    }
    Ok(())
}
