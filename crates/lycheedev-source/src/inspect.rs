//! Dedicated pinned-source inspection, including exact-path symbol selectors.
use crate::{assets, git, index};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence, selection::SourcePin, storage,
};
use serde_json::{Value, json};
use std::path::Path;

const TARGET_LIMIT: usize = 25;

pub(crate) fn target_with_control(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    symbol: Option<&str>,
    path: Option<&str>,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    // Go's public InspectSourceTarget ensures the pinned index before the
    // browser trims or validates target text. Preserve that first-error order.
    let index = index::build_with_control(home, pin, control)?;
    let symbol = symbol.unwrap_or("").trim();
    let path = path.unwrap_or("").trim();
    if symbol.is_empty() && path.is_empty() {
        return Err(ToolkitError::new(
            "command.failed",
            "codebase.inspect_target_required",
            5,
        ));
    }
    if !symbol.is_empty() && !path.is_empty() {
        return Err(ToolkitError::new(
            "command.failed",
            "codebase.inspect_target_conflict",
            5,
        ));
    }
    if path.len() > 4096 {
        return Err(ToolkitError::new(
            "command.failed",
            "codebase.invalid_span",
            5,
        ));
    }
    // A symbol may name an exact indexed document OR asset. A symbol-only query
    // projection cannot establish that distinction because it drops assets.
    if !symbol.is_empty() {
        let mut exact = false;
        if git::source_path(symbol) {
            for doc in &index.documents {
                control.check()?;
                exact |= doc.path == symbol;
            }
            for asset in &index.assets {
                control.check()?;
                exact |= asset.path == symbol;
            }
        }
        if !exact {
            return index.search_with_control(
                pin,
                snapshot,
                symbol,
                "precise",
                "",
                TARGET_LIMIT,
                "",
                control,
            );
        }
    }
    inspect_path(
        &index,
        pin,
        snapshot,
        if symbol.is_empty() { path } else { symbol },
        control,
    )
}

