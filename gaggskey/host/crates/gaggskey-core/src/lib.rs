//! Long-lived connection to the keyboard.
//!
//! [`Manager::spawn`] owns a worker thread that discovers keyboards (USB and
//! BLE), connects to the selected one (or the first found), keeps a
//! [`Snapshot`] of identity + state, forwards every change to the listener,
//! executes queued commands, and reconnects after any failure.
//!
//! Several keyboards may be attached; [`Manager::select`] switches between
//! them by name. Only one is connected at a time.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gaggskey_link::{Client, Device};
use gaggskey_proto::{Command, Hello, State};
use serde::Serialize;

const RETRY: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);
/// How often a connected session re-enumerates USB for newly plugged keyboards.
const USB_RESCAN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub connected: bool,
    /// Keyboards found at the last scan (one entry per name, USB preferred).
    pub devices: Vec<Device>,
    /// Name of the keyboard this session targets (user choice or first found).
    pub selected: Option<String>,
    /// "usb …" / "ble …"
    pub via: Option<String>,
    pub hello: Option<Hello>,
    pub layer_names: Vec<String>,
    pub state: Option<State>,
    /// Keymap with labels; filled shortly after connecting.
    pub keymap: Option<gaggskey_proto::keymap::Keymap>,
    /// Last connection error while disconnected.
    pub error: Option<String>,
}

type Publish<'a> = &'a dyn Fn(&dyn Fn(&mut Snapshot));
type Reply = Sender<Result<Vec<u8>, String>>;

pub struct Manager {
    cmds: Sender<(Command, Reply)>,
    snap: Arc<Mutex<Snapshot>>,
    want: Arc<Mutex<Option<String>>>,
}

impl Manager {
    /// `on_change` runs on the worker thread after every snapshot change.
    pub fn spawn(on_change: impl Fn(&Snapshot) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let snap = Arc::new(Mutex::new(Snapshot::default()));
        let want = Arc::new(Mutex::new(None));
        let (s, w) = (snap.clone(), want.clone());
        thread::Builder::new()
            .name("gaggskey-core".into())
            .spawn(move || worker(rx, s, w, on_change))
            .expect("spawn worker");
        Self { cmds: tx, snap, want }
    }

    /// Switch to the keyboard with this name. The current session ends and
    /// the worker reconnects to it (within ~50 ms when connected).
    pub fn select(&self, name: String) {
        *self.want.lock().unwrap() = Some(name);
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

fn worker(
    rx: Receiver<(Command, Reply)>,
    snap: Arc<Mutex<Snapshot>>,
    want: Arc<Mutex<Option<String>>>,
    on_change: impl Fn(&Snapshot),
) {
    let publish = |f: &dyn Fn(&mut Snapshot)| {
        let mut s = snap.lock().unwrap();
        f(&mut s);
        on_change(&s);
    };

    loop {
        match session(&rx, &want, &publish) {
            Ok(()) => return, // all senders dropped: app is quitting
            Err(e) => {
                if e == SWITCH {
                    // Drop the old keyboard's data, then reconnect right away.
                    let next = want.lock().unwrap().clone();
                    publish(&|s| {
                        *s = Snapshot {
                            devices: s.devices.clone(),
                            selected: next.clone(),
                            ..Default::default()
                        };
                    });
                    continue;
                }
                let changed = {
                    let s = snap.lock().unwrap();
                    s.connected || s.error.as_deref() != Some(&e)
                };
                if changed {
                    publish(&|s| {
                        *s = Snapshot {
                            error: Some(e.clone()),
                            devices: s.devices.clone(),
                            selected: s.selected.clone(),
                            ..Default::default()
                        };
                    });
                }
            }
        }
        // Fail queued commands fast while disconnected, then retry.
        let until = Instant::now() + RETRY;
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            if want_changed(&want, &snap) {
                break;
            }
            let left = left.min(POLL * 4);
            match rx.recv_timeout(left) {
                Ok((_, reply)) => {
                    let _ = reply.send(Err("keyboard not connected".into()));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Session-ending reason meaning "user picked another keyboard".
const SWITCH: &str = "switching keyboard";

fn want_changed(want: &Mutex<Option<String>>, snap: &Mutex<Snapshot>) -> bool {
    let w = want.lock().unwrap().clone();
    w.is_some() && w != snap.lock().unwrap().selected
}

/// Pick the wanted keyboard by name, else the first found.
fn pick(devices: &[Device], want: Option<&str>) -> Option<Device> {
    want.and_then(|w| devices.iter().find(|d| d.name == w))
        .or_else(|| devices.first())
        .cloned()
}

/// Keep BLE entries from the last full scan, refresh USB ones.
fn merge_usb(prev: &[Device], usb: Vec<Device>) -> Vec<Device> {
    let mut out = usb;
    for d in prev.iter().filter(|d| d.via == gaggskey_link::Via::Ble) {
        if !out.iter().any(|o| o.name == d.name) {
            out.push(d.clone());
        }
    }
    out
}

/// One connected session. Ok(()) = shut down, Err = reconnect.
fn session(rx: &Receiver<(Command, Reply)>, want: &Mutex<Option<String>>, publish: Publish) -> Result<(), String> {
    let devices = gaggskey_link::list_devices();
    let wanted = want.lock().unwrap().clone();
    let Some(dev) = pick(&devices, wanted.as_deref()) else {
        publish(&|s| {
            s.devices = devices.clone();
            s.selected = wanted.clone();
        });
        return Err("no keyboard with GaggsKey found (USB or BLE)".into());
    };
    // The selection follows what we actually connect to.
    *want.lock().unwrap() = Some(dev.name.clone());
    publish(&|s| {
        s.devices = devices.clone();
        s.selected = Some(dev.name.clone());
    });
    let mut c = Client::open(&dev).map_err(|e| e.to_string())?;
    let hello = c.hello().map_err(|e| e.to_string())?;
    let names = c.layer_names(hello.layers);
    let state = c.state().map_err(|e| e.to_string())?;
    let via = c.describe();
    publish(&|s| {
        *s = Snapshot {
            connected: true,
            devices: devices.clone(),
            selected: Some(dev.name.clone()),
            via: Some(via.clone()),
            hello: Some(hello.clone()),
            layer_names: names.clone(),
            state: Some(state.clone()),
            keymap: None,
            error: None,
        };
    });
    match c.keymap(&names) {
        Ok(km) => publish(&|s| s.keymap = km.clone()),
        Err(gaggskey_link::Error::Io(e)) => return Err(e),
        Err(e) => eprintln!("keymap fetch failed: {e}"),
    }

    let mut known = devices.clone();
    let mut rescan = Instant::now() + USB_RESCAN;
    loop {
        if want.lock().unwrap().as_deref() != Some(dev.name.as_str()) {
            return Err(SWITCH.into());
        }
        if Instant::now() >= rescan {
            rescan = Instant::now() + USB_RESCAN;
            let merged = merge_usb(&known, gaggskey_link::usb::UsbTransport::list());
            if merged != known {
                known = merged;
                publish(&|s| s.devices = known.clone());
            }
        }
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
