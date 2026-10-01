//! Synchronisation helpers shared by the scanner and its callers.
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock a mutex, recovering from poisoning (a panicked worker must not take
/// the whole UI down with it).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
