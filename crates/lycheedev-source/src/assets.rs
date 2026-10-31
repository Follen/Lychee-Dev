//! Asset header facts and shared asset-search normalization.
use super::index::AssetRecord;
use lycheedev_core::evidence::BlobRef;
#[cfg(test)]
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const MAX_BYTES: usize = 64 << 20;

pub(crate) fn lowercase(value: &str) -> String {
    // Paths use one lowercase Unicode scalar per input scalar, so a case
    // mapping cannot expand a path component into multiple characters.
    value
        .chars()
        .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
        .collect()
}

pub(crate) fn normalize_path(value: &str) -> String {
    lowercase(&value.replace('/', "\\"))
}

pub(crate) fn record(
    path: &str,
    bytes: usize,
    raw: &[u8],
    hash: &str,
    local: Option<BlobRef>,
) -> AssetRecord {
    let extension = format!(
        ".{}",
        Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    );
    let mime = match extension.as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".tga" => "image/x-tga",
        ".blp" => "image/x-blp",
        ".dds" => "image/vnd-ms.dds",
        ".ttf" => "font/ttf",
        ".otf" => "font/otf",
        ".ogg" => "audio/ogg",
        ".mp3" => "audio/mpeg",
        ".wav" => "audio/wav",
        _ => "application/octet-stream",
    };
    let (width, height, format) = if extension == ".png"
        && raw.len() >= 24
        && raw[..8] == [137, 80, 78, 71, 13, 10, 26, 10]
        && raw[8..12] == 13u32.to_be_bytes()
        && &raw[12..16] == b"IHDR"
    {
        (
            u32::from_be_bytes(raw[16..20].try_into().expect("checked PNG width")),
            u32::from_be_bytes(raw[20..24].try_into().expect("checked PNG height")),
            "png",
        )
    } else if (extension == ".jpg" || extension == ".jpeg") && raw.len() > 4 {
        (0, 0, "jpeg")
    } else if extension == ".blp" && raw.len() >= 20 && matches!(&raw[..4], b"BLP1" | b"BLP2") {
        (
            u32::from_le_bytes(raw[12..16].try_into().expect("checked BLP width")),
            u32::from_le_bytes(raw[16..20].try_into().expect("checked BLP height")),
            "blp",
        )
    } else {
        (0, 0, extension.strip_prefix('.').unwrap_or(&extension))
    };
    AssetRecord {
        path: path.into(),
        content_hash: hash.into(),
        bytes,
        mime: mime.into(),
        format: format.into(),
        extension,
        width,
        height,
        local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_dimensions_require_the_initial_ihdr_chunk() {
        // PNG's first chunk is a 13-byte IHDR. These are header observations,
        // not proof that the remaining compressed image is decodable.
        for (length, kind, expected) in [
            (13u32, b"IHDR", (123, 456)),
            (13, b"IDAT", (0, 0)),
            (12, b"IHDR", (0, 0)),
            (13, b"ihdr", (0, 0)),
        ] {
            let mut raw = vec![0; 33];
            raw[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            raw[8..12].copy_from_slice(&length.to_be_bytes());
            raw[12..16].copy_from_slice(kind);
            raw[16..20].copy_from_slice(&123u32.to_be_bytes());
            raw[20..24].copy_from_slice(&456u32.to_be_bytes());
            raw[24] = 8;
            raw[25] = 6;
            let row = record("image.png", raw.len(), &raw, "fixed", None);
            assert_eq!((row.width, row.height), expected, "{length}/{kind:?}");
        }
    }

    #[test]
    fn blp_dimensions_require_a_recognized_header() {
        for magic in [b"BLP1", b"BLP2", b"BLP9", b"nope"] {
            let mut raw = vec![0; 20];
            raw[..4].copy_from_slice(magic);
            raw[12..16].copy_from_slice(&64u32.to_le_bytes());
            raw[16..20].copy_from_slice(&32u32.to_le_bytes());
            let row = record("image.blp", raw.len(), &raw, "fixed", None);
            let expected = if matches!(magic, b"BLP1" | b"BLP2") {
                (64, 32)
            } else {
                (0, 0)
            };
            assert_eq!((row.width, row.height), expected, "{magic:?}");
        }
    }

    #[test]
    fn asset_normalization_preserves_simple_unicode_and_both_slashes() {
        for (input, expected) in [
            ("Assets/ÜBER.BLP", "assets\\über.blp"),
            ("ASSETS\\ASCII.PNG", "assets\\ascii.png"),
            ("İ/ẞ/Σ", "i\\ß\\σ"),
        ] {
            assert_eq!(normalize_path(input), expected);
        }
        assert_eq!(lowercase("ÜBER.blp"), "über.blp");
    }

    #[test]
    fn asset_metadata_preserves_headers_fallbacks_and_metadata_only_rows() {
        let mut png = vec![0; 24];
        png[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        png[8..12].copy_from_slice(&13u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&123u32.to_be_bytes());
        png[20..24].copy_from_slice(&456u32.to_be_bytes());
        let row = serde_json::to_value(record(
            "Image.PNG",
            png.len(),
            &png,
            "hash",
            Some(BlobRef {
                sha256: "hash".into(),
                bytes: png.len() as u64,
            }),
        ))
        .unwrap();
        assert_eq!(
            row,
            json!({"path":"Image.PNG","contentHash":"hash","bytes":24,"extension":".png","mime":"image/png","format":"png","width":123,"height":456,"local":{"sha256":"hash","bytes":24}})
        );
        let mut blp = vec![0; 20];
        blp[..4].copy_from_slice(b"BLP2");
        blp[12..16].copy_from_slice(&64u32.to_le_bytes());
        blp[16..20].copy_from_slice(&32u32.to_le_bytes());
        let row = serde_json::to_value(record("a.blp", 20, &blp, "h", None)).unwrap();
        assert_eq!(row["width"], 64);
        assert_eq!(row["height"], 32);
        assert_eq!(row["mime"], "image/x-blp");
        for (path, raw, format) in [
            ("a.jpg", vec![0; 4], "jpg"),
            ("a.jpg", vec![0; 5], "jpeg"),
            ("a.jpeg", vec![], "jpeg"),
            ("a.png", vec![0; 24], "png"),
            ("a.blp", vec![0; 19], "blp"),
        ] {
            let row = serde_json::to_value(record(path, raw.len(), &raw, "", None)).unwrap();
            assert_eq!(row["format"], format);
            assert_eq!(row["width"], 0);
            assert_eq!(row["height"], 0);
        }
        for (ext, mime) in [
            ("tga", "image/x-tga"),
            ("dds", "image/vnd-ms.dds"),
            ("ttf", "font/ttf"),
            ("otf", "font/otf"),
            ("ogg", "audio/ogg"),
            ("mp3", "audio/mpeg"),
            ("wav", "audio/wav"),
            ("m2", "application/octet-stream"),
            ("wmo", "application/octet-stream"),
        ] {
            let row = serde_json::to_value(record(
                &format!("asset.{ext}"),
                MAX_BYTES + 1,
                &[],
                "",
                None,
            ))
            .unwrap();
            assert_eq!(row["mime"], mime);
            assert_eq!(row["format"], ext);
            assert_eq!(row["contentHash"], "");
            assert_eq!(row["width"], 0);
            assert_eq!(row["height"], 0);
        }
    }
}
