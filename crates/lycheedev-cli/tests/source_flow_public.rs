use lycheedev_core::{
    evidence,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::git;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git_run(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Flow fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Flow fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("LYCHEEDEV_SOURCE_FLOW_KEEP_FIXTURE").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
#[ignore = "requires a verified release root with bundled Lua 5.1; invokes the real CLI and owned native worker"]
fn pinned_context_flow_returns_common_proofs_and_committed_capture() {
    let release = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_FLOW_RELEASE").expect("verified release root"),
    );
    let root = std::env::temp_dir().join(format!(
        "lycheedev-public-flow-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _cleanup = Fixture(root.clone());
    let home = root.join("home");
    workspace::initialize(&home).unwrap();
    let repository = root.join("repository");
    fs::create_dir_all(repository.join("Interface/AddOns/Blizzard_APIDocumentationGenerated"))
        .unwrap();
    let prefix = "function Demo()\n Use(Source())\nend\nfunction Namespaced()\n C_Flow.Use(C_Flow.Source())\nend\nfunction Alias()\n local use=Use\n local source=Source\n local value=source()\n use(value)\nend\nfunction Unknown(value)\n Use(value)\nend\n";
    // A normal-sized file must not inherit the prototype's 64 KiB admission ceiling.
    let main = format!("{prefix}--{}\n", "large source comment ".repeat(4000));
    fs::write(repository.join("main.lua"), &main).unwrap();
    fs::write(
        repository.join("Main.toc"),
        "## Interface: 120100\nmain.lua\n",
    )
    .unwrap();
    let metadata = |namespace: &str| {
        r#"local API={Name="Flow",Type="System",Namespace="$NS",Functions={{Name="Source",Type="Function",SecretReturns=true,Returns={{Name="value",Type="number"}}},{Name="Use",Type="Function",SecretArguments="NotAllowed",Arguments={{Name="value",Type="number"}}}}}
APIDocumentation:AddDocumentationTable(API)
"#.replace("$NS",namespace)
    };
    let docs = repository.join("Interface/AddOns/Blizzard_APIDocumentationGenerated");
    fs::write(docs.join("FlowDocumentation.lua"), metadata("")).unwrap();
    fs::write(docs.join("NamespacedDocumentation.lua"), metadata("C_Flow")).unwrap();
    git_run(&repository, &["init"]);
    git_run(&repository, &["add", "."]);
    git_run(&repository, &["commit", "-m", "fixed public flow source"]);
    let commit = git_run(&repository, &["rev-parse", "HEAD"]);
    let mirror = git::mirror(&home, "wow-ui-source");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    git_run(
        &repository,
        &["clone", "--bare", ".", mirror.to_str().unwrap()],
    );
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                requested_ref: "fixture".into(),
                exact_commit: commit,
                parser_revision: "source-v1".into(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let mut observations = Vec::new();
    for (name, count) in [("Demo", 1), ("Namespaced", 1), ("Alias", 1), ("Unknown", 0)] {
        let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
            .args([
                "source",
                "context",
                "--snapshot",
                &pin.id,
                "--symbol",
                name,
                "--flow",
                "--release",
                release.to_str().unwrap(),
                "--home",
                home.to_str().unwrap(),
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert!(output.status.success(), "{envelope}");
        assert_eq!(envelope["schema"], "lycheedev.result.v1");
        assert_eq!(envelope["ok"], true);
        let flow = &envelope["result"]["flow"];
        assert_eq!(flow["schema"], "lycheedev.source-flow.v1");
        assert_eq!(
            flow["findings"]["rows"].as_array().unwrap().len(),
            count,
            "{name}: {flow}"
        );
        assert_eq!(
            flow["complete"], false,
            "unknown API effects remain explicit"
        );
        assert_eq!(flow["syntaxCheck"]["passed"], true);
        assert_eq!(flow["lifecycle"]["retired"], true);
        assert!(
            flow["lifecycle"]["elapsedMillisecondsThroughRetirement"]
                .as_u64()
                .unwrap()
                <= 2000
        );
        assert_eq!(
            flow["loadEvidence"]["candidates"][0]["ordered"],
            serde_json::json!(["main.lua"])
        );
        assert!(flow["inputIdentity"]["files"][0]["bytes"].as_u64().unwrap() > 65536);
        let graph = &flow["commonFacts"];
        for finding in flow["findings"]["rows"].as_array().unwrap() {
            let call = finding["call"].as_u64().unwrap() as usize;
            let site = &graph["calls"][call]["site"];
            let excerpt = &main
                [site["start"].as_u64().unwrap() as usize..site["end"].as_u64().unwrap() as usize];
            assert!(excerpt.contains("Use") || excerpt.contains("use("));
            let candidate = &finding["candidates"][0];
            assert!(!candidate["steps"].as_array().unwrap().is_empty());
            assert_eq!(candidate["seed"]["slot"], 1);
            let binding = candidate["seed"]["binding"].as_u64().unwrap() as usize;
            assert_eq!(
                graph["host_runtime"]["bindings"][binding]["declaration"]["effect"],
                "unknown"
            );
        }
        if name == "Unknown" {
            assert!(
                flow["gaps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|gap| gap["code"] == "dependency_value_unavailable")
            );
        }
        let capture = &envelope["captures"][0];
        let bytes = evidence::read_blob(&home, &capture["blob"], 4 << 20).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            envelope["result"]
        );
        observations.push(envelope);
    }
    if std::env::var_os("LYCHEEDEV_SOURCE_FLOW_KEEP_FIXTURE").is_some() {
        fs::write(
            root.join("report.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"home":home,"pin":pin,"results":observations}),
            )
            .unwrap(),
        )
        .unwrap();
        println!("{}", root.display());
    }
}
