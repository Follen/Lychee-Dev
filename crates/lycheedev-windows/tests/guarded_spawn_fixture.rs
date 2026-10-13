//! Standalone Windows test helper; build and seal in the governed test lane.
//! No product mode; bounded root/descendant, stdio bytes and exclusive markers.
use std::{
    fs,
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Command, Stdio},
    time::Duration,
};
use windows::Win32::{
    Foundation::{FILETIME, HANDLE},
    System::Threading::GetProcessTimes,
};
fn created(handle: HANDLE) -> u64 {
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u).unwrap();
    }
    (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64
}
fn main() {
    let Ok(role) = std::env::var("LYCHEEDEV_SUSPENDED_ROLE") else {
        return;
    };
    let token = std::env::var("LYCHEEDEV_SUSPENDED_TOKEN").unwrap();
    assert!(token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()));
    let root = std::env::current_dir().unwrap();
    if role == "grandchild" {
        fs::write(
            root.join(format!("grandchild-{token}.txt")),
            token.as_bytes(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(20));
        return;
    }
    assert_eq!(role, "root");
    assert_eq!(
        std::env::args().nth(1).unwrap(),
        "参数 with space\\quote\"tail\\"
    );
    assert_eq!(
        std::env::var("LYCHEEDEV_SUSPENDED_UNICODE").unwrap(),
        "环境值中文"
    );
    // This first payload marker and immediate spawn must be impossible before
    // the primary thread is resumed, even with an intentional assignment delay.
    fs::write(root.join(format!("root-{token}.txt")), token.as_bytes()).unwrap();
    let mut grandchild = Command::new(std::env::current_exe().unwrap())
        .env("LYCHEEDEV_SUSPENDED_ROLE", "grandchild")
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let grand_created = created(HANDLE(grandchild.as_raw_handle()));
    println!("READY|{token}|{}|{grand_created}", grandchild.id());
    std::io::stdout().flush().unwrap();
    let mut size = [0; 4];
    std::io::stdin().read_exact(&mut size).unwrap();
    let size = u32::from_le_bytes(size) as usize;
    assert!(size <= 256);
    let mut bytes = vec![0; size];
    std::io::stdin().read_exact(&mut bytes).unwrap();
    std::io::stdout().write_all(&bytes).unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(b"stderr\0\xfffixture").unwrap();
    std::io::stderr().flush().unwrap();
    // Root and descendant stay alive for exact Job membership/termination.
    let mut stop = [0; 1];
    let _ = std::io::stdin().read_exact(&mut stop);
    // Defensive bounded shutdown if the host closes stdin without terminating
    // the Job; custody is still the enclosing test's Job while the helper runs.
    let _ = grandchild.kill();
    let _ = grandchild.wait();
}
