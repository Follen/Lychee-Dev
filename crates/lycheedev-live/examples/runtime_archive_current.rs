//! Read-only module code corroboration; never reads private heap or sends input.
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result = (|| -> Result<serde_json::Value, String> {
        if args.len() != 3 {
            return Err("usage: runtime_archive_current <pid> <manifest>".into());
        }
        let pid = args[1].parse().map_err(|_| "runtime.pid_argument")?;
        let process = lycheedev_windows::Process::open(pid)?;
        let module = process.main_module()?;
        let pe = lycheedev_live::locator::Pe::parse(module.disk())?;
        let archive = lycheedev_live::runtime_archive::Archive::open(
            std::path::Path::new(&args[2]),
            &pe,
            ".text",
        )?;
        let control = lycheedev_core::control::OperationControl::default();
        let validation = archive.validate_current(&process, &module, &control)?;
        serde_json::to_value(validation).map_err(|e| e.to_string())
    })();
    match result {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
