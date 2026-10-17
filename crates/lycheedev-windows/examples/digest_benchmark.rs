//! Isolated legacy-hash screening. No production decoder/backend is selected.
#![forbid(unsafe_code)]

use md5::{Digest, Md5};
use sha2::Sha256;
use std::{
    fs::{File, symlink_metadata},
    hint::black_box,
    io::Read,
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const SCHEMA: &str = "lycheedev.md5-stream-benchmark.v1";
const CHUNK: usize = 65536;
const PAYLOAD_BYTES: usize = 17 * 1024 * 1024;
const PAYLOAD_MD5: &str = "c9fa02f009aa62f0b0504a86cc983e2b";
const PAYLOAD_SHA256: &str = "397fa93fdb906d5a278075e087c5b1bef5c5f5fbfb3a080b438a3ba30f435db3";
const ROOT_MD5: &str = "8cea531c64ce544535550008b29e781c";
const ROOT_SHA256: &str = "554c8419b746697073fe5d0e956e380b98e7b439e925ca28ec8001d457a41d44";
const PAIRS: usize = 6;

fn main() {
    if let Err(error) = run() {
        eprintln!("digest_benchmark: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        legacy();
        return Ok(());
    }
    if args.len() != 2 || args[0] != "--fixture-stream" {
        return Err(
            "usage: digest_benchmark [--fixture-stream <authenticated-44-byte-root>]".into(),
        );
    }
    stream_fixture(Path::new(&args[1]))
}

// The original no-argument comparator and its arithmetic means stay intact.
// CNG is whole-message, using its existing internal 1 MiB chunks. It is never
// labeled a persistent 64 KiB streaming comparison.
fn legacy() {
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
            black_box(Md5::digest(black_box(&bytes)));
        }
        let rust_ms = rust.elapsed().as_secs_f64() * 200.0;
        let native = Instant::now();
        for _ in 0..5 {
            black_box(
                lycheedev_windows::legacy_digest::legacy_md5(
                    black_box(&bytes),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Md5Clone,
    Md5Consume,
    Sha256Md5Consume,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Md5Clone => "md5_clone",
            Self::Md5Consume => "md5_consume",
            Self::Sha256Md5Consume => "sha256_md5_consume",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Point {
    BeforeNew,
    BeforeUpdate(usize),
    BeforeFinalize,
    AfterClone,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stop {
    Cancelled,
    Deadline,
}
struct Sample {
    md5: [u8; 16],
    sha256: Option<[u8; 32]>,
    updates: usize,
    init: u128,
    update: u128,
    clone: u128,
    finalize: u128,
    drop: u128,
    total: u128,
    unattributed: u128,
}

// Each message has its own inline RustCrypto states. Nothing is allocated or
// printed inside the update loop. Check callbacks use the one caller deadline;
// early returns drop every state before returning the original typed stop.
fn sample(
    bytes: &[u8],
    mode: Mode,
    mut check: impl FnMut(Point) -> Result<(), Stop>,
) -> Result<Sample, Stop> {
    let total = Instant::now();
    check(Point::BeforeNew)?;
    let phase = Instant::now();
    let mut md5 = Some(black_box(Md5::new()));
    let mut sha256 = (mode == Mode::Sha256Md5Consume).then(|| black_box(Sha256::new()));
    let init = phase.elapsed().as_nanos();
    let phase = Instant::now();
    let mut updates = 0;
    for (index, chunk) in bytes.chunks(CHUNK).enumerate() {
        check(Point::BeforeUpdate(index))?;
        // Same fused ordering as the verified decoded-cache reader: SHA first.
        if let Some(hash) = &mut sha256 {
            hash.update(black_box(chunk));
        }
        md5.as_mut().expect("live digest").update(black_box(chunk));
        updates += 1;
    }
    let update = phase.elapsed().as_nanos();
    check(Point::BeforeFinalize)?;
    let (finishing, clone) = if mode == Mode::Md5Clone {
        let phase = Instant::now();
        let cloned = black_box(md5.as_ref().expect("live digest").clone());
        let elapsed = phase.elapsed().as_nanos();
        check(Point::AfterClone)?;
        (cloned, elapsed)
    } else {
        (md5.take().expect("live digest"), 0)
    };
    let phase = Instant::now();
    let digest: [u8; 16] = black_box(finishing.finalize()).into();
    let sha_digest = sha256.take().map(|hash| {
        let digest: [u8; 32] = black_box(hash.finalize()).into();
        digest
    });
    let finalize = phase.elapsed().as_nanos();
    let phase = Instant::now();
    // Clone mode keeps the original owner alive until this explicit boundary.
    // Consume modes have already destroyed their states during finalize.
    drop(black_box(md5));
    drop(black_box(sha256));
    let dropped = phase.elapsed().as_nanos();
    let total = total.elapsed().as_nanos();
    let measured = init + update + clone + finalize + dropped;
    let unattributed = total.checked_sub(measured).expect("disjoint clock scopes");
    Ok(Sample {
        md5: digest,
        sha256: sha_digest,
        updates,
        init,
        update,
        clone,
        finalize,
        drop: dropped,
        total,
        unattributed,
    })
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    text
}
fn expect_digest(bytes: &[u8], md5: &str, sha256: &str) -> Result<(), String> {
    if hex(&Md5::digest(bytes)) != md5 || hex(&Sha256::digest(bytes)) != sha256 {
        return Err("fixture identity differs".into());
    }
    Ok(())
}
fn root_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err("Root input must be absolute".into());
    }
    let metadata = symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != 44 {
        return Err("Root input must be a regular 44-byte file".into());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    // Every path after acquisition consumes the original handle through the
    // checked close, including metadata and bounded-read failures.
    let result = (|| -> Result<Vec<u8>, String> {
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() != 44 {
            return Err("opened Root input differs".into());
        }
        let mut bytes = Vec::with_capacity(45);
        (&mut file)
            .take(45)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() != 44 {
            return Err("Root input size changed".into());
        }
        Ok(bytes)
    })();
    let closed = lycheedev_windows::filesystem::close_file(file);
    let bytes = match (result, closed) {
        (Ok(bytes), Ok(())) => bytes,
        (Err(read), Ok(())) => return Err(read),
        (Ok(_), Err(close)) => return Err(close.to_string()),
        (Err(read), Err(close)) => return Err(format!("{read}; Root close: {close}")),
    };
    expect_digest(&bytes, ROOT_MD5, ROOT_SHA256)?;
    Ok(bytes)
}

// Independent one-shot RustCrypto/CNG reference results check split handling.
// All fixture construction, authentication and correctness checks are outside
// the recorded sample clocks, but remain in the complete owned process wall.
fn check_vector(bytes: &[u8], expected: &str) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    let native = lycheedev_windows::legacy_digest::legacy_md5(
        bytes,
        Instant::now() + Duration::from_secs(30),
        &cancel,
    )
    .map_err(|error| error.to_string())?;
    if hex(&native) != expected || hex(&Md5::digest(bytes)) != expected {
        return Err("one-shot MD5 vector differs".into());
    }
    let sha_expected: [u8; 32] = Sha256::digest(bytes).into();
    for policy in 0..4 {
        for mode in [Mode::Md5Clone, Mode::Md5Consume, Mode::Sha256Md5Consume] {
            let mut md5 = Md5::new();
            let mut sha256 = (mode == Mode::Sha256Md5Consume).then(Sha256::new);
            let mut offset = 0usize;
            let mut index = 0;
            while offset < bytes.len() {
                let step = match policy {
                    0 => 1,
                    1 => 7,
                    2 => CHUNK,
                    _ => [63, 1, 8193, 7, CHUNK, 56][index % 6],
                };
                let end = offset.saturating_add(step).min(bytes.len());
                if policy == 3 {
                    md5.update([]);
                    if let Some(hash) = &mut sha256 {
                        hash.update([]);
                    }
                }
                if let Some(hash) = &mut sha256 {
                    hash.update(&bytes[offset..end]);
                }
                md5.update(&bytes[offset..end]);
                offset = end;
                index += 1;
            }
            md5.update([]);
            if let Some(hash) = &mut sha256 {
                hash.update([]);
            }
            let actual = if mode == Mode::Md5Clone {
                md5.clone().finalize()
            } else {
                md5.finalize()
            };
            if hex(&actual) != expected
                || sha256
                    .map(|hash| <[u8; 32]>::from(hash.finalize()))
                    .is_some_and(|digest| digest != sha_expected)
            {
                return Err("incremental digest vector differs".into());
            }
        }
    }
    Ok(())
}
fn correctness() -> Result<(), String> {
    let vectors: [(&[u8], &str); 7] = [
        (b"", "d41d8cd98f00b204e9800998ecf8427e"),
        (b"a", "0cc175b9c0f1b6a831c399e269772661"),
        (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
        (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            b"abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ];
    for (bytes, expected) in vectors {
        check_vector(bytes, expected)?;
    }
    for length in [55, 56, 63, 64, 65, 65535, 65536, 65537] {
        let bytes: Vec<_> = (0..length)
            .map(|index| ((index * 73 + index / 997) % 256) as u8)
            .collect();
        check_vector(&bytes, &hex(&Md5::digest(&bytes)))?;
    }
    let bytes = vec![120; CHUNK * 2];
    for stop in [Stop::Cancelled, Stop::Deadline] {
        for mode in [Mode::Md5Clone, Mode::Md5Consume, Mode::Sha256Md5Consume] {
            for point in [
                Point::BeforeNew,
                Point::BeforeUpdate(1),
                Point::BeforeFinalize,
                Point::AfterClone,
            ] {
                if point == Point::AfterClone && mode != Mode::Md5Clone {
                    continue; // Consuming owners never clone.
                }
                let mut stopped = false;
                let result = sample(&bytes, mode, |observed| {
                    assert!(!stopped, "work continued after a typed stop");
                    if observed == point {
                        stopped = true;
                        Err(stop)
                    } else {
                        Ok(())
                    }
                });
                if !stopped || !matches!(result, Err(actual) if actual == stop) {
                    return Err("typed stop or early-drop boundary differs".into());
                }
            }
        }
    }
    let cancelled = AtomicBool::new(true);
    if lycheedev_windows::legacy_digest::legacy_md5(
        b"abc",
        Instant::now() + Duration::from_secs(30),
        &cancelled,
    )
    .err()
    .as_deref()
        != Some("digest.cancelled")
    {
        return Err("CNG initial cancellation type differs".into());
    }
    let running = AtomicBool::new(false);
    if lycheedev_windows::legacy_digest::legacy_md5(b"abc", Instant::now(), &running)
        .err()
        .as_deref()
        != Some("digest.deadline")
    {
        return Err("CNG expired deadline type differs".into());
    }
    Ok(())
}
fn stream_fixture(path: &Path) -> Result<(), String> {
    let root = root_bytes(path)?;
    let payload = vec![120; PAYLOAD_BYTES];
    expect_digest(&payload, PAYLOAD_MD5, PAYLOAD_SHA256)?;
    correctness()?;
    // Verify exact retained inputs through CNG too, without pretending its
    // whole-slice facade has an externally persistent streaming owner.
    let cancel = AtomicBool::new(false);
    for (bytes, expected) in [
        (root.as_slice(), ROOT_MD5),
        (payload.as_slice(), PAYLOAD_MD5),
    ] {
        let native = lycheedev_windows::legacy_digest::legacy_md5(
            bytes,
            Instant::now() + Duration::from_secs(30),
            &cancel,
        )
        .map_err(|error| error.to_string())?;
        if hex(&native) != expected {
            return Err("CNG retained input identity differs".into());
        }
    }
    println!(
        "{{\"schema\":\"{SCHEMA}\",\"kind\":\"contract\",\"pairs\":{PAIRS},\"chunkBytes\":{CHUNK},\"messages\":2,\"updatesPerPair\":273,\"inputConstructionAndAuthenticationTimed\":false,\"correctness\":{{\"rfcVectors\":7,\"binaryBoundaryCases\":8,\"splitPolicies\":4,\"cancelCases\":10,\"deadlineCases\":10,\"cngInitialStopCases\":2}},\"cngStreaming\":false}}"
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    for iteration in 0..PAIRS {
        let mut modes = [Mode::Md5Clone, Mode::Md5Consume, Mode::Sha256Md5Consume];
        if iteration % 2 == 1 {
            modes.reverse();
        }
        for mode in modes {
            for (message, bytes, expected_md5, expected_sha) in [
                ("root", root.as_slice(), ROOT_MD5, ROOT_SHA256),
                ("payload", payload.as_slice(), PAYLOAD_MD5, PAYLOAD_SHA256),
            ] {
                let row = sample(bytes, mode, |_| {
                    if Instant::now() >= deadline {
                        Err(Stop::Deadline)
                    } else {
                        Ok(())
                    }
                })
                .map_err(|stop| format!("fixture sample stopped: {stop:?}"))?;
                if hex(&row.md5) != expected_md5
                    || row
                        .sha256
                        .is_some_and(|digest| hex(&digest) != expected_sha)
                    || row.updates != bytes.len().div_ceil(CHUNK)
                {
                    return Err("timed sample differs".into());
                }
                let digest = hex(&row.md5);
                let sha256 = row
                    .sha256
                    .map(|digest| format!("\"{}\"", hex(&digest)))
                    .unwrap_or_else(|| "null".into());
                println!(
                    "{{\"schema\":\"{SCHEMA}\",\"kind\":\"sample\",\"iteration\":{iteration},\"mode\":\"{}\",\"message\":\"{message}\",\"bytes\":{},\"updates\":{},\"md5\":\"{digest}\",\"sha256\":{sha256},\"initNanos\":{},\"updateNanos\":{},\"cloneNanos\":{},\"finalizeNanos\":{},\"dropNanos\":{},\"totalNanos\":{},\"unattributedNanos\":{}}}",
                    mode.name(),
                    bytes.len(),
                    row.updates,
                    row.init,
                    row.update,
                    row.clone,
                    row.finalize,
                    row.drop,
                    row.total,
                    row.unattributed
                );
            }
        }
    }
    if Instant::now() >= deadline {
        return Err("fixture sample deadline exhausted before completion".into());
    }
    println!(
        "{{\"schema\":\"{SCHEMA}\",\"kind\":\"complete\",\"samples\":36,\"correctnessPassed\":true}}"
    );
    Ok(())
}
