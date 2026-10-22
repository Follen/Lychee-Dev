//! Bounded native worker request serialization.
use serde::Serialize;
use std::io::Write;
pub(crate) fn serialize(value: &impl Serialize, maximum: usize) -> lycheedev_core::Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            let needed = self
                .bytes
                .len()
                .checked_add(input.len())
                .filter(|n| *n <= self.maximum)
                .ok_or_else(|| std::io::Error::other("Source frame byte budget"))?;
            if needed > self.bytes.capacity() {
                let target = needed
                    .max(self.bytes.capacity().max(1024).saturating_mul(2))
                    .min(self.maximum);
                self.bytes
                    .try_reserve_exact(target - self.bytes.len())
                    .map_err(|_| std::io::Error::other("Source frame allocation budget"))?;
            }
            self.bytes.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut out, value)
        .map_err(|_| lycheedev_core::ToolkitError::new("source.native_frame_limit", "Source frame exceeds budget", 3))?;
    Ok(out.bytes)
}
