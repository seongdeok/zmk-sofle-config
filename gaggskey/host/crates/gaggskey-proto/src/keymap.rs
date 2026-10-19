//! Keymap introspection: wire parsing + human-readable key labels.

use serde::Serialize;

use crate::ProtoError;

/// `KEYMAP_INFO` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KeymapInfo {
    pub keys: u8,
    pub layers: u8,
    pub behaviors: u8,
    pub layout_keys: u8,
}

impl KeymapInfo {
    pub fn parse(d: &[u8]) -> Result<Self, ProtoError> {
        match d {
            [k, l, b, p, ..] => Ok(Self { keys: *k, layers: *l, behaviors: *b, layout_keys: *p }),
            _ => Err(ProtoError::Payload("keymap info")),
        }
    }
}

/// Raw binding as sent by the firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawBinding {
    /// index into the behavior name table, 0xFF = unknown
    pub behavior: u8,
    pub param1: u32,
    pub param2: u32,
}

fn uvar(d: &[u8], i: &mut usize) -> Result<u32, ProtoError> {
    let mut v: u32 = 0;
    for shift in (0..35).step_by(7) {
        let b = *d.get(*i).ok_or(ProtoError::Payload("uvar"))?;
        *i += 1;
        v |= ((b & 0x7F) as u32) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(ProtoError::Payload("uvar"))
}

/// `KEYMAP_GET` reply: `[n] n x ([bidx][uvar p1][uvar p2])`.
pub fn parse_bindings(d: &[u8]) -> Result<Vec<RawBinding>, ProtoError> {
    let n = *d.first().ok_or(ProtoError::Payload("bindings"))? as usize;
    let mut i = 1;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let behavior = *d.get(i).ok_or(ProtoError::Payload("bindings"))?;
        i += 1;
        let param1 = uvar(d, &mut i)?;
        let param2 = uvar(d, &mut i)?;
        out.push(RawBinding { behavior, param1, param2 });
    }
    Ok(out)
}

/// Physical key geometry, ZMK units (1 key = 100, rotation in 1/100 deg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KeyGeom {
    pub w: i16,
    pub h: i16,
    pub x: i16,
    pub y: i16,
    pub r: i16,
    pub rx: i16,
    pub ry: i16,
}

impl KeyGeom {
    pub fn parse(d: &[u8]) -> Result<Self, ProtoError> {
        if d.len() < 14 {
            return Err(ProtoError::Payload("layout"));
        }
        let g = |i: usize| i16::from_le_bytes([d[2 * i], d[2 * i + 1]]);
        Ok(Self { w: g(0), h: g(1), x: g(2), y: g(3), r: g(4), rx: g(5), ry: g(6) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    Key,
    Mod,
    Layer,
    Trans,
    None,
    Bt,
    Out,
    Mouse,
    Media,
    System,
    Other,
}

/// What the UI draws on one key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyLabel {
    /// main legend (tap action)
    pub tap: String,
    /// secondary legend (hold action / qualifier)
    pub hold: Option<String>,
    pub kind: KeyKind,
    /// raw `&behavior p1 p2` for tooltips
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Keymap {
    pub keys: Vec<KeyGeom>,
    /// `layers[layer][pos]`
    pub layers: Vec<Vec<KeyLabel>>,
}

// ------------------------------------------------------------- HID usages

const MODS: [&str; 8] = ["Ctrl", "Shift", "Alt", "Gui", "RCtrl", "RShift", "RAlt", "RGui"];

fn mod_bits(bits: u8) -> Vec<&'static str> {
    (0..8).filter(|i| bits >> i & 1 == 1).map(|i| MODS[i]).collect()
}

fn keyboard_usage(id: u16) -> Option<&'static str> {
    const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    Some(match id {
        0x04..=0x1D => {
            let i = (id - 0x04) as usize;
            return Some(&LETTERS[i..i + 1]);
        }
        0x1E => "1",
        0x1F => "2",
        0x20 => "3",
        0x21 => "4",
        0x22 => "5",
        0x23 => "6",
        0x24 => "7",
        0x25 => "8",
        0x26 => "9",
        0x27 => "0",
        0x28 => "Enter",
        0x29 => "Esc",
        0x2A => "⌫",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "-",
        0x2E => "=",
        0x2F => "[",
        0x30 => "]",
        0x31 => "\\",
        0x32 => "#",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",
        0x39 => "Caps",
        0x3A..=0x45 => {
            const F: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
            F[(id - 0x3A) as usize]
        }
        0x46 => "PrtSc",
        0x47 => "ScrLk",
        0x48 => "Pause",
        0x49 => "Ins",
        0x4A => "Home",
        0x4B => "PgUp",
        0x4C => "Del",
        0x4D => "End",
        0x4E => "PgDn",
        0x4F => "→",
        0x50 => "←",
        0x51 => "↓",
        0x52 => "↑",
        0x53 => "NumLk",
        0x54 => "KP/",
        0x55 => "KP*",
        0x56 => "KP-",
        0x57 => "KP+",
        0x58 => "KPEnt",
        0x59..=0x61 => {
            const K: [&str; 9] = ["KP1", "KP2", "KP3", "KP4", "KP5", "KP6", "KP7", "KP8", "KP9"];
            K[(id - 0x59) as usize]
        }
        0x62 => "KP0",
        0x63 => "KP.",
        0x64 => "\\|",
        0x65 => "Menu",
        0x68..=0x73 => {
            const F: [&str; 12] =
                ["F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24"];
            F[(id - 0x68) as usize]
        }
        0x87 => "Ro",
        0x88 => "Kana",
        0x89 => "¥",
        0x8A => "Henk",
        0x8B => "Muhen",
        0x90 => "한/영",
        0x91 => "한자",
        0xE0 => "Ctrl",
        0xE1 => "Shift",
        0xE2 => "Alt",
        0xE3 => "Gui",
        0xE4 => "RCtrl",
        0xE5 => "RShift",
        0xE6 => "RAlt",
        0xE7 => "RGui",
        _ => return None,
    })
}

fn shifted(id: u16) -> Option<&'static str> {
    Some(match id {
        0x1E => "!",
        0x1F => "@",
        0x20 => "#",
        0x21 => "$",
        0x22 => "%",
        0x23 => "^",
        0x24 => "&",
        0x25 => "*",
        0x26 => "(",
        0x27 => ")",
        0x2D => "_",
        0x2E => "+",
        0x2F => "{",
        0x30 => "}",
        0x31 => "|",
        0x33 => ":",
        0x34 => "\"",
        0x35 => "~",
        0x36 => "<",
        0x37 => ">",
        0x38 => "?",
        _ => return None,
    })
}

