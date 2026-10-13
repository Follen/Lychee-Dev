//! Two actual OS processes/windows, directed keys and read-only private buffers.
//! This is a platform fixture, not Retail ABI, WGC, Lua or full Live acceptance.
use lycheedev_windows::{JobGuard, Process, WindowIdentity};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::windows::process::CommandExt,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
const BARRIER: u32 = WM_APP + 71;

/// Invoked only as this integration test's owned child; no product flags exist.
pub fn child_entry() {
    let Ok(marker) = std::env::var("LYCHEEDEV_OWNED_INSTANCE_FIXTURE") else {
        return;
    };
    assert!(marker.len() == 32 && marker.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut buffer = format!("owned-instance:{marker}")
        .into_bytes()
        .into_boxed_slice();
    let count = Arc::new(AtomicU32::new(0));
    let messages = count.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (barrier_tx, barrier_rx) = mpsc::channel();
    let window = std::thread::spawn(move || unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Lychee owned fixture"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .unwrap();
        ready_tx.send(hwnd.0 as usize).unwrap();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if (WM_KEYFIRST..=WM_KEYLAST).contains(&msg.message) {
                messages.fetch_add(1, Ordering::Relaxed);
            }
            if msg.message == BARRIER {
                barrier_tx.send(()).unwrap();
            }
            if msg.message == WM_APP + 72 {
                break;
            }
        }
        DestroyWindow(hwnd).unwrap();
    });
    let hwnd = ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let process = Process::open(std::process::id()).unwrap();
    let module = process.main_module().unwrap();
    println!(
        "READY|{}|{}|{}|{}|{}|{}",
        process.identity().pid,
        process.identity().created,
        hwnd,
        buffer.as_ptr() as usize,
        buffer.len(),
        module.hash()
    );
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines().take(65536) {
        let command = line.unwrap();
        if let Some(hex) = command.strip_prefix("WIRE|") {
            assert!(hex.len() <= 1_049_000 && hex.len() % 2 == 0);
            let observed = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect::<Vec<_>>()
                .into_boxed_slice();
            if buffer != observed {
                buffer = observed;
            }
            println!("WIRE|{}|{}", buffer.as_ptr() as usize, buffer.len());
            std::io::stdout().flush().unwrap();
            continue;
        }
        match command.as_str() {
            "STATE" => {
                unsafe {
                    PostMessageW(Some(HWND(hwnd as *mut _)), BARRIER, WPARAM(0), LPARAM(0))
                        .unwrap();
                }
                barrier_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                println!("STATE|{}", count.load(Ordering::Relaxed));
                std::io::stdout().flush().unwrap();
            }
            "STOP" => break,
            _ => panic!("unknown fixture command"),
        }
    }
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_APP + 72,
            WPARAM(0),
            LPARAM(0),
        )
        .unwrap();
    }
    window.join().unwrap();
    drop(buffer);
}

pub struct Owned {
    child: Child,
    _job: JobGuard,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    pub target: WindowIdentity,
    pub process: Process,
    address: u64,
    length: usize,
    pub marker: String,
    pub hash: String,
    _module: lycheedev_windows::process::ModulePlan,
}
impl Owned {
    pub fn spawn(entry: &str) -> Self {
        let marker = lycheedev_windows::random::token().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", entry, "--nocapture"])
            .env("LYCHEEDEV_OWNED_INSTANCE_FIXTURE", &marker)
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let job = JobGuard::attach(&child).unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let ready = read_record(&mut output, "READY|");
        let parts = ready.split('|').collect::<Vec<_>>();
        assert_eq!(parts.len(), 7);
        let process = Process::open(parts[1].parse().unwrap()).unwrap();
        assert_eq!(process.identity().pid, child.id());
        assert_eq!(process.identity().created, parts[2].parse::<u64>().unwrap());
        assert_eq!(process.parent_pid().unwrap(), std::process::id());
        let target = WindowIdentity {
            process: process.identity().clone(),
            hwnd: parts[3].parse().unwrap(),
        };
        target.verify().unwrap();
        let module = process.main_module().unwrap();
        let hash = module.hash().to_owned();
        assert_eq!(hash, parts[6]);
        Self {
            child,
            _job: job,
            input,
            output,
            target,
            process,
            address: parts[4].parse().unwrap(),
            length: parts[5].parse().unwrap(),
            marker,
            hash,
            _module: module,
        }
    }
    pub fn state(&mut self) -> u32 {
        self.input.write_all(b"STATE\n").unwrap();
        self.input.flush().unwrap();
        read_record(&mut self.output, "STATE|")
            .strip_prefix("STATE|")
            .unwrap()
            .parse()
            .unwrap()
    }
    pub fn bytes(&self) -> Vec<u8> {
        assert!(self.length <= 256);
        let mut bytes = vec![0; self.length];
        self.process.read_private(self.address, &mut bytes).unwrap();
        bytes
    }
    /// Test-only carrier: copy observed Lua wire into the selected owned OS process,
    /// then read those actual MEM_PRIVATE bytes through its read-only handle.
    pub fn stage_wire(&mut self, wire: &[u8]) -> (u64, Vec<u8>) {
        assert!(wire.len() <= 524_480);
        let encoded = wire
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.input
            .write_all(format!("WIRE|{encoded}\n").as_bytes())
            .unwrap();
        self.input.flush().unwrap();
        let response = read_record(&mut self.output, "WIRE|");
        let parts = response.split('|').collect::<Vec<_>>();
        let address = parts[1].parse().unwrap();
        let length: usize = parts[2].parse().unwrap();
        assert_eq!(length, wire.len());
        let mut bytes = vec![0; length];
        self.process.read_private(address, &mut bytes).unwrap();
        assert_eq!(bytes, wire);
        (address, bytes)
    }
}
fn read_record(output: &mut BufReader<ChildStdout>, prefix: &str) -> String {
    for _ in 0..16 {
        let mut bytes = Vec::new();
        let read = (&mut *output)
            .take(4096)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        assert!(
            read > 0 && read < 4096,
            "owned child bounded record unavailable"
        );
        let line = String::from_utf8(bytes).unwrap();
        if let Some(offset) = line.find(prefix) {
            return line[offset..].trim().into();
        }
    }
    panic!("owned child record budget exhausted")
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.input.write_all(b"STOP\n");
        let _ = self.input.flush();
        // The owned process Job guard also kills descendants on failure.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
