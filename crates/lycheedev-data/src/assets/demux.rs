//! VP9 AVI use case. Every frame borrows the single authenticated input buffer;
//! all output names preflight before writing, with the manifest published last.
use super::{avi, fault, input, output};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    evidence::{self, CaptureInput},
    storage,
};
use serde_json::{Value, json};
use std::path::Path;
const MANIFEST: &str = "demux-manifest.json";

fn capture(
    inv: &Invocation,
    bytes: &[u8],
    maximum: usize,
    media_type: &str,
    provenance: Value,
    complete: bool,
    truncated: bool,
) -> Result<Value> {
    evidence::commit_capture_bounded(
        &inv.home,
        CaptureInput {
            bytes,
            maximum_bytes: maximum,
            media_type,
            provenance,
            complete,
            truncated,
        },
        &inv.control,
    )
}
pub fn run(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let path = inv.flag("--path").filter(|value| !value.is_empty());
    let id = inv
        .flag("--file-id")
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?
        .filter(|id| *id != 0);
    let output_path = inv.flag("--output").filter(|value| !value.is_empty());
    if output_path.is_none()
        || path.is_some() == id.is_some()
        || path.is_none() && !inv.flag_bool("--cdn") && inv.flag("--installation").is_none()
        || path.is_some()
            && (inv.flag("--snapshot").is_some()
                || inv.flag_bool("--cdn")
                || inv.flag("--installation").is_some())
    {
        return Err(ToolkitError::arguments(
            "asset demux requires --output and exactly one input: --path, or --snapshot/--file-id with --installation or --cdn",
        ));
    }
    let maximum = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    if maximum < 12 {
        return Err(fault("records.demux_input_limit", 3));
    }
    let max_frames = inv.usize_flag("--max-frames", 100000, 1, 1 << 24)?;
    let directory = output::OutputDirectory::open(
        &inv.home,
        Path::new(output_path.expect("validated output")),
        inv.flag_bool("--overwrite"),
    )?;
    directory.preflight(MANIFEST)?;
    let (bytes, source, source_capture, snapshot, data_build, input_name) = if let Some(path) = path
    {
        let (bytes, canonical) = input::local(inv, Path::new(path), maximum)?;
        let source_capture = capture(
            inv,
            &bytes,
            maximum,
            "video/x-msvideo",
            json!({"kind":"asset-demux-source","locator":format!("file:{canonical}"),"snapshot":""}),
            true,
            false,
        )?;
        let source = json!({"kind":"file","input":path,"path":canonical,"bytes":bytes.len(),"sha256":source_capture["blob"]["sha256"]});
        (
            bytes,
            source,
            source_capture,
            String::new(),
            String::new(),
            path.to_owned(),
        )
    } else {
        let input = input::capture_casc(inv, maximum)?;
        let input_name = format!("casc:fdid:{}", id.expect("validated ID"));
        let source = json!({"kind":"casc","input":input_name,"fileDataID":id,"snapshot":input.snapshot,"pin":input.reading["pin"],"contentKey":input.reading["entry"]["contentKey"],"encodingKey":input.reading["payloadEncodingKey"],"bytes":input.bytes.len(),"sha256":input.capture["blob"]["sha256"]});
        (
            input.bytes,
            source,
            input.capture,
            input.snapshot,
            input.data_build,
            input_name,
        )
    };
    let parsed = avi::parse(
        &bytes,
        avi::Limits {
            max_frames,
            max_frame_bytes: 256 << 20,
            max_total_bytes: 1 << 30,
            allow_partial: inv.flag_bool("--allow-partial"),
        },
        &inv.control,
    )?;
    let names: Vec<String> = parsed
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| format!("frame-{:06}.{}", index + 1, frame.chunk))
        .collect();
    for name in &names {
        inv.control.check()?;
        directory.preflight(name)?;
    }
    directory.preflight(MANIFEST)?;
    let mut captures = vec![source_capture.clone()];
    let mut frames = Vec::new();
    for (index, frame) in parsed.frames.iter().enumerate() {
        inv.control.check()?;
        let frame_capture = capture(
            inv,
            frame.payload,
            256 << 20,
            "video/VP9",
            json!({"kind":"asset-demux-frame","locator":format!("{input_name}#frame={index}"),"snapshot":snapshot,"dataBuild":data_build}),
            true,
            false,
        )?;
        let mut frame_info = json!({"index":index,"type":"key","timestamp":frame.timestamp,"duration":frame.duration,"size":frame.payload.len(),"chunk":frame.chunk,"offset":frame.offset,"output":names[index],"sha256":frame_capture["blob"]["sha256"],"blob":frame_capture["blob"],"capture":frame_capture["id"]});
        if !frame.sub_frames.is_empty() {
            frame_info["subFrames"] = serde_json::to_value(&frame.sub_frames)?;
        }
        frames.push(frame_info);
        captures.push(frame_capture);
    }
    let mut manifest = json!({"schema":"lycheedev.asset-demux.v1","input":input_name,"source":source,"output":output_path,"width":parsed.width,"height":parsed.height,"frameRate":parsed.frame_rate,"frameCount":parsed.frames.len(),"enumeratedFrames":parsed.enumerated_frames,"enumeratedBytes":parsed.enumerated_bytes,"frames":frames,"complete":!parsed.truncated,"truncated":parsed.truncated,"limits":{"maxInputBytes":maximum,"maxFrames":max_frames,"maxFrameBytes":256<<20,"maxOutputBytes":1u64<<30},"sourceCapture":source_capture["id"]});
    if !snapshot.is_empty() {
        manifest["snapshot"] = json!(snapshot);
    }
    if !parsed.truncation_reason.is_empty() {
        manifest["truncationReason"] = json!(parsed.truncation_reason);
    }
    if !parsed.warnings.is_empty() {
        manifest["warnings"] = serde_json::to_value(&parsed.warnings)?;
    }
    // Serialize with an admission bound before allocating a publication body.
    let body = storage::bounded_json(&manifest, 16 << 20)?;
    let manifest_capture = capture(
        inv,
        &body,
        16 << 20,
        "application/json",
        json!({"kind":"asset-demux-manifest","locator":input_name,"snapshot":snapshot,"dataBuild":data_build}),
        !parsed.truncated,
        parsed.truncated,
    )?;
    captures.insert(1, manifest_capture);
    for (name, frame) in names.iter().zip(&parsed.frames) {
        directory.publish(inv, name, frame.payload, 1 << 30)?;
    }
    directory.publish(inv, MANIFEST, &body, 16 << 20)?;
    let mut outcome = Outcome::new(manifest);
    outcome.complete = !parsed.truncated;
    outcome.selection = json!({"snapshot":snapshot});
    outcome.provenance = captures;
    if parsed.truncated {
        outcome.warnings.push(json!(
            "Demux output is truncated; inspect truncationReason and limits."
        ));
    }
    Ok(outcome)
}
