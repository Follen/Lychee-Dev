use md5::{Digest, Md5};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn main() {
    let cancel = AtomicBool::new(false);
    for mib in [1, 16, 64] {
        let bytes: Vec<u8> = (0..mib * 1024 * 1024)
            .map(|i| ((i * 73 + i / 997) % 256) as u8)
            .collect();
        let expected = Md5::digest(&bytes);
        let actual = lycheedev_windows::legacy_digest::legacy_md5(
            &bytes,
            Instant::now() + Duration::from_secs(30),
            &cancel,
        )
        .unwrap();
        assert_eq!(actual.as_slice(), expected.as_slice());
        let rust = Instant::now();
        for _ in 0..5 {
            std::hint::black_box(Md5::digest(std::hint::black_box(&bytes)));
        }
        let rust_ms = rust.elapsed().as_secs_f64() * 200.0;
        let native = Instant::now();
        for _ in 0..5 {
            std::hint::black_box(
                lycheedev_windows::legacy_digest::legacy_md5(
                    std::hint::black_box(&bytes),
                    Instant::now() + Duration::from_secs(30),
                    &cancel,
                )
                .unwrap(),
            );
        }
        let native_ms = native.elapsed().as_secs_f64() * 200.0;
        println!(
            "{{\"mib\":{mib},\"rustCryptoMillis\":{rust_ms:.3},\"cngMillis\":{native_ms:.3},\"sameDigest\":true,\"ratio\":{:.4}}}",
            native_ms / rust_ms
        );
    }
}
