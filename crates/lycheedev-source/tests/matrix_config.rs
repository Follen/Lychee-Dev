use lycheedev_core::control::OperationControl;
use lycheedev_source::matrix;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn directory() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!(
            "matrix-config-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn config_is_strict_relative_bounded_and_controlled() {
    let dir = directory();
    let file = dir.join("矩阵 文件.json");
    let target = json!({"id":"a","toc":"A.toc","product":"retail","ref":"refs/heads/live"});
    fs::write(
        &file,
        serde_json::to_vec(&json!({"path":"sub/../中文 addon","targets":[target.clone()]}))
            .unwrap(),
    )
    .unwrap();
    let (config, root) = matrix::read_config(&file, &Default::default()).unwrap();
    assert_eq!(config.targets[0].reference, "refs/heads/live");
    assert_eq!(root, std::path::absolute(dir.join("中文 addon")).unwrap());
    for value in [
        json!({"targets":[]}),
        json!({"targets":[target.clone()],"extra":true}),
        json!({"targets":[{"id":"a","toc":"A.toc","product":"retail","ref":"x","extra":true}]}),
        json!({"targets":[{"id":"a","toc":"A.toc","product":"retail"}]}),
        json!({"targets":[{"id":"a","toc":"","product":"retail","ref":"x"}]}),
    ] {
        fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            matrix::read_config(&file, &Default::default())
                .unwrap_err()
                .code,
            "codebase.invalid_matrix_config"
        );
    }
    fs::write(
        &file,
        serde_json::to_vec(&json!({"targets":[target.clone(),target]})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        matrix::read_config(&file, &Default::default())
            .unwrap_err()
            .code,
        "codebase.duplicate_target_id"
    );
    fs::write(&file, vec![b' '; (1 << 20) + 1]).unwrap();
    assert_eq!(
        matrix::read_config(&file, &Default::default())
            .unwrap_err()
            .code,
        "codebase.invalid_matrix_config"
    );
    let signal = lycheedev_core::control::TrackedCancellation::new();
    signal.cancel();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), signal);
    assert_eq!(
        matrix::read_config(&file, &control).unwrap_err().exit_code,
        7
    );
}
