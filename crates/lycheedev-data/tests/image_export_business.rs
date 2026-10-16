//! Actual asset export through authenticated CASC, held evidence and atomic
//! destinations. Encoded streams are decoded again to verify every RGBA byte.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Cell, Fixture, Table, table};
use lycheedev_core::{evidence, metadata::Metadata, storage};
use lycheedev_data::assets::texture;
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::PathBuf, sync::atomic::Ordering};

fn blp(encoding: u8, alpha: u8, mode: u8, width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let header = if encoding == 1 { 1172 } else { 148 };
    let mut raw = vec![0; header];
    raw[..4].copy_from_slice(b"BLP2");
    raw[4] = 1;
    raw[8..12].copy_from_slice(&[encoding, alpha, mode, 0x11]);
    raw[12..16].copy_from_slice(&width.to_le_bytes());
    raw[16..20].copy_from_slice(&height.to_le_bytes());
    raw[20..24].copy_from_slice(&(header as u32).to_le_bytes());
    raw[84..88].copy_from_slice(&(pixels.len() as u32).to_le_bytes());
    raw.extend_from_slice(pixels);
    raw
}
fn tables() -> Vec<Table> {
    let rgba = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 20, 40, 60, 72];
    let bgra: Vec<_> = rgba.chunks_exact(4).flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]]).collect();
    let mut palette = blp(1, 4, 0, 2, 1, &[0, 0, 0xf1]);
    palette[148..152].copy_from_slice(&[3, 5, 7, 99]);
    let colors = [0, 0xf8, 0xe0, 7, 0xe4, 0xe4, 0xe4, 0xe4];
    let mut bc2 = vec![0x21; 8]; bc2.extend(colors);
    let mut bc3 = vec![10, 20, 0xfe, 0, 0, 0, 0, 0]; bc3.extend(colors);
    let mut mip = blp(3, 8, 0, 2, 2, &[1, 2, 3, 4]);
    mip[20..24].fill(0); mip[84..88].fill(0);
    mip[24..28].copy_from_slice(&148u32.to_le_bytes());
    mip[88..92].copy_from_slice(&4u32.to_le_bytes());
    let mut truncated = blp(3, 8, 0, 2, 2, &bgra); truncated.pop();
    [
        ("BGRA", blp(3, 8, 0, 2, 2, &bgra)),
        ("Palette", palette),
        ("BCOne", blp(2, 0, 0, 3, 1, &colors)),
        ("BCTwo", blp(2, 4, 1, 2, 1, &bc2)),
        ("BCThree", blp(2, 8, 7, 2, 1, &bc3)),
        ("SparseMip", mip),
        ("Truncated", truncated),
        ("Unsupported", b"BLP1unsupported".to_vec()),
    ].into_iter().enumerate().map(|(index, (name, raw))| {
        let mut item = table(name, 501 + index as u32, &[("ID", "uint")], &[vec![Cell::U(7)]]);
        item.raw = raw;
        item
    }).collect()
}
fn decoded(encoding: &str, raw: &[u8]) -> (u32, u32, Vec<u8>) {
    if encoding == "png" {
        let mut reader = png::Decoder::new(Cursor::new(raw)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(frame.buffer_size());
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        (frame.width, frame.height, pixels)
    } else {
        let mut reader = image_webp::WebPDecoder::new(Cursor::new(raw)).unwrap();
        assert!(!reader.is_lossy());
        assert!(reader.has_alpha());
        let (width, height) = reader.dimensions();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.read_image(&mut pixels).unwrap();
        (width, height, pixels)
    }
}
fn project(mut rgba: Vec<u8>, channels: &str) -> Vec<u8> {
    for pixel in rgba.chunks_exact_mut(4) {
        if channels.len() == 1 {
            let index = b"rgba".iter().position(|&name| name == channels.as_bytes()[0]).unwrap();
            let value = pixel[index]; pixel.copy_from_slice(&[value, value, value, 255]);
        } else {
            for (index, name) in b"rgba".iter().enumerate() {
                if !channels.as_bytes().contains(name) { pixel[index] = if index == 3 { 255 } else { 0 }; }
            }
        }
    }
    rgba
}
#[test]
fn image_export_preserves_all_formats_channels_pixels_and_three_capture_chain() {
    let tables = tables();
    let fixture = Fixture::new(&tables);
    for encoding in ["png", "webp"] {
        for (index, item) in tables[..6].iter().enumerate() {
            let mip = usize::from(index == 5);
            let expected = texture::decode(&item.raw, mip, 16 << 20, &Default::default()).unwrap();
            let channels: &[&str] = if index == 0 { &["rgba", "rgb", "r", "g", "b", "a", "ra", "bg", "abgr"] } else { &["rgba"] };
            for channels in channels {
                let output = fixture.root.join(format!("{}-{channels}.{encoding}", item.id));
                let id = item.id.to_string(); let mip = mip.to_string();
                let inv = fixture.invocation("asset export", &[("--file-id", &id), ("--encoding", encoding),
                    ("--mipmap", &mip), ("--channels", channels), ("--output", output.to_str().unwrap())]);
                let found = lycheedev_data::dispatch(&inv).unwrap();
                let raw = fs::read(&output).unwrap();
                let (width, height, pixels) = decoded(encoding, &raw);
                assert_eq!((width, height), (expected.description.width, expected.description.height));
                assert_eq!(pixels, project(expected.pixels.clone(), channels));
                assert_eq!(found.data["image"]["pixelsSHA256"], storage::sha256(&pixels));
                assert_eq!(found.data["image"]["texture"], serde_json::to_value(&expected.description).unwrap());
                assert_eq!(found.data["image"]["channels"], "rgba".chars().filter(|name| channels.contains(*name)).collect::<String>());
                assert_eq!(found.provenance.len(), 3);
                assert_eq!(found.data["sourceCapture"], found.provenance[0]);
                assert_eq!(found.data["artifactCapture"], found.provenance[2]);
                assert_eq!(found.data["content"], found.provenance[2]["blob"]);
                assert_eq!(found.data["content"]["sha256"], storage::sha256(&raw));
                for capture in &found.provenance {
                    assert_eq!(evidence::read_capture(&fixture.home, capture["id"].as_str().unwrap()).unwrap(), *capture);
                }
                assert_eq!(evidence::read_blob(&fixture.home, &found.provenance[0]["blob"], 1 << 20).unwrap(), item.raw);
                assert_eq!(evidence::read_blob(&fixture.home, &found.provenance[2]["blob"], 1 << 20).unwrap(), raw);
                let manifest = evidence::read_blob(&fixture.home, &found.provenance[1]["blob"], 1 << 20).unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&manifest).unwrap(), found.data);
            }
        }
    }
}

