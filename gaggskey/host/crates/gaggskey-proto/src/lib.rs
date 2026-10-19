//! GaggsKey wire protocol v1.
//!
//! Mirror of `gaggskey/firmware/include/gaggskey/proto.h` — keep in sync.
//! Every frame is exactly [`FRAME_SIZE`] bytes, little-endian:
//! `[magic][ver][type][seq][len][payload…]`.

use serde::Serialize;

pub mod keymap;

pub const FRAME_SIZE: usize = 32;
pub const HEADER_SIZE: usize = 5;
pub const PAYLOAD_MAX: usize = FRAME_SIZE - HEADER_SIZE;
pub const MAGIC: u8 = 0x5F;
pub const PROTO_VERSION: u8 = 1;

pub const HID_USAGE_PAGE: u16 = 0xFF0B;
pub const HID_USAGE: u16 = 0x01;
pub const USB_VID: u16 = 0x1D50;
pub const USB_PID: u16 = 0x615E;

pub const BT_SERVICE_UUID: &str = "5f0b0001-6761-6767-736b-657900000000";
pub const BT_CHRC_UUID: &str = "5f0b0002-6761-6767-736b-657900000000";

pub const BATTERY_UNKNOWN: u8 = 0xFF;
const GUARD: [u8; 2] = [0xB0, 0x07];

pub type Frame = [u8; FRAME_SIZE];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType {
    Hello = 0x01,
    GetState = 0x02,
    State = 0x03,
    Cmd = 0x10,
    Ack = 0x11,
    Error = 0x7F,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0x01 => Self::Hello,
            0x02 => Self::GetState,
            0x03 => Self::State,
            0x10 => Self::Cmd,
            0x11 => Self::Ack,
            0x7F => Self::Error,
            _ => return None,
        })
    }
}

/// Commands the host can send (`MsgType::Cmd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    LayerTo(u8),
    LayerOn(u8),
    LayerOff(u8),
    LayerToggle(u8),
    LayerName(u8),
    BtSelect(u8),
    BtNext,
    BtPrev,
    BtClear,
    OutSelect(Output),
    OutToggle,
    KeymapInfo,
    BehaviorName(u8),
    /// (layer, start position)
    KeymapGet(u8, u8),
    LayoutGet(u8),
    Reset,
    Bootloader,
}

impl Command {
    pub fn id(&self) -> u8 {
        use Command::*;
        match self {
            LayerTo(_) => 0x01,
            LayerOn(_) => 0x02,
            LayerOff(_) => 0x03,
            LayerToggle(_) => 0x04,
            LayerName(_) => 0x05,
            BtSelect(_) => 0x10,
            BtNext => 0x11,
            BtPrev => 0x12,
            BtClear => 0x13,
            OutSelect(_) => 0x20,
            OutToggle => 0x21,
            KeymapInfo => 0x30,
            BehaviorName(_) => 0x31,
            KeymapGet(..) => 0x32,
            LayoutGet(_) => 0x33,
            Reset => 0x7E,
            Bootloader => 0x7F,
        }
    }

    fn args(&self) -> Vec<u8> {
        use Command::*;
        match *self {
            LayerTo(l) | LayerOn(l) | LayerOff(l) | LayerToggle(l) | LayerName(l) => vec![l],
            BtSelect(p) | BehaviorName(p) | LayoutGet(p) => vec![p],
            KeymapGet(l, s) => vec![l, s],
            OutSelect(o) => vec![o as u8],
            Reset | Bootloader => GUARD.to_vec(),
            BtNext | BtPrev | BtClear | OutToggle | KeymapInfo => vec![],
        }
    }