fn consumer_usage(id: u16) -> Option<&'static str> {
    Some(match id {
        0x30 => "Power",
        0x6F => "Bri+",
        0x70 => "Bri-",
        0xB5 => "⏭",
        0xB6 => "⏮",
        0xB7 => "⏹",
        0xB8 => "⏏",
        0xCD => "⏯",
        0xB0 => "Play",
        0xB1 => "Pause",
        0xE2 => "Mute",
        0xE9 => "Vol+",
        0xEA => "Vol-",
        0x183 => "Media",
        0x18A => "Mail",
        0x192 => "Calc",
        0x194 => "Files",
        0x221 => "Search",
        0x223 => "Home",
        0x224 => "Back",
        0x225 => "Fwd",
        0x227 => "Refresh",
        _ => return None,
    })
}

/// Label for a ZMK keycode (`(mods << 24) | (page << 16) | id`).
pub fn keycode_label(code: u32) -> (String, KeyKind) {
    let mods = (code >> 24) as u8;
    let page = (code >> 16 & 0xFF) as u8;
    let id = (code & 0xFFFF) as u16;
    match page {
        0 | 0x07 => {
            if mods == 0x02 || mods == 0x20 {
                if let Some(s) = shifted(id) {
                    return (s.into(), KeyKind::Key);
                }
            }
            let base = keyboard_usage(id).map(str::to_string).unwrap_or_else(|| format!("0x{id:02X}"));
            let kind = if (0xE0..=0xE7).contains(&id) { KeyKind::Mod } else { KeyKind::Key };
            if mods == 0 {
                (base, kind)
            } else {
                let mut parts = mod_bits(mods);
                parts.push(&base);
                (parts.join("+"), kind)
            }
        }
        0x0C => (
            consumer_usage(id).map(str::to_string).unwrap_or_else(|| format!("C:0x{id:X}")),
            KeyKind::Media,
        ),
        _ => (format!("{page:02X}:{id:02X}"), KeyKind::Other),
    }
}

fn layer_name(names: &[String], id: u32) -> String {
    names
        .get(id as usize)
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("L{id}"))
}

fn mouse_buttons(b: u32) -> String {
    const N: [&str; 5] = ["LClk", "RClk", "MClk", "MB4", "MB5"];
    let v: Vec<_> = (0..5).filter(|i| b >> i & 1 == 1).map(|i| N[i]).collect();
    if v.is_empty() { format!("MB{b}") } else { v.join("+") }
}

fn mouse_dir(v: u32) -> String {
    let y = (v & 0xFFFF) as u16 as i16;
    let x = (v >> 16) as u16 as i16;
    match (x.signum(), y.signum()) {
        (0, -1) => "↑",
        (0, 1) => "↓",
        (-1, 0) => "←",
        (1, 0) => "→",
        (-1, -1) => "↖",
        (1, -1) => "↗",
        (-1, 1) => "↙",
        (1, 1) => "↘",
        _ => "·",
    }
    .into()
}

fn lbl(tap: impl Into<String>, hold: Option<String>, kind: KeyKind) -> KeyLabel {
    KeyLabel { tap: tap.into(), hold, kind, raw: String::new() }
}

