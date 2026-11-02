//! Read-only managed-tree inspection for Live doctor. The selected tree is
//! bounded and every opened file receives checked close under caller control.
#![forbid(unsafe_code)]
use super::*;
use lycheedev_core::control::OperationControl;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
const RECEIPT_LIMIT: usize = 1 << 20;
const PAYLOAD_LIMIT: u64 = 256 << 20;
/// Current product payload families. LuaLS is not part of the delivered
/// product; only the managed luac tool ships under tool/.
const PAYLOAD_FAMILIES: &[&str] = &["addon/", "skill/", "tool/luac/"];
fn invalid(detail: &str) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", detail, 4)
}
fn payload(detail: impl Into<String>) -> ToolkitError {
    ToolkitError::new("delivery.invalid_payload", detail, 4)
}
/// Same state categories as fixed Go InspectInstallation. A diagnostic does not
/// acquire maintenance, create files, adopt unknown bytes or authorize repair.
pub fn inspect_main_controlled(
    path: &Path,
    component: &str,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    if !matches!(component, "addon" | "skill") {
        return Err(invalid("invalid component"));
    }
    let info = match fs::symlink_metadata(path) {
        Ok(info) => info,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(json!({"state":"absent"}));
        }
        Err(e) => return Err(e.into()),
    };
    control.check()?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Ok(json!({"state":"unmanaged","detail":"target is not an ordinary directory"}));
    }
    let marker = path.join(MARKER);
    let info = match fs::symlink_metadata(&marker) {
        Ok(info) => info,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(json!({"state":"unmanaged"}));
        }
        Err(e) => return Err(e.into()),
    };
    control.check()?;
    if !info.is_file() || info.file_type().is_symlink() || info.len() > RECEIPT_LIMIT as u64 {
        return Err(invalid("receipt file"));
    }
    storage::guard_path(path, &marker)?;
    let (mut file, digest) = storage::locked_sha256(&marker, RECEIPT_LIMIT as u64, control)?;
    let inspect = (|| {
        file.seek(SeekFrom::Start(0))?;
        let mut raw = Vec::new();
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            if count > RECEIPT_LIMIT.saturating_sub(raw.len()) {
                return Err(invalid("receipt encoding or size"));
            }
            raw.extend_from_slice(&block[..count]);
        }
        if storage::sha256(&raw) != digest {
            return Err(invalid("receipt changed during read"));
        }
        let receipt: Receipt =
            serde_json::from_slice(&raw).map_err(|e| invalid(&format!("receipt: {e}")))?;
        if receipt.schema != "lycheedev.installation.v1"
            || receipt.component != component
            || receipt.version.is_empty()
            || !valid_digest(&receipt.commit, 40)
        {
            return Err(invalid("receipt identity"));
        }
        match inspect_inventory(path, &receipt.resources, component, control) {
            Ok(()) => Ok(json!({"state":"managed","receipt":receipt})),
            Err(e)
                if e.code == "delivery.invalid_payload"
                    && e.details["schema"] != storage::FILE_CLEANUP_SCHEMA =>
            {
                Ok(json!({"state":"modified","receipt":receipt,"detail":e.to_string()}))
            }
            Err(e) => Err(e),
        }
    })();
    // Keep the receipt stable through all dependent inventory checks.
    storage::finish_file(file, inspect, control)
}
fn resource_path(name: &str) -> bool {
    if name.len() > 240
        || !PAYLOAD_FAMILIES
            .iter()
            .any(|family| name.starts_with(family))
    {
        return false;
    }
    let parts: Vec<_> = name.split('/').collect();
    if parts.get(1).is_some_and(|p| p.eq_ignore_ascii_case(MARKER)) {
        return false;
    }
    parts.iter().all(|part| {
        if part.is_empty()
            || matches!(*part, "." | "..")
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c < ' ' || c == '\u{7f}' || "\\:<>\"|?*".contains(c))
        {
            return false;
        }
        let base = part.split('.').next().unwrap_or("").to_uppercase();
        if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
            return false;
        }
        let chars: Vec<_> = base.chars().collect();
        !(chars.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && "123456789¹²³".contains(chars[3]))
    })
}
fn inspect_inventory(
    root: &Path,
    inventory: &[Resource],
    component: &str,
    control: &OperationControl,
) -> Result<()> {
    if inventory.is_empty() || inventory.len() > 32768 {
        return Err(payload("inventory size"));
    }
    let mut entries = BTreeMap::new();
    let mut total = 0u64;
    for resource in inventory {
        control.check()?;
        if !resource_path(&resource.path) || resource.bytes > PAYLOAD_LIMIT.saturating_sub(total) {
            return Err(payload(format!("resource {:?}", resource.path)));
        }
        if !valid_digest(&resource.sha256, 64) {
            return Err(payload(format!("digest {:?}", resource.path)));
        }
        let Some(relative) = resource.path.strip_prefix(&format!("{component}/")) else {
            return Err(payload("component mismatch"));
        };
        let key = relative.to_lowercase();
        if key == MARKER {
            return Err(payload("reserved installation marker"));
        }
        if entries.insert(key, (relative, resource)).is_some() {
            return Err(payload(format!("duplicate {relative:?}")));
        }
        total += resource.bytes;
    }
    for key in entries.keys() {
        let mut parent = key.as_str();
        while let Some((head, _)) = parent.rsplit_once('/') {
            if entries.contains_key(head) {
                return Err(payload(format!("file-directory collision {head:?}")));
            }
            parent = head;
        }
    }
    let mut stack = vec![root.to_path_buf()];
    let mut seen = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut nodes = 1usize;
    while let Some(directory) = stack.pop() {
        control.check()?;
        let mut children = Vec::new();
        for child in fs::read_dir(&directory)? {
            control.check()?;
            nodes += 1;
            if nodes > 65536 {
                return Err(payload("directory budget"));
            }
            children.push(child?);
        }
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            control.check()?;
            let path = child.path();
            let name = path
                .strip_prefix(root)
                .map_err(|_| payload("path escapes root"))?
                .to_str()
                .ok_or_else(|| payload("non-UTF8 path"))?
                .replace('\\', "/");
            let kind = child.file_type()?;
            if name == MARKER {
                if !kind.is_file() {
                    return Err(payload("installation marker"));
                }
                continue;
            }
            let key = name.to_lowercase();
            if !paths.insert(key.clone()) || kind.is_symlink() {
                return Err(payload(format!("link or path collision {name:?}")));
            }
            if kind.is_dir() {
                if !resource_path(&format!("{component}/{name}/placeholder")) {
                    return Err(payload(format!("directory {name:?}")));
                }
                storage::guard_path(root, &path)?;
                stack.push(path);
                continue;
            }
            let Some((relative, resource)) = entries.get(&key) else {
                return Err(payload(format!("unlisted or special file {name:?}")));
            };
            if *relative != name || !kind.is_file() {
                return Err(payload(format!("unlisted or special file {name:?}")));
            }
            storage::guard_path(root, &path)?;
            let metadata = fs::metadata(&path)?;
            if metadata.len() != resource.bytes {
                return Err(payload(format!("size {name:?}")));
            }
            let (file, digest) = storage::locked_sha256(&path, resource.bytes, control)?;
            let compared = (|| {
                let info = file.metadata()?;
                if !info.is_file() || info.len() != resource.bytes {
                    return Err(payload(format!("size {name:?}")));
                }
                if digest != resource.sha256 {
                    return Err(payload(format!("content {name:?}")));
                }
                control.check()
            })();
            let compared = storage::finish_file(file, compared, control);
            #[cfg(test)]
            let compared = crate::status_control_tests::after_inventory_finish(compared, control);
            compared?;
            seen.insert(key);
        }
    }
    if seen.len() != entries.len() {
        return Err(payload("missing resources"));
    }
    control.check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "doctor-inspection-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn managed(&self) {
            fs::write(self.0.join("main.lua"), b"return 1\n").unwrap();
            let receipt = Receipt {
                schema: "lycheedev.installation.v1".into(),
                component: "addon".into(),
                version: "3.2.0".into(),
                commit: "a".repeat(40),
                resources: vec![Resource {
                    path: "addon/main.lua".into(),
                    bytes: 9,
                    sha256: storage::sha256(b"return 1\n"),
                }],
            };
            fs::write(self.0.join(MARKER), serde_json::to_vec(&receipt).unwrap()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn controlled_doctor_installation_states_and_read_only() {
        let f = Fixture::new();
        let c = OperationControl::default();
        assert_eq!(
            inspect_main_controlled(&f.0.join("missing"), "addon", &c).unwrap()["state"],
            "absent"
        );
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &c).unwrap()["state"],
            "unmanaged"
        );
        f.managed();
        let before = fs::read(f.0.join(MARKER)).unwrap();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &c).unwrap()["state"],
            "managed"
        );
        fs::write(f.0.join("main.lua"), b"return 2\n").unwrap();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &c).unwrap()["state"],
            "modified"
        );
        fs::remove_file(f.0.join("main.lua")).unwrap();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &c).unwrap()["state"],
            "modified"
        );
        assert_eq!(fs::read(f.0.join(MARKER)).unwrap(), before);
    }
    #[test]
    fn controlled_doctor_rejects_cancelled_and_oversize_receipt() {
        let f = Fixture::new();
        let c = OperationControl::default();
        c.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &c).unwrap_err().code,
            "command.cancelled"
        );
        fs::write(f.0.join(MARKER), vec![b' '; RECEIPT_LIMIT + 1]).unwrap();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &OperationControl::default())
                .unwrap_err()
                .code,
            "delivery.invalid_installation"
        );
    }
    #[test]
    fn controlled_doctor_path_grammar_and_duplicate_receipt() {
        // Negative evidence: historical LuaLS payloads and foreign protocol
        // paths are not current delivery families and must be rejected.
        for name in [
            "addon/../x",
            "addon/NUL.lua",
            "addon/COM¹.lua",
            "addon/x.",
            "addon/x\\y",
            "addon/.lycheedev-install.json",
            "tool/luals/bin/lua-language-server.exe",
            "tool/other/file",
            "protocol/file.lua",
        ] {
            assert!(!resource_path(name), "{name}");
        }
        for name in [
            "addon/Lychee Dev.toc",
            "skill/SKILL.md",
            "tool/luac/luac.exe",
            "tool/luac/runtime.json",
        ] {
            assert!(resource_path(name), "{name}");
        }
        let f = Fixture::new();
        f.managed();
        let raw = fs::read_to_string(f.0.join(MARKER)).unwrap().replace(
            "\"component\":\"addon\"",
            "\"component\":\"addon\",\"component\":\"addon\"",
        );
        fs::write(f.0.join(MARKER), raw).unwrap();
        assert_eq!(
            inspect_main_controlled(&f.0, "addon", &OperationControl::default())
                .unwrap_err()
                .code,
            "delivery.invalid_installation"
        );
    }
}
