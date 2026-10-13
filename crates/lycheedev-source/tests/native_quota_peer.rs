//! Standalone Windows-only, Cargo-governed test helper; never shipped runtime.
//! Default test invocation is inactive and performs no allocation.
//! The allocator retains committed pages until production Rpc::drop kills it.
//! Allocation failure alone does not prove a Job limit caused the refusal.
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, Read, Write},
    process::{Command, Stdio},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(
        address: *mut std::ffi::c_void,
        size: usize,
        flags: u32,
        protect: u32,
    ) -> *mut std::ffi::c_void;
    fn GetLastError() -> u32;
}

fn commit_up_to(maximum: usize) -> (usize, u32) {
    let mut bytes = 0;
    // Reserving+committing uses Windows commit charge, with no artificial RSS
    // sum or output allocation. Keep all addresses alive until process exit.
    for size in [16 << 20, 1 << 20] {
        while bytes + size <= maximum {
            let address = unsafe { VirtualAlloc(std::ptr::null_mut(), size, 0x3000, 4) };
            if address.is_null() {
                if size == 1 << 20 {
                    return (bytes, unsafe { GetLastError() });
                }
                break;
            }
            bytes += size;
        }
    }
    (bytes, 0)
}

fn read_frame(input: &mut impl BufRead) -> Vec<u8> {
    let mut size = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let count = Read::take(&mut *input, 8193).read_line(&mut line).unwrap();
        header_bytes += count;
        assert!(count > 0 && header_bytes <= 8192);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            size = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let size = size.unwrap();
    assert!(size < 4096);
    let mut raw = vec![0; size];
    Read::read_exact(input, &mut raw).unwrap();
    raw
}

fn reply(id: u32, result: &str) {
    let raw = format!("{{\"id\":{id},\"result\":{result}}}");
    let mut output = std::io::stdout().lock();
    write!(output, "Content-Length: {}\r\n\r\n{raw}", raw.len()).unwrap();
    output.flush().unwrap();
}

fn main() {
    assert_eq!(std::env::consts::OS, "windows");
    let nonce = match std::env::var("LYCHEEDEV_QUOTA_HELPER_AUTH") {
        Err(std::env::VarError::NotPresent) => {
            println!(
                r#"{{"schema":"lycheedev.source-native-quota-helper.v1","state":"inactive","allocation":false}}"#
            );
            return;
        }
        Ok(nonce) => nonce,
        Err(error) => panic!("invalid test nonce: {error}"),
    };
    assert!(
        nonce.len() == 32
            && nonce
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    let mode = std::env::args().nth(1).unwrap();
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    if mode == "descendant" {
        let mut gate = String::new();
        input.read_line(&mut gate).unwrap();
        assert_eq!(gate, "commit\n");
        let (bytes, error) = commit_up_to(128 << 20);
        println!("{bytes} {error}");
        std::io::stdout().flush().unwrap();
        // Parent leaves this pipe open until Job retirement; no CPU spin.
        let mut until_close = String::new();
        input.read_line(&mut until_close).unwrap();
        return;
    }
    assert!(mode == "direct" || mode == "aggregate");
    let start = read_frame(&mut input);
    assert!(
        String::from_utf8(start)
            .unwrap()
            .contains("fixture/prepare")
    );
    // This spawn is intentionally after the first Rpc input, therefore after
    // Rpc has attached the root to its child Job. It tests post-attach inheritance;
    // it cannot establish the product's pre-attachment spawn race is absent.
    let mut descendant = if mode == "aggregate" {
        Some(
            Command::new(std::env::current_exe().unwrap())
                .arg("descendant")
                .creation_flags(0x08000000)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    } else {
        None
    };
    reply(
        1,
        &format!(
            "{{\"nonce\":\"{nonce}\",\"descendantPid\":{}}}",
            descendant.as_ref().map_or(0, |p| p.id())
        ),
    );
    let allocate = read_frame(&mut input);
    assert!(
        String::from_utf8(allocate)
            .unwrap()
            .contains("fixture/allocate")
    );
    let mut descendant_bytes = 0;
    if let Some(child) = &mut descendant {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"commit\n")
            .unwrap();
        child.stdin.as_mut().unwrap().flush().unwrap();
        let mut report = String::new();
        std::io::BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut report)
            .unwrap();
        let mut columns = report.split_whitespace();
        descendant_bytes = columns.next().unwrap().parse::<usize>().unwrap();
        assert_eq!(columns.next(), Some("0"));
        assert_eq!(descendant_bytes, 128 << 20);
    }
    let (bytes, error) = commit_up_to((2 << 30) + (32 << 20));
    reply(
        2,
        &format!(
            "{{\"nonce\":\"{nonce}\",\"directCommittedBytes\":{bytes},\"descendantCommittedBytes\":{descendant_bytes},\"allocationError\":{error}}}"
        ),
    );
    // Stay alive with pages committed; exact Rpc Job cleanup retires the tree.
    let _ = read_frame(&mut input);
    panic!("unexpected third fixture request");
}