/// Turn a binding into a legend. `behavior` is the ZMK device (DT node) name.
pub fn label(behavior: &str, p1: u32, p2: u32, layers: &[String]) -> KeyLabel {
    let key = |c: u32| keycode_label(c);
    let mut l = match behavior {
        "key_press" => {
            let (t, k) = key(p1);
            lbl(t, None, k)
        }
        "transparent" => lbl("▽", None, KeyKind::Trans),
        "none" => lbl("", None, KeyKind::None),
        "momentary_layer" => lbl(layer_name(layers, p1), Some("hold".into()), KeyKind::Layer),
        "to_layer" => lbl(layer_name(layers, p1), Some("to".into()), KeyKind::Layer),
        "toggle_layer" => lbl(layer_name(layers, p1), Some("toggle".into()), KeyKind::Layer),
        "sticky_layer" => lbl(layer_name(layers, p1), Some("sticky".into()), KeyKind::Layer),
        "layer_tap" => lbl(key(p2).0, Some(layer_name(layers, p1)), KeyKind::Layer),
        "mod_tap" => lbl(key(p2).0, Some(key(p1).0), KeyKind::Mod),
        "sticky_key" => lbl(key(p1).0, Some("sticky".into()), KeyKind::Mod),
        "key_toggle" => lbl(key(p1).0, Some("toggle".into()), KeyKind::Key),
        "bluetooth" => match p1 {
            0 => lbl("BT Clr", None, KeyKind::Bt),
            1 => lbl("BT ▶", None, KeyKind::Bt),
            2 => lbl("BT ◀", None, KeyKind::Bt),
            3 => lbl(format!("BT {}", p2 + 1), None, KeyKind::Bt),
            4 => lbl("BT ClrAll", None, KeyKind::Bt),
            5 => lbl(format!("BT {}", p2 + 1), Some("disc".into()), KeyKind::Bt),
            _ => lbl("BT ?", None, KeyKind::Bt),
        },
        "outputs" => lbl(
            match p1 {
                0 => "Out Tog",
                1 => "USB",
                2 => "BLE",
                _ => "Out ?",
            },
            None,
            KeyKind::Out,
        ),
        "mouse_key_press" => lbl(mouse_buttons(p1), None, KeyKind::Mouse),
        "mouse_move" => lbl(format!("🖱{}", mouse_dir(p1)), None, KeyKind::Mouse),
        "mouse_scroll" => lbl(format!("⇳{}", mouse_dir(p1)), Some("scroll".into()), KeyKind::Mouse),
        "caps_word" => lbl("CapsW", None, KeyKind::Mod),
        "key_repeat" => lbl("Repeat", None, KeyKind::Key),
        "grave_escape" => lbl("Esc", Some("`".into()), KeyKind::Key),
        "sysreset" => lbl("Reset", None, KeyKind::System),
        "bootload" => lbl("Boot", None, KeyKind::System),
        "z_so_off" => lbl("Off", None, KeyKind::System),
        "studio_unlock" => lbl("Unlock", None, KeyKind::System),
        "extpower" => lbl(["ExtOff", "ExtOn", "ExtTog"].get(p1 as usize).copied().unwrap_or("Ext"), None, KeyKind::System),
        "rgb_ug" => lbl("RGB", None, KeyKind::System),
        "bcklight" => lbl("BL", None, KeyKind::System),
        // Custom behaviors (usually hold-taps: p1 = hold, p2 = tap).
        other if p2 != 0 => {
            let hold = if p1 < 32 { layer_name(layers, p1) } else { key(p1).0 };
            let kind = if p1 < 32 { KeyKind::Layer } else { KeyKind::Mod };
            let _ = other;
            lbl(key(p2).0, Some(hold), kind)
        }
        other if p1 != 0 => lbl(key(p1).0, Some(other.into()), KeyKind::Other),
        other => lbl(other, None, KeyKind::Other),
    };
    l.raw = format!("&{behavior} {p1:#x} {p2:#x}");
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints() {
        // n=2: kp A (0x070004), trans
        let d = [2, 0, 0x84, 0x80, 0x1C, 0, 5, 0, 0];
        let b = parse_bindings(&d).unwrap();
        assert_eq!(b[0], RawBinding { behavior: 0, param1: 0x07_0004, param2: 0 });
        assert_eq!(b[1], RawBinding { behavior: 5, param1: 0, param2: 0 });
    }

    #[test]
    fn labels() {
        let names: Vec<String> = ["BAS", "NAV"].iter().map(|s| s.to_string()).collect();
        assert_eq!(label("key_press", 0x07_0004, 0, &names).tap, "A");
        assert_eq!(label("key_press", 0x02_07_001E, 0, &names).tap, "!");
        assert_eq!(label("key_press", 0x04_07_001D, 0, &names).tap, "Alt+Z");
        assert_eq!(label("key_press", 0x0C_00E2, 0, &names).tap, "Mute");
        let ht = label("thumb_layer_tap", 1, 0x07_0029, &names);
        assert_eq!((ht.tap.as_str(), ht.hold.as_deref()), ("Esc", Some("NAV")));
        let hm = label("hmr_l", 0x07_00E0, 0x07_0004, &names);
        assert_eq!((hm.tap.as_str(), hm.hold.as_deref()), ("A", Some("Ctrl")));
        assert_eq!(label("bluetooth", 3, 2, &names).tap, "BT 3");
        assert_eq!(label("mouse_move", 0xFDA8_0000, 0, &names).tap, "🖱←");
    }
}
