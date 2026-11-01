//! One operation-owned native HTTP client. Auth never follows a redirect and
//! a transport failure never retries an author mutation.
use futures_util::{StreamExt, stream};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use reqwest::{
    Client, Method, Url,
    header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, Write},
    time::Duration,
};
use tokio::{runtime::Runtime, time::MissedTickBehavior};
use tokio_util::io::ReaderStream;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Origin {
    CurseforgeCore,
    CurseforgeUpload,
    NewbeeCreator,
    NewbeeNext,
    NewbeeMetadata,
    DdUi,
    DdCcApi,
    HeyboxAccount,
    HeyboxWorkshop,
    HeyboxLogin,
    HeyboxOpen,
    WechatOpen,
    WechatPoll,
    Modus,
    ModusStatic,
}
impl Origin {
    fn url(self) -> &'static str {
        match self {
            Self::CurseforgeCore => "https://api.curseforge.com/",
            Self::CurseforgeUpload => "https://wow.curseforge.com/",
            Self::NewbeeCreator => "https://api.newbeebox.com/",
            Self::NewbeeNext => "https://api.next.newbeebox.com/",
            Self::NewbeeMetadata => "https://cdn2.newbeebox.com/",
            Self::DdUi => "https://uiapi.w.163.com/",
            Self::DdCcApi => "https://api.cc.163.com/",
            Self::HeyboxAccount => "https://api.xiaoheihe.cn/",
            Self::HeyboxWorkshop => "https://workshopapi.xiaoheihe.cn/",
            Self::HeyboxLogin => "https://login.xiaoheihe.cn/",
            Self::HeyboxOpen => "https://open.xiaoheihe.cn/",
            Self::WechatOpen => "https://open.weixin.qq.com/",
            Self::WechatPoll => "https://long.open.weixin.qq.com/",
            Self::Modus => "https://app.modus.cool/",
            Self::ModusStatic => "https://cdn.modus.cool/",
        }
    }
}

