//! Raw request-target adapter over the already locked hyper HTTP/1 connectors.
//! Routing, TLS validation, cancellation and body accounting remain caller-owned.
//! No HTTP parser, proxy protocol, resolver worker or alternate runtime lives here.
use super::*;
use hyper::{
    Request, Uri,
    rt::{Read, ReadBufCursor, Write},
};
use hyper_util::{
    client::legacy::{
        Client,
        connect::{Connect, Connected, Connection, HttpConnector, proxy::SocksV5},
    },
    rt::TokioExecutor,
};
use std::{
    error::Error,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tower_service::Service;

type BoxError = Box<dyn Error + Send + Sync>;
type Direct = hyper_tls::HttpsConnector<HttpConnector>;

mod connect;
mod target;
use connect::{ExactName, ExactTunnel, NamedTls};
use target::Target;

/// Normal requests keep the existing reqwest route. Use the raw connector if
/// WHATWG changes any wire or endpoint identity, or cannot represent the URL.
pub(super) fn target(locator: &str) -> Result<Option<Target>> {
    let target = Target::parse(locator)?;
    let normalized = reqwest::Url::parse(locator).ok().map(|mut parsed| {
        parsed.set_fragment(None);
        parsed.to_string()
    });
    if normalized.as_deref() == Some(target.wire.to_string().as_str()) {
        Ok(None)
    } else {
        Ok(Some(target))
    }
}

pub(super) struct Prepared {
    request: Request<reqwest::Body>,
    target: Target,
    proxy: Option<reqwest::Url>,
    direct: Direct,
    tls: tokio_native_tls::TlsConnector,
}
impl Prepared {
    pub(super) fn new(target: Target, headers: HeaderMap, _locator: &str) -> Result<Self> {
        let proxy = crate::remote::proxy::select_environment(&target.route)?;
        Self::with_target(target, headers, proxy)
    }
    #[cfg(test)]
    fn with_proxy(uri: Uri, headers: HeaderMap, proxy: Option<reqwest::Url>) -> Result<Self> {
        Self::with_target(Target::parse(&uri.to_string())?, headers, proxy)
    }
    fn with_target(
        target: Target,
        mut headers: HeaderMap,
        proxy: Option<reqwest::Url>,
    ) -> Result<Self> {
        // Native roots, certificate and hostname validation are never disabled.
        // No ALPN protocols are requested: this adapter is strictly HTTP/1.
        let tls: tokio_native_tls::TlsConnector = native_tls::TlsConnector::builder()
            .build()
            .map_err(|_| http("platform TLS configuration failed"))?
            .into();
        let mut tcp = HttpConnector::new();
        tcp.enforce_http(false);
        tcp.set_nodelay(true);
        let direct = hyper_tls::HttpsConnector::from((tcp, tls.clone()));
        let mut request = Request::new(reqwest::Body::from(Vec::<u8>::new()));
        headers.insert(
            reqwest::header::HOST,
            reqwest::header::HeaderValue::from_str(target.wire.authority().unwrap().as_str())
                .map_err(|_| http("invalid remote Host header"))?,
        );
        *request.uri_mut() = target.wire.clone();
        *request.headers_mut() = headers;
        Ok(Self {
            request,
            target,
            proxy,
            direct,
            tls,
        })
    }
    pub(super) async fn send(mut self) -> Result<reqwest::Response> {
        let secure = self.target.wire.scheme_str() == Some("https");
        let tls = secure.then(|| (self.target.tls_host.clone(), self.tls.clone()));
        let Some(proxy) = self.proxy.take() else {
            // Only an actual direct dial interprets this numeric port. HTTP
            // proxy requests retain the original authority for the proxy.
            let port = self
                .target
                .dial_port
                .parse::<u16>()
                .map_err(|_| http("invalid remote port"))?;
            let name = self
                .target
                .dial_host
                .parse()
                .map_err(|_| http("invalid remote dial name"))?;
            let mut tcp = HttpConnector::new_with_resolver(ExactName { name });
            tcp.enforce_http(false);
            tcp.set_nodelay(true);
            // This private URI is only HttpConnector input. ExactName always
            // substitutes the admitted Go dial name; it never resolves this
            // placeholder. Neither HTTP nor TLS receives the placeholder.
            let destination: Uri = format!("http://lycheedev-dial.invalid:{port}/")
                .parse()
                .map_err(|_| http("invalid remote dial port"))?;
            return send(
                NamedTls {
                    inner: tcp,
                    destination,
                    tls,
                },
                self.request,
            )
            .await;
        };
        let port = proxy
            .port_or_known_default()
            .unwrap_or(if proxy.scheme() == "socks5h" {
                1080
            } else {
                80
            });
        let host = proxy
            .host_str()
            .ok_or_else(|| http("invalid proxy endpoint"))?;
        let proxy_uri: Uri = format!(
            "{}://{host}:{port}/",
            if proxy.scheme() == "https" {
                "https"
            } else {
                "http"
            }
        )
        .parse()
        .map_err(|_| http("invalid proxy endpoint"))?;
        if proxy.scheme() == "socks5h" {
            self.target
                .dial_port
                .parse::<u16>()
                .map_err(|_| http("invalid remote port"))?;
            let mut tcp = HttpConnector::new();
            tcp.enforce_http(false);
            let mut socks = SocksV5::new(proxy_uri, tcp).local_dns(false);
            if !proxy.username().is_empty() || proxy.password().is_some() {
                socks = socks.with_auth(
                    user_text(proxy.username())?,
                    user_text(proxy.password().unwrap_or(""))?,
                );
            }
            let destination = self.target.dial.clone().ok_or_else(|| {
                http("SOCKS target cannot be represented by the public connector")
            })?;
            return send(
                NamedTls {
                    inner: socks,
                    destination,
                    tls,
                },
                self.request,
            )
            .await;
        }
        let authorization = proxy_authorization(&proxy)?;
        if self.request.uri().scheme_str() == Some("https") {
            let destination = self.target.dial.clone().ok_or_else(|| {
                http("CONNECT target cannot be represented by the public connector")
            })?;
            let tunnel = ExactTunnel {
                inner: self.direct,
                proxy: proxy_uri,
                host: self.target.connect_host.clone(),
                authorization,
            };
            return send(
                NamedTls {
                    inner: tunnel,
                    destination,
                    tls,
                },
                self.request,
            )
            .await;
        }
        if let Some(authorization) = authorization {
            self.request
                .headers_mut()
                .insert(reqwest::header::PROXY_AUTHORIZATION, authorization);
        }
        send(
            Forward {
                inner: self.direct,
                proxy: proxy_uri,
            },
            self.request,
        )
        .await
    }
}
fn proxy_authorization(proxy: &reqwest::Url) -> Result<Option<reqwest::header::HeaderValue>> {
    if proxy.username().is_empty() && proxy.password().is_none() {
        return Ok(None);
    }
    let reference = redirect_url::RedirectUrl::absolute(proxy.as_str())?;
    reference
        .authorization()?
        .map(|raw| {
            let mut value = reqwest::header::HeaderValue::from_str(&raw)
                .map_err(|_| http("invalid proxy authorization"))?;
            value.set_sensitive(true);
            Ok(value)
        })
        .transpose()
}
fn user_text(raw: &str) -> Result<String> {
    // The locked public SOCKS connector accepts String credentials, matching
    // reqwest's existing adapter. Keep its UTF-8 replacement behavior explicit.
    let bytes = redirect_url::unescape_userinfo(raw)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
async fn send<C>(connector: C, request: Request<reqwest::Body>) -> Result<reqwest::Response>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let client: Client<C, reqwest::Body> = Client::builder(TokioExecutor::new())
        .pool_max_idle_per_host(0)
        .retry_canceled_requests(false)
        .build(connector);
    let response = client
        .request(request)
        .await
        .map_err(|error| http(error.to_string()))?;
    let (parts, incoming) = response.into_parts();
    // Public conversion disables automatic decompression. Existing controlled
    // read_response/drain/gzip code owns the same actual-delivery accounting.
    Ok(reqwest::Response::from(hyper::Response::from_parts(
        parts,
        reqwest::Body::wrap(incoming),
    )))
}

/// HTTP forward proxy: connect to the selected proxy and mark absolute-form.
/// TLS-to-proxy is provided by the same strict connector, not a custom handshake.
#[derive(Clone)]
struct Forward<C> {
    inner: C,
    proxy: Uri,
}
struct ForwardIo<T>(T);
impl<C> Service<Uri> for Forward<C>
where
    C: Service<Uri>,
    C::Future: Send + 'static,
    C::Response: Send + 'static,
    C::Error: Into<BoxError>,
{
    type Response = ForwardIo<C::Response>;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, BoxError>> + Send>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }
    fn call(&mut self, _: Uri) -> Self::Future {
        let future = self.inner.call(self.proxy.clone());
        Box::pin(async move { future.await.map(ForwardIo).map_err(Into::into) })
    }
}
impl<T: Connection> Connection for ForwardIo<T> {
    fn connected(&self) -> Connected {
        self.0.connected().proxy(true)
    }
}
impl<T: Read + Unpin> Read for ForwardIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buffer)
    }
}
impl<T: Write + Unpin> Write for ForwardIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write_vectored(cx, bytes)
    }
}
#[cfg(test)]
mod tests;
