//! One fixed CONNECT request-line projection for Go's raw Unicode authority.
//! hyper owns request headers, response parsing and upgrade. No arbitrary
//! header replacement and no TLS bytes are inspected or changed.
use super::*;

pub(super) const PLACEHOLDER: &str = "lycheedev-connect.invalid:443";
const PREFIX: &[u8] = b"CONNECT lycheedev-connect.invalid:443 HTTP/1.1\r\n";

pub(super) struct ConnectTargetIo<T> {
    inner: T,
    prefix_at: Option<usize>,
    replacement: Option<Vec<u8>>,
    written: usize,
}
impl<T> ConnectTargetIo<T> {
    pub(super) fn new(inner: T, target: Option<&str>) -> io::Result<Self> {
        if target.is_some_and(|target| target.bytes().any(|b| b < 0x20 || b == 0x7f)) {
            return Err(io::Error::other(
                "invalid control character in CONNECT target",
            ));
        }
        Ok(Self {
            inner,
            prefix_at: target.map(|_| 0),
            replacement: target.map(|target| format!("CONNECT {target} HTTP/1.1\r\n").into_bytes()),
            written: 0,
        })
    }
}
impl<T: Write + Unpin> ConnectTargetIo<T> {
    fn flush_replacement(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.prefix_at.is_some() {
            return Poll::Ready(Ok(()));
        }
        if let Some(bytes) = &self.replacement {
            while self.written < bytes.len() {
                let n = std::task::ready!(
                    Pin::new(&mut self.inner).poll_write(cx, &bytes[self.written..])
                )?;
                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.written += n;
            }
        }
        self.replacement = None;
        Poll::Ready(Ok(()))
    }
}
impl<T: Read + Write + Unpin> Read for ConnectTargetIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.flush_replacement(cx))?;
        Pin::new(&mut this.inner).poll_read(cx, buffer)
    }
}
impl<T: Write + Unpin> Write for ConnectTargetIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        std::task::ready!(this.flush_replacement(cx))?;
        if let Some(at) = &mut this.prefix_at {
            let n = bytes.len().min(PREFIX.len() - *at);
            if bytes[..n] != PREFIX[*at..*at + n] {
                return Poll::Ready(Err(io::Error::other("unexpected CONNECT request prefix")));
            }
            *at += n;
            if *at == PREFIX.len() {
                this.prefix_at = None;
            }
            // Count consumed hyper input, not the different replacement size.
            // The private buffer owns the accepted bytes until flush/drop.
            return Poll::Ready(Ok(n));
        }
        Pin::new(&mut this.inner).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.prefix_at.is_some_and(|at| at != 0) {
            // hyper encodes a complete request line before writing. A partial
            // prefix cannot be committed or leaked as an ASCII placeholder.
            return Poll::Ready(Err(io::Error::other("incomplete CONNECT request prefix")));
        }
        std::task::ready!(this.flush_replacement(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.prefix_at.is_some_and(|at| at != 0) {
            return Poll::Ready(Err(io::Error::other("incomplete CONNECT request prefix")));
        }
        std::task::ready!(this.flush_replacement(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