#[test]
fn image_export_faults_and_overwrite_keep_existing_destinations_intact() {
    let fixture = Fixture::new(&tables());
    for (file, flag, value, expected) in [
        ("501", "--max-pixels", "3", "texture.pixel_limit"),
        ("501", "--mipmap", "15", "texture.mipmap_unavailable"),
        ("501", "--channels", "rr", "records.export_options"),
        ("507", "--mipmap", "0", "texture.invalid_format"),
        ("508", "--mipmap", "0", "texture.unsupported_format"),
    ] {
        let output = fixture.root.join(format!("failed-{file}-{value}.png"));
        let inv = fixture.invocation("asset export", &[("--file-id", file), ("--encoding", "png"),
            (flag, value), ("--output", output.to_str().unwrap())]);
        assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, expected);
        assert!(!output.exists());
    }
    let captures = Metadata::open(&fixture.home, false).unwrap().list::<Value>("capture/", 100).unwrap();
    assert!(captures.iter().all(|capture| capture["provenance"]["kind"] == "asset"));
    let output = fixture.root.join("replace.webp");
    let alias = fixture.root.join("old-image.bin");
    fs::write(&output, b"old bytes").unwrap(); fs::hard_link(&output, &alias).unwrap();
    let mut inv = fixture.invocation("asset export", &[("--file-id", "501"), ("--encoding", "webp"),
        ("--output", output.to_str().unwrap())]);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.export_conflict");
    inv.flags.insert("--overwrite".into(), "true".into());
    lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(fs::read(&alias).unwrap(), b"old bytes");
    let current = fs::read(&output).unwrap();
    assert_eq!(decoded("webp", &current).0, 2);
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    assert_eq!(fs::read(output).unwrap(), current);
}