pub(crate) struct Http {
    runtime: Runtime,
    client: Client,
    control: OperationControl,
    #[cfg(test)]
    test_origin: Option<Url>,
}
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Vec<u8>,
}
impl Http {
    pub(crate) fn begin_operation(&mut self, control: &OperationControl) -> Result<()> {
        control.check()?;
        self.control = control.clone();
        Ok(())
    }
    pub(crate) fn new(control: &OperationControl) -> Result<Self> {
        Self::open(control, false)
    }
    pub(crate) fn direct(control: &OperationControl) -> Result<Self> {
        Self::open(control, true)
    }
    fn open(control: &OperationControl, direct: bool) -> Result<Self> {
        control.check()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                ToolkitError::new(
                    "publish.http_runtime",
                    "cannot initialize the publishing HTTP runtime",
                    3,
                )
            })?;
        let builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .https_only(true)
            .user_agent(concat!("Lychee-Dev/", env!("CARGO_PKG_VERSION")));
        let client = if direct { builder.no_proxy() } else { builder }
            .build()
            .map_err(|_| {
                ToolkitError::new(
                    "publish.http_client",
                    "cannot initialize the publishing HTTP client",
                    3,
                )
            })?;
        Ok(Self {
            runtime,
            client,
            control: control.clone(),
            #[cfg(test)]
            test_origin: None,
        })
    }

    fn url(&self, origin: Origin, path: &str, query: &[(&str, String)]) -> Result<Url> {
        #[cfg(test)]
        let base = self
            .test_origin
            .as_ref()
            .map(Url::as_str)
            .unwrap_or(origin.url());
        #[cfg(not(test))]
        let base = origin.url();
        let mut url = Url::parse(base).expect("fixed publisher origin");
        if !path.starts_with('/') || path.contains(['?', '#', '\\']) {
            return Err(ToolkitError::new(
                "publish.http_path",
                "invalid publisher route",
                8,
            ));
        }
        url.set_path(path);
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        }
        Ok(url)
    }

    pub(crate) fn get(
        &self,
        origin: Origin,
        path: &str,
        query: &[(&str, String)],
        headers: HeaderMap,
    ) -> Result<Value> {
        self.json_request(Method::GET, origin, path, query, headers, None, false)
    }

    pub(crate) fn post(
        &self,
        origin: Origin,
        path: &str,
        headers: HeaderMap,
        body: &Value,
        mutation: bool,
    ) -> Result<Value> {
        self.json_request(
            Method::POST,
            origin,
            path,
            &[],
            headers,
            Some(body),
            mutation,
        )
    }

    pub(crate) fn json_request(
        &self,
        method: Method,
        origin: Origin,
        path: &str,
        query: &[(&str, String)],
        headers: HeaderMap,
        body: Option<&Value>,
        mutation: bool,
    ) -> Result<Value> {
        self.control.check()?;
        let request = self
            .client
            .request(method, self.url(origin, path, query)?)
            .headers(headers)
            .timeout(self.control.remaining()?);
        let request = if let Some(body) = body {
            let bytes = serde_json::to_vec(body).map_err(|_| {
                ToolkitError::new("publish.json_encode", "cannot encode publisher request", 8)
            })?;
            request.header(CONTENT_TYPE, "application/json").body(bytes)
        } else {
            request
        };
        self.execute(request, mutation)
    }

    /// A native signer has already signed these exact bytes. Check the fixed
    /// service again before attaching author headers, then send without retry.
    pub(crate) fn signed_json(
        &self,
        origin: Origin,
        url: Url,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        mutation: bool,
    ) -> Result<Value> {
        let request = self.signed_request(origin, url, headers, body)?;
        self.execute(request, mutation)
    }
    fn signed_request(
        &self,
        origin: Origin,
        url: Url,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::RequestBuilder> {
        let expected = Url::parse(origin.url()).expect("fixed publisher origin");
        if url.origin() != expected.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ToolkitError::new(
                "publish.signed_origin",
                "native signature changed the author service",
                3,
            ));
        }
        #[cfg(test)]
        let url = {
            let mut url = url;
            if let Some(origin) = &self.test_origin {
                url.set_scheme(origin.scheme()).unwrap();
                url.set_host(origin.host_str()).unwrap();
                url.set_port(origin.port()).unwrap();
            }
            url
        };
        let method = if body.is_some() {
            Method::POST
        } else {
            Method::GET
        };
        let request = self
            .client
            .request(method, url)
            .headers(headers)
            .timeout(self.control.remaining()?);
        let request = if let Some(bytes) = body {
            request.body(bytes)
        } else {
            request
        };
        Ok(request)
    }
    /// OAuth callers inspect an individual redirect and its cookies before
    /// choosing the next fixed route. No redirect is followed by the client.
    pub(crate) fn signed_response(
        &self,
        origin: Origin,
        url: Url,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        mutation: bool,
        timeout: Duration,
    ) -> Result<Response> {
        self.raw_response(
            self.signed_request(origin, url, headers, body)?
                .timeout(self.control.remaining()?.min(timeout)),
            mutation,
            true,
        )
    }
    pub(crate) fn form(
        &self,
        origin: Origin,
        path: &str,
        headers: HeaderMap,
        fields: &[(&str, &str)],
    ) -> Result<Value> {
        self.control.check()?;
        let request = self
            .client
            .post(self.url(origin, path, &[])?)
            .headers(headers)
            .timeout(self.control.remaining()?)
            .form(fields);
        self.execute(request, true)
    }

    pub(crate) fn multipart(
        &self,
        origin: Origin,
        path: &str,
        headers: HeaderMap,
        metadata: &Value,
        filename: &str,
        file: File,
    ) -> Result<Value> {
        self.control.check()?;
        // Header values are derived from the already admitted local basename.
        // CR/LF, quotes and backslashes are not valid multipart parameters.
        if filename.is_empty() || filename.contains(['\r', '\n', '"', '\\']) {
            return Err(ToolkitError::invalid(
                "upload basename cannot be represented in multipart form-data",
            ));
        }
        let boundary = format!(
            "----lychee-{}",
            lycheedev_core::storage::sha256(
                format!(
                    "{}:{}",
                    std::process::id(),
                    lycheedev_core::storage::utc_now()
                )
                .as_bytes()
            )
        );
        let metadata = serde_json::to_string(metadata).map_err(|_| {
            ToolkitError::new("publish.metadata", "cannot encode upload metadata", 8)
        })?;
        let prefix = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/zip\r\n\r\n").into_bytes();
        self.multipart_body(origin, path, headers, &boundary, prefix, file)
    }

    pub(crate) fn multipart_file(
        &self,
        origin: Origin,
        path: &str,
        headers: HeaderMap,
        fields: &[(&str, String)],
        filename: &str,
        content_type: &str,
        file: File,
    ) -> Result<Value> {
        if filename.is_empty() || filename.contains(['\r', '\n', '"', '\\']) {
            return Err(ToolkitError::invalid("invalid multipart upload basename"));
        }
        let boundary = format!(
            "----lychee-{}",
            lycheedev_windows::random::token().map_err(|_| ToolkitError::new(
                "publish.multipart",
                "cannot create multipart boundary",
                3
            ))?
        );
        let mut prefix = String::new();
        for (name, value) in fields {
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(ToolkitError::new(
                    "publish.multipart_field",
                    "invalid multipart field name",
                    8,
                ));
            }
            prefix.push_str(&format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        prefix.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"));
        self.multipart_body(origin, path, headers, &boundary, prefix.into_bytes(), file)
    }

    fn multipart_body(
        &self,
        origin: Origin,
        path: &str,
        headers: HeaderMap,
        boundary: &str,
        prefix: Vec<u8>,
        file: File,
    ) -> Result<Value> {
        let suffix = format!("\r\n--{boundary}--\r\n").into_bytes();
        let length = file
            .metadata()?
            .len()
            .checked_add(prefix.len() as u64)
            .and_then(|n| n.checked_add(suffix.len() as u64))
            .ok_or_else(|| ToolkitError::invalid("multipart upload length overflow"))?;
        let chunks = stream::once(async move { Ok::<_, io::Error>(prefix) })
            .chain(
                ReaderStream::new(tokio::fs::File::from_std(file))
                    .map(|chunk| chunk.map(|bytes| bytes.to_vec())),
            )
            .chain(stream::once(async move { Ok::<_, io::Error>(suffix) }));
        let request = self
            .client
            .request(Method::POST, self.url(origin, path, &[])?)
            .headers(headers)
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(CONTENT_LENGTH, length)
            .timeout(self.control.remaining()?)
            .body(reqwest::Body::wrap_stream(chunks));
        self.execute(request, true)
    }

    fn execute(&self, request: reqwest::RequestBuilder, mutation: bool) -> Result<Value> {
        self.execute_response(request, mutation, true)
    }
    /// Artifact reads deliberately inherit no author headers or redirects.
    pub(crate) fn download_file(&self, url: Url, file: &mut File, maximum: u64) -> Result<u64> {
        self.control.check()?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ToolkitError::invalid(
                "artifact download requires a credential-free HTTPS authority",
            ));
        }
        #[cfg(test)]
        let url = {
            let mut url = url;
            if let Some(origin) = &self.test_origin {
                url.set_scheme(origin.scheme()).unwrap();
                url.set_host(origin.host_str()).unwrap();
                url.set_port(origin.port()).unwrap();
            }
            url
        };
        let request = self.client.get(url).timeout(self.control.remaining()?);
        let result=self.runtime.block_on(async{
            let future=async{
                let mut response=request.send().await.map_err(|_|http_error("publish.download_network","artifact download did not produce a complete response",false,None))?;
                let status=response.status();if status.as_u16()!=200{return Err(http_error("publish.download_status","artifact download was rejected",false,Some(status.as_u16())));}
                if response.content_length().is_some_and(|n|n>maximum){return Err(ToolkitError::invalid("artifact download exceeds its size limit"));}
                let mut length=0u64;
                while let Some(bytes)=response.chunk().await.map_err(|_|http_error("publish.download_incomplete","artifact download was interrupted",false,None))?{
                    self.control.check()?;length=length.checked_add(bytes.len()as u64).filter(|n|*n<=maximum).ok_or_else(||ToolkitError::invalid("artifact download exceeds its size limit"))?;file.write_all(&bytes)?;
                }
                file.sync_all()?;Ok(length)
            };
            tokio::pin!(future);let mut tick=tokio::time::interval(Duration::from_millis(20));tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop{tokio::select!{value=&mut future=>return value,_=tick.tick()=>self.control.check()?,}}
        });
        self.control.check()?;
        result
    }
    pub(crate) fn verify_download(
        &self,
        home: &std::path::Path,
        provider: crate::provider::Provider,
        url: Url,
        bytes: u64,
        sha256: Option<&str>,
        md5: Option<&str>,
    ) -> Result<bool> {
        use lycheedev_core::storage;
        use sha2::Digest;
        use std::io::{Read, Seek, SeekFrom};
        let directory = home.join("publish").join(provider.id()).join("checks");
        storage::guard_path(home, &directory)?;
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!(
            "{}.part",
            lycheedev_windows::random::token().map_err(|_| ToolkitError::new(
                "publish.check_id",
                "cannot create temporary readback identity",
                3
            ))?
        ));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            let length = self.download_file(url, &mut file, bytes)?;
            file.seek(SeekFrom::Start(0))?;
            let mut sha = sha2::Sha256::new();
            let mut md = md5::Md5::new();
            let mut buffer = [0u8; 65536];
            loop {
                self.control.check()?;
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                sha.update(&buffer[..n]);
                md.update(&buffer[..n]);
            }
            Ok(length == bytes
                && sha256.is_none_or(|expected| hex::encode(sha.finalize()) == expected)
                && md5.is_none_or(|expected| hex::encode(md.finalize()) == expected))
        })();
        let result = storage::finish_file(file, result, &self.control);
        match std::fs::remove_file(&path) {Ok(())=>result,Err(error)=>Err(ToolkitError::new("publish.check_cleanup","artifact readback finished; remove its temporary check file before continuing",3).with_details(json!({"temporary_file":path,"readback_completed":result.is_ok(),"io_kind":format!("{:?}",error.kind())})))}
    }
    pub(crate) fn put_file(&self, url: Url, headers: HeaderMap, file: File) -> Result<Value> {
        let response = self.put_file_response(url, headers, file)?;
        Ok(json!({"http_status":response.status}))
    }
    pub(crate) fn put_file_response(
        &self,
        url: Url,
        headers: HeaderMap,
        file: File,
    ) -> Result<Response> {
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ToolkitError::new(
                "publish.object_url",
                "object upload requires a credential-free HTTPS authority",
                4,
            ));
        }
        #[cfg(test)]
        let url = {
            let mut url = url;
            if let Some(origin) = &self.test_origin {
                url.set_scheme(origin.scheme()).unwrap();
                url.set_host(origin.host_str()).unwrap();
                url.set_port(origin.port()).unwrap();
            }
            url
        };
        let request = self
            .client
            .put(url)
            .headers(headers)
            .header(CONTENT_LENGTH, file.metadata()?.len())
            .timeout(self.control.remaining()?)
            .body(reqwest::Body::wrap_stream(ReaderStream::new(
                tokio::fs::File::from_std(file),
            )));
        self.raw_response(request, true, false)
    }
    fn execute_response(
        &self,
        request: reqwest::RequestBuilder,
        mutation: bool,
        json_response: bool,
    ) -> Result<Value> {
        let response = self.raw_response(request, mutation, false)?;
        if !json_response {
            return Ok(json!({"http_status":response.status}));
        }
        if response.body.is_empty() {
            return Ok(json!({}));
        }
        crate::input::value(&response.body).map_err(|_| {
            http_error(
                "publish.response_json",
                "publisher response was not valid unambiguous JSON",
                mutation,
                None,
            )
        })
    }
    fn raw_response(
        &self,
        request: reqwest::RequestBuilder,
        mutation: bool,
        redirects: bool,
    ) -> Result<Response> {
        self.control.check().map_err(|mut error| {
            if mutation {
                error.details = json!({"submission":"not_sent","replay_allowed":false});
            }
            error
        })?;
        let result=self.runtime.block_on(async {
            let future = async {
                let mut response = request.send().await.map_err(|cause| {
                    let mut error = http_error("publish.network","publisher request did not produce a complete response",mutation,None);
                    if mutation && cause.is_connect() { error.details["submission"] = json!("not_sent"); }
                    error
                })?;
                let status = response.status();
                if !status.is_success() && !(redirects && status.is_redirection()) {
                    // Bodies can contain signed URLs, phone numbers or echoed
                    // author input. Only the HTTP status crosses this boundary.
                    return Err(http_error("publish.http_status","publisher rejected the request",mutation,Some(status.as_u16())));
                }
                if response.content_length().is_some_and(|n|n>MAX_RESPONSE as u64) {
                    return Err(http_error("publish.response_limit","publisher response exceeded the metadata limit",mutation,None));
                }
                let headers = response.headers().clone();
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_|http_error("publish.response_incomplete","publisher response was interrupted",mutation,None))? {
                    if body.len().saturating_add(chunk.len())>MAX_RESPONSE { return Err(http_error("publish.response_limit","publisher response exceeded the metadata limit",mutation,None)); }
                    body.extend_from_slice(&chunk);
                }
                Ok(Response {status:status.as_u16(), headers, body})
            };
            tokio::pin!(future);
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    result = &mut future => return result,
                    _ = tick.tick() => if let Err(mut error) = self.control.check() {
                        if mutation { error.details=json!({"submission":"uncertain","replay_allowed":false}); }
                        return Err(error);
                    }
                }
            }
        });
        if !mutation {
            self.control.check()?;
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn loopback(control: &OperationControl, url: String) -> Self {
        let mut http = Self::new(control).unwrap();
        let origin = Url::parse(&url).unwrap();
        assert_eq!(origin.host_str(), Some("127.0.0.1"));
        http.test_origin = Some(origin);
        http.client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .build()
            .unwrap();
        http
    }
}

pub(crate) fn secret_header(name: &'static str, value: &str) -> Result<HeaderMap> {
    let mut header = HeaderValue::from_str(value).map_err(|_| {
        ToolkitError::new(
            "publish.credentials",
            "publisher credential is not a valid HTTP header",
            3,
        )
    })?;
    header.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(name, header);
    Ok(headers)
}

fn http_error(code: &str, message: &str, mutation: bool, status: Option<u16>) -> ToolkitError {
    ToolkitError::new(code, message, 5).with_details(json!({"http_status":status,
        "submission":if mutation {"uncertain"} else {"not_applicable"},"replay_allowed":!mutation}))
}

pub(crate) fn object_url(value: &str, backup: bool) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(value)
        .map_err(|_| ToolkitError::new("publish.object_url", "object upload URL is invalid", 4))?;
    if backup
        && url.scheme() == "http"
        && url.host_str() == Some("wclboxdata.cn-hangzhou.oss.aliyuncs.com")
        && url.port().is_none_or(|port| port == 80)
    {
        url.set_scheme("https").unwrap();
        url.set_port(None).unwrap();
    }
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ToolkitError::new(
            "publish.object_url",
            "object upload requires a credential-free HTTPS authority",
            4,
        ));
    }
    Ok(url)
}
