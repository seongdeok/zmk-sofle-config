#!/usr/bin/env python3
"""sofctl - talk to the Sofle Options+ firmware module over USB (hidraw).

Stdlib only. Protocol: optionsplus/firmware/include/optionsplus/proto.h

  sofctl status [--json]     one-shot state
  sofctl watch  [--json]     stream state pushes
  sofctl hello               device info
  sofctl layers              list layer ids + names
  sofctl layer <id>          switch to layer (layer_to)
  sofctl toggle <id>         toggle layer
  sofctl bt <n|next|prev>    select BLE profile
  sofctl out <usb|ble|toggle>
  sofctl reset | bootloader
"""
import argparse
import errno
import glob
import json
import os
import select
import struct
import sys
import time

FRAME = 32
HDR = 5
MAGIC = 0x5F
VER = 1
USAGE_PAGE = 0xFF0B
USAGE = 0x01

MSG_HELLO, MSG_GET_STATE, MSG_STATE = 0x01, 0x02, 0x03
MSG_CMD, MSG_ACK, MSG_ERROR = 0x10, 0x11, 0x7F

CMD = {
    "layer_to": 0x01, "layer_on": 0x02, "layer_off": 0x03, "layer_toggle": 0x04,
    "layer_name": 0x05, "bt_select": 0x10, "bt_next": 0x11, "bt_prev": 0x12,
    "bt_clear": 0x13, "out_select": 0x20, "out_toggle": 0x21,
    "reset": 0x7E, "bootloader": 0x7F,
}
GUARD = bytes([0xB0, 0x07])

CAPS = ["usb", "ble", "split", "periph_battery", "wpm", "dangerous"]
FLAGS = ["usb_powered", "periph_connected", "ble_profile_open", "ble_profile_connected"]
ACTIVITY = ["active", "idle", "sleep"]

STATE_FMT = "<IBBBBBBBBBBB"
HELLO_FMT = "<BHBB"


def find_device():
    """Return /dev/hidrawN whose report descriptor starts with our usage page."""
    want = bytes([0x06, USAGE_PAGE & 0xFF, USAGE_PAGE >> 8, 0x09, USAGE])
    for rd in sorted(glob.glob("/sys/class/hidraw/hidraw*/device/report_descriptor")):
        try:
            with open(rd, "rb") as f:
                if f.read(len(want)) == want:
                    return "/dev/" + rd.split("/")[4]
        except OSError:
            pass
    return None


class Link:
    def __init__(self, path):
        self.path = path
        self.fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
        self.seq = 0

    def close(self):
        os.close(self.fd)

    def send(self, mtype, payload=b""):
        self.seq = (self.seq % 255) + 1
        f = bytes([MAGIC, VER, mtype, self.seq, len(payload)]) + payload
        f = f.ljust(FRAME, b"\0")
        os.write(self.fd, b"\0" + f)  # leading 0 = report id (none)
        return self.seq

    def recv(self, timeout):
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return None
        try:
            d = os.read(self.fd, 64)
        except OSError as e:
            if e.errno == errno.EAGAIN:
                return None
            raise
        if len(d) < HDR or d[0] != MAGIC:
            return None
        return d[2], d[3], d[HDR:HDR + d[4]]

    def request(self, mtype, payload=b"", timeout=1.0):
        seq = self.send(mtype, payload)
        end = time.monotonic() + timeout
        while (left := end - time.monotonic()) > 0:
            m = self.recv(left)
            if m and m[1] == seq:
                return m
        raise TimeoutError("no reply from keyboard")

    def cmd(self, name, args=b"", timeout=1.0):
        t, _, p = self.request(MSG_CMD, bytes([CMD[name]]) + args, timeout)
        if t == MSG_ERROR:
            raise RuntimeError(f"device error {struct.unpack('b', p[:1])[0]}")
        status = struct.unpack("b", p[1:2])[0]
        if status < 0:
            raise RuntimeError(f"{name} failed: {os.strerror(-status)} ({status})")
        return p[2:]


