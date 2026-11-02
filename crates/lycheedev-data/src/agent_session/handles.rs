//! Session-local ownership, never a CAP alias or a process-global cache.
use crate::fault;
use lycheedev_core::Result;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleReceipt {
    pub handle: String,
    pub rows: usize,
    pub retained_bytes: usize,
    pub expires_after_ms: u64,
}
struct Entry<T> {
    owner: T,
    expires: Instant,
    bytes: usize,
}
/// The stored object must own every input, output and reservation it borrows.
/// Removing an entry drops that ownership; a borrowed page cannot outlive it.
pub(super) struct Handles<T> {
    entries: BTreeMap<String, Entry<T>>,
    epoch: String,
    next: u64,
    bytes: usize,
    max_count: usize,
    max_bytes: usize,
    ttl: Duration,
}
impl<T> Handles<T> {
    pub(super) fn new(
        epoch: String,
        max_count: usize,
        max_bytes: usize,
        ttl: Duration,
    ) -> Result<Self> {
        if epoch.is_empty() || max_count == 0 || max_bytes == 0 || ttl.is_zero() {
            return Err(fault(
                "data.session.invalid_limits",
                2,
                "Session handle limits must be positive",
            ));
        }
        Ok(Self {
            entries: BTreeMap::new(),
            epoch,
            next: 0,
            bytes: 0,
            max_count,
            max_bytes,
            ttl,
        })
    }
    pub(super) fn expire(&mut self, now: Instant) {
        self.entries.retain(|_, entry| {
            if entry.expires <= now {
                self.bytes -= entry.bytes;
                false
            } else {
                true
            }
        });
    }
    pub(super) fn insert(
        &mut self,
        owner: T,
        rows: usize,
        bytes: usize,
        now: Instant,
    ) -> Result<HandleReceipt> {
        self.expire(now);
        let total = self.bytes.checked_add(bytes).ok_or_else(capacity)?;
        if self.entries.len() >= self.max_count || total > self.max_bytes {
            return Err(capacity());
        }
        let next = self.next.checked_add(1).ok_or_else(capacity)?;
        let expires = now.checked_add(self.ttl).ok_or_else(capacity)?;
        let handle = format!("RES-{}-{next}", self.epoch);
        self.entries.insert(
            handle.clone(),
            Entry {
                owner,
                expires,
                bytes,
            },
        );
        self.next = next;
        self.bytes = total;
        Ok(HandleReceipt {
            handle,
            rows,
            retained_bytes: bytes,
            expires_after_ms: self.ttl.as_millis().min(u128::from(u64::MAX)) as u64,
        })
    }
    pub(super) fn get(&mut self, handle: &str, now: Instant) -> Result<&T> {
        self.expire(now);
        self.entries
            .get(handle)
            .map(|entry| &entry.owner)
            .ok_or_else(missing)
    }
    pub(super) fn release(&mut self, handle: &str, now: Instant) -> Result<()> {
        self.expire(now);
        let entry = self.entries.remove(handle).ok_or_else(missing)?;
        self.bytes -= entry.bytes;
        drop(entry);
        Ok(())
    }
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}
fn capacity() -> lycheedev_core::ToolkitError {
    fault(
        "data.session.handle_capacity",
        3,
        "Release results before retaining another handle",
    )
}
fn missing() -> lycheedev_core::ToolkitError {
    fault(
        "data.session.result_missing",
        4,
        "Result handle is expired, released, or belongs to another session",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Owner(Arc<AtomicUsize>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn owner_survives_pages_and_drops_on_release_expiry_and_session_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();
        let mut handles = Handles::new("epoch".into(), 2, 100, Duration::from_secs(1)).unwrap();
        let first = handles.insert(Owner(drops.clone()), 10, 50, now).unwrap();
        handles.get(&first.handle, now).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        handles.release(&first.handle, now).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let second = handles.insert(Owner(drops.clone()), 1, 50, now).unwrap();
        assert_ne!(first.handle, second.handle);
        assert!(
            handles
                .get(&second.handle, now + Duration::from_secs(1))
                .is_err()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        handles.insert(Owner(drops.clone()), 1, 50, now).unwrap();
        drop(handles);
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn capacity_rejects_without_evicting_live_results_and_drops_rejected_owner() {
        let drops = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();
        let mut handles = Handles::new("epoch".into(), 1, 50, Duration::from_secs(1)).unwrap();
        let first = handles.insert(Owner(drops.clone()), 1, 50, now).unwrap();
        assert!(handles.insert(Owner(drops.clone()), 1, 1, now).is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        handles.get(&first.handle, now).unwrap();
        assert!(handles.get("RES-other-1", now).is_err());
    }
}
