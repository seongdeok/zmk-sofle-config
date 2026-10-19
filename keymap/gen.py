#!/usr/bin/env python3
"""Generate per-keyboard ZMK keymaps from one shared base keymap.

Base   : config/corne.keymap      (Corne 3x6+3, 42 keys -- edit this one)
Overlay: keymap/<board>.toml       (only what differs from Corne)
Output : the file named by `output` in each toml (e.g. config/sofle.keymap)

    keymap/gen.py              regenerate every board
    keymap/gen.py --check      exit 1 if any generated file is stale (CI)
    keymap/gen.py --compare BOARD FILE
                               semantic diff of BOARD's generated keymap vs FILE
                               (resolved through cpp, so LSHFT == LEFT_SHIFT)

Overlay toml format (see sofle.toml / kyria.toml):

    output   = "config/sofle.keymap"
    includes = ["dt-bindings/zmk/ext_power.h"]   # added to the base includes
    layout   = '''                                 # physical key order, one row per line
    num0 num1 ...                                  # ints   = Corne key index (0..41)
    0 1 2 ...                                      # names  = extra keys; group = name without digits
    '''
    sensors  = "&inc_dec_kp C_VOL_DN C_VOL_UP"     # sensor-bindings for every layer (optional),
                                                   # or a [sensors] table: LAYER = "..." per layer

    [defines]            # replace base #defines (key-position lists for hold-trigger)
    LEFT_KEYS = "0 1 2"

    [behaviors.hmr]      # property overrides inside base behaviors; false removes a property
    flavor = '"tap-preferred"'
    [behaviors.mo_sk]    # node missing from base -> created (_node = node name)
    _node = "momentary_sticky_layer"

    [override.NAV]       # per-layer, Corne key index -> binding
    13 = "&kp LCTRL"

    [extras.BAS]         # per-layer, group -> bindings for that group's slots (default &trans)
    num = "&kp GRAVE &kp N1 ..."

Combo `key-positions` in the base are written as Corne indices and get remapped.
"""
import os
import re
import subprocess
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = os.path.join(ROOT, "config", "corne.keymap")
OVERLAY_DIR = os.path.join(ROOT, "keymap")
CORE_KEYS = 42


# ---------------------------------------------------------------- parsing helpers
def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), s, flags=re.S)
    return re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), s)


def find_block(text, start_re, pos=0):
    """Return (start, end) of a `name {...};` node; end is after the `;`."""
    clean = strip_comments(text)
    m = re.compile(start_re, re.M).search(clean, pos)
    if not m:
        return None
    i = clean.index("{", m.start())
    depth = 0
    for j in range(i, len(clean)):
        if clean[j] == "{":
            depth += 1
        elif clean[j] == "}":
            depth -= 1
            if depth == 0:
                k = clean.index(";", j)
                return m.start(), k + 1
    raise SyntaxError(f"unbalanced braces after {start_re}")


def child_nodes(text, block):
    """Yield (label, name, start, end) for direct children of block span."""
    s, e = block
    clean = strip_comments(text)
    i = clean.index("{", s) + 1
    depth = 0
    pat = re.compile(r"(?:([\w-]+)\s*:\s*)?([\w-]+)\s*\{")
    while i < e:
        m = pat.search(clean, i, e)
        if not m:
            return
        span = find_block(text, re.escape(clean[m.start():m.end()]), m.start())
        yield m.group(1), m.group(2), span[0], span[1]
        i = span[1]


def split_bindings(s):
    # split before every `&name` that starts a token (cpp output may contain bitwise `&`)
    parts = re.split(r"(?:^|(?<=\s))(?=&[A-Za-z_])", strip_comments(s))
    return [re.sub(r"\s+", " ", b).strip() for b in parts if b.strip()]


def prop(text, name):
    m = re.search(rf"(?<![\w-]){re.escape(name)}\s*=\s*(.*?);", strip_comments(text), re.S)
    return m.group(1).strip() if m else None


