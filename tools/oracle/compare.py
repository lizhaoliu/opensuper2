#!/usr/bin/env python3
"""Run one script through s2run and the oracle and diff every shot.

    compare.py --game DIR --out DIR (--script TEXT | --script-file F)
               [--s2run BIN] [--no-run] [--port-only] [--oracle-arg ARG ...]

OUT/s2run/ and OUT/oracle/ hold each side's shots; for every `shot NAME`
the report gives the number of differing pixels and their bounding boxes,
and OUT/NAME.diff.png shows s2run | oracle | differences (magenta over the
dimmed oracle frame).

The original presents 16-bit RGB565 frames, so s2run's 8-bit RGB palette
colours are quantised the way the original's palette-to-pixel table does
(FUN_00475a50: 5 bits per channel, green's low bit 0) before comparing; a
pixel differs only when the 16-bit values differ.
"""
import argparse
import fcntl
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
FIXTURES = os.environ.get("S2_ORACLE_FIXTURES", os.path.expanduser("~/games/super2sp/dump/oracle/fixtures"))
W, H = 640, 480
TILE = 8


def read_png(path):
    """Decode an 8-bit RGB or indexed PNG -> (w, h, list of (r, g, b) per pixel)."""
    with open(path, "rb") as f:
        data = f.read()
    pos = 8
    idat = b""
    pal = None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype = struct.unpack(">IIBB", body[:10])
        elif kind == b"PLTE":
            pal = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    assert depth == 8 and ctype in (2, 3), (path, depth, ctype)
    bpp = 3 if ctype == 2 else 1
    raw = zlib.decompress(idat)
    stride = w * bpp
    rows = []
    prev = bytearray(stride)
    p = 0
    for _ in range(h):
        ft = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        if ft == 2:
            line = bytearray((x + y) & 255 for x, y in zip(line, prev))
        for i in range(stride if ft in (1, 3, 4) else 0):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if ft == 1:
                line[i] = (line[i] + a) & 255
            elif ft == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif ft == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        rows.append(bytes(line))
        prev = line
    flat = b"".join(rows)
    if ctype == 2:
        px = [tuple(flat[i:i + 3]) for i in range(0, len(flat), 3)]
    else:
        px = [pal[v] for v in flat]
    return w, h, px


