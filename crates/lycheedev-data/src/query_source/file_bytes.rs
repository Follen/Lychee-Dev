//! Immutable file bytes retain their source allocation owner. Local bodies are
//! moved unchanged; remote bodies share the one authenticated allocation.
use crate::remote::session::SharedContent;
use std::{ops::Deref, sync::Arc};
pub struct FileBytes(Body);
enum Body {
    Local(crate::blte::Content),
    Remote(Arc<SharedContent>),
}
impl FileBytes {
    pub(crate) fn remote(content: Arc<SharedContent>) -> Self {
        Self(Body::Remote(content))
    }
    pub(crate) fn missing(&self) -> &[crate::blte::Missing] {
        match &self.0 {
            Body::Local(content) => &content.missing,
            Body::Remote(content) => &content.missing,
        }
    }
}
impl From<Vec<u8>> for FileBytes {
    fn from(bytes: Vec<u8>) -> Self {
        crate::blte::Content {
            bytes,
            missing: Vec::new(),
        }
        .into()
    }
}
impl From<crate::blte::Content> for FileBytes {
    fn from(content: crate::blte::Content) -> Self {
        Self(Body::Local(content))
    }
}
impl Deref for FileBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.0 {
            Body::Local(content) => &content.bytes,
            Body::Remote(content) => &content.bytes,
        }
    }
}
impl AsRef<[u8]> for FileBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_file_bytes_move_without_copy_or_public_serialization_change() {
        let raw = b"exact original local body".to_vec();
        let pointer = raw.as_ptr();
        let bytes = FileBytes::from(raw);
        assert_eq!(bytes.as_ptr(), pointer);
        assert_eq!(bytes.as_ref(), b"exact original local body");
        assert_eq!(
            lycheedev_core::storage::sha256(&bytes),
            lycheedev_core::storage::sha256(b"exact original local body")
        );
    }
}
