//! The session and batch commands: connect, scan, delete and copy.

use socketsweep_gui_contract::{Connected, CopyResult, DeletedMany, Device, NodeRef, ScanStarted};

use super::errors::CmdResult;
use super::{connect, copy, delete, scan, App};
use crate::lock;

impl App {
    pub fn list_devices(&self) -> CmdResult<Vec<Device>> {
        connect::list_devices(&self.inner)
    }

    pub fn connect(&self, serial: Option<String>, root: Option<String>) -> CmdResult<Connected> {
        connect::connect(&self.inner, serial, root)
    }

    pub fn disconnect(&self) -> CmdResult<()> {
        self.inner.end_session();
        Ok(())
    }

    pub fn start_scan(&self, root: Option<String>) -> CmdResult<ScanStarted> {
        scan::start(&self.inner, root)
    }

    pub fn cancel_scan(&self) -> CmdResult<()> {
        if let Some(scan) = lock(&self.inner.scan).clone() {
            scan.cancel();
        }
        Ok(())
    }

    pub fn delete_many(&self, nodes: Vec<NodeRef>) -> CmdResult<DeletedMany> {
        delete::delete_many(&self.inner, nodes)
    }

    pub fn copy_many(
        &self,
        nodes: Vec<NodeRef>,
        destination: Option<String>,
    ) -> CmdResult<CopyResult> {
        copy::copy_many(&self.inner, nodes, destination)
    }

    pub fn cancel_copy(&self) -> CmdResult<()> {
        copy::cancel_copy(&self.inner)
    }
}
