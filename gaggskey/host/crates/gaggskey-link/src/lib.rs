//! Transports and a blocking request/response client.
//!
//! A [`Transport`] moves raw 32-byte frames; [`Client`] adds sequence numbers,
//! reply matching and buffering of unsolicited state pushes.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gaggskey_proto::{self as proto, Ack, Command, Frame, Hello, Message, MsgType, State};

pub mod ble;
pub mod usb;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("keyboard not found ({0})")]
    NotFound(&'static str),
    #[error("transport: {0}")]
    Io(String),
    #[error("timed out waiting for reply")]
    Timeout,
    #[error("protocol: {0}")]
    Proto(#[from] proto::ProtoError),
    #[error("device rejected frame (status {0})")]
    Device(i8),
    #[error("command failed: {}", errno_str(*.0))]
    Command(i8),
    #[error("unexpected reply {0:?}")]
    Unexpected(MsgType),
}

fn errno_str(e: i8) -> String {
    let name = match -(e as i32) {
        1 => "EPERM",
        2 => "ENOENT",
        5 => "EIO",
        13 => "EACCES",
        16 => "EBUSY",
        22 => "EINVAL",
        95 | 134 => "ENOTSUP",
        _ => "",
    };
    format!("{name} ({e})")
}

pub type Result<T> = std::result::Result<T, Error>;

/// Moves whole frames. Implementations must be usable from one thread at a time.
pub trait Transport: Send {
    fn send(&mut self, frame: &Frame) -> Result<()>;
    /// Wait up to `timeout` (None = forever) for one frame.
    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<Vec<u8>>>;
    fn describe(&self) -> String;
}

pub struct Client {
    t: Box<dyn Transport>,
    seq: u8,
    pending_pushes: VecDeque<State>,
    pub timeout: Duration,
}

impl Client {
    pub fn new(t: Box<dyn Transport>) -> Self {
        Self { t, seq: 0, pending_pushes: VecDeque::new(), timeout: Duration::from_secs(1) }
    }

    /// Connect over the first available transport: USB first, then BLE.
    pub fn open_any() -> Result<Self> {
        match usb::UsbTransport::open() {
            Ok(t) => Ok(Self::new(Box::new(t))),
            Err(usb_err) => match ble::BleTransport::open() {
                Ok(t) => Ok(Self::new(Box::new(t))),
                Err(ble_err) => Err(Error::Io(format!("USB: {usb_err}; BLE: {ble_err}"))),
            },
        }
    }

    pub fn describe(&self) -> String {
        self.t.describe()
    }

    fn next_seq(&mut self) -> u8 {
        self.seq = self.seq % 255 + 1; // never 0: reserved for pushes
        self.seq
    }

    fn request(&mut self, kind: MsgType, payload: &[u8]) -> Result<Message> {
        let seq = self.next_seq();
        self.t.send(&proto::encode(kind, seq, payload))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let Some(buf) = self.t.recv(Some(left))? else { continue };
            let Ok(m) = proto::decode(&buf) else { continue };
            if m.seq == seq {
                return match m.kind {
                    MsgType::Error => Err(Error::Device(m.payload.first().copied().unwrap_or(0) as i8)),
                    _ => Ok(m),
                };
            }
            if m.kind == MsgType::State && m.seq == 0 {
                if let Ok(s) = State::parse(&m.payload) {
                    self.pending_pushes.push_back(s);
                }
            }
        }
    }

    pub fn hello(&mut self) -> Result<Hello> {
        let m = self.request(MsgType::Hello, &[])?;
        Ok(Hello::parse(&m.payload)?)
    }

    pub fn state(&mut self) -> Result<State> {
        let m = self.request(MsgType::GetState, &[])?;
        match m.kind {
            MsgType::State => Ok(State::parse(&m.payload)?),
            k => Err(Error::Unexpected(k)),
        }
    }

    /// Send a command; returns ACK data on success.
    pub fn command(&mut self, cmd: Command) -> Result<Vec<u8>> {
        let m = self.request(MsgType::Cmd, &cmd.payload())?;
        if m.kind != MsgType::Ack {
            return Err(Error::Unexpected(m.kind));
        }
        let ack = Ack::parse(&m.payload)?;
        if ack.status < 0 {
            return Err(Error::Command(ack.status));
        }
        Ok(ack.data)
    }

    pub fn layer_names(&mut self, count: u8) -> Vec<String> {
        (0..count)
            .map(|i| {
                self.command(Command::LayerName(i))
                    .map(|d| String::from_utf8_lossy(&d).into_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Block until the next unsolicited state push (`timeout` None = forever).
    pub fn next_push(&mut self, timeout: Option<Duration>) -> Result<Option<State>> {
        if let Some(s) = self.pending_pushes.pop_front() {
            return Ok(Some(s));
        }
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if left.is_some_and(|l| l.is_zero()) {
                return Ok(None);
            }
            let Some(buf) = self.t.recv(left)? else { continue };
            if let Ok(m) = proto::decode(&buf) {
                if m.kind == MsgType::State {
                    return Ok(Some(State::parse(&m.payload)?));
                }
            }
        }
    }
}