def parse_layers(text):
    km = find_block(text, r"^\s*keymap\s*\{")
    layers = []
    for _lbl, name, s, e in child_nodes(text, km):
        body = text[s:e]
        b = prop(body, "bindings")
        dn = prop(body, "display-name")
        layers.append({
            "node": name,
            "name": dn.strip('"') if dn else None,
            "bindings": split_bindings(b[1:-1]) if b else [],
            "sensors": prop(body, "sensor-bindings"),
        })
    return km, layers


# ---------------------------------------------------------------- overlay model
class Board:
    def __init__(self, path):
        self.id = os.path.splitext(os.path.basename(path))[0]
        with open(path, "rb") as f:
            self.cfg = tomllib.load(f)
        self.output = os.path.join(ROOT, self.cfg["output"])
        self.rows = []
        for line in self.cfg["layout"].strip().splitlines():
            row = [int(t) if t.isdigit() else t for t in line.split()]
            if row:
                self.rows.append(row)
        self.slots = [t for r in self.rows for t in r]
        core = sorted(t for t in self.slots if isinstance(t, int))
        if core != list(range(CORE_KEYS)):
            raise ValueError(f"{self.id}: layout must contain Corne keys 0..{CORE_KEYS-1} exactly once")
        self.core_pos = {t: i for i, t in enumerate(self.slots) if isinstance(t, int)}

    def groups(self):
        g = {}
        for t in self.slots:
            if isinstance(t, str):
                g.setdefault(re.sub(r"\d+$", "", t), []).append(t)
        return g


def board_files():
    return sorted(os.path.join(OVERLAY_DIR, f) for f in os.listdir(OVERLAY_DIR) if f.endswith(".toml"))


# ---------------------------------------------------------------- generation
def gen_layer_bindings(board, base_layer, idx):
    name = base_layer["name"] or str(idx)
    core = base_layer["bindings"]
    if len(core) != CORE_KEYS:
        raise ValueError(f"base layer {name}: {len(core)} bindings, expected {CORE_KEYS}")
    over = board.cfg.get("override", {}).get(name, {})
    extras = board.cfg.get("extras", {}).get(name, {})
    slot_val = {}
    groups = board.groups()
    for g, val in extras.items():
        if g not in groups:
            raise ValueError(f"{board.id}: extras.{name}.{g}: no such group in layout")
        b = split_bindings(val) if isinstance(val, str) else val
        if len(b) != len(groups[g]):
            raise ValueError(f"{board.id}: extras.{name}.{g}: {len(b)} bindings for {len(groups[g])} keys")
        slot_val.update(zip(groups[g], b))
    out_rows = []
    for row in board.rows:
        r = []
        for t in row:
            if isinstance(t, int):
                r.append(over.get(str(t), core[t]))
            else:
                r.append(slot_val.get(t, "&trans"))
        out_rows.append(r)
    for k in over:
        if not k.isdigit() or int(k) >= CORE_KEYS:
            raise ValueError(f"{board.id}: override.{name}.{k}: bad Corne index")
    return out_rows


def fmt_rows(rows):
    n = max(len(r) for r in rows)
    w = [max((len(r[c]) for r in rows if c < len(r)), default=0) + 2 for c in range(n)]
    return "\n".join("".join(b.ljust(w[c]) for c, b in enumerate(r)).rstrip() for r in rows)


def apply_behavior_overrides(text, board):
    beh = find_block(text, r"^\s*behaviors\s*\{")
    if not beh:
        return text
    nodes = {lbl: (s, e) for lbl, _n, s, e in child_nodes(text, beh)}
    edits = []          # (start, end, replacement)
    new_nodes = []
    for lbl, props in board.cfg.get("behaviors", {}).items():
        props = dict(props)
        node = props.pop("_node", lbl)
        if lbl not in nodes:
            body = "".join(f"            {k} = {v};\n" for k, v in props.items() if v is not False)
            new_nodes.append(f"\n        {lbl}: {node} {{\n{body}        }};\n")
            continue
        s, e = nodes[lbl]
        body = text[s:e]
        for k, v in props.items():
            pat = re.compile(rf"^(\s*){re.escape(k)}\s*=.*?;[^\n]*\n", re.M | re.S)
            if v is False:
                body, n = pat.subn("", body)
            else:
                body, n = pat.subn(lambda m: f"{m.group(1)}{k} = {v};\n", body)
                if n == 0:
                    body = re.sub(r"\n(\s*)\};\s*$", lambda m: f"\n            {k} = {v};\n{m.group(1)}}};", body)
        edits.append((s, e, body))
    if new_nodes:
        close = text.rindex("}", beh[0], beh[1])
        line_start = text.rindex("\n", 0, close) + 1
        edits.append((line_start, line_start, "".join(new_nodes)[1:]))
    for s, e, rep in sorted(edits, reverse=True):
        text = text[:s] + rep + text[e:]
    return text


