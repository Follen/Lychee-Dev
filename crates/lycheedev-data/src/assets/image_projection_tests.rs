//! The former per-pixel loop remains the byte/hash/control oracle here.
use super::*;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn legacy_transform_and_hash(pixels: &mut [u8], channels: &str, budget: &Budget) -> Result<String> {
    let mut digest = Sha256::new();
    for block in pixels.chunks_mut(16384) {
        budget.charge(Cost::work(1))?;
        for pixel in block.as_chunks_mut::<4>().0 {
            if channels.len() == 1 {
                let selected = b"rgba"
                    .iter()
                    .position(|&name| name == channels.as_bytes()[0])
                    .expect("normalized channels");
                let value = pixel[selected];
                pixel.copy_from_slice(&[value, value, value, 255]);
            } else {
                for (component, name) in b"rgba".iter().enumerate() {
                    if !channels.as_bytes().contains(name) {
                        pixel[component] = if component == 3 { 255 } else { 0 };
                    }
                }
            }
        }
        digest.update(block);
    }
    Ok(crate::casc::hex(&digest.finalize()))
}

fn compare_projection(input: &[u8], channels: &str, work: u64, stopped: bool, expired: bool) {
    let deadline = if expired {
        Instant::now() - Duration::from_secs(1)
    } else {
        Instant::now() + Duration::from_secs(60)
    };
    let cancel = Arc::new(AtomicBool::new(stopped));
    let limits = Cost {
        decode_work: work,
        ..Cost::defaults()
    };
    let old_budget = Budget::new(limits, deadline, Some(cancel.clone()));
    let new_budget = Budget::new(limits, deadline, Some(cancel));
    let mut expected_pixels = input.to_vec();
    let mut actual_pixels = input.to_vec();
    let expected = legacy_transform_and_hash(&mut expected_pixels, channels, &old_budget)
        .map_err(|error| (error.code, error.message, error.exit_code));
    let actual = transform_and_hash(&mut actual_pixels, channels, &new_budget)
        .map_err(|error| (error.code, error.message, error.exit_code));
    assert_eq!(
        actual, expected,
        "channels={channels}, work={work}, stopped={stopped}, expired={expired}"
    );
    assert_eq!(actual_pixels, expected_pixels);
    assert_eq!(
        new_budget.snapshot().unwrap(),
        old_budget.snapshot().unwrap()
    );
    assert_eq!(
        new_budget.logical_snapshot().unwrap(),
        old_budget.logical_snapshot().unwrap()
    );
}
fn channels() -> impl Iterator<Item = String> {
    (1u8..16).map(|mask| {
        b"rgba"
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1u8 << *i) != 0)
            .map(|(_, &name)| name as char)
            .collect()
    })
}
fn pixels(bytes: usize) -> Vec<u8> {
    (0..bytes)
        .map(|i| ((i * 173 + i / 7) & 255) as u8)
        .collect()
}

#[test]
fn fixed_channels_match_original_all_subsets_alpha_and_hash_block_boundaries() {
    for bytes in [0, 4, 16380, 16384, 16388, 32772] {
        let pixels = pixels(bytes);
        for channels in channels().chain(std::iter::once("abgr".into())) {
            compare_projection(&pixels, &channels, 100, false, false);
        }
    }
    assert!(matches!(Channels::new("rgba"), Channels::Identity));
    assert!(matches!(Channels::new("abgr"), Channels::Identity));
    let shared = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        None,
    );
    let mut alpha_edges = [1, 2, 3, 0, 4, 5, 6, 1, 7, 8, 9, 254, 10, 11, 12, 255];
    let original = alpha_edges;
    let hash = transform_and_hash(&mut alpha_edges, "rgba", &shared).unwrap();
    assert_eq!(alpha_edges, original);
    assert_eq!(hash, lycheedev_core::storage::sha256(&original));
    assert_eq!(shared.snapshot().unwrap(), Cost::work(1));
}

#[test]
fn fixed_channels_preserve_work_exhaustion_cancel_deadline_and_partial_mutation() {
    let pixels = pixels(32772);
    for channels in channels() {
        for work in 0..=3 {
            compare_projection(&pixels, &channels, work, false, false);
        }
        compare_projection(&pixels, &channels, 100, true, false);
        compare_projection(&pixels, &channels, 100, false, true);
    }
}

#[test]
fn fixed_channels_keep_identical_png_and_webp_bytes() {
    let original = pixels(16 * 4);
    for channels in channels() {
        for encoding in ["png", "webp"] {
            let mut artifacts = Vec::new();
            for former in [true, false] {
                let shared = Budget::new(
                    Cost::defaults(),
                    Instant::now() + Duration::from_secs(60),
                    None,
                );
                let mut pixels = original.clone();
                let hash = if former {
                    legacy_transform_and_hash(&mut pixels, &channels, &shared).unwrap()
                } else {
                    transform_and_hash(&mut pixels, &channels, &shared).unwrap()
                };
                let mut output = Output {
                    bytes: Vec::new(),
                    maximum: 1 << 20,
                    budget: &shared,
                    allocation: None,
                    error: None,
                };
                if encoding == "png" {
                    png(&mut output, &pixels, 4, 4).unwrap();
                } else {
                    webp(&mut output, &pixels, 4, 4).unwrap();
                }
                assert!(output.error.is_none());
                artifacts.push((hash, output.bytes.clone(), shared.snapshot().unwrap()));
                drop(output);
                assert_eq!(shared.snapshot().unwrap().retained_bytes, 0);
            }
            assert_eq!(artifacts[0], artifacts[1], "{encoding}, {channels}");
        }
    }
}
