use super::*;
/// Exact caller-resolved candidate paths. Inspection only observes names with
/// symlink_metadata and never opens legacy files, including SavedVariables.
#[derive(Clone, Debug)]
pub struct LegacyRoot {
    pub path: PathBuf,
    pub kind: String,
}
pub fn legacy_candidates(
    user_home: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<LegacyRoot> {
    let mut roots = Vec::new();
    if let Some(home) = user_home.filter(|p| !p.as_os_str().is_empty()) {
        for (name, kind) in [
            (".wowdoc", "wowdoc"),
            (".wowdata", "wowdata"),
            (".lycheedev", "lycheedev-legacy"),
        ] {
            roots.push(LegacyRoot {
                path: home.join(name),
                kind: kind.into(),
            })
        }
    }
    if let Some(local) = local_app_data.filter(|p| !p.as_os_str().is_empty()) {
        roots.push(LegacyRoot {
            path: local.join("LycheeDev/automation"),
            kind: "lycheedev-automation".into(),
        })
    }
    roots
}
fn markers(kind: &str) -> &'static [&'static str] {
    match kind {
        "wowdoc" => &[
            "catalog",
            "gitstore",
            "objectstore",
            "indexer",
            "store",
            "bin",
            "wowdoc.json",
        ],
        "wowdata" => &[
            "wowdata.config.v1",
            "config.json",
            "profiles",
            "builds",
            "cache",
            "storage",
            "state",
            "bin",
            "python",
        ],
        "lycheedev-legacy" => &[
            "config.json",
            "python",
            "bin",
            "npm",
            "storage",
            "state",
            "cache",
            "profiles",
        ],
        "lycheedev-automation" => &[
            "automation.py",
            "instances.py",
            "python",
            "config.json",
            "task-registry.json",
        ],
        _ => &[],
    }
}
pub(super) fn inspect(roots: &[LegacyRoot], control: &OperationControl) -> Result<Value> {
    let mut present = Vec::new();
    let mut data = serde_json::Map::new();
    for root in roots {
        control.check()?;
        let (detected, detail) = match fs::symlink_metadata(&root.path) {
            Err(_) => (false, "absent".to_owned()),
            Ok(info) if !info.is_dir() => {
                (true, "present but not a directory; left untouched".into())
            }
            Ok(_) => {
                let mut found = Vec::new();
                for marker in markers(&root.kind) {
                    control.check()?;
                    if fs::symlink_metadata(root.path.join(marker)).is_ok() {
                        found.push(*marker)
                    }
                }
                found.sort();
                (
                    true,
                    if found.is_empty() {
                        "present without known markers; isolated, never read".into()
                    } else {
                        format!(
                            "legacy layout detected (markers: {}); isolated, never read",
                            found.join(", ")
                        )
                    },
                )
            }
        };
        control.check()?;
        if detected {
            present.push(format!(
                "{} at {} ({detail})",
                root.kind,
                root.path.display()
            ))
        }
        data.insert(
            root.kind.clone(),
            json!({"path":root.path,"detected":detected,"detail":detail}),
        );
    }
    let mut value = check(
        "vault.legacy_roots",
        true,
        "ok",
        "",
        if present.is_empty() {
            "no legacy data locations detected".into()
        } else {
            present.join("; ")
        },
        if present.is_empty() {
            ""
        } else {
            "legacy data stays in place and is never read; remove it manually whenever you like"
        },
    );
    value["data"] = Value::Object(data);
    Ok(value)
}