def remap_combos(text, board):
    def fix(m):
        nums = [board.core_pos[int(x)] for x in m.group(2).split()]
        return f"{m.group(1)}<{' '.join(map(str, nums))}>"
    return re.sub(r"(key-positions\s*=\s*)<([\d\s]+)>", fix, text)


def generate(board, base_text):
    text = base_text
    # defines
    for name, val in board.cfg.get("defines", {}).items():
        pat = re.compile(rf"^#define\s+{name}\b(?:.*\\\n)*.*\n", re.M)
        line = f"#define {name} {val}\n"
        text, n = pat.subn(lambda m: line, text)
        if n == 0:
            text = re.sub(r"(#include[^\n]*\n)(?!.*#include)", lambda m: m.group(1) + line, text, count=1, flags=re.S)
    # includes
    inc = "".join(f"#include <{i}>\n" for i in board.cfg.get("includes", []) if f"<{i}>" not in text)
    if inc:
        last = list(re.finditer(r"^#include[^\n]*\n", text, re.M))[-1]
        text = text[:last.end()] + inc + text[last.end():]
    text = apply_behavior_overrides(text, board)
    text = remap_combos(text, board)
    # keymap
    km, layers = parse_layers(text)
    sensors = board.cfg.get("sensors")
    names = board.cfg.get("layer_names", {})
    parts = ["keymap {\n        compatible = \"zmk,keymap\";\n"]
    for i, L in enumerate(layers):
        rows = gen_layer_bindings(board, L, i)
        dn = names.get(L["name"] or str(i), L["name"])
        body = f"\n        {L['node']} {{\n"
        if dn:
            body += f"            display-name = \"{dn}\";\n"
        body += "            bindings = <\n" + fmt_rows(rows) + "\n            >;\n"
        sb = sensors.get(L["name"] or str(i)) if isinstance(sensors, dict) else sensors
        if sb:
            body += f"\n            sensor-bindings = <{sb}>;\n"
        parts.append(body + "        };\n")
    parts.append("    };")
    text = text[:km[0]].rstrip() + "\n\n    " + "".join(parts) + text[km[1]:]
    hdr = (f"// GENERATED by keymap/gen.py from config/corne.keymap + keymap/{board.id}.toml\n"
           f"// DO NOT EDIT -- change the base keymap or the overlay, then run keymap/gen.py\n\n")
    return hdr + text


# ---------------------------------------------------------------- semantic compare (cpp)
INC = ["zmk/app/include", "zephyr/include"]


