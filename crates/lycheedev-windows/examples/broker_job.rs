fn main() {
    use std::os::windows::process::CommandExt;
    let mut job = lycheedev_windows::BrokerJob::own_current_process(2usize << 30).unwrap();
    let memory = job.memory_accounting().unwrap();
    assert_eq!(memory.job_memory_limit, 2u64 << 30);
    assert!(memory.active_processes >= 1);
    assert!(memory.aggregate_commit.is_none());
    println!("job telemetry {:?}", memory);
    let mut child = std::process::Command::new("cmd.exe")
        .args(["/c", "exit", "0"])
        .creation_flags(0x08000000)
        .spawn()
        .unwrap();
    assert!(child.wait().unwrap().success());
    job.disarm().unwrap();
    drop(job);
    println!(
        "broker self+child aggregate commit job configured 2147483648; normal disarm survived; RSS/limit-pressure not measured"
    );
}
