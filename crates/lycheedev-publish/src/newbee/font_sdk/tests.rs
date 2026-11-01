use super::*;
use crate::test_support::TestDir;

fn asar(path: &Path, entry: Value, dist: bool, bytes: &[u8]) {
    let files = if dist {
        json!({"dist":{"files":{"nbb-core.win32-x64-msvc.node":entry}}})
    } else {
        json!({"nbb-core.win32-x64-msvc.node":entry})
    };
    let header = serde_json::to_vec(
        &json!({"files":{"node_modules":{"files":{"nbb-core":{"files":files}}}}}),
    )
    .unwrap();
    let mut file = File::create(path).unwrap();
    for value in [
        4u32,
        header.len() as u32 + 8,
        header.len() as u32 + 4,
        header.len() as u32,
    ] {
        file.write_all(&value.to_le_bytes()).unwrap();
    }
    file.write_all(&header).unwrap();
    file.write_all(bytes).unwrap();
}
#[test]
fn only_the_fixed_packed_napi_module_is_extracted_from_an_independent_asar_fixture() {
    let dir = TestDir::new();
    let path = dir.0.join("app.asar");
    let control = OperationControl::default();
    for dist in [false, true] {
        asar(
            &path,
            json!({"size":4,"offset":"3"}),
            dist,
            b"padNAPIunselected",
        );
        assert_eq!(
            extract(&mut File::open(&path).unwrap(), &control).unwrap(),
            b"NAPI"
        );
    }
    for entry in [
        json!({"size":4,"offset":"3","unpacked":true}),
        json!({"size":4,"offset":"999999"}),
        json!({"size":0,"offset":"0"}),
        json!({"size":134217728,"offset":"0"}),
    ] {
        asar(&path, entry, true, b"padNAPI");
        assert_eq!(
            extract(&mut File::open(&path).unwrap(), &control)
                .unwrap_err()
                .exit_code,
            3
        );
    }
}
fn module(dir: &TestDir, name: &str, text: &str) -> PathBuf {
    let path = dir.0.join(format!("{name}.cjs"));
    fs::write(&path, text).unwrap();
    path
}
#[test]
fn owned_node_facade_keeps_before_send_distinct_from_uncertain_and_hides_vendor_diagnostics() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    let missing = module(&dir, "missing", "exports.OssUploader = class {};");
    assert_eq!(
        invoke(&missing, &dir.0, &control).unwrap_err().details["submission"],
        "not_sent"
    );
    for (name, body) in [
        ("throw", "throw new Error('PRIVATE_SDK_DIAGNOSTIC');"),
        (
            "bad",
            "return {code:1,data:'https://objects.invalid/?signature=PRIVATE_SDK_DIAGNOSTIC'};",
        ),
        (
            "reject",
            "return {code:0,message:'PRIVATE_SDK_DIAGNOSTIC'};",
        ),
    ] {
        let binding = module(
            &dir,
            name,
            &format!(
                "exports.OssUploader = class {{ async uploadFolder() {{ console.log('PRIVATE_SDK_DIAGNOSTIC'); {body} }} }};"
            ),
        );
        let error = invoke(&binding, &dir.0, &control).unwrap_err();
        assert_eq!(error.details["submission"], "uncertain");
        assert!(!serde_json::to_string(&error).unwrap().contains("PRIVATE_"));
    }
}
#[test]
fn font_upload_stages_only_selected_locked_bytes_and_reuses_the_original_content_index() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    let selected = dir.0.join("one.ttf");
    fs::write(&selected, b"selected-font").unwrap();
    fs::write(dir.0.join("two.ttf"), b"unselected-font").unwrap();
    let binding = module(
        &dir,
        "sdk-facade",
        r#"const fs=require('node:fs'),path=require('node:path');
      exports.setLogCallback=cb=>cb('PRIVATE_VENDOR_LOG');
      exports.OssUploader=class { constructor(options) { if(JSON.parse(options).apiBase!=='https://api.next.newbeebox.com/uploadserver')throw Error('origin'); }
        async uploadFolder(input) {const folder=JSON.parse(input).path;
          if(JSON.stringify(fs.readdirSync(folder))!==JSON.stringify(['one.ttf']))throw Error('unselected files');
          if(fs.readFileSync(path.join(folder,'one.ttf'),'utf8')!=='selected-font')throw Error('bytes');
          fs.appendFileSync(path.join(__dirname,'calls'),'1');return {code:1,data:'font_index_123'};}
      };
    "#,
    );
    let sdk = FontSdk {
        module_sha256: storage::sha256(&fs::read(&binding).unwrap()),
        binding,
        client_version: "fixture".into(),
    };
    let (file, sha256) = storage::locked_sha256(&selected, u64::MAX, &control).unwrap();
    let fonts = vec![LockedFont {
        name: "one.ttf".into(),
        file,
        sha256,
    }];
    for _ in 0..2 {
        let result = sdk.upload(&dir.0, &fonts, &control).unwrap();
        assert_eq!(result.index, "font_index_123");
        assert!(!result.cleanup_pending);
    }
    assert_eq!(fs::read(dir.0.join("calls")).unwrap(), b"1");
    assert!(
        fs::read_dir(dir.0.join("publish/newbee/sdk/work"))
            .unwrap()
            .next()
            .is_none()
    );
    for font in fonts {
        storage::finish_file(font.file, Ok(()), &control).unwrap();
    }
    let records = fs::read_dir(dir.0.join("publish/newbee/records/backup-font")).unwrap();
    for record in records {
        let text = fs::read_to_string(record.unwrap().path().join("active.json")).unwrap();
        assert!(text.contains("font_index_123"));
        assert!(!text.contains("selected-font"));
        assert!(!text.contains("PRIVATE_VENDOR_LOG"));
    }
}
