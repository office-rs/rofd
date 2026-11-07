//! Process-wide Converter handle table for the JNI boundary.
//!
//! D15 contract: one instance is touched by one thread at a time. JNI
//! calls TAKE the instance out of the table for the duration of the call
//! and PUT it back after — a second concurrent call on the same handle
//! observes `InUse` instead of racing. Ids are monotonically increasing
//! u64s and never reused, so a stale Java-side handle (close already
//! happened, PhantomReference cleanup pending) can only ever yield
//! `Stale` — no ABA on id reuse.
//!
//! rofd note (D15, revised 2026-11-07): the kernel `Converter` is `Send`
//! today (only Arc'd registry snapshots inside; pinned by a kernel test),
//! which is what makes this Mutex-guarded table sound. The lease remains
//! load-bearing anyway: it prevents two JVM threads from using the SAME
//! handle concurrently, independent of the kernel's current `Send` status.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeError {
    /// The handle exists but another call currently owns the instance.
    InUse,
    /// No such handle (never existed, or already closed).
    Stale,
}

struct Slots {
    next_id: u64,
    map: HashMap<u64, Option<rofd_convert::Converter>>,
}

pub struct HandleTable {
    slots: Mutex<Slots>,
}

impl HandleTable {
    /// Empty table; ids start at 1 (0 is the JNI-side null handle).
    pub fn new() -> HandleTable {
        HandleTable {
            slots: Mutex::new(Slots {
                next_id: 1,
                map: HashMap::new(),
            }),
        }
    }

    /// Register an instance, returning its fresh (never-reused) id.
    pub fn insert(&self, c: rofd_convert::Converter) -> u64 {
        let mut s = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = s.next_id;
        s.next_id += 1;
        s.map.insert(id, Some(c));
        id
    }

    /// Lease the instance for the duration of one call. `Err(InUse)` if
    /// another call holds it; `Err(Stale)` if the id is unknown/closed.
    pub fn take(&self, id: u64) -> Result<rofd_convert::Converter, TakeError> {
        let mut s = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match s.map.get_mut(&id) {
            Some(slot) => slot.take().ok_or(TakeError::InUse),
            None => Err(TakeError::Stale),
        }
    }

    /// Return a leased instance. If `close` removed the slot while the
    /// instance was leased, this is a no-op (the entry must not resurrect
    /// under a stale id) and the instance drops here.
    pub fn put(&self, id: u64, c: rofd_convert::Converter) {
        let mut s = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = s.map.get_mut(&id) {
            *slot = Some(c);
        }
    }

    /// Remove and drop an instance. Idempotent per the Java close()
    /// contract: closing an already-closed id reports `Stale` (harmless
    /// for the PhantomReference last-resort path, which races explicit
    /// closes by design).
    pub fn close(&self, id: u64) -> Result<(), TakeError> {
        let mut s = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match s.map.remove(&id) {
            // Whether or not the instance was leased, the map entry is
            // gone; a leased converter's later put() now no-ops.
            Some(_) => Ok(()),
            None => Err(TakeError::Stale),
        }
    }
}

impl Default for HandleTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sync_for_static_mounting() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<HandleTable>();
    }
}
