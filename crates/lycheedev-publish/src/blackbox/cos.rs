//! Native single-object COS PUT used by Workshop plugin and backup uploads.
//! Signature: https://cloud.tencent.com/document/product/436/7778
//! Upload: https://cloud.tencent.cn/document/product/436/7749
use super::*;
use base64::Engine;
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST, HeaderMap, HeaderValue},
};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Seek, SeekFrom},
};

pub(super) struct Object {
    url: Url,
    path: String,
    key_id: String,
    secret: String,
    token: String,
    pub public_url: String,
}
impl Object {
    pub fn plugin(value: &Value) -> Result<Self> {
        let info = &value["info"];
        let files = value["files"]
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| protocol_error("COS allocation must identify one original archive"))?;
        let region = info["region"].as_str().unwrap_or("ap-shanghai");
        let bucket = field(info, "bucket")?;
        if !region.starts_with("ap-")
            || !region.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(protocol_error(
                "COS allocation has an invalid bucket/region",
            ));
        }
        let host = format!("{bucket}.cos.{region}.myqcloud.com");
        let public = field(info, "host")?;
        if public != "wow.max-c.com" {
            return Err(protocol_error(
                "plugin allocation has an unsupported public archive host",
            ));
        }
        let key = field(&files[0], "key")?;
        if !key.starts_with("wow/module/") {
            return Err(protocol_error(
                "plugin allocation has an unsupported object key",
            ));
        }
        Self::new(&host, key, &info["Credentials"], public)
    }
    pub fn new(host: &str, key: &str, credentials: &Value, public_host: &str) -> Result<Self> {
        let host_ok = host.split_once(".cos.").is_some_and(|(bucket, region)| {
            !bucket.is_empty()
                && bucket
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && region.strip_suffix(".myqcloud.com").is_some_and(|r| {
                    r.starts_with("ap-") && r.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
                })
        });
        if !host_ok
            || key.is_empty()
            || key.starts_with('/')
            || key
                .split('/')
                .any(|p| p.is_empty() || matches!(p, "." | ".."))
            || key.chars().any(|c| c.is_control() || c == '\\')
        {
            return Err(protocol_error("COS object allocation is invalid"));
        }
        let make = |host: &str| -> Result<Url> {
            let mut url = Url::parse(&format!("https://{host}/"))
                .map_err(|_| protocol_error("invalid object host"))?;
            if url.host_str() != Some(host)
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(protocol_error("invalid object host"));
            }
            url.path_segments_mut()
                .map_err(|_| protocol_error("invalid object path"))?
                .clear()
                .extend(key.split('/'));
            Ok(url)
        };
        let credentials = credentials.get("Credentials").unwrap_or(credentials);
        Ok(Self {
            url: make(host)?,
            path: format!("/{key}"),
            key_id: field(credentials, "TmpSecretID")?.into(),
            secret: field(credentials, "TmpSecretKey")?.into(),
            token: field(credentials, "Token")?.into(),
            public_url: make(public_host)?.to_string(),
        })
    }
    pub fn put(
        &self,
        http: &crate::transport::Http,
        file: &mut File,
        md5: &str,
        require_etag: bool,
    ) -> Result<Value> {
        let bytes = file.metadata()?.len();
        if bytes > 5_000_000_000 {
            return Err(crate::input::invalid(
                "$.file",
                "COS single PUT supports archives up to 5 GB",
            ));
        }
        let md5_bytes =
            hex::decode(md5).map_err(|_| protocol_error("invalid original archive MD5"))?;
        if md5_bytes.len() != 16 {
            return Err(protocol_error("invalid original archive MD5"));
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            HOST,
            HeaderValue::from_str(self.url.host_str().unwrap()).unwrap(),
        );
        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&bytes.to_string()).unwrap(),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/zip"));
        headers.insert(
            "content-md5",
            HeaderValue::from_str(&base64::engine::general_purpose::STANDARD.encode(md5_bytes))
                .unwrap(),
        );
        headers.insert("x-cos-storage-class", HeaderValue::from_static("STANDARD"));
        let mut token = HeaderValue::from_str(&self.token)
            .map_err(|_| protocol_error("COS security token is invalid"))?;
        token.set_sensitive(true);
        headers.insert("x-cos-security-token", token);
        let start = now() - 60;
        let time = format!("{start};{}", start + 3600);
        let authorization = signature(&self.key_id, &self.secret, &self.path, &headers, &time)?;
        let mut auth = HeaderValue::from_str(&authorization)
            .map_err(|_| protocol_error("COS credential is invalid"))?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        file.seek(SeekFrom::Start(0))?;
        let response = http.put_file_response(self.url.clone(), headers, file.try_clone()?)?;
        let etag = response
            .headers
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_ascii_lowercase());
        if (require_etag && etag.is_none()) || etag.as_ref().is_some_and(|s| s != md5) {
            return Err(ToolkitError::new(
                "publish.blackbox_object_pending",
                "object PUT response did not confirm the original archive MD5; do not resend",
                6,
            )
            .with_details(json!({"submission":"uncertain","replay_allowed":false})));
        }
        Ok(
            json!({"http_status":response.status,"bytes":bytes,"md5":md5,"content_md5_acknowledged":true,"etag_matches_md5":etag.is_some(),"protocol":"cos-put"}),
        )
    }
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(|| protocol_error("COS allocation omitted a required field"))
}
pub(super) use crate::local_backup::md5;
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
// RFC 2104 with the existing SHA-1 primitive; no vendor SDK or runtime.
fn hmac(key: &[u8], message: &[u8]) -> String {
    let key = if key.len() > 64 {
        Sha1::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    let mut inner = [0x36u8; 64];
    let mut outer = [0x5cu8; 64];
    for (i, b) in key.iter().enumerate() {
        inner[i] ^= b;
        outer[i] ^= b;
    }
    let mut hash = Sha1::new();
    hash.update(inner);
    hash.update(message);
    let inner = hash.finalize();
    let mut hash = Sha1::new();
    hash.update(outer);
    hash.update(inner);
    hex::encode(hash.finalize())
}
fn signature(
    id: &str,
    secret: &str,
    path: &str,
    headers: &HeaderMap,
    time: &str,
) -> Result<String> {
    if !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(protocol_error("COS temporary identity is invalid"));
    }
    let headers = headers
        .iter()
        .map(|(k, v)| {
            Ok((
                encode(k.as_str()).to_ascii_lowercase(),
                encode(
                    v.to_str()
                        .map_err(|_| protocol_error("COS header is invalid"))?,
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let names = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let pairs = headers
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let http = format!("put\n{path}\n\n{pairs}\n");
    let to_sign = format!(
        "sha1\n{time}\n{}\n",
        hex::encode(Sha1::digest(http.as_bytes()))
    );
    let key = hmac(secret.as_bytes(), time.as_bytes());
    let signature = hmac(key.as_bytes(), to_sign.as_bytes());
    Ok(format!(
        "q-sign-algorithm=sha1&q-ak={id}&q-sign-time={time}&q-key-time={time}&q-header-list={names}&q-url-param-list=&q-signature={signature}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc2202_hmac_and_independent_cos_request_vector() {
        assert_eq!(
            hmac(&[0x0b; 20], b"Hi There"),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
        assert_eq!(
            hmac(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            "aa4ae5e15272d00e95705637ce8a3b55ed402112"
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            HOST,
            HeaderValue::from_static("bucket1-1254000000.cos.ap-beijing.myqcloud.com"),
        );
        headers.insert(
            "x-cos-content-sha1",
            HeaderValue::from_static("7b502c3a1f48c8609ae212cdfb639dee39673f5e"),
        );
        headers.insert("x-cos-storage-class", HeaderValue::from_static("nearline"));
        let value = signature(
            "exampleId",
            "BQYIM75p8x0iWVFSIgqEKwFprpRSVHlz",
            "/testfile2",
            &headers,
            "1417773892;1417853898",
        )
        .unwrap();
        // Tencent's published request example, independently of our encoder.
        assert!(value.ends_with("q-signature=84f5be2187452d2fe276dbdca932143ef8161145"));
        assert_eq!(encode("a 空格/+"), "a%20%E7%A9%BA%E6%A0%BC%2F%2B");
    }
}
