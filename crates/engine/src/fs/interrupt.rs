//! The cancellation flag a file system call polls.

use std::sync::atomic::{AtomicBool, Ordering};

use socketsweep_wire::CANCEL_CHECK_ENTRIES;

use super::FsError;

/// A cancellation flag an [`Fs`] call polls at least every
/// [`CANCEL_CHECK_ENTRIES`] entries.
#[derive(Debug, Clone, Copy)]
pub struct Interrupt<'a> {
    flag: Option<&'a AtomicBool>,
}

impl<'a> Interrupt<'a> {
    /// Never raised.
    pub const NEVER: Interrupt<'static> = Interrupt { flag: None };

    pub fn new(flag: &'a AtomicBool) -> Self {
        Interrupt { flag: Some(flag) }
    }

    pub fn is_set(&self) -> bool {
        self.flag.is_some_and(|f| f.load(Ordering::Relaxed))
    }

    /// Call with the running entry count `i`: checks the flag every
    /// [`CANCEL_CHECK_ENTRIES`] entries and returns
    /// [`FsError::Interrupted`] once it is raised.
    pub fn poll(&self, i: usize) -> Result<(), FsError> {
        if i.is_multiple_of(CANCEL_CHECK_ENTRIES) && self.is_set() {
            Err(FsError::Interrupted)
        } else {
            Ok(())
        }
    }
}
