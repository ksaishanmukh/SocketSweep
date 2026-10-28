//! Why an action failed, and the `errno` behind an I/O error.

use std::io;
use std::path::Path;

use socketsweep_engine::errno;
use socketsweep_wire::{ErrCode, ServerMsg};

use super::guard::GuardError;

/// Why an action failed: the payload of `ServerMsg::Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError {
    pub code: ErrCode,
    pub errno: Option<i32>,
    pub message: String,
}

impl ActionError {
    pub fn new(code: ErrCode, message: impl Into<String>) -> Self {
        ActionError {
            code,
            errno: None,
            message: message.into(),
        }
    }

    pub fn cancelled() -> Self {
        ActionError::new(ErrCode::Cancelled, "cancelled")
    }

    /// From a failed file system call on `path`.
    pub fn io(e: &io::Error, what: &str, path: &Path) -> Self {
        let errno = errno_of(e);
        let code = match errno {
            errno::ENOENT => ErrCode::NotFound,
            errno::EACCES | 1 /* EPERM */ => ErrCode::PermissionDenied,
            _ => ErrCode::Io,
        };
        ActionError {
            code,
            errno: Some(errno),
            message: format!("cannot {what} {}: {e}", path.display()),
        }
    }

    /// From an engine `errno` (a scan root that could not be opened).
    pub fn errno(errno: i32, message: impl Into<String>) -> Self {
        let code = match errno {
            errno::ENOENT => ErrCode::NotFound,
            errno::EACCES | 1 => ErrCode::PermissionDenied,
            _ => ErrCode::Io,
        };
        ActionError {
            code,
            errno: Some(errno),
            message: message.into(),
        }
    }

    pub fn into_msg(self) -> ServerMsg {
        ServerMsg::Failed {
            code: self.code,
            errno: self.errno,
            message: self.message,
        }
    }
}

impl From<GuardError> for ActionError {
    fn from(e: GuardError) -> Self {
        let code = match &e {
            GuardError::NotFound(_) => ErrCode::NotFound,
            GuardError::OutsideRoot { .. } => ErrCode::OutsideRoot,
            GuardError::IsRoot(_) => ErrCode::ProtectedRoot,
            GuardError::NoFinalName(_) => ErrCode::BadRequest,
        };
        let errno = matches!(e, GuardError::NotFound(_)).then_some(errno::ENOENT);
        ActionError {
            code,
            errno,
            message: e.to_string(),
        }
    }
}

/// The `errno` behind an I/O error: the raw code on Unix, a close equivalent
/// elsewhere.
pub fn errno_of(e: &io::Error) -> i32 {
    #[cfg(unix)]
    if let Some(n) = e.raw_os_error() {
        return n;
    }
    match e.kind() {
        io::ErrorKind::NotFound => errno::ENOENT,
        io::ErrorKind::PermissionDenied => errno::EACCES,
        _ => errno::EIO,
    }
}