def resolve(path_or_text):
    text = path_or_text if "\n" in path_or_text else open(path_or_text).read()
    text = re.sub(r"#include\s*<behaviors\.dtsi>", "", text)
    args = ["cpp", "-P", "-nostdinc", "-undef", "-x", "assembler-with-cpp"]
    for i in INC:
        args += ["-I", os.path.join(ROOT, i)]
    out = subprocess.run(args, input=text, capture_output=True, text=True, check=True).stdout

    def ev(tok):
        try:
            return str(eval(tok, {}, {}))
        except Exception:
            return tok

    def norm(b):
        name, rest = (b.split(None, 1) + [""])[:2]
        params, depth, cur = [], 0, ""
        for ch in rest:
            if ch == " " and depth == 0:
                if cur:
                    params.append(cur)
                cur = ""
                continue
            depth += ch == "("
            depth -= ch == ")"
            cur += ch
        if cur:
            params.append(cur)
        return " ".join([name] + [ev(p) for p in params])

    _km, layers = parse_layers(out)
    res = {"layers": [[norm(b) for b in L["bindings"]] for L in layers],
           "sensors": [L["sensors"] and re.sub(r"\s+", " ", L["sensors"]) for L in layers]}
    beh = find_block(out, r"^\s*behaviors\s*\{")
    res["behaviors"] = {}
    for lbl, name, s, e in child_nodes(out, beh) if beh else []:
        props = {}
        for p in strip_comments(out[s:e]).split("{", 1)[1].rsplit("}", 1)[0].split(";"):
            p = re.sub(r"\s+", " ", p).strip()
            if not p:
                continue
            k, _, v = p.partition("=")
            k, v = k.strip(), v.strip()
            if k == "hold-trigger-key-positions":
                v = sorted(int(x) for x in re.findall(r"\d+", v))
            if k == "label":
                continue    # labels are cosmetic
            props[k] = v
        res["behaviors"][lbl] = props
    return res


def compare(a, b):
    diffs = []
    if len(a["layers"]) != len(b["layers"]):
        diffs.append(f"layer count {len(a['layers'])} != {len(b['layers'])}")
    for li, (la, lb) in enumerate(zip(a["layers"], b["layers"])):
        if len(la) != len(lb):
            diffs.append(f"layer {li}: {len(la)} keys != {len(lb)}")
            continue
        for k, (x, y) in enumerate(zip(la, lb)):
            if x != y:
                diffs.append(f"layer {li} key {k}: {x!r} != {y!r}")
        if a["sensors"][li] != b["sensors"][li]:
            diffs.append(f"layer {li} sensors: {a['sensors'][li]!r} != {b['sensors'][li]!r}")
    for lbl in sorted(set(a["behaviors"]) | set(b["behaviors"])):
        x, y = a["behaviors"].get(lbl), b["behaviors"].get(lbl)
        if x != y:
            diffs.append(f"behavior {lbl}: {x} != {y}")
    return diffs


# ---------------------------------------------------------------- absorb
def _q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _raw_behaviors(text):
    beh = find_block(text, r"^\s*behaviors\s*\{")
    d = {}
    for lbl, name, s, e in child_nodes(text, beh) if beh else []:
        props = {}
        for p in strip_comments(text[s:e]).split("{", 1)[1].rsplit("}", 1)[0].split(";"):
            p = re.sub(r"\s+", " ", p).strip()
            if p:
                k, _, v = p.partition("=")
                props[k.strip()] = v.strip()
        d[lbl] = (name, props)
    return d


