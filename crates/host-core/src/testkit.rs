//! A scripted fake daemon that speaks wire v4, for tests.
//!
//! [`FakeDaemon::serve`] runs one connection: it checks the `Hello` with the
//! real [`check_hello`], answers as configured, answers `Ping`s itself, and
//! hands every other frame to the test's handler with a [`Responder`] for
//! replying (from any thread). Plug it into a `FakeTransport` with
//! [`FakeDaemon::install`], which also plays the phone's shell: it reads the
//! token and socket name from the launch command, as the real daemon does.

use std::io::Write;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use socketsweep_transport::fake::{FakeTransport, PipeEnd};
use socketsweep_transport::{Duplex, Output};
use socketsweep_wire::{
    check_hello, encode_frame, read_frame, read_frame_body, token_from_hex, Caps, ClientFrame,
    ClientMsg, HelloCheck, HelloReply, Limits, ServerFrame, ServerMsg, Token,
};

use crate::daemon::parse_launch_command;
use crate::lock;

/// How the fake daemon answers the `Hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Greeting {
    Accept,
    /// Answer `Refused` whatever the token.
    Refuse,
    /// Answer `WrongVersion` with this version.
    WrongVersion(u16),
    /// Accept, then never answer `Ping`s (and send nothing unprompted).
    AcceptThenSilent,
}

/// Sends frames to the host. Clone it into a thread to stream.
#[derive(Clone)]
pub struct Responder {
    out: Arc<Mutex<Box<dyn Duplex>>>,
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

/// A scripted daemon. Cheap to clone.
#[derive(Clone)]
pub struct FakeDaemon {
    pub greeting: Greeting,
    handler: Handler,
    /// Every frame after the `Hello`, `Ping`s included.
    pub received: Arc<Mutex<Vec<ClientFrame>>>,
}

impl FakeDaemon {
    pub fn new(handler: impl Fn(&ClientFrame, &Responder) + Send + Sync + 'static) -> Self {
        FakeDaemon {
            greeting: Greeting::Accept,
            handler: Arc::new(handler),
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_greeting(mut self, greeting: Greeting) -> Self {
        self.greeting = greeting;
        self
    }

    /// Every message received so far (frame ids dropped), `Ping`s excluded.
    pub fn messages(&self) -> Vec<ClientMsg> {
        lock(&self.received)
            .iter()
            .filter(|f| !matches!(f.msg, ClientMsg::Ping { .. }))
            .map(|f| f.msg.clone())
            .collect()
    }

    /// Serve one connection until the host hangs up or sends `Shutdown`.
    pub fn serve(&self, end: PipeEnd, token: Token) {
        let out: Box<dyn Duplex> = end.try_clone().unwrap();
        let responder = Responder {
            out: Arc::new(Mutex::new(out)),
        };
        let mut input = end;
        let Ok(Some(body)) = read_frame_body(&mut input) else {
            return;
        };
        let check = match self.greeting {
            Greeting::Refuse => HelloCheck::Refused,
            Greeting::WrongVersion(v) => {
                let bytes = encode_frame(&HelloReply::WrongVersion { version: v }).unwrap();
                let _ = lock(&responder.out).write_all(&bytes);
                return;
            }
            _ => check_hello(&body, &token),
        };
        let hello = match check {
            HelloCheck::Accepted(hello) => hello,
            rejected => {
                let reply = rejected.rejection().unwrap();
                let _ = lock(&responder.out).write_all(&encode_frame(&reply).unwrap());
                return;
            }
        };
        let welcome = hello.welcome(Caps::all(), "fake".into(), 4, Limits::default());
        let bytes = encode_frame(&HelloReply::Welcome(welcome)).unwrap();
        if lock(&responder.out).write_all(&bytes).is_err() {
            return;
        }

        while let Ok(Some(frame)) = read_frame::<_, ClientFrame>(&mut input) {
            lock(&self.received).push(frame.clone());
            match frame.msg {
                ClientMsg::Ping { nonce } => {
                    if self.greeting != Greeting::AcceptThenSilent {
                        responder.send(frame.id, ServerMsg::Pong { nonce });
                    }
                }
                ClientMsg::Shutdown => return,
                _ => (self.handler)(&frame, &responder),
            }
        }
    }

    /// Play the phone's shell and daemon on `serial` of `transport`: answer
    /// the commands the host runs, and when it launches the daemon, serve
    /// tunnels to the socket it named with the token it passed.
    pub fn install(&self, transport: &Arc<FakeTransport>, serial: &str) {
        let weak: Weak<FakeTransport> = Arc::downgrade(transport);
        let daemon = self.clone();
        let serial_owned = serial.to_string();
        transport.on_shell(serial, move |command| {
            if let Some(launch) = parse_launch_command(command) {
                let t = weak.upgrade()?;
                let token = token_from_hex(&launch.token_hex)?;
                let d = daemon.clone();
                t.on_tunnel(&serial_owned, &launch.socket, move |end| {
                    d.serve(end, token)
                });
            }
            Some(Output {
                exit: Some(0),
                ..Output::default()
            })
        });
    }
}

/// Wait up to `timeout` for `cond`.
pub fn wait_for(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}