def parse_state(p):
    (ls, hi, dflt, bc, bp, tr, prof, cmask, wpm, ind, act, fl) = struct.unpack_from(STATE_FMT, p)
    pct = lambda v: None if v == 0xFF else v
    return {
        "layers_active": [i for i in range(32) if ls >> i & 1] or [dflt],
        "layer": hi,
        "default_layer": dflt,
        "battery": {"left": pct(bc), "right": pct(bp)},
        "output": "ble" if tr else "usb",
        "ble_profile": prof,
        "ble_connected": [i for i in range(8) if cmask >> i & 1],
        "wpm": wpm,
        "caps_lock": bool(ind & 2),
        "num_lock": bool(ind & 1),
        "activity": ACTIVITY[act] if act < 3 else act,
        **{k: bool(fl >> i & 1) for i, k in enumerate(FLAGS)},
    }


def parse_hello(p):
    ver, caps, layers, profiles = struct.unpack_from(HELLO_FMT, p)
    name = p[struct.calcsize(HELLO_FMT):].split(b"\0")[0].decode(errors="replace")
    return {"name": name, "proto": ver, "layers": layers, "ble_profiles": profiles,
            "caps": [c for i, c in enumerate(CAPS) if caps >> i & 1]}


def fmt_state(s, names=None):
    b = s["battery"]
    bat = lambda v: "--" if v is None else f"{v}%"
    lname = (names or {}).get(s["layer"], "")
    return (f"layer {s['layer']}{' ' + lname if lname else ''}  "
            f"bat L {bat(b['left'])} R {bat(b['right'])}{'' if s['periph_connected'] else ' (R offline)'}  "
            f"out {s['output'].upper()}  bt {s['ble_profile']}"
            f"{'*' if s['ble_profile_connected'] else ''}  wpm {s['wpm']}  "
            f"{'CAPS ' if s['caps_lock'] else ''}{s['activity']}")


def layer_names(link, count):
    out = {}
    for i in range(count):
        try:
            out[i] = link.cmd("layer_name", bytes([i])).decode(errors="replace")
        except Exception:
            out[i] = ""
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dev", help="hidraw path (auto-detect by default)")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("action")
    ap.add_argument("arg", nargs="?")
    a = ap.parse_args()

    path = a.dev or find_device()
    if not path:
        sys.exit("Options+ interface not found (flash left half with Options+ firmware, connect USB)")
    try:
        link = Link(path)
    except PermissionError:
        sys.exit(f"no permission on {path}: install optionsplus/host/udev/70-sofle-optionsplus.rules")

    out = lambda obj, txt: print(json.dumps(obj) if a.json else txt, flush=True)

    if a.action == "hello":
        h = parse_hello(link.request(MSG_HELLO)[2])
        out(h, f"{h['name']}  proto v{h['proto']}  layers {h['layers']}  "
               f"bt profiles {h['ble_profiles']}  caps {','.join(h['caps'])}  ({path})")
    elif a.action == "status":
        s = parse_state(link.request(MSG_GET_STATE)[2])
        out(s, fmt_state(s))
    elif a.action == "watch":
        h = parse_hello(link.request(MSG_HELLO)[2])
        names = layer_names(link, h["layers"])
        s = parse_state(link.request(MSG_GET_STATE)[2])
        out(s, fmt_state(s, names))
        while True:
            m = link.recv(None)
            if m and m[0] == MSG_STATE:
                s = parse_state(m[2])
                out(s, fmt_state(s, names))
    elif a.action == "layers":
        h = parse_hello(link.request(MSG_HELLO)[2])
        names = layer_names(link, h["layers"])
        out(names, "\n".join(f"{i}: {n}" for i, n in names.items()))
    elif a.action in ("layer", "toggle"):
        link.cmd("layer_to" if a.action == "layer" else "layer_toggle", bytes([int(a.arg)]))
    elif a.action == "bt":
        if a.arg in ("next", "prev"):
            link.cmd("bt_" + a.arg)
        else:
            link.cmd("bt_select", bytes([int(a.arg)]))
    elif a.action == "out":
        if a.arg == "toggle":
            link.cmd("out_toggle")
        else:
            link.cmd("out_select", bytes([1 if a.arg == "ble" else 0]))
    elif a.action in ("reset", "bootloader"):
        link.cmd(a.action, GUARD)
    else:
        ap.error(f"unknown action {a.action}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
    except (TimeoutError, RuntimeError) as e:
        sys.exit(str(e))
