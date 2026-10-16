//! Small adapters for the already locked public DNS/TLS/HTTP1 connectors.
use super::*;
/// Keep Go's exact canonical dial name, including an IPv6 zone or a Lookup
/// failure's original Unicode name. The official resolver owns its OS task.
#[derive(Clone)]
pub(super) struct ExactName {
    pub(super) name: hyper_util::client::legacy::connect::dns::Name,
}
impl Service<hyper_util::client::legacy::connect::dns::Name> for ExactName {
    type Response = <hyper_util::client::legacy::connect::dns::GaiResolver as Service<
        hyper_util::client::legacy::connect::dns::Name,
    >>::Response;
    type Error = io::Error;
    type Future = <hyper_util::client::legacy::connect::dns::GaiResolver as Service<
        hyper_util::client::legacy::connect::dns::Name,
    >>::Future;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, _: hyper_util::client::legacy::connect::dns::Name) -> Self::Future {
        hyper_util::client::legacy::connect::dns::GaiResolver::new().call(self.name.clone())
    }
}

/// Public connector composition: send the admitted dial identity to the inner
/// connector, then verify TLS against the distinct Go verification name. Uses
/// the locked native TLS and hyper-tls stream owner without custom TLS/HTTP IO.
#[derive(Clone)]
pub(super) struct NamedTls<C> {
    pub(super) inner: C,
    pub(super) destination: Uri,
    pub(super) tls: Option<(String, tokio_native_tls::TlsConnector)>,
}
impl<C> Service<Uri> for NamedTls<C>
where
    C: Service<Uri>,
    C::Response: Read + Write + Send + Unpin + 'static,
    C::Future: Send + 'static,
    C::Error: Into<BoxError>,
{
    type Response = hyper_tls::MaybeHttpsStream<C::Response>;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, BoxError>> + Send>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }
    fn call(&mut self, _: Uri) -> Self::Future {
        let connecting = self.inner.call(self.destination.clone());
        let tls = self.tls.clone();
        Box::pin(async move {
            let stream = connecting.await.map_err(Into::into)?;
            Ok(match tls {
                Some((name, connector)) => {
                    hyper_tls::MaybeHttpsStream::Https(hyper_util::rt::TokioIo::new(
                        connector
                            .connect(&name, hyper_util::rt::TokioIo::new(stream))
                            .await?,
                    ))
                }
                None => hyper_tls::MaybeHttpsStream::Http(stream),
            })
        })
    }
}

/// Use hyper's standard CONNECT parser and upgrade owner. Its request Uri is
/// serialized as-is, so a legal padded port and zone remain in the CONNECT
/// request-target while Host follows Go's zone-free wire spelling.
#[derive(Clone)]
pub(super) struct ExactTunnel<C> {
    pub(super) inner: C,
    pub(super) proxy: Uri,
    pub(super) host: String,
    pub(super) authorization: Option<reqwest::header::HeaderValue>,
}
impl<C> Service<Uri> for ExactTunnel<C>
where
    C: Service<Uri>,
    C::Response: Read + Write + Send + Unpin + 'static,
    C::Future: Send + 'static,
    C::Error: Into<BoxError>,
{
    type Response = TunnelIo;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, BoxError>> + Send>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }
    fn call(&mut self, destination: Uri) -> Self::Future {
        let connecting = self.inner.call(self.proxy.clone());
        let host = self.host.clone();
        let authorization = self.authorization.clone();
        Box::pin(async move {
            let stream = connecting.await.map_err(Into::into)?;
            let authority = destination
                .authority()
                .ok_or_else(|| io::Error::other("missing CONNECT authority"))?;
            let mut request = Request::builder()
                .method(hyper::Method::CONNECT)
                .uri(authority.as_str())
                .header(reqwest::header::HOST, host)
                .header(reqwest::header::USER_AGENT, "Go-http-client/1.1")
                .body(reqwest::Body::from(Vec::<u8>::new()))?;
            if let Some(authorization) = authorization {
                request
                    .headers_mut()
                    .insert(reqwest::header::PROXY_AUTHORIZATION, authorization);
            }
            let (mut sender, connection) = hyper::client::conn::http1::handshake(stream).await?;
            let exchange = async move {
                let mut response = sender.send_request(request).await?;
                if response.status() != hyper::StatusCode::OK {
                    return Err(io::Error::other(format!(
                        "proxy CONNECT returned {}",
                        response.status()
                    ))
                    .into());
                }
                Ok::<TunnelIo, BoxError>(TunnelIo(hyper::upgrade::on(&mut response).await?))
            };
            let connection = connection.with_upgrades();
            tokio::pin!(connection, exchange);
            // Both futures live in the request's original controlled send.
            // Cancellation drops both here; no detached driver/task survives.
            tokio::select! {
                result = &mut exchange => result,
                result = &mut connection => { result?; exchange.await }
            }
        })
    }
}
pub(super) struct TunnelIo(hyper::upgrade::Upgraded);
impl Connection for TunnelIo {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}
impl Read for TunnelIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buffer)
    }
}
impl Write for TunnelIo {
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
