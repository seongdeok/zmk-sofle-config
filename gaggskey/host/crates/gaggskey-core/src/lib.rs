//! Long-lived connection to the keyboard.
//!
//! [`Manager::spawn`] owns a worker thread that connects (USB first, then BLE),
//! keeps a [`Snapshot`] of identity + state, forwards every change to the
//! listener, executes queued commands, and reconnects after any failure.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use gaggskey_link::Client;
use gaggskey_proto::{Command, Hello, State};
use serde::Serialize;

const RETRY: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub connected: bool,
    /// "usb …" / "ble …"
    pub via: Option<String>,
    pub hello: Option<Hello>,
    pub layer_names: Vec<String>,
    pub state: Option<State>,
    /// Last connection error while disconnected.
    pub error: Option<String>,
}

type Publish<'a> = &'a dyn Fn(&dyn Fn(&mut Snapshot));
type Reply = Sender<Result<Vec<u8>, String>>;

pub struct Manager {
    cmds: Sender<(Command, Reply)>,
    snap: Arc<Mutex<Snapshot>>,
}

impl Manager {
    /// `on_change` runs on the worker thread after every snapshot change.
    pub fn spawn(on_change: impl Fn(&Snapshot) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let snap = Arc::new(Mutex::new(Snapshot::default()));
        let s = snap.clone();
        thread::Builder::new()
            .name("gaggskey-core".into())
            .spawn(move || worker(rx, s, on_change))
            .expect("spawn worker");
        Self { cmds: tx, snap }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().unwrap().clone()
    }

    /// Queue a command and wait for its result (ACK data).
    pub fn command(&self, cmd: Command, timeout: Duration) -> Result<Vec<u8>, String> {
        let (tx, rx) = mpsc::channel();
        self.cmds.send((cmd, tx)).map_err(|_| "worker stopped".to_string())?;
        rx.recv_timeout(timeout).map_err(|_| "keyboard did not answer".to_string())?
    }
}

fn worker(rx: Receiver<(Command, Reply)>, snap: Arc<Mutex<Snapshot>>, on_change: impl Fn(&Snapshot)) {
    let publish = |f: &dyn Fn(&mut Snapshot)| {
        let mut s = snap.lock().unwrap();
        f(&mut s);
        on_change(&s);
    };

    loop {
        match session(&rx, &publish) {
            Ok(()) => return, // all senders dropped: app is quitting
            Err(e) => {
                let changed = {
                    let s = snap.lock().unwrap();
                    s.connected || s.error.as_deref() != Some(&e)
                };
                if changed {
                    publish(&|s| {
                        *s = Snapshot { error: Some(e.clone()), ..Default::default() };
                    });
                }
            }
        }
        // Fail queued commands fast while disconnected, then retry.
        let until = std::time::Instant::now() + RETRY;
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok((_, reply)) => {
                    let _ = reply.send(Err("keyboard not connected".into()));
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// One connected session. Ok(()) = shut down, Err = reconnect.
fn session(rx: &Receiver<(Command, Reply)>, publish: Publish) -> Result<(), String> {
    let mut c = Client::open_any().map_err(|e| e.to_string())?;
    let hello = c.hello().map_err(|e| e.to_string())?;
    let names = c.layer_names(hello.layers);
    let state = c.state().map_err(|e| e.to_string())?;
    let via = c.describe();
    publish(&|s| {
        *s = Snapshot {
            connected: true,
            via: Some(via.clone()),
            hello: Some(hello.clone()),
            layer_names: names.clone(),
            state: Some(state.clone()),
            error: None,
        };
    });

    loop {
        loop {
            match rx.try_recv() {
                Ok((cmd, reply)) => {
                    let r = c.command(cmd);
                    let fatal = matches!(r, Err(gaggskey_link::Error::Io(_)));
                    let _ = reply.send(r.map_err(|e| e.to_string()));
                    if fatal {
                        return Err("connection lost".into());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        match c.next_push(Some(POLL)) {
            Ok(Some(st)) => publish(&|s| s.state = Some(st.clone())),
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}