#[test]
#[ignore = "prepare authenticated BLP inputs for actual public Go/Rust image export"]
fn prepare_image_export_public_fixture() {
    let root = PathBuf::from(std::env::var_os("LYCHEEDEV_IMAGE_EXPORT_FIXTURE").expect("fresh fixture directory"));
    let tables = tables();
    let fixture = Fixture::at(root, &tables, "zhCN", 0x40);
    let cases: Vec<_> = tables[..6].iter().enumerate().map(|(index, item)| {
        let mipmap = usize::from(index == 5);
        let image = texture::decode(&item.raw, mipmap, 16 << 20, &Default::default()).unwrap();
        json!({"name":item.name,"fileDataID":item.id,"mipmap":mipmap,
            "texture":image.description,"pixelsRGBA":image.pixels,"pixelsSHA256":storage::sha256(&image.pixels)})
    }).collect();
    fs::write(fixture.root.join("image-cases.json"), serde_json::to_vec_pretty(&json!({
        "cases":cases,"encodings":["png","webp"],"channels":["rgba","a","abgr"],
        "comparison":"lossless decoded RGBA/pixelsSHA256 and image/source metadata; codec bitstreams have independent artifact hashes",
        "malformedFileDataID":507,"unsupportedFileDataID":508
    })).unwrap()).unwrap();
}

#[test]
fn large_image_export_uses_default_verified_decode_and_preserves_every_pixel() {
    // Larger than the CPU threshold, clipped in both BC dimensions. These go
    // through actual default CASC/VerifiedBlob/image/export consumers, including
    // the shared safety ledger and all three durable capture identities.
    let (width, height) = (1031u32, 519u32);
    let count = width as usize * height as usize;
    let mut inputs = Vec::new();
    for (index, (name, encoding, alpha, mode)) in [
        ("LargePalette", 1u8, 1u8, 0u8),
        ("LargeBGRA", 3, 8, 0),
        ("LargeBCOne", 2, 0, 0),
        ("LargeBCTwo", 2, 4, 1),
        ("LargeBCThree", 2, 8, 7),
    ].into_iter().enumerate() {
        let bytes = match encoding {
            1 => count + count.div_ceil(8),
            3 => count * 4,
            _ => (width as usize).div_ceil(4) * (height as usize).div_ceil(4) * if mode == 0 { 8 } else { 16 },
        };
        let payload: Vec<_> = (0..bytes).map(|i| ((i * 173 + i / 7) & 255) as u8).collect();
        let mut raw = blp(encoding, alpha, mode, width, height, &payload);
        if encoding == 1 {
            for (index, color) in raw[148..1172].chunks_exact_mut(4).enumerate() {
                color.copy_from_slice(&[index as u8, (index * 3) as u8, (index * 7) as u8, 99]);
            }
        }
        let mut item = table(name, 701 + index as u32, &[("ID", "uint")], &[vec![Cell::U(7)]]);
        item.raw = raw;
        inputs.push(item);
    }
    let fixture = Fixture::new(&inputs);
    for item in &inputs {
        // The public pure decoder intentionally stays serial: an independent
        // ordering oracle for the default budgeted worker implementation.
        let expected = texture::decode(&item.raw, 0, 16 << 20, &Default::default()).unwrap();
        for encoding in ["png", "webp"] {
            let output = fixture.root.join(format!("large-{}.{encoding}", item.id));
            let id = item.id.to_string();
            let inv = fixture.invocation("asset export", &[("--file-id", &id), ("--encoding", encoding), ("--output", output.to_str().unwrap())]);
            let result = lycheedev_data::dispatch(&inv).unwrap();
            let raw = fs::read(&output).unwrap();
            let (actual_width, actual_height, pixels) = decoded(encoding, &raw);
            assert_eq!((actual_width, actual_height), (width, height));
            assert_eq!(pixels, expected.pixels);
            assert_eq!(result.data["image"]["texture"], serde_json::to_value(&expected.description).unwrap());
            assert_eq!(result.data["image"]["pixelsSHA256"], storage::sha256(&pixels));
            assert_eq!(result.provenance.len(), 3);
            assert_eq!(result.data["sourceCapture"], result.provenance[0]);
            assert_eq!(result.data["artifactCapture"], result.provenance[2]);
            assert_eq!(result.data["content"]["sha256"], storage::sha256(&raw));
            assert_eq!(evidence::read_blob(&fixture.home, &result.provenance[0]["blob"], 8 << 20).unwrap(), item.raw);
            let manifest = evidence::read_blob(&fixture.home, &result.provenance[1]["blob"], 1 << 20).unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&manifest).unwrap(), result.data);
        }
    }
}