fn excerpt(raw: &[u8]) -> String {
    // Match Go numberedLines(1, 6, 0), including empty/trailing lines and its
    // historical CRLF/terminal-CR handling, without allocating the full text.
    let mut offset = 0usize;
    raw.split(|b| *b == b'\n')
        .take(6)
        .enumerate()
        .map(|(n, line)| {
            let newline = offset + line.len() < raw.len();
            offset += line.len() + 1;
            let line = if newline {
                line.strip_suffix(b"\r").unwrap_or(line)
            } else {
                line
            };
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            format!("{}: {}", n + 1, String::from_utf8_lossy(line))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn asset_local(
    index: &index::Index,
    pin: &SourcePin,
    asset: &index::AssetRecord,
    control: &OperationControl,
) -> Result<String> {
    let digest = asset.content_hash.as_str();
    if digest.is_empty() {
        return Ok(String::new());
    }
    let reference = json!({"sha256":digest,"bytes":asset.bytes});
    // Indexing already archives assets. Authenticate those original bytes on
    // every inspection; only native absence permits fixed-Git reconstruction.
    let cached = evidence::read_blob_bounded_optional_with_check(
        &index.home,
        &reference,
        assets::MAX_BYTES,
        control,
        || Ok(()),
    )?;
    if cached.is_none() {
        let path = asset.path.as_str();
        let (raw, _) =
            git::asset_blob_with_control(&index.home, pin, path, assets::MAX_BYTES, control)?;
        if asset.bytes != raw.len() || storage::sha256(&raw) != digest {
            return Err(ToolkitError::new(
                "codebase.asset_content_mismatch",
                "Indexed asset differs from the fixed Git object",
                4,
            ));
        }
        evidence::publish_blob_bounded(&index.home, &raw, assets::MAX_BYTES, control)?;
    }
    Ok(evidence::blob_path(&index.home, digest)?
        .to_string_lossy()
        .into_owned())
}

fn inspect_path(
    index: &index::Index,
    pin: &SourcePin,
    snapshot: &str,
    target: &str,
    control: &OperationControl,
) -> Result<Value> {
    let probe = assets::lowercase(target);
    let mut files = Vec::new();
    let mut assets = Vec::new();
    for doc in &index.documents {
        control.check()?;
        if assets::lowercase(&doc.path).contains(&probe) {
            files.push(doc);
        }
    }
    for asset in &index.assets {
        control.check()?;
        if assets::lowercase(asset.path.as_str()).contains(&probe) {
            assets.push(asset);
        }
    }
    let path_order = |a: &str, b: &str| (b == target).cmp(&(a == target)).then(a.cmp(b));
    files.sort_by(|a, b| path_order(&a.path, &b.path));
    assets.sort_by(|a, b| path_order(a.path.as_str(), b.path.as_str()));
    control.check()?;
    let truncated = files.len() + assets.len() > TARGET_LIMIT;
    let mut originals = index.original_reader();
    let result = (|| {
        let mut results = Vec::new();
        let mut source_bytes = 0usize;
        let mut source_documents = 0usize;
        for doc in files.into_iter().take(TARGET_LIMIT) {
            control.check()?;
            let raw = originals.read(doc, control)?;
            source_bytes += raw.len();
            source_documents += 1;
            results.push(json!({"kind":"file","name":doc.path.rsplit('/').next().unwrap_or(&doc.path),"path":doc.path,"line":1,"matchedBy":"path","role":index::role(&doc.path),"score":100,"scoreParts":{"match":100},"contentHash":doc.sha256,"excerpt":excerpt(&raw)}));
        }
        for asset in assets.into_iter().take(TARGET_LIMIT - results.len()) {
            control.check()?;
            let path = asset.path.as_str();
            let local = asset_local(index, pin, asset, control)?;
            let excerpt = format!(
                "format={} mime={} bytes={} width={} height={} local={}",
                asset.format.as_str(),
                asset.mime.as_str(),
                asset.bytes,
                asset.width,
                asset.height,
                local
            );
            results.push(json!({"kind":"asset","name":path.rsplit('/').next().unwrap_or(path),"path":path,"line":1,"matchedBy":"path","role":"project","score":100,"contentHash":asset.content_hash,"excerpt":excerpt}));
        }
        let mut response = json!({"sourceId":pin.repository,"product":pin.product,"requestedRef":pin.requested_ref,"matchedTag":null,"resolvedCommit":pin.exact_commit,"snapshotId":snapshot,"results":results,"complete":index.summary.complete&&!truncated,"truncated":truncated,"coverage":index.coverage()});
        if pin.requested_ref.is_empty() {
            response
                .as_object_mut()
                .expect("inspection response")
                .remove("requestedRef");
        }
        response["coverage"]["io"]["sourceBytesRead"] = json!(source_bytes);
        response["coverage"]["io"]["sourceDocumentsRead"] = json!(source_documents);
        if results.is_empty() {
            response["suggestions"] =
                json!(["use query --topic asset with a filename or a shorter path"]);
        }
        control.check()?;
        Ok(response)
    })();
    originals.finish(result, control)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspect_excerpt_uses_fffd_for_invalid_utf8_like_query_and_context() {
        let raw = b"function Before() end\n-- Night Web\x92s Hollow\r\nfunction After() end\n";
        let text = excerpt(raw);
        assert!(text.contains("2: -- Night Web\u{fffd}s Hollow"), "{text}");
        assert!(text.contains("3: function After() end"), "{text}");
    }
    #[test]
    fn inspect_six_lines_match_reference_empty_crlf_terminal_and_long_files() {
        for (raw, expected) in [
            ("", "1: "),
            ("a\n", "1: a\n2: "),
            ("a\r\nb\rc\r", "1: a\n2: b\rc"),
            ("a\r\r\r\nb", "1: a\r\n2: b"),
            (
                "1\n2\n3\n4\n5\n6\n7\n",
                "1: 1\n2: 2\n3: 3\n4: 4\n5: 5\n6: 6",
            ),
        ] {
            assert_eq!(excerpt(raw.as_bytes()), expected);
        }
    }

    #[test]
    fn inspect_metadata_only_asset_has_no_original_access_or_invented_dimensions() {
        let pin = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            exact_commit: "a".repeat(40),
            parser_revision: git::PARSER.into(),
            requested_ref: String::new(),
        };
        let index = index::Index {
            documents: vec![],
            symbols: vec![],
            assets: vec![assets::record(
                "Huge.blp",
                assets::MAX_BYTES + 1,
                &[],
                "",
                None,
            )],
            summary: crate::index::model::test_summary(
                json!({"repository":pin.repository,"product":pin.product,"commit":pin.exact_commit,"parser":pin.parser_revision,"complete":true,"skippedDocuments":0,"diagnostics":0}),
            ),
            compatibility_gaps: Default::default(),
            home: Default::default(),
        };
        let result = inspect_path(
            &index,
            &pin,
            "PIN",
            "Huge.blp",
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(
            result["results"],
            json!([{"kind":"asset","name":"Huge.blp","path":"Huge.blp","line":1,"matchedBy":"path","role":"project","score":100,"contentHash":"","excerpt":format!("format=blp mime=image/x-blp bytes={} width=0 height=0 local=", assets::MAX_BYTES + 1)}])
        );
        assert_eq!(result["complete"], true);
        assert_eq!(result["matchedTag"], Value::Null);
        assert!(result.get("requestedRef").is_none());
    }
}
