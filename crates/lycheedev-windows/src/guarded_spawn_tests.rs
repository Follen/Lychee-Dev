//! Actual owned-OS regressions; require sealed helper EXE and evidence root.
//! Requires an explicitly sealed standalone fixture-helper EXE, never a game.
use super::*;
use crate::Process;
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};
use windows::Win32::System::Pipes::PeekNamedPipe;
const CURRENT_LUALS_CHILD_LIMIT: usize = (2 << 30) - (256 << 20); // existing source/luals.rs; no limit change
struct Case {
    directory: PathBuf,
    helper: PathBuf,
    helper_hash: String,
    token: String,
    _held_helper: fs::File,
}
impl Case {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(
            std::env::var_os("LYCHEEDEV_SUSPENDED_FIXTURE_ROOT").expect("explicit evidence root"),
        );
        let helper = PathBuf::from(
            std::env::var_os("LYCHEEDEV_SUSPENDED_HELPER").expect("sealed fixture EXE"),
        );
        let helper_hash =
            std::env::var("LYCHEEDEV_SUSPENDED_HELPER_SHA256").expect("sealed helper hash");
        assert!(root.is_absolute() && helper.is_absolute() && helper.is_file());
        let mut held_helper = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&helper)
            .unwrap();
        let metadata = held_helper.metadata().unwrap();
        assert!(metadata.is_file() && metadata.len() <= 128 << 20);
        let mut bytes = Vec::new();
        Read::read_to_end(&mut (&mut held_helper).take((128 << 20) + 1), &mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, metadata.len());
        assert_eq!(hex_digest(&bytes), helper_hash);
        let token = crate::random::token().unwrap();
        let directory = root.join(format!("suspended-{name}-{token}-中文"));
        assert!(!directory.exists());
        fs::create_dir_all(&directory).unwrap();
        Self {
            directory,
            helper,
            helper_hash,
            token,
            _held_helper: held_helper,
        }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.helper);
        cmd.arg("参数 with space\\quote\"tail\\")
            .current_dir(&self.directory)
            .env("LYCHEEDEV_SUSPENDED_ROLE", "root")
            .env("LYCHEEDEV_SUSPENDED_TOKEN", &self.token)
            .env("LYCHEEDEV_SUSPENDED_UNICODE", "环境值中文")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
    fn assert_not_started(&self, child: &Child) {
        assert!(
            !self
                .directory
                .join(format!("root-{}.txt", self.token))
                .exists()
        );
        assert!(
            !self
                .directory
                .join(format!("grandchild-{}.txt", self.token))
                .exists()
        );
        // Actual OS pipe availability, not a delayed callback or CPU inference.
        for pipe in [
            child.stdout.as_ref().unwrap().as_raw_handle(),
            child.stderr.as_ref().unwrap().as_raw_handle(),
        ] {
            let mut available = 0;
            unsafe {
                PeekNamedPipe(HANDLE(pipe), None, 0, None, Some(&mut available), None).unwrap();
            }
            assert_eq!(available, 0);
        }
    }
    fn proof(&self, text: &str) {
        fs::write(
            self.directory.join("proof.txt"),
            format!(
                "helper={}\nhelperSHA256={}\n{text}\n",
                self.helper.display(),
                self.helper_hash
            ),
        )
        .unwrap();
    }
}
fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}
fn held(child: &Child) -> Process {
    let process = Process::open(child.id()).unwrap();
    assert_eq!(process.identity().created, creation(child).unwrap());
    assert_eq!(process.parent_pid().unwrap(), std::process::id());
    process
}
fn ordinary_path(path: PathBuf) -> PathBuf {
    path.to_str()
        .and_then(|value| value.strip_prefix(r"\\?\"))
        .map(PathBuf::from)
        .unwrap_or(path)
}
struct SystemConsoleImage {
    path: PathBuf,
    hash: String,
    _held: fs::File,
}
fn system_console_image() -> SystemConsoleImage {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    assert!(length > 0 && (length as usize) < buffer.len());
    let directory = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
    let path = ordinary_path(directory.join("conhost.exe").canonicalize().unwrap());
    let mut held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let metadata = held.metadata().unwrap();
    assert!(metadata.is_file() && metadata.len() <= 32 << 20);
    let mut raw = Vec::new();
    Read::read_to_end(&mut (&mut held).take((32 << 20) + 1), &mut raw).unwrap();
    assert_eq!(raw.len() as u64, metadata.len());
    SystemConsoleImage {
        path,
        hash: hex_digest(&raw),
        _held: held,
    }
}
fn observed_members(members: &crate::job::JobMembership) -> (Vec<Process>, String) {
    let mut held = Vec::new();
    let mut rows = String::new();
    for &pid in &members.member_pids {
        match Process::open(pid) {
            Ok(process) => {
                let image = process.image_path();
                let hash = process.main_module().map(|module| module.hash().to_owned());
                rows.push_str(&format!("pid={} created={} imagePath={image:?} imageSHA256={hash:?} parentPid={:?} memory={:?}\n",
                    process.identity().pid, process.identity().created, process.parent_pid(), process.memory()));
                held.push(process);
            }
            Err(error) => rows.push_str(&format!("pid={pid} openError={error:?}\n")),
        }
    }
    (held, rows)
}
struct ConsoleRole {
    pid: u32,
    created: u64,
    parent_pid: u32,
    parent_created: u64,
    path: PathBuf,
    hash: String,
}
impl ConsoleRole {
    fn observation(&self) -> String {
        format!(
            "pid={} created={} parentPid={} parentCreated={} imagePath={} imageSHA256={} role=system-console authenticode=not_claimed\n",
            self.pid,
            self.created,
            self.parent_pid,
            self.parent_created,
            self.path.display(),
            self.hash
        )
    }
}
fn console_roles(
    members: &crate::job::JobMembership,
    held: &[Process],
    peers: &[&Process],
    image: &SystemConsoleImage,
) -> Result<Vec<ConsoleRole>, String> {
    if held.len() != members.member_pids.len() {
        return Err("unobserved Job member".into());
    }
    for peer in peers {
        if !held
            .iter()
            .any(|member| member.identity() == peer.identity())
        {
            return Err("known held peer absent from exact Job membership".into());
        }
        peer.verify()?;
    }
    let mut roles = Vec::new();
    let mut parents = std::collections::BTreeSet::new();
    for member in held {
        member.verify()?;
        if peers
            .iter()
            .any(|peer| peer.identity() == member.identity())
        {
            continue;
        }
        let actual_path = ordinary_path(member.image_path()?);
        if !actual_path
            .to_string_lossy()
            .eq_ignore_ascii_case(&image.path.to_string_lossy())
        {
            return Err(format!("unknown member image: {}", actual_path.display()));
        }
        if member.main_module()?.hash() != image.hash {
            return Err("system console loaded module original hash mismatch".into());
        }
        let parent_pid = member.parent_pid()?;
        let parent = peers
            .iter()
            .find(|peer| peer.identity().pid == parent_pid)
            .ok_or_else(|| "console parent is not a known held peer".to_owned())?;
        if member.identity().created < parent.identity().created || !parents.insert(parent_pid) {
            return Err("console creation/one-per-known-parent admission failed".into());
        }
        roles.push(ConsoleRole {
            pid: member.identity().pid,
            created: member.identity().created,
            parent_pid,
            parent_created: parent.identity().created,
            path: actual_path,
            hash: image.hash.clone(),
        });
    }
    Ok(roles)
}
struct Running {
    child: Option<Child>,
    job: Option<JobGuard>,
    readers: Vec<JoinHandle<()>>,
}
impl Running {
    fn terminate_and_join(&mut self) {
        self.job.as_ref().unwrap().terminate().unwrap();
        let child = self.child.as_mut().unwrap();
        child.stdin.take();
        let end = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        child.wait().unwrap();
        for reader in self.readers.drain(..) {
            reader.join().unwrap();
        }
        let end = Instant::now() + Duration::from_secs(3);
        while self
            .job
            .as_ref()
            .unwrap()
            .memory_accounting()
            .unwrap()
            .active_processes
            != 0
        {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        self.child.take();
        self.job.take();
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            child.stdin.take();
            let end = Instant::now() + Duration::from_secs(2);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            if child.try_wait().ok().flatten().is_some() {
                let _ = child.wait();
            }
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}
#[test]
#[ignore = "requires sealed standalone helper and explicit evidence root"]
fn real_child_cannot_run_before_resume_and_stdio_descendant_are_owned() {
    let case = Case::new("normal");
    let console_image = system_console_image();
    let mut observed = None;
    let stages = Cell::new(0u8);
    let (child, job) = spawn_with_probe(
        case.command(),
        CURRENT_LUALS_CHILD_LIMIT,
        Instant::now() + Duration::from_secs(8),
        || false,
        |stage, child, job| {
            case.assert_not_started(child);
            match stage {
                PrepareStage::CreatedSuspended => {
                    observed = Some(held(child));
                    stages.set(stages.get() | 1);
                    std::thread::sleep(Duration::from_millis(200));
                    case.assert_not_started(child);
                }
                PrepareStage::JobAssigned => {
                    let job = job.unwrap();
                    assert_eq!(job.membership().unwrap().member_pids, vec![child.id()]);
                    assert_eq!(
                        job.memory_accounting().unwrap().job_memory_limit,
                        CURRENT_LUALS_CHILD_LIMIT as u64
                    );
                    stages.set(stages.get() | 2);
                }
                PrepareStage::BeforeResume => {
                    stages.set(stages.get() | 4);
                }
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(stages.get(), 7);
    let original = observed.unwrap();
    let child_identity = original.identity().clone();
    let mut running = Running {
        child: Some(child),
        job: Some(job),
        readers: vec![],
    };
    let out = running.child.as_mut().unwrap().stdout.take().unwrap();
    let mut err = running.child.as_mut().unwrap().stderr.take().unwrap();
    let mut out_reader = BufReader::new(out);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (bytes_tx, bytes_rx) = mpsc::sync_channel(1);
    let (err_tx, err_rx) = mpsc::sync_channel(1);
    running.readers.push(std::thread::spawn(move || {
        let mut line = String::new();
        out_reader.by_ref().take(257).read_line(&mut line).unwrap();
        assert!(line.len() <= 256 && line.ends_with('\n'));
        ready_tx.send(line).unwrap();
        let mut bytes = vec![0; 256];
        out_reader.read_exact(&mut bytes).unwrap();
        bytes_tx.send(bytes).unwrap();
        let mut tail = Vec::new();
        out_reader.take(2).read_to_end(&mut tail).unwrap();
        assert!(
            tail.is_empty(),
            "stdout must reach exact EOF after custody termination"
        );
    }));
    running.readers.push(std::thread::spawn(move || {
        let mut bytes = vec![0; 15];
        err.read_exact(&mut bytes).unwrap();
        err_tx.send(bytes).unwrap();
        let mut tail = Vec::new();
        err.take(2).read_to_end(&mut tail).unwrap();
        assert!(
            tail.is_empty(),
            "stderr must reach exact EOF after custody termination"
        );
    }));
    let ready = ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    // Save every actual member, including unknown/open-failed members, before
    // asserting fixture identities, role admission or exact expected set.
    let membership = running.job.as_ref().unwrap().membership().unwrap();
    let (held_members, member_rows) = observed_members(&membership);
    let observation = format!(
        "readyFrame={ready:?}\njobMembership={membership:#?}\n{member_rows}systemConsolePath={} systemConsoleSHA256={} heldReadLock=true authenticode=not_claimed\n",
        console_image.path.display(),
        console_image.hash
    );
    assert!(observation.len() <= 8 << 20);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(case.directory.join("members-before-admission.txt"))
        .unwrap()
        .write_all(observation.as_bytes())
        .unwrap();
    let fields: Vec<_> = ready.trim().split('|').collect();
    assert_eq!(fields[0], "READY");
    assert_eq!(fields[1], case.token);
    assert_eq!(fields.len(), 4);
    let grand = Process::open(fields[2].parse().unwrap()).unwrap();
    assert_eq!(grand.identity().created, fields[3].parse::<u64>().unwrap());
    assert_eq!(grand.parent_pid().unwrap(), child_identity.pid);
    assert_eq!(grand.main_module().unwrap().hash(), case.helper_hash);
    let admitted = console_roles(
        &membership,
        &held_members,
        &[&original, &grand],
        &console_image,
    );
    let admission = match &admitted {
        Ok(roles) => roles
            .iter()
            .map(ConsoleRole::observation)
            .collect::<String>(),
        Err(error) => format!("consoleRoleAdmissionRejected={error:?}\n"),
    };
    fs::write(
        case.directory.join("console-role-admission.txt"),
        &admission,
    )
    .unwrap();
    let admitted = admitted.unwrap();
    let mut members = membership.member_pids;
    members.sort_unstable();
    let mut expected = vec![child_identity.pid, grand.identity().pid];
    expected.extend(admitted.iter().map(|role| role.pid));
    expected.sort_unstable();
    assert_eq!(members, expected);
    let payload: Vec<_> = (0..=255u8).collect();
    let input = running.child.as_mut().unwrap().stdin.as_mut().unwrap();
    input
        .write_all(&(payload.len() as u32).to_le_bytes())
        .unwrap();
    input.write_all(&payload).unwrap();
    input.flush().unwrap();
    // Observe actual echoed bytes BEFORE killing the Job, so termination cannot
    // race unread input and manufacture a truncated exchange. Keep stdin open.
    assert_eq!(
        bytes_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        payload
    );
    assert_eq!(
        err_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        b"stderr\0\xfffixture"
    );
    running.terminate_and_join();
    let never_cancel = AtomicBool::new(false);
    original
        .wait_exit(Instant::now() + Duration::from_secs(1), &never_cancel)
        .unwrap();
    grand
        .wait_exit(Instant::now() + Duration::from_secs(1), &never_cancel)
        .unwrap();
    for member in &held_members {
        member
            .wait_exit(Instant::now() + Duration::from_secs(1), &never_cancel)
            .unwrap();
    }
    case.proof(&format!("createdSuspendedBeforePayload=true\nstages=7\nchild={} created={}\ngrand={} created={}\nexactJobMembers={members:?}\n{admission}allObservedHeldMembersRetired=true\npipeBytes=256\njobActiveProcesses=0\nreadersJoined=true\n",child_identity.pid,child_identity.created,grand.identity().pid,grand.identity().created));
}
#[test]
#[ignore = "requires sealed standalone helper and explicit evidence root"]
fn cancelled_or_injected_prepare_failure_never_resumes_and_reaps_original_child() {
    for cancellation in [true, false] {
        let case = Case::new(if cancellation {
            "cancel"
        } else {
            "prepare-failure"
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let mut observed = None;
        let result = spawn_with_probe(
            case.command(),
            CURRENT_LUALS_CHILD_LIMIT,
            Instant::now() + Duration::from_secs(8),
            || cancel.load(Ordering::Acquire),
            |stage, child, job| {
                case.assert_not_started(child);
                if stage == PrepareStage::JobAssigned {
                    observed = Some(held(child));
                    assert_eq!(
                        job.unwrap().membership().unwrap().member_pids,
                        vec![child.id()]
                    );
                    if cancellation {
                        cancel.store(true, Ordering::Release);
                    } else {
                        return Err("fixture.injected_prepare_failure".into());
                    }
                }
                Ok(())
            },
        );
        let failure = match result {
            Ok(_) => panic!("unexpected resume"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure.cause,
            if cancellation {
                "startup.cancelled"
            } else {
                "fixture.injected_prepare_failure"
            }
        );
        assert!(failure.cleanup.child_waited);
        assert!(failure.cleanup.error.is_none());
        assert_eq!(failure.cleanup.job_empty, Some(true));
        let original = observed.unwrap();
        original
            .wait_exit(
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(
            !case
                .directory
                .join(format!("root-{}.txt", case.token))
                .exists()
        );
        assert!(
            !case
                .directory
                .join(format!("grandchild-{}.txt", case.token))
                .exists()
        );
        case.proof(&format!(
            "failure={}\nchild={} created={}\nneverResumed=true\nchildWaited=true\ncleanup={:?}\n",
            failure.cause,
            original.identity().pid,
            original.identity().created,
            failure.cleanup
        ));
    }
}