    pub fn payload(&self) -> Vec<u8> {
        let mut p = vec![self.id()];
        p.extend(self.args());
        p
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum Output {
    Usb = 0,
    Ble = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Active,
    Idle,
    Sleep,
    Unknown,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtoError {
    #[error("frame too short ({0} bytes)")]
    Short(usize),
    #[error("bad magic 0x{0:02x}")]
    Magic(u8),
    #[error("unsupported protocol version {0}")]
    Version(u8),
    #[error("payload length {0} out of range")]
    Length(u8),
    #[error("unknown message type 0x{0:02x}")]
    Type(u8),
    #[error("payload too short for {0}")]
    Payload(&'static str),
}

/// A decoded frame header + payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: MsgType,
    pub seq: u8,
    pub payload: Vec<u8>,
}

pub fn encode(kind: MsgType, seq: u8, payload: &[u8]) -> Frame {
    assert!(payload.len() <= PAYLOAD_MAX, "payload too large");
    let mut f = [0u8; FRAME_SIZE];
    f[..HEADER_SIZE].copy_from_slice(&[MAGIC, PROTO_VERSION, kind as u8, seq, payload.len() as u8]);
    f[HEADER_SIZE..HEADER_SIZE + payload.len()].copy_from_slice(payload);
    f
}

pub fn decode(buf: &[u8]) -> Result<Message, ProtoError> {
    if buf.len() < HEADER_SIZE {
        return Err(ProtoError::Short(buf.len()));
    }
    if buf[0] != MAGIC {
        return Err(ProtoError::Magic(buf[0]));
    }
    if buf[1] != PROTO_VERSION {
        return Err(ProtoError::Version(buf[1]));
    }
    let len = buf[4];
    if len as usize > PAYLOAD_MAX || HEADER_SIZE + len as usize > buf.len() {
        return Err(ProtoError::Length(len));
    }
    let kind = MsgType::from_u8(buf[2]).ok_or(ProtoError::Type(buf[2]))?;
    Ok(Message { kind, seq: buf[3], payload: buf[HEADER_SIZE..HEADER_SIZE + len as usize].to_vec() })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hello {
    pub name: String,
    pub proto: u8,
    pub caps: Vec<&'static str>,
    pub layers: u8,
    pub ble_profiles: u8,
}

const CAPS: [&str; 6] = ["usb", "ble", "split", "periph_battery", "wpm", "dangerous"];

impl Hello {
    pub fn parse(p: &[u8]) -> Result<Self, ProtoError> {
        if p.len() < 5 {
            return Err(ProtoError::Payload("hello"));
        }
        let caps = u16::from_le_bytes([p[1], p[2]]);
        let name = &p[5..];
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        Ok(Self {
            name: String::from_utf8_lossy(&name[..end]).into_owned(),
            proto: p[0],
            caps: CAPS.iter().enumerate().filter(|(i, _)| caps >> i & 1 == 1).map(|(_, c)| *c).collect(),
            layers: p[3],
            ble_profiles: p[4],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Battery {
    pub left: Option<u8>,
    pub right: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct State {
    pub layers_active: Vec<u8>,
    pub layer: u8,
    pub default_layer: u8,
    pub battery: Battery,
    pub output: Output,
    pub ble_profile: u8,
    pub ble_connected: Vec<u8>,
    pub wpm: u8,
    pub num_lock: bool,
    pub caps_lock: bool,
    pub scroll_lock: bool,
    pub activity: Activity,
    pub usb_powered: bool,
    pub periph_connected: bool,
    pub ble_profile_open: bool,
    pub ble_profile_connected: bool,
}

pub const STATE_SIZE: usize = 15;

impl State {
    pub fn parse(p: &[u8]) -> Result<Self, ProtoError> {
        if p.len() < STATE_SIZE {
            return Err(ProtoError::Payload("state"));
        }
        let ls = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let bat = |v: u8| (v != BATTERY_UNKNOWN).then_some(v);
        let bits = |v: u32, n: u8| (0..n).filter(|i| v >> i & 1 == 1).collect::<Vec<u8>>();
        let (ind, fl) = (p[12], p[14]);
        let mut layers_active = bits(ls, 32);
        if layers_active.is_empty() {
            layers_active.push(p[5]);
        }
        Ok(Self {
            layers_active,
            layer: p[4],
            default_layer: p[5],
            battery: Battery { left: bat(p[6]), right: bat(p[7]) },
            output: if p[8] == 1 { Output::Ble } else { Output::Usb },
            ble_profile: p[9],
            ble_connected: bits(p[10] as u32, 8),
            wpm: p[11],
            num_lock: ind & 1 != 0,
            caps_lock: ind & 2 != 0,
            scroll_lock: ind & 4 != 0,
            activity: match p[13] {
                0 => Activity::Active,
                1 => Activity::Idle,
                2 => Activity::Sleep,
                _ => Activity::Unknown,
            },
            usb_powered: fl & 1 != 0,
            periph_connected: fl & 2 != 0,
            ble_profile_open: fl & 4 != 0,
            ble_profile_connected: fl & 8 != 0,
        })
    }
}

/// Parsed `MsgType::Ack` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub cmd: u8,
    /// 0 or negative errno from firmware
    pub status: i8,
    pub data: Vec<u8>,
}

impl Ack {
    pub fn parse(p: &[u8]) -> Result<Self, ProtoError> {
        if p.len() < 2 {
            return Err(ProtoError::Payload("ack"));
        }
        Ok(Self { cmd: p[0], status: p[1] as i8, data: p[2..].to_vec() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from the real keyboard over BLE (2026-10-19).
    const HELLO_RX: &str = "5f 01 01 07 1b 01 3f 00 07 05 47 61 67 67 73 20 53 6f 66 6c 65 00 00 00 00 00 00 00 00 00 00 00";
    const STATE_RX: &str = "5f 01 03 00 0f 00 00 00 00 00 00 4d 41 01 01 03 00 00 01 0a 00 00 00 00 00 00 00 00 00 00 00 00";

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    #[test]
    fn hello_vector() {
        let m = decode(&hex(HELLO_RX)).unwrap();
        assert_eq!((m.kind, m.seq), (MsgType::Hello, 7));
        let h = Hello::parse(&m.payload).unwrap();
        assert_eq!(h.name, "Gaggs Sofle");
        assert_eq!((h.layers, h.ble_profiles), (7, 5));
        assert_eq!(h.caps.len(), 6);
    }

    #[test]
    fn state_vector() {
        let m = decode(&hex(STATE_RX)).unwrap();
        assert_eq!((m.kind, m.seq), (MsgType::State, 0));
        let s = State::parse(&m.payload).unwrap();
        assert_eq!(s.layers_active, vec![0]);
        assert_eq!(s.battery, Battery { left: Some(77), right: Some(65) });
        assert_eq!(s.output, Output::Ble);
        assert_eq!(s.ble_profile, 1);
        assert_eq!(s.ble_connected, vec![0, 1]);
        assert!(s.periph_connected && s.ble_profile_connected && !s.usb_powered);
    }

    #[test]
    fn encode_cmd() {
        let f = encode(MsgType::Cmd, 9, &Command::LayerTo(1).payload());
        assert_eq!(&f[..7], &[0x5f, 1, 0x10, 9, 2, 0x01, 0x01]);
        assert_eq!(Command::Bootloader.payload(), vec![0x7f, 0xb0, 0x07]);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(decode(&[0; 32]), Err(ProtoError::Magic(0)));
        assert_eq!(decode(&[0x5f, 1, 3]), Err(ProtoError::Short(3)));
        let mut f = encode(MsgType::State, 0, &[]);
        f[4] = 40;
        assert_eq!(decode(&f), Err(ProtoError::Length(40)));
    }
}
