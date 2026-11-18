//! The two requests that are more than one message: delete and copy.

use std::sync::mpsc::Receiver;

use socketsweep_wire::{ClientMsg, DirId, ReqId, ServerMsg, Totals};

use super::{Client, ClientError};

impl Client {
    /// Delete `path` and wait for the phone to confirm, reporting
    /// `DeleteProgress` to `progress`. Returns the items removed and their
    /// totals. Waits as long as the delete takes: only a dead connection ends
    /// the wait early.
    pub fn delete(
        &self,
        path: Vec<u8>,
        hint: Option<(ReqId, DirId)>,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ClientError> {
        let (_, rx) = self.request(ClientMsg::Delete { path, hint })?;
        loop {
            match rx.recv() {
                Ok(ServerMsg::DeleteProgress { items }) => progress(items),
                Ok(ServerMsg::Deleted { items, totals }) => return Ok((items, totals)),
                Ok(ServerMsg::Failed {
                    code,
                    errno,
                    message,
                }) => {
                    return Err(ClientError::Failed {
                        code,
                        errno,
                        message,
                    })
                }
                Ok(other) => {
                    return Err(ClientError::Protocol(format!(
                        "unexpected reply to delete: {other:?}"
                    )))
                }
                Err(_) => return Err(self.closed_error()),
            }
        }
    }

    /// Ask for the copy manifest of `path`. The channel carries
    /// [`ServerMsg::CopyManifest`] parts until the `last` one, or a `Failed`.
    pub fn prepare_copy(&self, path: Vec<u8>) -> Result<(ReqId, Receiver<ServerMsg>), ClientError> {
        self.request(ClientMsg::PrepareCopy { path })
    }
}
