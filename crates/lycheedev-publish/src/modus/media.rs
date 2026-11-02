use super::{
    protocol::{Api, checked, data},
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::{Mutation, completed, digest},
    provider::Provider,
    transport::Origin,
};
use lycheedev_core::storage;
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};
pub struct MediaUpload {
    path: PathBuf,
}
impl MediaUpload {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "media", "upload")?.admit(&doc)?;
        Ok(Self {
            path: cwd.join(doc["file"].as_str().unwrap()),
        })
    }
    pub fn schema() -> Value {
        input_shape(Provider::Modus, "media", "upload")
            .unwrap()
            .schema()
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let (file, sha256) = storage::locked_sha256(&self.path, package::MAX_ZIP, control)?;
        let result = (|| {
            Ok(
                json!({"platform":"modus","resource":"media","planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"file":self.path,"bytes":file.metadata()?.len(),"sha256":sha256,"client":"main","next":"Upload once, then reuse its returned reference in image_url. A media receipt and image-byte verification are separate facts."}),
            )
        })();
        storage::finish_file(file, result, control)
    }
    pub fn execute(
        &self,
        home: &Path,
        recover_key: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        let (mut file, sha256) = storage::locked_sha256(&self.path, package::MAX_ZIP, control)?;
        let result = (|| {
            let api = Api {
                http: crate::transport::Http::new(control)?,
                session: Some(auth::Session::load(Client::Main, control)?),
            };
            self.run(home, &api, &mut file, &sha256, recover_key, control)
        })();
        storage::finish_file(file, result, control)
    }
    fn run(
        &self,
        home: &Path,
        api: &Api,
        file: &mut File,
        sha256: &str,
        recover: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        let account = api.account()?;
        let bytes = file.metadata()?.len();
        let input = json!({"account":account,"sha256":sha256,"bytes":bytes});
        let scope = digest(&input)?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            "media",
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            "upload",
            &input,
            control,
        )?;
        if let Some(result) = completed(&m) {
            return Ok(result);
        }
        let recovered = m.pending();
        if !recovered {
            if recover.is_some() {
                return Err(ToolkitError::arguments(
                    "--recover-key applies only to an existing pending image upload",
                ));
            }
            let filename = self
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("$.file", "image basename must be UTF-8"))?;
            let mime = match self
                .path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase()
                .as_str()
            {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "gif" => "image/gif",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            };
            m.prepare_with_evidence(json!({"sha256":sha256,"bytes":bytes}), control)?;
            file.seek(SeekFrom::Start(0))?;
            let response = api
                .http
                .multipart_file(
                    Origin::Modus,
                    "/api/game/data/file/upload/file/image",
                    api.headers(None)?,
                    &[],
                    filename,
                    mime,
                    file.try_clone()?,
                )
                .and_then(checked)
                .map_err(|e| m.failure(e))?;
            let receipt=record(data(response)).map_err(|e|m.readback_error("image upload returned no reusable reference; inspect the original client result before recovering",e))?;
            m.remember(json!({"receipt":receipt,"sha256":sha256,"bytes":bytes}))?;
            m.acknowledged()?;
        }
        let (receipt, verified_bytes) = if let Some(receipt) =
            m.evidence().and_then(|e| e.get("receipt"))
        {
            (receipt.clone(), false)
        } else {
            let key=recover.ok_or_else(||m.pending_error("original image upload has no recorded key; inspect its ModUs client result and supply --recover-key for byte readback; no image was uploaded again"))?;
            let url = plugin::resource_url(key)?;
            if !api
                .http
                .verify_download(home, Provider::Modus, url, bytes, Some(sha256), None)
                .map_err(|e| m.readback_error("original image needs byte readback", e))?
            {
                return Err(m.pending_error(
                    "selected image bytes differ from the original input; no image was reuploaded",
                ));
            }
            (record(json!({"key":key}))?, true)
        };
        let result = json!({"platform":"modus","resource":"media","reference":receipt["reference"],"key":receipt["key"],"url":receipt["url"],"bytes":bytes,"sha256":sha256,"uploaded":true,"verified":true,"image_bytes_verified":verified_bytes,"verification_scope":if verified_bytes{"original_image_bytes"}else{"provider_media_receipt"},"operation_id":m.id(),"recovered":recovered});
        m.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
fn record(value: Value) -> Result<Value> {
    let url = ["downloadUrl", "url", "fileUrl"]
        .iter()
        .find_map(|k| value[*k].as_str());
    let reference = ["cosStoreKey", "cosStoreUrl", "key"]
        .iter()
        .find_map(|k| value[*k].as_str())
        .map(str::to_owned)
        .or_else(|| {
            url.and_then(|s| reqwest::Url::parse(s).ok())
                .map(|u| u.path().trim_start_matches('/').into())
        })
        .ok_or_else(|| malformed("image response omitted its object key"))?;
    if reference.trim().is_empty()
        || reference.contains(['?', '#'])
        || reference.chars().any(char::is_control)
    {
        return Err(malformed(
            "image response did not contain a permanent object key",
        ));
    }
    let url = url.map(str::to_owned).unwrap_or_else(|| {
        if reference.starts_with("https://") || reference.starts_with("http://") {
            reference.clone()
        } else {
            format!(
                "https://cdn.modus.cool/{}",
                reference.trim_start_matches('/')
            )
        }
    });
    let parsed = reqwest::Url::parse(&url)
        .map_err(|_| malformed("image response omitted its display URL"))?;
    if !["http", "https"].contains(&parsed.scheme())
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(malformed(
            "image response URL is not a permanent public reference",
        ));
    }
    Ok(json!({"reference":reference,"key":reference,"url":url}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        modus::{leveldb::tests::control, tests::api},
        test_support::{Reply, Server, TestDir},
    };
    fn account() -> Reply {
        Reply::Json(json!({"code":200,"data":{"user":{"userId":7}}}))
    }
    #[test]
    fn native_image_multipart_receipt_and_unknown_response_have_distinct_recovery() {
        let d = TestDir::new();
        let path = d.0.join("author.png");
        std::fs::write(&path, b"independent image bytes").unwrap();
        let request = MediaUpload { path: path.clone() };
        let (mut file, sha) = storage::locked_sha256(&path, 1024, &control()).unwrap();
        let server = Server::new(vec![
            account(),
            Reply::Json(
                json!({"code":200,"data":{"cosStoreKey":"modus/image/author.png","downloadUrl":"https://cdn.modus.cool/modus/image/author.png"}}),
            ),
        ]);
        let result = request
            .run(
                &d.0.join("known"),
                &api(&server, Client::Main),
                &mut file,
                &sha,
                None,
                &control(),
            )
            .unwrap();
        assert_eq!(result["verification_scope"], "provider_media_receipt");
        assert_eq!(result["image_bytes_verified"], false);
        let captured = server.finish();
        assert!(
            captured[1]
                .line
                .starts_with("POST /api/game/data/file/upload/file/image ")
        );
        assert!(
            String::from_utf8_lossy(&captured[1].body)
                .contains("name=\"file\"; filename=\"author.png\"")
        );
        assert!(
            captured[1]
                .body
                .windows(23)
                .any(|w| w == b"independent image bytes")
        );
        let server = Server::new(vec![account(), Reply::Drop]);
        assert_eq!(
            request
                .run(
                    &d.0.join("unknown"),
                    &api(&server, Client::Main),
                    &mut file,
                    &sha,
                    None,
                    &control()
                )
                .unwrap_err()
                .details["submission"],
            "uncertain"
        );
        server.finish();
        let server = Server::new(vec![
            account(),
            Reply::Bytes(b"independent image bytes".to_vec()),
        ]);
        let result = request
            .run(
                &d.0.join("unknown"),
                &api(&server, Client::Main),
                &mut file,
                &sha,
                Some("modus/image/author.png"),
                &control(),
            )
            .unwrap();
        assert_eq!(result["image_bytes_verified"], true);
        let captured = server.finish();
        assert!(captured[1].line.starts_with("GET /modus/image/author.png "));
        assert!(!captured[1].headers.contains("authorization"));
        drop(file);
    }
}
