//! Optional bounded prefix observation of the already-owned stderr pipe.
//! No callback, path, new reader thread, or influence on process outcomes.
use std::{
    io::{self, Read},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};
pub(crate) const CAPACITY: usize = 64 * 1024;
const ACTIVE: u8 = 0;
const LIMIT: u8 = 1;
const CONTENDED: u8 = 2;
const POISONED: u8 = 3;
#[derive(Clone)]
pub(crate) struct StderrObservation {
    inner: Arc<Inner>,
}
struct Inner {
    bytes: Mutex<Vec<u8>>,
    stopped: AtomicU8,
}
pub(crate) struct Snapshot {
    pub bytes: Vec<u8>,
    pub state: &'static str,
}
impl StderrObservation {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                bytes: Mutex::new(Vec::with_capacity(CAPACITY)),
                stopped: AtomicU8::new(ACTIVE),
            }),
        }
    }
    fn stop(&self, reason: u8) {
        let _ = self.inner.stopped.compare_exchange(
            ACTIVE,
            reason,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    fn record(&self, chunk: &[u8]) {
        if chunk.is_empty() || self.inner.stopped.load(Ordering::Acquire) != ACTIVE {
            return;
        }
        let mut bytes = match self.inner.bytes.try_lock() {
            Ok(bytes) => bytes,
            Err(std::sync::TryLockError::WouldBlock) => {
                self.stop(CONTENDED);
                return;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                self.stop(POISONED);
                return;
            }
        };
        // There is one producer. Once any chunk is missed, never append later
        // chunks: otherwise two partial lines could form a fabricated record.
        if self.inner.stopped.load(Ordering::Acquire) != ACTIVE {
            return;
        }
        let count = chunk.len().min(CAPACITY - bytes.len());
        bytes.extend_from_slice(&chunk[..count]);
        if count < chunk.len() {
            self.stop(LIMIT);
        }
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        match self.inner.bytes.try_lock() {
            Ok(bytes) => Snapshot {
                bytes: bytes.clone(),
                state: match self.inner.stopped.load(Ordering::Acquire) {
                    ACTIVE => "prefix",
                    LIMIT => "prefix_limit",
                    CONTENDED => "prefix_contended",
                    _ => "prefix_poisoned",
                },
            },
            Err(std::sync::TryLockError::WouldBlock) => Snapshot {
                bytes: Vec::new(),
                state: "unavailable_contended",
            },
            Err(std::sync::TryLockError::Poisoned(_)) => Snapshot {
                bytes: Vec::new(),
                state: "unavailable_poisoned",
            },
        }
    }
}
pub(super) struct ObservedReader<R> {
    pub reader: R,
    pub observation: StderrObservation,
}
impl<R: Read> Read for ObservedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(buffer)?;
        self.observation.record(&buffer[..count]);
        Ok(count)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_chunks_preserve_exact_prefix() {
        let o = StderrObservation::new();
        for chunk in [b"{abc".as_slice(), b"}\n{", b"def}\n"] {
            o.record(chunk);
        }
        assert_eq!(o.snapshot().bytes, b"{abc}\n{def}\n");
    }
    #[test]
    fn fixed_capacity_stops_without_joining_later_bytes() {
        let o = StderrObservation::new();
        o.record(&vec![b'a'; CAPACITY - 2]);
        o.record(b"1234");
        o.record(b"later\n");
        let s = o.snapshot();
        assert_eq!(s.bytes.len(), CAPACITY);
        assert_eq!(&s.bytes[CAPACITY - 2..], b"12");
        assert_eq!(s.state, "prefix_limit");
    }
    #[test]
    fn contention_stops_capture_permanently_and_snapshot_never_waits() {
        let o = StderrObservation::new();
        o.record(b"prefix");
        {
            let _guard = o.inner.bytes.lock().unwrap();
            o.record(b"missing");
            assert_eq!(o.snapshot().state, "unavailable_contended");
        }
        o.record(b"suffix\n");
        let s = o.snapshot();
        assert_eq!(s.bytes, b"prefix");
        assert_eq!(s.state, "prefix_contended");
    }
    #[test]
    fn clone_owned_by_reader_outlives_parent() {
        let o = StderrObservation::new();
        let retained = o.clone();
        drop(o);
        retained.record(b"owned");
        assert_eq!(retained.snapshot().bytes, b"owned");
    }
    #[test]
    fn observing_preserves_original_read_cap_and_output() {
        let o = StderrObservation::new();
        let r = ObservedReader {
            reader: io::Cursor::new(b"abcdef"),
            observation: o.clone(),
        };
        let mut bytes = Vec::new();
        r.take(4).read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abcd");
        assert_eq!(o.snapshot().bytes, bytes);
    }
    #[test]
    fn read_failure_remains_original_failure() {
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "original"))
            }
        }
        let o = StderrObservation::new();
        let mut r = ObservedReader {
            reader: Failed,
            observation: o.clone(),
        };
        assert_eq!(
            r.read(&mut [0; 8]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(o.snapshot().bytes.is_empty());
    }
    #[test]
    fn poisoned_capture_is_unavailable_without_replacing_read_error() {
        let observation = StderrObservation::new();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observation.inner.bytes.lock().unwrap();
            panic!("poison fixture");
        }));
        observation.record(b"ignored");
        assert_eq!(observation.snapshot().state, "unavailable_poisoned");
        let mut reader = ObservedReader {
            reader: io::Cursor::new(b"original"),
            observation,
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"original");
    }
}