def absorb(board, path, base_text):
    """Rewrite board's toml so it reproduces the full keymap at `path`.

    Keeps output/includes/layout/layer_names; recomputes defines, sensors,
    behaviors, override and extras as the diff against the base."""
    other = open(path).read()
    rb, ro = resolve(base_text), resolve(path)
    _, bl = parse_layers(base_text)
    _, ol = parse_layers(other)
    names = [L["name"] for L in bl]
    if len(ol) != len(bl):
        raise ValueError(f"{path}: {len(ol)} layers, base has {len(bl)}")
    cfg = board.cfg
    out = [f"# {board.id}: Corne 대비 차이만 적는다. 베이스 = config/corne.keymap",
           "# 생성: python3 keymap/gen.py  (생성된 config/*.keymap 은 직접 고치지 말 것)",
           f"# 키보드 키맵 전체를 흡수: python3 keymap/gen.py --absorb {board.id} <file.keymap>",
           "", f"output = {_q(cfg['output'])}"]
    incs = list(dict.fromkeys(cfg.get("includes", []) + [
        i for i in re.findall(r"#include <([^>]+)>", other) if f"<{i}>" not in base_text]))
    out.append("includes = [" + ", ".join(_q(i) for i in incs) + "]")
    out += ["", "# 물리 키 순서. 숫자 = Corne 키 번호(0..41), 이름 = Corne에 없는 키(그룹 = 끝 숫자 뺀 이름)",
            "layout = '''", cfg["layout"].strip("\n"), "'''"]
    if cfg.get("layer_names"):
        out += ["", "[layer_names]"] + [f"{k} = {_q(v)}" for k, v in cfg["layer_names"].items()]
    defs = re.findall(r"^#define\s+(\w+)[ \t]+((?:.*\\\n)*.*)$", other, re.M)
    if defs:
        out += ["", "[defines]"] + [f"{n} = {_q(' '.join(v.replace(chr(92), '').split()))}" for n, v in defs]
    sens = [(names[i], L["sensors"]) for i, L in enumerate(ol) if L["sensors"]]
    if sens:
        out += ["", "[sensors]"] + [f"{n} = {_q(re.sub(r'\s+', ' ', v.strip('<>').strip()))}" for n, v in sens]
    bb, ob = _raw_behaviors(base_text), _raw_behaviors(other)
    for lbl, (name, props) in ob.items():
        bp = bb.get(lbl, (None, {}))[1]
        ch = {"_node": name} if lbl not in bb else {}
        for k, v in props.items():
            if (k != "label" or lbl not in bb) and bp.get(k) != v:
                ch[k] = v
        for k in bp:
            if k not in props and k != "label":
                ch[k] = False
        if ch:
            out += ["", f"[behaviors.{lbl}]"]
            for k, v in ch.items():
                key = k if re.fullmatch(r"[A-Za-z0-9_-]+", k) else _q(k)
                out.append(f"{key} = {'false' if v is False else _q(v)}")
    total = 0
    for li, n in enumerate(names):
        if len(ol[li]["bindings"]) != len(board.slots):
            raise ValueError(f"{path}: layer {n} has {len(ol[li]['bindings'])} keys, layout has {len(board.slots)}")
        ov = [f"{t} = {_q(ol[li]['bindings'][i])}".ljust(44) + f"# corne: {bl[li]['bindings'][t]}"
              for i, t in enumerate(board.slots)
              if isinstance(t, int) and ro["layers"][li][i] != rb["layers"][li][t]]
        total += len(ov)
        if ov:
            out += ["", f"[override.{n}]"] + ov
    for li, n in enumerate(names):
        grp = {}
        for i, t in enumerate(board.slots):
            if isinstance(t, str):
                grp.setdefault(re.sub(r"\d+$", "", t), []).append(ol[li]["bindings"][i])
        grp = {g: v for g, v in grp.items() if any(x != "&trans" for x in v)}
        if grp:
            out += ["", f"[extras.{n}]"] + [f"{g} = {_q('  '.join(v))}" for g, v in grp.items()]
    tpath = os.path.join(OVERLAY_DIR, board.id + ".toml")
    open(tpath, "w").write("\n".join(out) + "\n")
    print(f"  {os.path.relpath(tpath, ROOT)}: absorbed {path} ({total} overrides)")


# ---------------------------------------------------------------- main
def main(argv):
    base_text = open(BASE).read()
    boards = [Board(p) for p in board_files()]
    if argv[:1] == ["--absorb"]:
        absorb(next(x for x in boards if x.id == argv[1]), argv[2], base_text)
        boards = [Board(p) for p in board_files()]
        argv = []
    if argv[:1] == ["--compare"]:
        b = next(x for x in boards if x.id == argv[1])
        diffs = compare(resolve(generate(b, base_text)), resolve(argv[2]))
        print("\n".join(diffs) or f"{b.id}: identical to {argv[2]}")
        return 1 if diffs else 0
    stale = 0
    for b in boards:
        out = generate(b, base_text)
        cur = open(b.output).read() if os.path.exists(b.output) else None
        rel = os.path.relpath(b.output, ROOT)
        if cur == out:
            print(f"  {rel}: up to date")
        elif argv[:1] == ["--check"]:
            print(f"  {rel}: STALE -- run keymap/gen.py")
            stale += 1
        else:
            open(b.output, "w").write(out)
            print(f"  {rel}: written")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
