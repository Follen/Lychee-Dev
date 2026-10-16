//! Raw request-target adapter over the already locked hyper HTTP/1 connectors.
//! Routing, TLS validation, cancellation and body accounting remain caller-owned.
//! HTTP/TLS parsing and DNS stay in public upstream owners. Only the bounded
//! fixed-Go SOCKS negotiation adapts a contract absent from the public connector.
use super::*;
use hyper::{
    Request, Uri,
    rt::{Read, ReadBufCursor, Write},
};
use hyper_util::{
    client::legacy::{
        Client,
        connect::{Connect, Connected, Connection, HttpConnector},
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

mod connect;
mod connect_target;
mod socks;
mod target;
use connect::{ExactName, ExactTunnel, NamedTls};
use socks::GoSocks;
use target::Target;

/// Normal requests keep the existing reqwest route. Use the raw connector if
/// WHATWG changes any wire or endpoint identity, or cannot represent the URL.
pub(super) fn target(locator: &str) -> Result<Option<Target>> {
    let (target, raw) = classify(locator)?;
    Ok(raw.then_some(target))
}
fn classify(locator: &str) -> Result<(Target, bool)> {
    let target = Target::parse(locator)?;
    let normalized = reqwest::Url::parse(locator).ok().map(|mut parsed| {
        parsed.set_fragment(None);
        parsed.to_string()
    });
    let raw = normalized.as_deref() != Some(target.wire.to_string().as_str());
    Ok((target, raw))
}

/// Select one Go proxy route per attempt. All proxied requests use the exact
/// connector: the proxy's authority must not pass through WHATWG either.
pub(super) fn prepare(locator: &str, headers: &HeaderMap) -> Result<Option<Prepared>> {
    let (target, raw) = classify(locator)?;
    let proxy = crate::remote::proxy::select_environment_raw(&target.route)?;
    if !raw && proxy.is_none() {
        return Ok(None);
    }
    Prepared::with_route(target, headers.clone(), proxy).map(Some)
}

pub(super) struct Prepared {
    request: Request<reqwest::Body>,
    target: Target,
    proxy: Option<crate::remote::proxy::ProxyEndpoint>,
    tls: tokio_native_tls::TlsConnector,
}
impl Prepared {
    #[cfg(test)]
    fn with_proxy(uri: Uri, headers: HeaderMap, proxy: Option<reqwest::Url>) -> Result<Self> {
        Self::with_target(Target::parse(&uri.to_string())?, headers, proxy)
    }
    #[cfg(test)]
    fn with_target(
        target: Target,
        headers: HeaderMap,
        proxy: Option<reqwest::Url>,
    ) -> Result<Self> {
        let proxy = proxy.map(|p| crate::remote::proxy::ProxyEndpoint {
            scheme: p.scheme().to_owned(),
            authority: p
                .as_str()
                .split_once("://")
                .unwrap()
                .1
                .split('/')
                .next()
                .unwrap()
                .to_owned(),
        });
        Self::with_route(target, headers, proxy)
    }
    fn with_route(
        target: Target,
        mut headers: HeaderMap,
        proxy: Option<crate::remote::proxy::ProxyEndpoint>,
    ) -> Result<Self> {
        // Native roots, certificate and hostname validation are never disabled.
        // No ALPN protocols are requested: this adapter is strictly HTTP/1.
        let tls: tokio_native_tls::TlsConnector = native_tls::TlsConnector::builder()
            .build()
            .map_err(|_| http("platform TLS configuration failed"))?
            .into();
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
            tls,
        })
    }
    pub(super) async fn send(mut self) -> Result<reqwest::Response> {
        let secure = self.target.wire.scheme_str() == Some("https");
        let tls = secure.then(|| (self.target.tls_host.clone(), self.tls.clone()));
        let Some(proxy) = self.proxy.take() else {
            return send(direct(&self.target, tls)?, self.request).await;
        };
        let reference =
            redirect_url::RedirectUrl::absolute(&format!("http://{}", proxy.authority))?;
        let credential = reference
            .authority
            .as_deref()
            .and_then(|a| a.rsplit_once('@'))
            .map(|(u, _)| u);
        let authority = reference
            .authority
            .as_deref()
            .unwrap()
            .rsplit('@')
            .next()
            .unwrap();
        let scheme = if proxy.scheme == "https" {
            "https"
        } else {
            "http"
        };
        let mut endpoint = Target::parse(&format!("{scheme}://{authority}/"))?;
        if proxy.scheme == "socks5h" && !explicit_port(authority) {
            endpoint.dial_port = "1080".into();
        }
        let proxy_tls =
            (proxy.scheme == "https").then(|| (endpoint.tls_host.clone(), self.tls.clone()));
        let connector = direct(&endpoint, proxy_tls)?;
        // This Uri is connector plumbing only. The connector always dials the
        // exact selected proxy name, and its TLS name is separately captured.
        let proxy_uri: Uri = "http://lycheedev-proxy.invalid/".parse().unwrap();
        if proxy.scheme == "socks5h" {
            let credentials = credential
                .map(|raw| {
                    let (user, password) = raw.split_once(':').unwrap_or((raw, ""));
                    Ok::<_, ToolkitError>((
                        redirect_url::unescape_userinfo(user)?,
                        redirect_url::unescape_userinfo(password)?,
                    ))
                })
                .transpose()?;
            let socks = GoSocks {
                inner: connector,
                proxy: proxy_uri,
                host: self.target.dial_host.clone(),
                port: self.target.dial_port.clone(),
                credentials,
            };
            // GoSocks holds the exact destination bytes; this URI is unused
            // connector plumbing and cannot change the SOCKS address.
            let destination: Uri = "http://lycheedev-socks.invalid/".parse().unwrap();
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
        let authorization = reference
            .authorization()?
            .map(|value| {
                let mut value = reqwest::header::HeaderValue::from_str(&value)
                    .map_err(|_| http("invalid proxy authorization"))?;
                value.set_sensitive(true);
                Ok::<_, ToolkitError>(value)
            })
            .transpose()?;
        if self.request.uri().scheme_str() == Some("https") {
            // ExactTunnel owns the original target; the passed connector URI
            // is never used as an origin or as a proxy destination.
            let destination: Uri = "http://lycheedev-connect.invalid/".parse().unwrap();
            let tunnel = ExactTunnel {
                inner: connector,
                proxy: proxy_uri,
                host: self.target.connect_host.clone(),
                target: self.target.connect_target.clone(),
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
                inner: connector,
                proxy: proxy_uri,
            },
            self.request,
        )
        .await
    }
}
fn explicit_port(authority: &str) -> bool {
    let suffix = authority
        .strip_prefix('[')
        .and_then(|s| s.split_once(']'))
        .map_or(authority, |(_, s)| s);
    suffix.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty())
}
fn direct(
    target: &Target,
    tls: Option<(String, tokio_native_tls::TlsConnector)>,
) -> Result<NamedTls<HttpConnector<ExactName>>> {
    // Never reinterpret an invalid port as 80/443. This runs inside the
    // controlled/admitted send, preserving the original attempt accounting.
    let port = target
        .dial_port
        .parse::<u16>()
        .map_err(|_| http("invalid remote port"))?;
    let name = target
        .dial_host
        .parse()
        .map_err(|_| http("invalid remote dial name"))?;
    let mut tcp = HttpConnector::new_with_resolver(ExactName { name });
    tcp.enforce_http(false);
    tcp.set_nodelay(true);
    let destination = format!("http://lycheedev-dial.invalid:{port}/")
        .parse()
        .map_err(|_| http("invalid remote dial port"))?;
    Ok(NamedTls {
        inner: tcp,
        destination,
        tls,
    })
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