def write_png(path, w, h, rgb):
    raw = b"".join(b"\0" + rgb[y * w * 3:(y + 1) * w * 3] for y in range(h))

    def chunk(kind, body):
        c = kind + body
        return struct.pack(">I", len(body)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def to565(c):
    """FUN_00475a50's palette-to-pixel table: (r & 0xf8) << 8 | (g & 0xf8) << 3 | b >> 3."""
    r, g, b = c
    return (r & 0xF8) << 8 | (g & 0xF8) << 3 | b >> 3


def expand(v):
    r, g, b = (v >> 11) & 31, (v >> 5) & 63, v & 31
    return (r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)


def boxes(mask):
    """Bounding boxes of the differing pixels, clustered over 8x8 tiles (8-connected)."""
    tw, th = (W + TILE - 1) // TILE, (H + TILE - 1) // TILE
    tiles = {}
    for i, d in enumerate(mask):
        if d:
            x, y = i % W, i // W
            k = (x // TILE, y // TILE)
            b = tiles.get(k)
            tiles[k] = (min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)) if b else (x, y, x, y)
    seen = set()
    out = []
    for k in tiles:
        if k in seen:
            continue
        stack = [k]
        seen.add(k)
        bb = None
        n = 0
        while stack:
            t = stack.pop()
            b = tiles[t]
            bb = b if bb is None else (min(bb[0], b[0]), min(bb[1], b[1]), max(bb[2], b[2]), max(bb[3], b[3]))
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    u = (t[0] + dx, t[1] + dy)
                    if u in tiles and u not in seen:
                        seen.add(u)
                        stack.append(u)
        out.append(bb)
    out.sort(key=lambda b: -(b[2] - b[0] + 1) * (b[3] - b[1] + 1))
    return out


def compare(name, port_png, oracle_png, out_dir):
    if not os.path.exists(port_png) or not os.path.exists(oracle_png):
        return {"name": name, "missing": [p for p in (port_png, oracle_png) if not os.path.exists(p)]}
    _, _, a = read_png(port_png)
    _, _, b = read_png(oracle_png)
    va = [to565(c) for c in a]
    vb = [to565(c) for c in b]
    mask = [x != y for x, y in zip(va, vb)]
    n = sum(mask)
    bbs = boxes(mask) if n else []
    if n:
        rgb = bytearray()
        for y in range(H):
            row = slice(y * W, (y + 1) * W)
            for v in va[row]:
                rgb += bytes(expand(v))
            for v in vb[row]:
                rgb += bytes(expand(v))
            for v, d in zip(vb[row], mask[row]):
                if d:
                    rgb += b"\xff\x00\xff"
                else:
                    r, g, bb = expand(v)
                    rgb += bytes((r // 4, g // 4, bb // 4))
        write_png(os.path.join(out_dir, name + ".diff.png"), 3 * W, H, bytes(rgb))
    return {"name": name, "diff": n, "boxes": bbs}


class Tee:
    def __init__(self, *streams):
        self.streams = streams

    def write(self, s):
        for st in self.streams:
            st.write(s)
            st.flush()

    def flush(self):
        for st in self.streams:
            st.flush()


def cache_key(args, saves):
    """Everything the oracle's output depends on."""
    import hashlib
    h = hashlib.sha1()
    h.update(args.script.encode())
    for p in (os.path.join(HERE, "oracle.py"), os.path.expanduser("~/games/super2sp/re/WSUPERSP_unpacked.exe")):
        with open(p, "rb") as f:
            h.update(f.read())
    for n in sorted(os.listdir(saves)) if saves else []:
        with open(os.path.join(saves, n), "rb") as f:
            h.update(n.encode() + f.read())
    cfg = find_ci(args.game, "SUPERSP.CFG")
    if cfg:
        with open(cfg, "rb") as f:
            h.update(f.read())
    h.update(" ".join(args.oracle_arg).encode())
    h.update(b"sound-watch-v1")  # every oracle run logs FUN_00419b1d (sound requests)
    return h.hexdigest()[:16]


def save_mask(data, n_items):
    """Offsets of a save that hold whatever was in memory (never compared):
    the copyright block after its NUL (the loader seeks past it) and each
    unit's runtime pointers (+0xad, +0xb1)."""
    mask = set()
    base = 9 + 0x32
    end = base + 0xff
    nul = data.find(b"\0", base, end)
    mask.update(range(nul + 1 if nul >= 0 else end, end))
    p = end + 0x50 + 0x1f + 0x1d + 4 + 0xa0 + 0x1c
    for _ in range(2):
        if p + 4 > len(data):
            break
        n = struct.unpack_from("<I", data, p)[0]
        p += 4
        if n > 200:
            break
        for k in range(n):
            mask.update(range(p + k * 0x16A + 0xAD, p + k * 0x16A + 0xB5))
        p += n * 0x16A + 4 + 4 + 4 + 2 + 4 + n_items + 0x21
    return mask


SOUND_FN = "419b1d"  # FUN_00419b1d(id, slot): the game's sound request


def sound_requests_port(path):
    """(frame, id, slot) of every sound request s2run logged (S2_SOUND_TRACE)."""
    out = []
    with open(path, errors="replace") as f:
        for line in f:
            m = re.search(r"\[sound\] f(\d+) play (\d+) slot (\d+)", line)
            if m:
                out.append((int(m.group(1)), int(m.group(2)), int(m.group(3))))
    return out


def sound_requests_oracle(path):
    """(frame, id, slot) of every FUN_00419b1d call the oracle logged (--watch)."""
    out = []
    with open(path, errors="replace") as f:
        for line in f:
            m = re.search(r"\[f(\d+) t\d+\] watch FUN_00419b1d\((0x[0-9a-f]+), (0x[0-9a-f]+)", line)
            if m:
                out.append((int(m.group(1)), int(m.group(2), 16) & 0xffff, int(m.group(3), 16)))
    return out


def compare_sounds(port_log, oracle_log):
    """Both programs' sound requests, in order: frame, sound id and voice slot."""
    if not (os.path.exists(port_log) and os.path.exists(oracle_log)):
        return None
    a, b = sound_requests_port(port_log), sound_requests_oracle(oracle_log)
    if a == b:
        return "%-24s identical (%d requests)" % ("sounds", len(a))
    k = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
    pa = a[k] if k < len(a) else None
    pb = b[k] if k < len(b) else None
    return "%-24s differ at request %d: s2run %s, original %s (%d vs %d requests)" % (
        "sounds", k + 1, pa, pb, len(a), len(b))


def compare_files(port_game, oracle_scratch, saves):
    """The SAVE files and SUPERSP.CFG each program wrote (those that differ
    from what it started with), compared byte for byte."""
    out = []
    start = {}
    for n in os.listdir(saves) if saves else []:
        with open(os.path.join(saves, n), "rb") as f:
            start[n.upper()] = f.read()

    def files(root):
        got = {}
        d = find_ci(root, "SAVE") if os.path.isdir(root) else None
        for n in os.listdir(d) if d else []:
            with open(os.path.join(d, n), "rb") as f:
                got["SAVE\\" + n.upper()] = f.read()
        cfg = find_ci(root, "SUPERSP.CFG") if os.path.isdir(root) else None
        if cfg:
            with open(cfg, "rb") as f:
                got["SUPERSP.CFG"] = f.read()
        return got

    a, b = files(port_game), files(oracle_scratch)
    n_items = 0
    items = find_ci(port_game, "ITEMSP.BIN") if os.path.isdir(port_game) else None
    if items:
        with open(items, "rb") as f:
            n_items = struct.unpack("<h", f.read(2))[0]
    for name in sorted(set(a) | set(b)):
        x, y = a.get(name), b.get(name)
        orig = start.get(name.split("\\")[-1]) if name.startswith("SAVE") else None
        if x == y:
            if x is not None and x != orig and name != "SUPERSP.CFG":
                out.append("%-24s written, identical" % ("file " + name))
            continue
        if x is None or y is None:
            out.append("%-24s only written by %s" % ("file " + name, "the oracle" if x is None else "s2run"))
            continue
        mask = save_mask(x, n_items) if name.startswith("SAVE") and len(x) == len(y) else set()
        diffs = [i for i in range(max(len(x), len(y)))
                 if (i >= len(x) or i >= len(y) or x[i] != y[i]) and i not in mask]
        if not diffs:
            out.append("%-24s written, identical (uninitialised bytes ignored)" % ("file " + name))
            continue
        out.append("%-24s differs: %d byte(s) (first at %#x), sizes %d / %d" % (
            "file " + name, len(diffs), diffs[0], len(x), len(y)))
    return out


def clock_split(port_log, oracle_log):
    """The first frame at which the two virtual clocks disagree, if any."""
    try:
        with open(port_log) as a, open(oracle_log) as b:
            for la, lb in zip(a, b):
                if la != lb:
                    a, b = la.split(), lb.split()
                    pa, pb = ("present" if len(x) > 2 else "idle" for x in (a, b))
                    return "clocks part at frame %s: s2run %s ms (%s), oracle %s ms (%s)" % (a[0], a[1], pa, b[1], pb)
    except (OSError, ValueError):
        return None
    return None


def find_ci(d, name):
    for n in os.listdir(d):
        if n.lower() == name.lower():
            return os.path.join(d, n)
    return None


def overlay(src, dst, saves):
    """A private game directory for s2run: entries of `src` linked, SAVE\\ copied from
    `saves` and the CFG copied, so both programs start alike and s2run's
    autosaves never reach the real SAVE folder (the oracle keeps its own scratch)."""
    if os.path.isdir(dst):
        shutil.rmtree(dst)
    os.makedirs(os.path.join(dst, "SAVE"))
    for n in os.listdir(src):
        p = os.path.join(src, n)
        if n.upper() == "SAVE":
            continue
        if n.upper().endswith(".CFG"):
            shutil.copyfile(p, os.path.join(dst, n))
        else:
            os.symlink(os.path.realpath(p), os.path.join(dst, n))
    if saves:
        for n in os.listdir(saves):
            if os.path.isfile(os.path.join(saves, n)):
                shutil.copyfile(os.path.join(saves, n), os.path.join(dst, "SAVE", n))
    return dst


def shot_names(script):
    names = []
    for raw in script.replace("\n", ";").split(";"):
        w = raw.split()
        if len(w) >= 2 and w[0] == "shot":
            names.append(w[1])
    return names


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", default=os.environ.get("S2_GAME_DIR"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--script", default="")
    ap.add_argument("--script-file")
    ap.add_argument("--s2run", default=os.path.join(REPO, "target", "release", "s2run"))
    ap.add_argument("--no-run", action="store_true", help="only diff the shots already in OUT")
    ap.add_argument("--keep-oracle", action="store_true", help="rerun s2run only, reusing OUT/oracle")
    ap.add_argument("--log", help="also write everything printed to this file")
    ap.add_argument("--saves", help="SAVE folder both programs start from (default: the script's "
                                    "'#! saves NAME' fixture, else the game's SAVE)")
    ap.add_argument("--cache", help="reuse the oracle's shots from CACHE/<hash of script, saves, exe, oracle.py>")
    ap.add_argument("--oracle-arg", action="append", default=[])
    ap.add_argument("--port-only", action="store_true", help="only run s2run (on the private game dir) and stop")
    args = ap.parse_args()
    if args.log:
        sys.stdout = Tee(sys.stdout, open(args.log, "w"))
    if args.script_file:
        with open(args.script_file) as f:
            args.script = f.read()
    bad = [line for line in args.script.splitlines() if line.lstrip().startswith("#") and ";" in line]
    if bad:  # ';' separates steps even in a comment, so the rest would run as a step
        print("';' in a comment line splits it into steps: %s" % bad[0])
        sys.exit(2)
    for line in args.script.splitlines():  # directives: "#! saves NAME" (a fixture) or a folder
        w = line.split()
        if len(w) == 3 and w[:2] == ["#!", "saves"] and not args.saves:
            args.saves = w[2] if os.path.isabs(w[2]) else os.path.join(FIXTURES, w[2])
    if args.saves and not os.path.isdir(args.saves):
        print("SAVE fixture %s missing: run tools/oracle/make_fixtures.sh" % args.saves)
        sys.exit(2)
    out = os.path.abspath(args.out)
    port_dir, oracle_dir = os.path.join(out, "s2run"), os.path.join(out, "oracle")
    os.makedirs(port_dir, exist_ok=True)
    saves = args.saves or find_ci(args.game, "SAVE")
    if args.cache and not args.no_run:
        oracle_dir = os.path.join(args.cache, cache_key(args, saves))
    if not args.no_run:
        # One oracle per output folder at a time (held until exit): two runs in
        # one folder (a cache entry, or a sweep check run twice at once) share
        # its scratch SAVE folder, and each program then sees the other's saves.
        os.makedirs(os.path.dirname(oracle_dir), exist_ok=True)
        lock = open(oracle_dir + ".lock", "w")
        fcntl.flock(lock, fcntl.LOCK_EX)
    if args.cache and not args.no_run:
        names = shot_names(args.script)
        if all(os.path.exists(os.path.join(oracle_dir, n)) for n in [m + ".png" for m in names] + ["frames.log"]):
            args.keep_oracle = True
        elif os.path.isdir(oracle_dir):
            shutil.rmtree(oracle_dir)
    if not args.no_run:
        game = overlay(args.game, os.path.join(out, "game"), saves)
        procs = [("s2run", subprocess.Popen([args.s2run, "--game", game, "--out", port_dir, "--script",
                                              args.script, "--frame-log", os.path.join(port_dir, "frames.log")],
                                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                             env=dict(os.environ, S2_SOUND_TRACE="1")))]
        if not args.keep_oracle and not args.port_only:
            procs.append(("oracle", subprocess.Popen(
                [sys.executable, "-I", os.path.join(HERE, "oracle.py"), "--game", args.game, "--out", oracle_dir,
                 "--script", args.script, "--frame-log", os.path.join(oracle_dir, "frames.log"),
                 "--watch", SOUND_FN]
                + (["--saves", saves] if saves else []) + args.oracle_arg,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)))
        for label, p in procs:
            text = p.communicate()[0].decode(errors="replace")
            with open(os.path.join(out, label + ".txt"), "w") as f:
                f.write(text)
            if p.returncode:
                print("%s exited with %d:\n%s" % (label, p.returncode, text[-2000:]))
    if args.port_only:
        print("s2run's shots are in %s" % port_dir)
        return
    lines = []
    results = []
    for name in shot_names(args.script):
        r = compare(name, os.path.join(port_dir, name + ".png"), os.path.join(oracle_dir, name + ".png"), out)
        results.append(r)
        if "missing" in r:
            line = "%-24s MISSING %s" % (name, ", ".join(r["missing"]))
        elif r["diff"] == 0:
            line = "%-24s identical" % name
        else:
            bb = " ".join("(%d,%d)-(%d,%d)" % b for b in r["boxes"][:6])
            more = " +%d more" % (len(r["boxes"]) - 6) if len(r["boxes"]) > 6 else ""
            line = "%-24s %7d px differ in %d box(es): %s%s" % (name, r["diff"], len(r["boxes"]), bb, more)
        print(line)
        lines.append(line)
    with open(os.path.join(out, "report.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")
    if not args.no_run:
        for line in compare_files(os.path.join(out, "game"), os.path.join(oracle_dir, "scratch"), saves):
            print(line)
            lines.append(line)
            results.append({"name": line, "diff": 0 if " identical" in line else 1})
    if not args.no_run:
        line = compare_sounds(os.path.join(out, "s2run.txt"), os.path.join(oracle_dir, "oracle.log"))
        if line:
            print(line)
            lines.append(line)
            results.append({"name": "sounds", "diff": 0 if " identical" in line else 1})
    part = clock_split(os.path.join(port_dir, "frames.log"), os.path.join(oracle_dir, "frames.log"))
    if part:
        lines.append(part)
        print(part)
        with open(os.path.join(out, "report.txt"), "a") as f:
            f.write(part + "\n")
    bad = [r for r in results if r.get("diff", 1)]
    print("%d/%d shots identical" % (len(results) - len(bad), len(results)))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
