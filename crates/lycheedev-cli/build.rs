use std::process::Command;
fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().into())
        .unwrap_or_default()
}
fn main() {
    println!("cargo:rerun-if-env-changed=LYCHEEDEV_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=LYCHEEDEV_BUILD_DIRTY");
    let commit =
        std::env::var("LYCHEEDEV_BUILD_COMMIT").unwrap_or_else(|_| git(&["rev-parse", "HEAD"]));
    let dirty = std::env::var("LYCHEEDEV_BUILD_DIRTY")
        .unwrap_or_else(|_| (!git(&["status", "--porcelain"]).is_empty()).to_string());
    println!("cargo:rustc-env=LYCHEEDEV_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=LYCHEEDEV_BUILD_DIRTY={dirty}");
    let rust = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    println!("cargo:rustc-env=LYCHEEDEV_RUST_VERSION={rust}");
}
