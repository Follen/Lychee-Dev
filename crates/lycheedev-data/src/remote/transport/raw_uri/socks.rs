// SPDX-License-Identifier: BSD-3-Clause
// Copyright 2009 The Go Authors.
// Adapted from Go 1.27.1 net/http/socks_bundle.go (bundled x/net/internal/socks).
// The complete Go notice is retained in ../GO-LICENSE and package notices.
//! Fixed Go net/http SOCKS5 handshake compatibility over the existing connector.
//! No socket resolver, TLS, HTTP parser, task, runtime or reusable proxy API.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub(super) struct GoSocks<C> {
    pub(super) inner: C,
    pub(super) proxy: Uri,
    pub(super) host: Vec<u8>,
    pub(super) port: String,
    pub(super) credentials: Option<(Vec<u8>, Vec<u8>)>,
}
impl<C> Service<Uri> for GoSocks<C>
where
    C: Service<Uri>,
    C::Response: Read + Write + Send + Unpin + 'static,
    C::Future: Send + 'static,
    C::Error: Into<BoxError>,
{
    type Response = C::Response;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, BoxError>> + Send>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }
    fn call(&mut self, _: Uri) -> Self::Future {
        let connecting = self.inner.call(self.proxy.clone());
        let host = self.host.clone();
        let port = self.port.clone();
        let credentials = self.credentials.clone();
        Box::pin(async move {
            let stream = connecting.await.map_err(Into::into)?;
            // Go validates the target port after acquiring the proxy socket,
            // before emitting a SOCKS greeting. Failure owns/drops that socket.
            let port = port.parse::<u16>().map_err(|_| invalid("invalid port"))?;
            let mut stream = hyper_util::rt::TokioIo::new(stream);
            let mut buffer = [0u8; 513];
            let greeting: &[u8] = if credentials.is_some() {
                &[5, 2, 0, 2]
            } else {
                &[5, 1, 0]
            };
            stream.write_all(greeting).await?;
            stream.read_exact(&mut buffer[..2]).await?;
            if buffer[0] != 5 {
                return Err(invalid("unexpected protocol version").into());
            }
            if buffer[1] == 255 {
                return Err(invalid("no acceptable authentication methods").into());
            }
            if let Some((user, password)) = credentials {
                match buffer[1] {
                    0 => (),
                    2 => {
                        if user.is_empty() || user.len() > 255 || password.len() > 255 {
                            return Err(invalid("invalid username/password").into());
                        }
                        buffer[0] = 1;
                        buffer[1] = user.len() as u8;
                        buffer[2..2 + user.len()].copy_from_slice(&user);
                        buffer[2 + user.len()] = password.len() as u8;
                        buffer[3 + user.len()..3 + user.len() + password.len()]
                            .copy_from_slice(&password);
                        stream
                            .write_all(&buffer[..3 + user.len() + password.len()])
                            .await?;
                        stream.read_exact(&mut buffer[..2]).await?;
                        if buffer[0] != 1 {
                            return Err(invalid("invalid username/password version").into());
                        }
                        if buffer[1] != 0 {
                            return Err(invalid("username/password authentication failed").into());
                        }
                    }
                    _ => return Err(invalid("unsupported authentication method").into()),
                }
            }
            // With no configured Authenticate callback, fixed Go accepts every
            // selected method except FF and proceeds. Preserve that ordering.
            buffer[..3].copy_from_slice(&[5, 1, 0]);
            let size = match std::str::from_utf8(&host)
                .ok()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            {
                Some(std::net::IpAddr::V4(ip)) => {
                    buffer[3] = 1;
                    buffer[4..8].copy_from_slice(&ip.octets());
                    8
                }
                Some(std::net::IpAddr::V6(ip)) => match ip.to_ipv4_mapped() {
                    Some(ip) => {
                        buffer[3] = 1;
                        buffer[4..8].copy_from_slice(&ip.octets());
                        8
                    }
                    None => {
                        buffer[3] = 4;
                        buffer[4..20].copy_from_slice(&ip.octets());
                        20
                    }
                },
                None => {
                    if host.len() > 255 {
                        return Err(invalid("FQDN too long").into());
                    }
                    buffer[3] = 3;
                    buffer[4] = host.len() as u8;
                    buffer[5..5 + host.len()].copy_from_slice(&host);
                    5 + host.len()
                }
            };
            buffer[size..size + 2].copy_from_slice(&port.to_be_bytes());
            stream.write_all(&buffer[..size + 2]).await?;
            stream.read_exact(&mut buffer[..4]).await?;
            if buffer[0] != 5 {
                return Err(invalid("unexpected protocol version").into());
            }
            if buffer[1] != 0 {
                return Err(invalid("SOCKS proxy connection failed").into());
            }
            if buffer[2] != 0 {
                return Err(invalid("non-zero reserved field").into());
            }
            let length = match buffer[3] {
                1 => 6,
                4 => 18,
                3 => {
                    stream.read_exact(&mut buffer[..1]).await?;
                    usize::from(buffer[0]) + 2
                }
                _ => return Err(invalid("unknown address type").into()),
            };
            stream.read_exact(&mut buffer[..length]).await?;
            Ok(stream.into_inner())
        })
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}
