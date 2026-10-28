//! Turning every failure into the GUI's [`CommandError`].

use socketsweep_gui_contract::{CommandError, ErrorCode};
use socketsweep_transport::TransportError;
use socketsweep_wire::ErrCode;

use crate::client::ClientError;
use crate::copy::CopyError;
use crate::daemon::LaunchError;
use crate::tree::TreeError;

pub(super) type CmdResult<T> = Result<T, CommandError>;

pub(super) fn err(code: ErrorCode, message: impl Into<String>) -> CommandError {
    CommandError::new(code, message)
}

pub(super) fn tree_err(e: TreeError) -> CommandError {
    let code = match e {
        TreeError::Stale => ErrorCode::Stale,
        TreeError::Removed => ErrorCode::Removed,
        TreeError::UnknownNode(_) | TreeError::RootNotDeletable => ErrorCode::Invalid,
        TreeError::Protocol(_) => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

pub(super) fn transport_err(e: TransportError) -> CommandError {
    err(ErrorCode::Device, e.to_string())
}

pub(super) fn client_err(e: ClientError) -> CommandError {
    let code = match &e {
        ClientError::Failed {
            code: ErrCode::Cancelled,
            ..
        } => ErrorCode::Cancelled,
        ClientError::Failed {
            code: ErrCode::OutsideRoot | ErrCode::ProtectedRoot | ErrCode::BadRequest,
            ..
        } => ErrorCode::Invalid,
        ClientError::Closed(_) => ErrorCode::Device,
        _ => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

pub(super) fn launch_err(e: LaunchError) -> CommandError {
    let code = match &e {
        LaunchError::BadRoot(_) => ErrorCode::Invalid,
        LaunchError::Unsupported(_) | LaunchError::Missing(_) | LaunchError::Transport(_) => {
            ErrorCode::Device
        }
        LaunchError::Handshake(_) | LaunchError::NeverAnswered { .. } => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

pub(super) fn copy_err(e: CopyError) -> CommandError {
    let code = match &e {
        CopyError::Cancelled => ErrorCode::Cancelled,
        CopyError::Local { .. } => ErrorCode::Io,
        CopyError::Transport(_) => ErrorCode::Device,
    };
    err(code, e.to_string())
}
