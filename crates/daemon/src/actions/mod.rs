//! `actions`: everything that touches files by path rather than by scan —
//! resolving request paths through the guard, delete, and the copy manifest.
//!
//! [`Actions`] is the contract the session uses. [`LocalActions`] is the real
//! one, over `std::fs` and confined to the daemon's `--root`; it is portable,
//! so the session's tests run it on any host. `socketsweep-sim` implements it
//! over `SimFs` for `FakeDaemon`.
//!
//! # Safety rules (all enforced in [`guard`] and [`delete_tree`])
//!
//! - A delete or copy names an entry: only its parent is canonicalised, and the
//!   final component is never followed. Deleting a symbolic link removes the
//!   link; nothing below a link is ever touched.
//! - Everything resolves to somewhere strictly beneath the root (a scan or a
//!   copy may also name the root itself); the root is never deleted.
//! - Within a deleted directory, entries are judged by `lstat`, so a link to a
//!   directory is unlinked, not descended into.

//!
//! # Layout
//!
//! `error` holds [`ActionError`]; `paths` and `metadata` convert paths and
//! file metadata; `delete` and `manifest` walk a tree; `local` is
//! [`LocalActions`].

pub mod guard;

mod delete;
mod error;
mod local;
mod manifest;
mod metadata;
mod paths;

#[cfg(test)]
mod tests;

use std::sync::atomic::AtomicBool;

#[cfg(doc)]
use socketsweep_wire::ErrCode;
use socketsweep_wire::{CopyItem, Totals};

pub use delete::delete_tree;
pub use error::{errno_of, ActionError};
pub use local::LocalActions;
pub use manifest::manifest_tree;
pub use paths::{bytes_to_path, path_to_bytes};

/// What the session needs to act on paths. Paths are bytes; the `resolve_*`
/// results are what the other methods take, and what is reported back.
pub trait Actions: Send + Sync {
    /// `StartScan`: the root as the engine should walk it. Follows every
    /// link (`/sdcard` is a chain of them); must be the allowed root or
    /// beneath it.
    fn resolve_scan_root(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// `Delete`: an entry strictly beneath the root, final component not
    /// followed.
    fn resolve_delete(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// `PrepareCopy`: as [`Actions::resolve_delete`], but the root itself is
    /// allowed.
    fn resolve_copy(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// Whether `scan_path` (built by a scan from its root and listed names)
    /// and `resolved` (from a `resolve_*`) name the same entry. Byte
    /// equality, except where the platform spells separators two ways.
    fn same_entry(&self, scan_path: &[u8], resolved: &[u8]) -> bool {
        scan_path == resolved
    }

    /// Remove `resolved` and everything below it, never following a link.
    /// Calls `progress` with the running item count, and stops with
    /// [`ErrCode::Cancelled`] once `cancel` is set. Returns the items removed
    /// and their totals, the removed entry itself included.
    fn delete(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ActionError>;

    /// List `resolved` and everything below it, each directory before its
    /// contents, the root first with an empty `rel`. Links are listed, never
    /// followed.
    fn manifest(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        emit: &mut dyn FnMut(CopyItem),
    ) -> Result<(), ActionError>;
}
