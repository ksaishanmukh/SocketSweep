//! How the fake daemon sends frames back to the host.

use std::io::Write;
use std::sync::{Arc, Mutex};

use socketsweep_transport::Duplex;
use socketsweep_wire::{encode_frame, ClientFrame, ServerFrame, ServerMsg};

use crate::lock;

/// Sends frames to the host. Clone it into a thread to stream.
#[derive(Clone)]
pub struct Responder {
    pub(super) out: Arc<Mutex<Box<dyn Duplex>>>,
}

impl Responder {
    /// Send one frame; false once the host has gone.
    pub fn send(&self, id: u32, msg: ServerMsg) -> bool {
        let bytes = encode_frame(&ServerFrame { id, msg }).unwrap();
        let mut out = lock(&self.out);
        out.write_all(&bytes).and_then(|()| out.flush()).is_ok()
    }
}

pub type Handler = Arc<dyn Fn(&ClientFrame, &Responder) + Send + Sync>;
