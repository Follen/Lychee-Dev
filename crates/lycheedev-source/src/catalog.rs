#![forbid(unsafe_code)]
use lycheedev_core::{Result, ToolkitError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub key: String,
    pub label: String,
    pub url: String,
    pub tracks: BTreeMap<String, String>,
}
pub fn repositories() -> Vec<Repository> {
    [
        (
            "wow-ui-source",
            "World of Warcraft UI Source Mirror",
            "https://github.com/Gethe/wow-ui-source.git",
            vec![
                ("retail", "live"),
                ("ptr", "ptr"),
                ("ptr2", "ptr2"),
                ("beta", "beta"),
                ("classic", "classic"),
                ("classic-ptr", "classic_ptr"),
                ("classic-beta", "classic_beta"),
                ("classic-era", "classic_era"),
                ("classic-era-ptr", "classic_era_ptr"),
                ("anniversary", "classic_anniversary"),
                ("titan", "classic_titan"),
                ("forever", "forever"),
            ],
        ),
        (
            "elvui",
            "ElvUI",
            "https://github.com/tukui-org/ElvUI.git",
            vec![("main", "main"), ("ptr", "ptr")],
        ),
        (
            "weakauras",
            "WeakAuras2",
            "https://github.com/WeakAuras/WeakAuras2.git",
            vec![("main", "main")],
        ),
        (
            "ndui",
            "NDui",
            "https://github.com/siweia/NDui.git",
            vec![
                ("main", "master"),
                ("classic", "Classic"),
                ("era", "Era"),
                ("anniversary", "Anniversary"),
                ("titan", "Titan"),
            ],
        ),
        (
            "ellesmereui",
            "EllesmereUI",
            "https://github.com/EllesmereGaming/EllesmereUI.git",
            vec![("main", "main")],
        ),
    ]
    .into_iter()
    .map(|(key, label, url, tracks)| Repository {
        key: key.into(),
        label: label.into(),
        url: url.into(),
        tracks: tracks
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
    })
    .collect()
}
pub fn lookup(key: &str) -> Result<Repository> {
    repositories()
        .into_iter()
        .find(|r| r.key == key)
        .ok_or_else(|| {
            ToolkitError::new(
                "codebase.unknown_repository",
                format!("Unknown source repository: {key}"),
                2,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_forever_track_uses_only_the_formal_product() {
        let repository = lookup("wow-ui-source").unwrap();
        assert_eq!(repository.tracks.get("forever").map(String::as_str), Some("forever"));
        for retired in ["forever-asia-beta", "forever-cn-beta"] {
            assert!(!repository.tracks.contains_key(retired));
        }
        assert_eq!(repository.tracks["classic-beta"], "classic_beta");
        assert_eq!(repository.tracks["classic-era"], "classic_era");
        assert_eq!(repository.tracks["anniversary"], "classic_anniversary");
        assert!(!repository.tracks.contains_key("cn-beta"));
        assert!(
            !lookup("elvui")
                .unwrap()
                .tracks
                .contains_key("forever-cn-beta")
        );
    }
}
