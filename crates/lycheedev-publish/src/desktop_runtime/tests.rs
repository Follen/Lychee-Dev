use super::*;
use std::fs;

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-desktop-sdk-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn candidate(&self, name: &str) -> PathBuf {
        let directory = self.root.join(name);
        fs::create_dir_all(&directory).unwrap();
        // Windows supplies this independent Authenticode input. Copying the
        // original PE preserves its signature; neither copy is ever executed.
        fs::copy(
            lycheedev_windows::filesystem::system_directory()
                .unwrap()
                .join("cmd.exe"),
            directory.join("synthetic.exe"),
        )
        .unwrap();
        fs::write(
            directory.join("resource.dat"),
            b"synthetic SDK resource; not executable",
        )
        .unwrap();
        directory
    }
    fn lookup(
        &self,
        roots: &[&std::path::Path],
        subdir: Option<&str>,
        publishers: &[&str],
    ) -> Result<Installation> {
        let body = include_str!("../desktop_runtime.ps1")
            .split_once("foreach ($root in ($candidates | Sort-Object)) {")
            .unwrap()
            .1;
        let candidates = serde_json::to_string(roots).unwrap();
        // Substitute only registry discovery with independent installation
        // roots. The production path selection/signature/output and Rust
        // validation execute unchanged in the real owned PowerShell process.
        let script = format!(
            r#"$ErrorActionPreference='Stop'
[Console]::InputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$request=[Console]::In.ReadToEnd() | ConvertFrom-Json
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$candidates=ConvertFrom-Json @'
{candidates}
'@
foreach ($root in ($candidates | Sort-Object)) {{{body}"#
        );
        lookup_script(
            &Lookup {
                name_prefix: "synthetic",
                program_subdirectory: subdir,
                executable: "synthetic.exe",
                resource: "resource.dat",
                publishers,
            },
            &script,
            &OperationControl::default(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn exact_image_verification_uses_stdin_paths_and_the_expected_publisher() {
    let fixture = Fixture::new();
    let directory = fixture.candidate("中文 'quoted' image");
    let path = directory.join("synthetic.exe");
    let control = OperationControl::default();
    let image = verify_image(&path, &["Microsoft Corporation"], &control).unwrap();
    assert_eq!(
        image.path.canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    assert_eq!(image.publisher, "Microsoft Corporation");
    assert!(!image.version.is_empty());
    assert!(verify_image(&path, &["NetEase (Hangzhou) Network Co., Ltd"], &control).is_err());
    assert!(
        verify_image(
            &directory.join("resource.dat"),
            &["Microsoft Corporation"],
            &control
        )
        .is_err()
    );
}
#[test]
fn installation_lookup_resolves_fixed_version_from_launcher_or_version_directory_and_keeps_direct_layout()
 {
    let f = Fixture::new();
    let versioned = f.candidate("中文安装/100130");
    for root in [versioned.parent().unwrap(), versioned.as_path()] {
        let result = f
            .lookup(&[root], Some("100130"), &["Microsoft Corporation"])
            .unwrap();
        assert_eq!(result.directory, versioned);
        assert_eq!(result.resource, versioned.join("resource.dat"));
        assert!(!result.version.is_empty());
    }
    let direct = f.candidate("direct");
    assert_eq!(
        f.lookup(&[&direct], None, &["Microsoft Corporation"])
            .unwrap()
            .directory,
        direct
    );
}
#[test]
fn installation_lookup_rejects_another_publisher_missing_resources_and_unselected_version() {
    let f = Fixture::new();
    let directory = f.candidate("root/100130");
    assert_eq!(
        f.lookup(
            &[&directory],
            Some("100130"),
            &["different synthetic publisher"]
        )
        .err()
        .unwrap()
        .exit_code,
        3
    );
    assert_eq!(
        f.lookup(
            &[directory.parent().unwrap()],
            Some("100129"),
            &["Microsoft Corporation"]
        )
        .err()
        .unwrap()
        .exit_code,
        3
    );
    fs::remove_file(directory.join("resource.dat")).unwrap();
    assert_eq!(
        f.lookup(&[&directory], Some("100130"), &["Microsoft Corporation"])
            .err()
            .unwrap()
            .exit_code,
        3
    );
}
