//! Captured original bytes and a manifest precede one atomic external publish.
use super::{fault, image, input, output};
use crate::capture;
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    evidence::{self, CaptureInput},
};
use serde_json::json;
use std::path::Path;

fn number(inv: &Invocation, flag: &str, default: usize) -> Result<usize> {
    inv.flag(flag)
        .map(str::parse)
        .transpose()
        .map_err(|_| ToolkitError::arguments(format!("invalid {flag}")))
        .map(|value| value.unwrap_or(default))
}
pub fn run(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    if inv.flag_bool("--cdn") && inv.flag("--installation").is_some() {
        return Err(ToolkitError::arguments(
            "choose either --cdn or --installation, not both",
        ));
    }
    if (!inv.flag_bool("--cdn") && inv.flag("--installation").is_none())
        || inv.flag("--file-id").is_none()
    {
        return Err(ToolkitError::arguments(
            "asset export requires --snapshot, --file-id, and either --installation or --cdn",
        ));
    }
    let output_path = inv
        .flag("--output")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ToolkitError::arguments("asset export requires --output"))?;
    let encoding = inv
        .flag("--encoding")
        .filter(|value| !value.is_empty())
        .unwrap_or("raw");
    let mipmap = number(inv, "--mipmap", 0)?;
    let channels = inv.flag("--channels").unwrap_or("");
    let pixels = number(inv, "--max-pixels", 0)?;
    if encoding == "raw" && (mipmap != 0 || !channels.is_empty() || pixels != 0)
        || !["raw", "png", "webp"].contains(&encoding)
        || mipmap > 15
        || encoding != "raw" && pixels > 64 << 20
        || channels.chars().any(|channel| !"rgba".contains(channel))
        || channels
            .chars()
            .any(|channel| channels.matches(channel).count() != 1)
    {
        return Err(fault("records.export_options", 2));
    }
    let destination = output::destination(
        &inv.home,
        Path::new(output_path),
        inv.flag_bool("--overwrite"),
    )?;
    let maximum = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    let mut source = input::capture_casc_verified(inv)?;
    let channels: String = if channels.is_empty() { "rgba".into() }
        else { "rgba".chars().filter(|name| channels.contains(*name)).collect() };
    let encoded = if encoding == "raw" { None } else {
        Some(image::encode(&mut source.blob, encoding, mipmap, &channels,
            if pixels == 0 { 16 << 20 } else { pixels }, maximum, &source.budget, &inv.control)?)
    };
    let artifact = if let Some(encoded) = &encoded {
        evidence::commit_capture_budgeted(
            &inv.home,
            CaptureInput {
                bytes: &encoded.bytes, maximum_bytes: maximum,
                media_type: &format!("image/{encoding}"),
                provenance: json!({"kind":"asset-image","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
                complete: true, truncated: false,
            },
            &inv.control,
            source.budget.as_ref(),
        )?
    } else { source.capture.clone() };
    let mut manifest = json!({"schema":"lycheedev.asset-export.v1","snapshot":source.snapshot,"encoding":encoding,"output":destination.path().to_string_lossy(),"content":artifact["blob"],"source":source.reading,"sourceCapture":source.capture});
    if let Some(encoded) = &encoded {
        manifest["image"] = serde_json::to_value(&encoded.info)?;
        manifest["artifactCapture"] = artifact.clone();
    }
    let body = capture::json_bytes(&manifest, &source.budget, 1 << 20,
        || ToolkitError::invalid("serialized JSON exceeds admission"))?;
    let capture = evidence::commit_capture_budgeted(
        &inv.home,
        CaptureInput {
            bytes: &body,
            maximum_bytes: 1 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"asset-export-manifest","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
            complete: true,
            truncated: false,
        },
        &inv.control,
        source.budget.as_ref(),
    )?;
    let bytes = artifact["blob"]["bytes"].as_u64().expect("committed artifact length");
    let _copy_buffer = source.budget.reserve_retained(65536)?;
    source.budget.charge(crate::resource::Cost::work(bytes.div_ceil(65536) as usize))?;
    if let Some(encoded) = &encoded {
        output::publish(inv, &destination, &encoded.bytes, maximum)?;
    } else {
        output::publish_reader(inv, &destination, &mut source.blob, bytes, maximum)?;
    }
    let mut outcome = Outcome::new(manifest);
    outcome.selection = json!({"snapshot":source.snapshot});
    outcome.provenance = vec![source.capture, capture];
    if encoding != "raw" {
        outcome.provenance.push(artifact);
    }
    Ok(outcome)
}
