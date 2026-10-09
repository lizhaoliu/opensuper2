#!/usr/bin/env python3
"""Ground-truth oracle: the original WSUPERSP.EXE, run headlessly under Unicorn.

    oracle.py --game DIR --out DIR (--script TEXT | --script-file FILE)
              [--exe UNPACKED.EXE] [--time-seed N] [--trace] [--max-frames N]

The unpacked original (image base 0x400000, entry 0x47b11f, imports rebuilt
in .idata2) runs instruction by instruction. Every import, and every method
of the DirectDraw objects it is handed, is answered here by a high-level
emulation; the program never reaches the host OS:

- files: read-only over the game directory; SAVE\\ and SUPERSP.CFG live in a
  scratch copy (OUT/scratch) so the program may write them. Nothing from the
  game directory is ever executed (MSS32.DLL/WDIR32.DLL are not loaded).
- input: the s2run script (frames/key/hold/up/move/click/rclick/shot/quit,
  same syntax and timing) becomes WM_* messages handed out by PeekMessageA,
  with Windows' WM_MOUSEMOVE coalescing, WM_CHAR from TranslateMessage and
  WM_LBUTTONDBLCLK (the window class has CS_DBLCLKS).
- DirectDraw: fake COM objects whose vtables point at hook stubs; surfaces
  are 16-bit RGB565 buffers in emulated memory. A blit or flip to the
  primary surface is a "present".
- time: s2run's virtual clock. A present costs 16 ms (one 60 Hz frame);
  an input poll (FUN_00474aac) that finds nothing costs 1 ms, as s2run's
  Sys::input_get; a busy wait on the clock (reads inside FUN_0046d626,
  FUN_0046d652 or FUN_0046d688) advances it 1 ms per read (after SPIN_FREE
  free reads) and so ends on the millisecond s2run jumps to. A
  script frame runs at every 16 ms boundary. GetLocalTime starts at 00:00:00.000, so the
  centisecond clock FUN_0046d5d6 equals s2run's ticks_cs. time(NULL)
  (FUN_0047ac1b) returns --time-seed, s2run's Seeder::fixed(0) by default.
- Miles (mss32): every AIL_* call succeeds silently; samples report done.

`shot NAME` writes OUT/NAME.png, the last presented frame as RGB, and
OUT/NAME.idx.png, the 8-bit frame and palette the platform layer converted
it from (for diagnosis). An unimplemented API or COM method is logged with
its arguments and stops the run.
"""
import argparse
import array
import collections
import fnmatch
import inspect
import os
import shutil
import struct
import sys
import time
import unicodedata
import zlib

import pefile
from unicorn import (UC_ARCH_X86, UC_HOOK_CODE, UC_HOOK_INTR, UC_HOOK_MEM_INVALID, UC_HOOK_MEM_UNMAPPED,
                     UC_HOOK_MEM_WRITE, UC_MODE_32, Uc, UcError)
from unicorn.x86_const import (UC_X86_REG_DS, UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EIP,
                               UC_X86_REG_ES, UC_X86_REG_ESP, UC_X86_REG_FS, UC_X86_REG_GDTR, UC_X86_REG_SS)

DEFAULT_EXE = os.path.expanduser("~/games/super2sp/re/WSUPERSP_unpacked.exe")

# Address space.
GDT = 0x00001000
STACK_LO, STACK_HI = 0x00100000, 0x00300000
HEAP_LO, HEAP_HI = 0x10000000, 0x30000000
VALLOC_LO, VALLOC_HI = 0x30000000, 0x40000000
STUB_LO, STUB_HI = 0x70000000, 0x70010000
COM_LO, COM_HI = 0x70100000, 0x70200000
MISC_LO, MISC_HI = 0x70200000, 0x70210000
TEB, PEB = 0x7FFDE000, 0x7FFDF000

SCREEN_W, SCREEN_H = 640, 480
FRAME_MS = 16
SPIN_FREE = 256  # clock reads without a present, message or idle before time is made to pass
# The clock waits: FUN_0046d626 (wait until t0 + n), FUN_0046d652 (sleep n cs) and
# FUN_0046d688 (still within n cs of t0, the test of every polling loop). Only
# their reads can be a busy wait; FUN_0046d5d6 called directly (a start time, the
# palette cycle's or an animation's timer) never waits.
CLOCK_WAITS = (0x46D626, 0x46D6B5)
INPUT_GET, INPUT_PEEK, INPUT_FLUSH, INPUT_WAIT = 0x474AAC, 0x474BB9, 0x474C54, 0x474C79  # platform input layer
INSTALL_DIR = "C:\\SUPER2SP"
MODULE_PATH = INSTALL_DIR + "\\WSUPERSP.EXE"
TIME_FN = 0x47AC1B  # CRT time(): FUN_0047ac1b

# Window messages.
WM_KEYDOWN, WM_KEYUP, WM_CHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_SYSCHAR = 0x100, 0x101, 0x102, 0x104, 0x105, 0x106
WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK = 0x200, 0x201, 0x202, 0x203
WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK = 0x204, 0x205, 0x206
VK_SHIFT, VK_CONTROL, VK_MENU = 0x10, 0x11, 0x12
HWND_MAIN = 0x00010010

# s2run's key names (crates/s2engine/src/bin/s2run.rs).
KEYS = {"RETURN": 0x0D, "ENTER": 0x0D, "ESCAPE": 0x1B, "ESC": 0x1B, "SPACE": 0x20, "LEFT": 0x25, "RIGHT": 0x27,
        "UP": 0x26, "DOWN": 0x28, "ALT": VK_MENU, "SHIFT": VK_SHIFT, "CTRL": VK_CONTROL, "TAB": 0x09}
EXTENDED_KEYS = {0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E}


def scan_code(k):
    """PC set-1 scan code of a virtual key (s2engine sys::scan_code)."""
    letters = [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19,
               0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C]
    if k == 0x1B:
        return 0x01
    if 0x31 <= k <= 0x39:
        return 0x02 + k - 0x31
    if k == 0x30:
        return 0x0B
    if 0x41 <= k <= 0x5A:
        return letters[k - 0x41]
    if 0x70 <= k <= 0x79:
        return 0x3B + k - 0x70
    return {0x08: 0x0E, 0x09: 0x0F, 0x0D: 0x1C, 0x11: 0x1D, 0x10: 0x2A, 0x12: 0x38, 0x20: 0x39, 0x24: 0x47,
            0x26: 0x48, 0x21: 0x49, 0x25: 0x4B, 0x27: 0x4D, 0x23: 0x4F, 0x28: 0x50, 0x22: 0x51}.get(k, 0)


def parse_script(text):
    """The s2run script language: steps separated by ';' or newlines, '#' comments."""
    steps = []
    for raw in text.replace("\n", ";").split(";"):
        w = raw.split()
        if not w or w[0].startswith("#"):
            continue
        op = w[0]
        if op == "frames":
            steps.append(("frames", int(w[1])))
        elif op in ("key", "hold", "up"):
            name = w[1].upper()
            if name in KEYS:
                k = KEYS[name]
            elif len(name) == 1 and name.isascii() and name.isalnum():
                k = ord(name)
            else:
                raise SystemExit("unknown key %s" % w[1])
            steps.append((op, k))
        elif op == "move":
            steps.append(("move", int(w[1]), int(w[2])))
        elif op in ("click", "rclick"):
            steps.append(("click", 0 if op == "click" else 1, int(w[1]), int(w[2])))
        elif op == "shot":
            steps.append(("shot", w[1]))
        elif op == "quit":
            steps.append(("quit",))
        else:
            raise SystemExit("unknown step %s" % op)
    return steps


def write_png(path, w, h, rows, palette=None):
    """rows: bytes of h rows, 3 bytes/pixel (RGB) or 1 byte/pixel with `palette`."""
    bpp = 1 if palette is not None else 3
    raw = b"".join(b"\0" + rows[y * w * bpp:(y + 1) * w * bpp] for y in range(h))

    def chunk(kind, data):
        c = kind + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 3 if palette is not None else 2, 0, 0, 0)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
    if palette is not None:
        out += chunk(b"PLTE", palette)
    out += chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(out)


def _expand565():
    t = []
    for v in range(65536):
        r, g, b = (v >> 11) & 31, (v >> 5) & 63, v & 31
        t.append(bytes(((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))))
    return t


RGB565 = None


class Stop(Exception):
    pass


class Surface:
    def __init__(self, oracle, w, h, caps, addr):
        self.w, self.h, self.caps = w, h, caps
        self.pitch = w * 2
        self.mem = oracle.alloc(self.pitch * h, zero=True)
        self.addr = addr
        self.back = None
        self.ckey = None
        self.clipper = 0
        self.refs = 1

    @property
    def primary(self):
        return bool(self.caps & 0x200)


class HostFile:
    def __init__(self, f, path, writable):
        self.f, self.path, self.writable = f, path, writable


class Oracle:
    def __init__(self, args):
        self.args = args
        self.game = os.path.abspath(args.game)
        self.out = os.path.abspath(args.out)
        os.makedirs(self.out, exist_ok=True)
        self.scratch = os.path.join(self.out, "scratch")
        self.setup_scratch()
        self.steps = parse_script(args.script)
        self.trace = args.trace
        self.log_file = open(os.path.join(self.out, "oracle.log"), "w")
        self.frame_log = open(args.frame_log, "w") if args.frame_log else None
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.stop_reason = None
        self.exit_code = 0
        self.stub_calls = 0
        self.wall_start = time.time()
        # Clock and script (s2run semantics).
        self.now = 0
        self.last_frame = 0
        self.serial = 0
        self.spin = {}
        self.pc = 0
        self.wait_until = 0
        self.frame = bytes(SCREEN_W * SCREEN_H * 2)
        self.frame_idx = None
        # Input.
        self.msgs = collections.deque()
        self.mouse = (320, 240)
        self.keys = [False] * 256  # physical (script) state
        self.sync_keys = [False] * 256  # GetKeyState: as of the last retrieved message
        self.buttons = [False, False]
        self.last_click = None  # (button, time, x, y) for double clicks
        self.wndproc = 0
        self.class_style = 0
        self.cursor_count = 0
        # Objects.
        self.handles = {}
        self.next_handle = 0x1000
        self.last_error = 0
        self.heap_next = HEAP_LO
        self.heap_free = collections.defaultdict(list)
        self.heap_sizes = {}
        self.valloc_next = VALLOC_LO
        self.com_next = COM_LO
        self.com = {}  # object address -> python object
        self.surfaces = []
        self.dcs = {}
        self.misc_next = MISC_LO
        self.stubs = {}
        self.stub_next = STUB_LO
        self.cb_stack = []
        self.window_longs = {}
        self.api_calls = collections.Counter()
        self.load()

    # ---- logging -------------------------------------------------------------------------------------------
    def log(self, msg):
        self.log_file.write("[f%d t%d] %s\n" % (self.serial, self.now, msg))
        self.log_file.flush()

    def fail(self, msg):
        self.log("STOP: " + msg)
        print("oracle: " + msg, file=sys.stderr)
        self.stop_reason = msg
        self.exit_code = 2
        self.uc.emu_stop()

    # ---- memory --------------------------------------------------------------------------------------------
    def r32(self, a):
        return struct.unpack("<I", self.uc.mem_read(a, 4))[0]

    def w32(self, a, v):
        self.uc.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))

    def r16(self, a):
        return struct.unpack("<H", self.uc.mem_read(a, 2))[0]

    def w16(self, a, v):
        self.uc.mem_write(a, struct.pack("<H", v & 0xFFFF))

    def rb(self, a, n):
        return bytes(self.uc.mem_read(a, n))

    def wb(self, a, data):
        self.uc.mem_write(a, bytes(data))

    def cstr(self, a, limit=4096):
        if a == 0:
            return b""
        out = bytearray()
        while len(out) < limit:
            chunk = self.rb(a + len(out), 64)
            i = chunk.find(b"\0")
            if i >= 0:
                out += chunk[:i]
                return bytes(out)
            out += chunk
        return bytes(out)

    def wstr(self, a, n=None):
        """UTF-16 string at a (n code units, or NUL-terminated)."""
        if n is not None and n >= 0:
            return self.rb(a, 2 * n).decode("utf-16-le", "replace")
        out = bytearray()
        while True:
            c = self.rb(a + len(out), 2)
            if c == b"\0\0":
                return out.decode("utf-16-le", "replace")
            out += c

    def misc(self, data):
        """Static data the program reads (command line, environment, ...)."""
        a = self.misc_next
        self.wb(a, data)
        self.misc_next = (a + len(data) + 15) & ~15
        return a

    def mapped(self, a, n):
        for lo, hi, _ in self.uc.mem_regions():
            if lo <= a and a + max(n, 1) - 1 <= hi:
                return True
        return False

    def alloc(self, n, zero=False):
        n = max(16, (n + 15) & ~15)
        lst = self.heap_free.get(n)
        if lst:
            a = lst.pop()
            if zero:
                self.wb(a, bytes(n))
        else:
            a = self.heap_next
            if a + n > HEAP_HI:
                self.fail("heap exhausted")
                return 0
            self.heap_next += n
        self.heap_sizes[a] = n
        return a

    def free(self, a):
        n = self.heap_sizes.pop(a, None)
        if n is None:
            return False
        self.heap_free[n].append(a)
        return True

    # ---- loading -------------------------------------------------------------------------------------------
    def setup_scratch(self):
        if os.path.isdir(self.scratch):
            shutil.rmtree(self.scratch)
        os.makedirs(os.path.join(self.scratch, "SAVE"))
        save = self.args.saves or self.find_ci(self.game, "SAVE")
        if save and os.path.isdir(save):
            for n in os.listdir(save):
                p = os.path.join(save, n)
                if os.path.isfile(p):
                    shutil.copyfile(p, os.path.join(self.scratch, "SAVE", n))
        cfg = self.find_ci(self.game, "SUPERSP.CFG")
        if cfg:
            shutil.copyfile(cfg, os.path.join(self.scratch, "SUPERSP.CFG"))

    def load(self):
        uc = self.uc
        pe = pefile.PE(self.args.exe)
        self.base = pe.OPTIONAL_HEADER.ImageBase
        size = (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF
        uc.mem_map(self.base, size)
        uc.mem_write(self.base, bytes(pe.get_memory_mapped_image()))
        self.entry = self.base + pe.OPTIONAL_HEADER.AddressOfEntryPoint
        for lo, hi in ((STACK_LO, STACK_HI), (HEAP_LO, HEAP_HI), (VALLOC_LO, VALLOC_HI), (STUB_LO, STUB_HI),
                       (COM_LO, COM_HI), (MISC_LO, MISC_HI), (TEB, PEB + 0x1000)):
            uc.mem_map(lo, hi - lo)
        uc.mem_write(STUB_LO, b"\xc3" * (STUB_HI - STUB_LO))
        self.setup_gdt()
        self.w32(TEB + 0x00, 0xFFFFFFFF)
        self.w32(TEB + 0x04, STACK_HI)
        self.w32(TEB + 0x08, STACK_LO)
        self.w32(TEB + 0x18, TEB)
        self.w32(TEB + 0x20, 0x100)
        self.w32(TEB + 0x24, 0x104)
        self.w32(TEB + 0x30, PEB)
        self.w32(PEB + 0x08, self.base)
        self.api_handlers = {}
        for name, fn in inspect.getmembers(self, inspect.ismethod):
            if name.startswith("api_"):
                self.api_handlers[name[4:]] = fn
        n = 0
        for imp in pe.DIRECTORY_ENTRY_IMPORT:
            dll = imp.dll.decode().lower()
            for f in imp.imports:
                fname = f.name.decode() if f.name else "#%d" % f.ordinal
                self.w32(f.address, self.api_stub(dll, fname))
                n += 1
        self.log("mapped %s: %d import slots" % (self.args.exe, n))
        self.exit_stub = self.make_stub("<process exit>", self.on_exit, 0)
        self.cb_ret = self.make_stub("<callback return>", None, 0)
        esp = STACK_HI - 0x100
        self.w32(esp, self.exit_stub)
        uc.reg_write(UC_X86_REG_ESP, esp)
        uc.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUB_LO, end=STUB_HI - 1)
        uc.hook_add(UC_HOOK_CODE, self.on_time, begin=TIME_FN, end=TIME_FN)
        for a in self.args.watch:
            uc.hook_add(UC_HOOK_CODE, self.on_watch, begin=a, end=a)
        for a in self.args.watch_write:
            # A hook matches on the access's first byte: start early enough for 8-byte writes.
            uc.hook_add(UC_HOOK_MEM_WRITE, self.on_watch_write, a, begin=a - 7, end=a)
        uc.hook_add(UC_HOOK_MEM_UNMAPPED | UC_HOOK_MEM_INVALID, self.on_fault)
        uc.hook_add(UC_HOOK_INTR, self.on_intr)

    def setup_gdt(self):
        uc = self.uc
        uc.mem_map(GDT, 0x1000)

        def desc(base, limit, access, flags):
            d = limit & 0xFFFF
            d |= (base & 0xFFFFFF) << 16
            d |= access << 40
            d |= ((limit >> 16) & 0xF) << 48
            d |= flags << 52
            d |= ((base >> 24) & 0xFF) << 56
            return struct.pack("<Q", d)

        uc.mem_write(GDT + 8, desc(0, 0xFFFFF, 0x9A | 0x60, 0xC))
        uc.mem_write(GDT + 16, desc(0, 0xFFFFF, 0x92 | 0x60, 0xC))
        uc.mem_write(GDT + 24, desc(TEB, 0xFFF, 0x92 | 0x60, 0x4))
        uc.mem_write(GDT + 32, desc(0, 0xFFFFF, 0x92, 0xC))
        uc.reg_write(UC_X86_REG_GDTR, (0, GDT, 0x1000, 0))
        uc.reg_write(UC_X86_REG_SS, 4 << 3)
        uc.reg_write(UC_X86_REG_DS, (2 << 3) | 3)
        uc.reg_write(UC_X86_REG_ES, (2 << 3) | 3)
        uc.reg_write(UC_X86_REG_FS, (3 << 3) | 3)

    # ---- stubs and dispatch --------------------------------------------------------------------------------
    def make_stub(self, name, handler, nargs, cdecl=False):
        a = self.stub_next
        self.stub_next += 4
        self.stubs[a] = (name, handler, nargs, cdecl)
        return a

    def api_stub(self, dll, name):
        key = name.split("@")[0].lstrip("_") if dll == "mss32.dll" else name
        for a, s in self.stubs.items():
            if s[0] == key:
                return a
        handler = self.api_handlers.get(key)
        if handler is None and dll == "mss32.dll":
            handler = (lambda nm: (lambda *a: self.ail_default(nm, *a)))(key)
        nargs = None
        if "@" in name:
            nargs = int(name.split("@")[1]) // 4
        cdecl = getattr(handler, "cdecl", False)
        if handler is not None and nargs is None:
            nargs = 0 if cdecl else len(inspect.signature(handler).parameters)
        return self.make_stub(key, handler, nargs or 0, cdecl)

    def arg(self, i):
        return self.r32(self.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * i)

    def on_stub(self, uc, addr, size, _):
        if self.stop_reason:
            uc.emu_stop()
            return
        self.stub_calls += 1
        if self.stub_calls & 0xFFF == 0 and time.time() - self.wall_start > self.args.wall_limit:
            return self.fail("wall-clock limit (frame %d)" % self.serial)
        if addr == self.cb_ret:
            eax = uc.reg_read(UC_X86_REG_EAX)
            ra, then = self.cb_stack.pop()
            uc.reg_write(UC_X86_REG_EAX, (then(eax) if then else eax) & 0xFFFFFFFF)
            uc.reg_write(UC_X86_REG_EIP, ra)
            return
        stub = self.stubs.get(addr)
        if stub is None:
            return self.fail("jump into stub area %#x" % addr)
        name, handler, nargs, cdecl = stub
        esp = uc.reg_read(UC_X86_REG_ESP)
        ra = self.r32(esp)
        if handler is None:
            vals = [self.r32(esp + 4 + 4 * i) for i in range(8)]
            return self.fail("unimplemented %s(%s) called from %#x" % (name, ", ".join("%#x" % v for v in vals), ra))
        args = [self.r32(esp + 4 + 4 * i) for i in range(nargs)]
        self.api_calls[name] += 1
        try:
            r = handler(*args)
        except Stop:
            uc.emu_stop()
            return
        except Exception as e:  # a bug in the emulation: stop with context
            import traceback
            self.log(traceback.format_exc())
            return self.fail("%s(%s) from %#x raised %r" % (name, ", ".join("%#x" % v for v in args), ra, e))
        if self.trace or (self.args.trace_from and self.serial >= self.args.trace_from):
            self.log("%s(%s) -> %s  [ra %#x]" % (name, ", ".join("%#x" % v for v in args),
                                                 r if not isinstance(r, int) else "%#x" % (r & 0xFFFFFFFF), ra))
        pop = 4 if cdecl else 4 + 4 * nargs
        if isinstance(r, tuple) and r and r[0] == "call":
            _, fn, cargs, then = r
            sp = esp + pop
            for v in reversed(cargs):
                sp -= 4
                self.w32(sp, v)
            sp -= 4
            self.w32(sp, self.cb_ret)
            self.cb_stack.append((ra, then))
            uc.reg_write(UC_X86_REG_ESP, sp)
            uc.reg_write(UC_X86_REG_EIP, fn)
            return
        uc.reg_write(UC_X86_REG_EAX, (r or 0) & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + pop)
        uc.reg_write(UC_X86_REG_EIP, ra)
        if self.stop_reason:
            uc.emu_stop()

    def on_time(self, uc, addr, size, _):
        """CRT time(NULL) (FUN_0047ac1b): fixed, as s2run's Seeder::fixed."""
        esp = uc.reg_read(UC_X86_REG_ESP)
        ra, p = self.r32(esp), self.r32(esp + 4)
        v = self.args.time_seed
        if p:
            self.w32(p, v)
        uc.reg_write(UC_X86_REG_EAX, v)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ra)

    def on_watch(self, uc, addr, size, _):
        esp = uc.reg_read(UC_X86_REG_ESP)
        vals = [self.r32(esp + 4 * i) for i in range(5)]
        self.log("watch FUN_%08x(%s) ecx=%#x from %#x" % (addr, ", ".join("%#x" % v for v in vals[1:]),
                                                         uc.reg_read(UC_X86_REG_ECX), vals[0]))

    def on_watch_write(self, uc, access, addr, size, value, watched):
        if not addr <= watched < addr + size:
            return
        self.log("write %#x <- %#x (%d bytes) at eip=%#x  chain %s" % (
            addr, value, size, uc.reg_read(UC_X86_REG_EIP), " ".join("%x" % a for a in self.backtrace(6))))

    def on_fault(self, uc, access, addr, size, value, _):
        eip = uc.reg_read(UC_X86_REG_EIP)
        self.fail("memory fault at eip=%#x addr=%#x size=%d access=%d" % (eip, addr, size, access))
        return False

    def on_intr(self, uc, intno, _):
        self.fail("interrupt %d at eip=%#x" % (intno, uc.reg_read(UC_X86_REG_EIP)))

    def on_exit(self):
        self.log("process returned from entry point")
        self.stop_reason = "process returned"
        raise Stop

    def run(self):
        # One emu_start without a timeout: emulation stops only for a stop
        # reason. A restart (as after a timeout) can resume on an instruction
        # whose code hook already ran, and a --watch hook then logs one call
        # twice. The wall-clock limit is checked in the API stub hook instead
        # (every game loop calls the API: the clock, the message pump).
        start = self.wall_start = time.time()
        try:
            self.uc.emu_start(self.entry, 0)
        except UcError as e:
            if not self.stop_reason:
                self.fail("emulation error %s at eip=%#x" % (e, self.uc.reg_read(UC_X86_REG_EIP)))
        if not self.stop_reason:
            self.fail("emulation ended at eip=%#x" % self.uc.reg_read(UC_X86_REG_EIP))
        self.log("stopped: %s after %d frames, %d ms virtual, %.1f s" % (self.stop_reason, self.serial, self.now,
                                                                           time.time() - start))
        self.log("api calls: " + ", ".join("%s=%d" % kv for kv in self.api_calls.most_common()))
        return self.exit_code

    # ---- clock and script ----------------------------------------------------------------------------------
    def backtrace(self, depth=8):
        """Return addresses up the EBP chain (the original keeps frame pointers)."""
        out = []
        ebp = self.uc.reg_read(UC_X86_REG_EBP)
        for _ in range(depth):
            if not (STACK_LO <= ebp < STACK_HI - 8):
                break
            out.append(self.r32(ebp + 4))
            ebp = self.r32(ebp)
        return out

    def present(self, surf):
        if self.args.log_presents:
            self.log("present %d: %s" % (self.serial + 1, " ".join("%x" % a for a in self.backtrace())))
        self.frame = self.rb(surf.mem, surf.pitch * surf.h)[:SCREEN_W * SCREEN_H * 2]
        self.frame_idx = self.capture_indexed()
        self.now += FRAME_MS
        self.last_frame = self.now
        self.spin = {}
        self.serial += 1
        self.frame_hook(True)

    def poll(self):
        """The program read the clock. Time passes only in a loop that does
        nothing but read it (a busy wait): once one call site has read it
        SPIN_FREE times without a present, a message or an idle input poll,
        each further read from that site advances 1 ms. s2run jumps straight
        to the end of such a wait; both end on the same millisecond."""
        # Count per call site (the return chain above the clock read): only a
        # site that keeps polling is a busy wait. A read from elsewhere right
        # after a wait has ended must not move time on, nor a read outside the
        # clock waits (FUN_00460fd0 cycles the palette once per map cell).
        site = tuple(self.backtrace(3))
        if not any(CLOCK_WAITS[0] <= a < CLOCK_WAITS[1] for a in site):
            return
        n = self.spin.get(site, 0) + 1
        self.spin[site] = n
        if n > SPIN_FREE:
            self.advance(self.now + 1)

    def input_idle(self):
        """s2run's Sys::input_get idles 1 ms when it pumps nothing and finds no
        matching event; the original's equivalent is FUN_00474aac (except
        inside FUN_00474c54, the flush, which s2run does not idle in). Called
        when the pump's PeekMessageA finds the queue empty."""
        ebp0 = self.uc.reg_read(UC_X86_REG_EBP)
        if not (STACK_LO <= ebp0 < STACK_HI - 8):
            return
        ebp1 = self.r32(ebp0)
        ra0 = self.r32(ebp0 + 4)
        if not (INPUT_GET <= ra0 < INPUT_PEEK) or not (STACK_LO <= ebp1 < STACK_HI - 16):
            return
        ra1 = self.r32(ebp1 + 4)
        if INPUT_FLUSH <= ra1 < INPUT_WAIT:
            return
        mask = self.r32(ebp1 + 0xC)
        sysobj = self.r32(0x4A2BA0)
        n = self.r32(sysobj + 0x10)
        if any(self.r32(sysobj + 0x1C + 0x24 * i) & mask for i in range(min(n, 100))):
            return
        self.spin = {}
        self.advance(self.now + 1)

    def advance(self, target):
        while target > self.now:
            frame_at = self.last_frame + FRAME_MS
            if frame_at > target:
                self.now = target
                return
            self.now = self.last_frame = frame_at
            self.serial += 1
            self.frame_hook()

    def frame_hook(self, presented=False):
        if self.frame_log:
            self.frame_log.write("%d %d%s\n" % (self.serial, self.now, " P" if presented else ""))
        while self.pc < len(self.steps) and self.serial >= self.wait_until:
            st = self.steps[self.pc]
            op = st[0]
            if op == "frames":
                self.wait_until = self.serial + st[1]
            elif op == "key":
                self.key_event(st[1], True)
                self.key_event(st[1], False)
            elif op == "hold":
                self.key_event(st[1], True)
            elif op == "up":
                self.key_event(st[1], False)
            elif op == "move":
                self.set_mouse(st[1], st[2])
            elif op == "click":
                self.set_mouse(st[2], st[3])
                self.button_event(st[1], True)
                self.button_event(st[1], False)
            elif op == "shot":
                self.save_shot(st[1])
            elif op == "quit":
                self.stop_reason = "quit"
            self.pc += 1
        if self.pc >= len(self.steps) and self.serial >= self.wait_until and not self.stop_reason:
            self.stop_reason = "script done"
        if self.args.max_frames and self.serial >= self.args.max_frames and not self.stop_reason:
            self.stop_reason = "max frames"
        if self.stop_reason:
            self.uc.emu_stop()

    def save_shot(self, name):
        global RGB565
        if RGB565 is None:
            RGB565 = _expand565()
        px = array.array("H")
        px.frombytes(self.frame)
        rgb = b"".join([RGB565[v] for v in px])
        p = os.path.join(self.out, name + ".png")
        write_png(p, SCREEN_W, SCREEN_H, rgb)
        if self.frame_idx:
            idx, pal = self.frame_idx
            write_png(os.path.join(self.out, name + ".idx.png"), SCREEN_W, SCREEN_H, idx, pal)
        if self.args.peek:
            with open(os.path.join(self.out, name + ".peek.txt"), "w") as f:
                for a, n in self.args.peek:
                    a = self.r32(a & 0x7FFFFFFF) if a & 0x80000000 else a  # high bit: dereference
                    data = self.rb(a, n).hex(" ") if self.mapped(a, n) else "(unmapped)"
                    f.write("%08x: %s\n" % (a, data))
        if self.args.dump_units:
            with open(os.path.join(self.out, name + ".units.txt"), "w") as f:
                f.write(self.units_text())
        print("frame %d t=%dms -> %s" % (self.serial, self.now, p))
        self.log("shot %s" % name)

    def units_text(self):
        """Both armies' unit records (docs/structs.md "Army"/"Unit"): the state behind a shot."""
        lines = []
        for side, army in (("player", 0x49F9F8), ("enemy", 0x4976C8)):
            for i in range(50):
                u = self.r32(army + 4 + 4 * i)
                if not u:
                    continue
                idx, char, kind = struct.unpack("<HHH", self.rb(u, 6))
                x, y = struct.unpack("<ii", self.rb(u + 0x23, 8))
                facing, frame, act = self.rb(u + 0x2B, 3)
                flags = self.r32(u + 0x30)
                name = self.cstr(u + 0x10, 15).decode("cp950", "replace")
                lines.append("%s %2d @%#x char %3d kind %3d at (%d,%d) facing %#x frame %d act %d flags %#010x %s"
                             % (side, idx, u, char, kind, x, y, facing, frame, act, flags, name))
        return "\n".join(lines) + "\n"

    def capture_indexed(self):
        """The platform layer's 8-bit shadow frame and palette (DAT_004a2ba0 + 0x1030: +8 palette, +0x618 page)."""
        sysobj = self.r32(0x4A2BA0)
        if not sysobj:
            return None
        pal_obj = sysobj + 0x1030
        try:
            pal = self.rb(pal_obj + 8, 0x400)
            idx = self.rb(pal_obj + 0x618, SCREEN_W * SCREEN_H)
        except UcError:
            return None
        return idx, b"".join(bytes((pal[i * 4 + 2], pal[i * 4 + 1], pal[i * 4])) for i in range(256))  # BGRX

    # ---- input ---------------------------------------------------------------------------------------------
    def post(self, msg, wparam, lparam):
        self.msgs.append([HWND_MAIN, msg, wparam, lparam, self.now, self.mouse[0], self.mouse[1]])

    def key_event(self, k, down):
        sc = scan_code(k)
        was = self.keys[k]
        self.keys[k] = down
        alt = self.keys[VK_MENU]
        sys_msg = alt or k == VK_MENU or k == 0x79
        lp = 1 | sc << 16
        if k in EXTENDED_KEYS:
            lp |= 1 << 24
        if alt and k != VK_MENU or (k == VK_MENU and down):
            lp |= 1 << 29
        if not down:
            lp |= 3 << 30
        elif was:  # auto-repeat: the key was already down
            lp |= 1 << 30
        if down:
            self.post(WM_SYSKEYDOWN if sys_msg else WM_KEYDOWN, k, lp)
        else:
            self.post(WM_SYSKEYUP if sys_msg else WM_KEYUP, k, lp)

    def mk_flags(self):
        f = (1 if self.buttons[0] else 0) | (2 if self.buttons[1] else 0)
        if self.keys[VK_SHIFT]:
            f |= 4
        if self.keys[VK_CONTROL]:
            f |= 8
        return f

    def set_mouse(self, x, y):
        p = (min(max(x, 0), SCREEN_W - 1), min(max(y, 0), SCREEN_H - 1))
        if p == self.mouse:
            return
        self.mouse = p
        lp = (p[1] & 0xFFFF) << 16 | (p[0] & 0xFFFF)
        if self.msgs and self.msgs[-1][1] == WM_MOUSEMOVE:
            m = self.msgs[-1]
            m[2], m[3], m[4], m[5], m[6] = self.mk_flags(), lp, self.now, p[0], p[1]
        else:
            self.post(WM_MOUSEMOVE, self.mk_flags(), lp)

    def button_event(self, b, down):
        self.buttons[b] = down
        x, y = self.mouse
        lp = (y & 0xFFFF) << 16 | (x & 0xFFFF)
        if down:
            msg = WM_LBUTTONDOWN if b == 0 else WM_RBUTTONDOWN
            lc = self.last_click
            if (self.class_style & 8 and lc and lc[0] == b and self.now - lc[1] <= 500 and abs(lc[2] - x) <= 2
                    and abs(lc[3] - y) <= 2):
                msg = WM_LBUTTONDBLCLK if b == 0 else WM_RBUTTONDBLCLK
                self.last_click = None
            else:
                self.last_click = (b, self.now, x, y)
        else:
            msg = WM_LBUTTONUP if b == 0 else WM_RBUTTONUP
        self.post(msg, self.mk_flags(), lp)

    def write_msg(self, p, m):
        self.wb(p, struct.pack("<IIIIIii", *m))

    # ---- kernel32: process, environment, version --------------------------------------------------------
    def api_GetVersion(self):
        return 0xC0000A04  # Windows 98

    def api_GetVersionExA(self, p):
        size = self.r32(p)
        self.wb(p + 4, struct.pack("<IIII", 4, 10, 0x040A07CE, 1) + b" A".ljust(128, b"\0"))
        return 1 if size >= 0x94 else 0

    def api_GetCommandLineA(self):
        if not hasattr(self, "cmdline"):
            self.cmdline = self.misc(b"WSUPERSP.EXE\0")
        return self.cmdline

    def api_GetStartupInfoA(self, p):
        self.wb(p, struct.pack("<I", 0x44) + bytes(0x40))

    def api_GetEnvironmentStrings(self):
        return self.misc(b"\0\0")

    def api_GetEnvironmentStringsW(self):
        return self.misc(b"\0\0\0\0")

    def api_FreeEnvironmentStringsA(self, p):
        return 1

    def api_FreeEnvironmentStringsW(self, p):
        return 1

    def api_GetEnvironmentVariableA(self, name, buf, n):
        self.last_error = 203
        return 0

    def api_SetEnvironmentVariableA(self, name, value):
        return 1

    def api_GetModuleHandleA(self, p):
        if p == 0:
            return self.base
        return self.module_handle(self.cstr(p).decode("latin1"))

    def module_handle(self, name):
        name = name.lower()
        if not name.endswith(".dll"):
            name += ".dll"
        if not hasattr(self, "modules"):
            self.modules = {}
        if name not in self.modules:
            self.modules[name] = 0x60000000 + 0x10000 * (len(self.modules) + 1)
        return self.modules[name]

    def api_LoadLibraryA(self, p):
        name = self.cstr(p).decode("latin1")
        self.log("LoadLibraryA(%s) -> fake module (nothing is loaded)" % name)
        return self.module_handle(name)

    def api_GetProcAddress(self, h, p):
        name = ("#%d" % p) if p < 0x10000 else self.cstr(p).decode("latin1")
        dll = next((n for n, v in getattr(self, "modules", {}).items() if v == h), "kernel32.dll")
        return self.api_stub(dll, name)

    def api_IsProcessorFeaturePresent(self, f):
        return 0

    def api_GetModuleFileNameA(self, h, buf, n):
        s = MODULE_PATH.encode()[:max(n - 1, 0)]
        self.wb(buf, s + b"\0")
        return len(s)

    def api_GetCurrentProcess(self):
        return 0xFFFFFFFF

    def api_GetCurrentProcessId(self):
        return 0x100

    def api_TerminateProcess(self, h, code):
        self.log("TerminateProcess(%d)" % code)
        self.stop_reason = "process exit %d" % code
        raise Stop

    def api_ExitProcess(self, code):
        self.log("ExitProcess(%d)" % code)
        self.stop_reason = "process exit %d" % code
        raise Stop

    def api_SetUnhandledExceptionFilter(self, f):
        return 0

    def api_UnhandledExceptionFilter(self, p):
        self.fail("UnhandledExceptionFilter called")
        raise Stop

    def api_RaiseException(self, code, flags, n, args):
        self.fail("RaiseException(%#x)" % code)
        raise Stop

    def api_RtlUnwind(self, frame, target, rec, value):
        # Only reached through C++/SEH unwinding, which a normal run never does.
        self.fail("RtlUnwind")
        raise Stop

    def api_GetLastError(self):
        return self.last_error

    def api_SetHandleCount(self, n):
        return n

    def api_GetStdHandle(self, n):
        return 0

    def api_SetStdHandle(self, n, h):
        return 1

    def api_GetFileType(self, h):
        return 1 if h in self.handles else 0

    def api_OutputDebugStringA(self, p):
        self.log("OutputDebugString: %s" % self.cstr(p).decode("cp950", "replace").rstrip())

    def api_GlobalMemoryStatus(self, p):
        self.wb(p, struct.pack("<IIIIIIII", 32, 50, 64 << 20, 32 << 20, 128 << 20, 96 << 20, 0x7FFE0000,
                               0x70000000))

    def api_GetACP(self):
        return 950

    def api_GetOEMCP(self):
        return 950

    def api_GetCPInfo(self, cp, p):
        self.wb(p, struct.pack("<I", 2) + b"?\0" + bytes([0x81, 0xFE]) + bytes(10))
        return 1

    def api_IsBadReadPtr(self, p, n):
        return 0 if self.mapped(p, n) else 1

    def api_IsBadWritePtr(self, p, n):
        return 0 if self.mapped(p, n) else 1

    def api_IsBadCodePtr(self, p):
        return 0 if self.mapped(p, 1) else 1

    # ---- kernel32: strings ---------------------------------------------------------------------------------
    @staticmethod
    def codec(cp):
        return "cp950" if cp in (0, 1, 3, 950) else ("cp%d" % cp)

    def api_MultiByteToWideChar(self, cp, flags, src, n, dst, dn):
        data = self.cstr(src) + b"\0" if n == 0xFFFFFFFF else self.rb(src, n)
        s = data.decode(self.codec(cp), "replace").encode("utf-16-le")
        units = len(s) // 2
        if dn == 0:
            return units
        if units > dn:
            self.last_error = 122
            return 0
        self.wb(dst, s)
        return units

    def api_WideCharToMultiByte(self, cp, flags, src, n, dst, dn, defchar, used):
        s = self.wstr(src) + "\0" if n == 0xFFFFFFFF else self.wstr(src, n)
        data = s.encode(self.codec(cp), "replace")
        if used:
            self.w32(used, 0)
        if dn == 0:
            return len(data)
        if len(data) > dn:
            self.last_error = 122
            return 0
        self.wb(dst, data)
        return len(data)

    @staticmethod
    def ctype1(ch):
        o = ord(ch)
        f = 0
        if o < 0x80:
            c = chr(o)
            if c.isupper():
                f |= 0x1 | 0x100
            if c.islower():
                f |= 0x2 | 0x100
            if c.isdigit():
                f |= 0x4
            if c in " \t\n\r\v\f":
                f |= 0x8
            if c in " \t":
                f |= 0x40
            if 0x21 <= o <= 0x7E and not c.isalnum():
                f |= 0x10
            if o < 0x20 or o == 0x7F:
                f |= 0x20
            if c in "0123456789abcdefABCDEF":
                f |= 0x80
            return f
        cat = unicodedata.category(ch)
        if cat == "Lu":
            f |= 0x101
        elif cat == "Ll":
            f |= 0x102
        elif cat.startswith("L"):
            f |= 0x100
        elif cat == "Nd":
            f |= 0x4
        elif cat.startswith("P") or cat.startswith("S"):
            f |= 0x10
        elif cat == "Zs":
            f |= 0x48
        elif cat.startswith("C"):
            f |= 0x20
        return f

    def api_GetStringTypeW(self, kind, src, n, out):
        s = self.wstr(src) if n == 0xFFFFFFFF else self.wstr(src, n)
        if kind != 1:
            self.wb(out, bytes(2 * len(s)))
            return 1
        self.wb(out, b"".join(struct.pack("<H", self.ctype1(c)) for c in s))
        return 1

    def api_GetStringTypeA(self, lcid, kind, src, n, out):
        data = self.cstr(src) if n == 0xFFFFFFFF else self.rb(src, n)
        types = [self.ctype1(chr(b)) if b < 0x80 else 0 for b in data]
        self.wb(out, b"".join(struct.pack("<H", t if kind == 1 else 0) for t in types))
        return 1

    def api_LCMapStringW(self, lcid, flags, src, n, dst, dn):
        s = self.wstr(src) + "\0" if n == 0xFFFFFFFF else self.wstr(src, n)
        if flags & 0x200:
            s = "".join(c.upper() if ord(c) < 0x80 else c for c in s)
        elif flags & 0x100:
            s = "".join(c.lower() if ord(c) < 0x80 else c for c in s)
        if dn == 0:
            return len(s)
        self.wb(dst, s[:dn].encode("utf-16-le"))
        return min(len(s), dn)

    def api_LCMapStringA(self, lcid, flags, src, n, dst, dn):
        data = self.cstr(src) + b"\0" if n == 0xFFFFFFFF else self.rb(src, n)
        if flags & 0x200:
            data = bytes(b - 32 if 0x61 <= b <= 0x7A else b for b in data)
        elif flags & 0x100:
            data = bytes(b + 32 if 0x41 <= b <= 0x5A else b for b in data)
        if dn == 0:
            return len(data)
        self.wb(dst, data[:dn])
        return min(len(data), dn)

    def api_CompareStringA(self, lcid, flags, s1, n1, s2, n2):
        a = self.cstr(s1) if n1 == 0xFFFFFFFF else self.rb(s1, n1)
        b = self.cstr(s2) if n2 == 0xFFFFFFFF else self.rb(s2, n2)
        if flags & 1:
            a, b = a.lower(), b.lower()
        return 1 if a < b else (2 if a == b else 3)

    def api_CompareStringW(self, lcid, flags, s1, n1, s2, n2):
        a = self.wstr(s1) if n1 == 0xFFFFFFFF else self.wstr(s1, n1)
        b = self.wstr(s2) if n2 == 0xFFFFFFFF else self.wstr(s2, n2)
        if flags & 1:
            a, b = a.lower(), b.lower()
        return 1 if a < b else (2 if a == b else 3)

    # ---- kernel32: memory ----------------------------------------------------------------------------------
    def api_HeapCreate(self, opts, init, mx):
        return 0x00500000

    def api_HeapDestroy(self, h):
        return 1

    def api_HeapAlloc(self, h, flags, n):
        return self.alloc(n, zero=bool(flags & 8))

    def api_HeapFree(self, h, flags, p):
        return 1 if (p == 0 or self.free(p)) else 0

    def api_HeapReAlloc(self, h, flags, p, n):
        old = self.heap_sizes.get(p)
        if old is None:
            return 0
        if n <= old:
            return p
        if flags & 0x10:  # HEAP_REALLOC_IN_PLACE_ONLY
            return 0
        q = self.alloc(n, zero=bool(flags & 8))
        self.wb(q, self.rb(p, old))
        self.free(p)
        return q

    def api_HeapSize(self, h, flags, p):
        n = self.heap_sizes.get(p)
        return 0xFFFFFFFF if n is None else n

    def api_VirtualAlloc(self, addr, n, kind, prot):
        n = (n + 0xFFF) & ~0xFFF
        if addr == 0:
            a = self.valloc_next
            self.valloc_next = (a + n + 0xFFFF) & ~0xFFFF
            if self.valloc_next > VALLOC_HI:
                self.last_error = 8
                return 0
            return a
        if not self.mapped(addr, n):
            self.last_error = 487
            return 0
        return addr & ~0xFFF  # committing a reserved range: it is mapped already

    def api_VirtualFree(self, addr, n, kind):
        if kind & 0x4000 and n:  # MEM_DECOMMIT
            self.wb(addr, bytes(n))
        return 1

    # ---- kernel32: time ------------------------------------------------------------------------------------
    def systemtime(self):
        ms = self.now
        s, ms = divmod(ms, 1000)
        m, s = divmod(s, 60)
        h, m = divmod(m, 60)
        d, h = divmod(h, 24)
        # Monday 1 June 1998 + d days (the day never matters for the shots).
        return struct.pack("<8H", 1998, 6, (1 + d) % 7, 1 + d, h, m, s, ms)

    def api_GetLocalTime(self, p):
        self.poll()
        self.wb(p, self.systemtime())

    def api_GetSystemTime(self, p):
        self.poll()
        self.wb(p, self.systemtime())

    def api_timeGetTime(self):
        self.poll()
        return self.now

    def api_GetTimeZoneInformation(self, p):
        self.wb(p, bytes(172))
        return 0

    # ---- kernel32: synchronisation (OutputDebugString's DBWIN protocol) -----------------------------------
    def api_OpenEventA(self, access, inherit, name):
        return 0

    def api_SetEvent(self, h):
        return 1

    def api_WaitForSingleObject(self, h, ms):
        return 0

    def api_CreateFileMappingA(self, h, sa, prot, hi, lo, name):
        return 0

    def api_MapViewOfFile(self, h, access, hi, lo, n):
        return 0

    # ---- kernel32: files -----------------------------------------------------------------------------------
    @staticmethod
    def find_ci(d, name):
        p = os.path.join(d, name)
        if os.path.exists(p):
            return p
        try:
            for n in os.listdir(d):
                if n.lower() == name.lower():
                    return os.path.join(d, n)
        except OSError:
            pass
        return None

    def resolve(self, raw):
        """Guest path -> (host path or None, writable, relative components)."""
        s = raw.decode("cp950", "replace").replace("/", "\\")
        if len(s) >= 2 and s[1] == ":":
            if not (s.upper() + "\\").startswith(INSTALL_DIR.upper() + "\\"):
                return None, False, None
            s = s[len(INSTALL_DIR):]
        parts = []
        for c in s.split("\\"):
            if c in ("", "."):
                continue
            if c == "..":
                if not parts:
                    return None, False, None
                parts.pop()
            else:
                parts.append(c)
        writable = bool(parts) and (parts[0].upper() == "SAVE" or [p.upper() for p in parts] == ["SUPERSP.CFG"])
        root = self.scratch if writable else self.game
        cur = root
        for i, c in enumerate(parts):
            nxt = self.find_ci(cur, c)
            if nxt is None:
                return os.path.join(cur, *parts[i:]), writable, parts
            cur = nxt
        return cur, writable, parts

    def new_handle(self, obj):
        h = self.next_handle
        self.next_handle += 4
        self.handles[h] = obj
        return h

    def api_CreateFileA(self, name, access, share, sa, disp, flags, tmpl):
        raw = self.cstr(name)
        path, writable, parts = self.resolve(raw)
        want_write = bool(access & 0x40000000) or disp in (1, 2, 4, 5)
        if path is None or (want_write and not writable):
            self.log("CreateFileA(%r, %#x, disp %d): denied (sandbox)" % (raw, access, disp))
            self.last_error = 5 if path is not None else 3
            return 0xFFFFFFFF
        exists = os.path.isfile(path)
        if disp == 1 and exists:
            self.last_error = 80
            return 0xFFFFFFFF
        if disp in (3, 5) and not exists:
            self.last_error = 2
            if self.trace:
                self.log("CreateFileA(%r): not found" % raw)
            return 0xFFFFFFFF
        if want_write:
            mode = "w+b" if disp in (1, 2, 5) or not exists else "r+b"
        else:
            mode = "rb"
        f = open(path, mode)
        self.last_error = 183 if exists and disp in (2, 4) else 0
        h = self.new_handle(HostFile(f, path, want_write))
        self.log("CreateFileA(%r, %#x, disp %d) -> %#x%s" % (raw, access, disp, h, " (scratch)" if writable else ""))
        return h

    def api_CloseHandle(self, h):
        o = self.handles.pop(h, None)
        if isinstance(o, HostFile):
            o.f.close()
        return 1 if o is not None else 0

    def api_ReadFile(self, h, buf, n, pread, ov):
        o = self.handles.get(h)
        if not isinstance(o, HostFile):
            self.last_error = 6
            return 0
        data = o.f.read(n)
        if data:
            self.wb(buf, data)
        if pread:
            self.w32(pread, len(data))
        return 1

    def api_WriteFile(self, h, buf, n, pwritten, ov):
        o = self.handles.get(h)
        data = self.rb(buf, n)
        if not isinstance(o, HostFile) or not o.writable:
            self.log("WriteFile(%#x): %r" % (h, data[:200]))
            if pwritten:
                self.w32(pwritten, n)
            return 1 if h in (0, 1, 2) or o is None else 0
        o.f.write(data)
        if pwritten:
            self.w32(pwritten, n)
        return 1

    def api_SetFilePointer(self, h, lo, phigh, method):
        o = self.handles.get(h)
        if not isinstance(o, HostFile):
            self.last_error = 6
            return 0xFFFFFFFF
        off = lo - (1 << 32) if lo & 0x80000000 and not phigh else lo
        if phigh:
            off = (self.r32(phigh) << 32 | lo)
            if off & (1 << 63):
                off -= 1 << 64
        base = {0: 0, 1: o.f.tell(), 2: os.fstat(o.f.fileno()).st_size}.get(method, 0)
        if base + off < 0:
            self.last_error = 131  # ERROR_NEGATIVE_SEEK
            return 0xFFFFFFFF
        o.f.seek(base + off)
        pos = o.f.tell()
        if phigh:
            self.w32(phigh, pos >> 32)
        return pos & 0xFFFFFFFF

    def api_SetEndOfFile(self, h):
        o = self.handles.get(h)
        if isinstance(o, HostFile) and o.writable:
            o.f.truncate()
            return 1
        return 0

    def api_FlushFileBuffers(self, h):
        o = self.handles.get(h)
        if isinstance(o, HostFile):
            o.f.flush()
        return 1

    def api_GetFileAttributesA(self, name):
        path, _, _ = self.resolve(self.cstr(name))
        if path is None or not os.path.exists(path):
            self.last_error = 2
            return 0xFFFFFFFF
        return 0x10 if os.path.isdir(path) else 0x20

    def find_data(self, p, path):
        st = os.stat(path)
        ft = 0x01BD8D0000000000  # 1998
        name = os.path.basename(path).encode("cp950", "replace")
        self.wb(p, struct.pack("<IQQQIIII", 0x10 if os.path.isdir(path) else 0x20, ft, ft, ft, 0,
                               st.st_size & 0xFFFFFFFF, 0, 0) + name[:259].ljust(260, b"\0") + bytes(14))

    def api_FindFirstFileA(self, pattern, p):
        raw = self.cstr(pattern)
        s = raw.replace(b"/", b"\\")
        i = s.rfind(b"\\")
        d, pat = (s[:i], s[i + 1:]) if i >= 0 else (b"", s)
        dpath, _, _ = self.resolve(d) if d else (self.game, False, [])
        names = []
        if dpath and os.path.isdir(dpath):
            pat_s = pat.decode("cp950", "replace").lower()
            names = sorted(os.path.join(dpath, n) for n in os.listdir(dpath)
                           if fnmatch.fnmatchcase(n.lower(), pat_s.replace("*.*", "*")))
        self.log("FindFirstFileA(%r): %d matches" % (raw, len(names)))
        if not names:
            self.last_error = 2
            return 0xFFFFFFFF
        self.find_data(p, names[0])
        return self.new_handle(collections.deque(names[1:]))

    def api_FindNextFileA(self, h, p):
        q = self.handles.get(h)
        if not q:
            self.last_error = 18
            return 0
        self.find_data(p, q.popleft())
        return 1

    # ---- advapi32 ------------------------------------------------------------------------------------------
    def api_RegOpenKeyExA(self, key, sub, opts, sam, out):
        self.log("RegOpenKeyExA(%#x, %r)" % (key, self.cstr(sub)))
        self.w32(out, 0x00007001)
        return 0

    def api_RegQueryValueExA(self, key, name, res, kind, data, size):
        return 2  # ERROR_FILE_NOT_FOUND: the AutoRun value keeps its default

    def api_RegSetValueExA(self, key, name, res, kind, data, size):
        self.log("RegSetValueExA(%r) ignored (sandbox)" % self.cstr(name))
        return 0

    def api_RegFlushKey(self, key):
        return 0

    def api_RegCloseKey(self, key):
        return 0

    # ---- user32: windows -----------------------------------------------------------------------------------
    def api_RegisterClassA(self, p):
        self.class_style = self.r32(p)
        self.wndproc = self.r32(p + 4)
        self.log("RegisterClassA: style %#x wndproc %#x" % (self.class_style, self.wndproc))
        return 0xC001

    def api_CreateWindowExA(self, ex, cls, title, style, x, y, w, h, parent, menu, inst, param):
        self.log("CreateWindowExA(%r, style %#x, %d x %d)" % (self.cstr(title), style, w, h))
        return HWND_MAIN

    def api_LoadIconA(self, inst, name):
        return 0x00020001

    def api_GetStockObject(self, i):
        return 0x00030000 + i

    def api_ShowWindow(self, h, cmd):
        return 0

    def api_UpdateWindow(self, h):
        return 1

    def api_SetForegroundWindow(self, h):
        return 1

    def api_DestroyWindow(self, h):
        self.log("DestroyWindow, chain %s" % " ".join("%x" % a for a in self.backtrace(10)))
        self.stop_reason = "window destroyed"
        raise Stop

    def api_DefWindowProcA(self, h, msg, wp, lp):
        return 0

    def api_GetWindowLongA(self, h, i):
        return self.window_longs.get(i & 0xFFFFFFFF, {0xFFFFFFF0: 0x10CF0000, 0xFFFFFFEC: 0}.get(i, 0))

    def api_SetWindowLongA(self, h, i, v):
        old = self.api_GetWindowLongA(h, i)
        self.window_longs[i] = v
        return old

    def api_GetMenu(self, h):
        return 0

    def api_AdjustWindowRectEx(self, p, style, menu, ex):
        return 1

    def api_SetWindowPos(self, h, after, x, y, cx, cy, flags):
        return 1

    def api_MoveWindow(self, h, x, y, w, hh, repaint):
        return 1

    def rect(self, p, l, t, r, b):
        self.wb(p, struct.pack("<iiii", l, t, r, b))

    def api_GetWindowRect(self, h, p):
        self.rect(p, 0, 0, SCREEN_W, SCREEN_H)
        return 1

    def api_GetClientRect(self, h, p):
        self.rect(p, 0, 0, SCREEN_W, SCREEN_H)
        return 1

    def api_SetRect(self, p, l, t, r, b):
        self.wb(p, struct.pack("<IIII", l, t, r, b))
        return 1

    def api_ClientToScreen(self, h, p):
        return 1

    def api_ScreenToClient(self, h, p):
        return 1

    def api_GetSystemMetrics(self, i):
        return {0: SCREEN_W, 1: SCREEN_H}.get(i, 0)

    def api_SystemParametersInfoA(self, action, ui, p, win):
        if action == 0x30 and p:  # SPI_GETWORKAREA
            self.rect(p, 0, 0, SCREEN_W, SCREEN_H)
        return 1

    def api_BeginPaint(self, h, p):
        self.wb(p, struct.pack("<I", 0x00040001) + bytes(60))
        return 0x00040001

    def api_EndPaint(self, h, p):
        return 1

    def api_GetDC(self, h):
        return 0x00040002

    def api_ReleaseDC(self, h, dc):
        return 1

    def api_GetDeviceCaps(self, dc, i):
        return {8: SCREEN_W, 10: SCREEN_H, 12: 16, 14: 1}.get(i, 0)

    def api_MessageBoxA(self, h, text, cap, kind):
        self.log("MessageBoxA(%r, %r)" % (self.cstr(cap).decode("cp950", "replace"),
                                          self.cstr(text).decode("cp950", "replace")))
        print("oracle: MessageBox: %s" % self.cstr(text).decode("cp950", "replace"), file=sys.stderr)
        return 1

    # ---- user32: cursor ------------------------------------------------------------------------------------
    def api_ShowCursor(self, show):
        self.cursor_count += 1 if show else -1
        return self.cursor_count & 0xFFFFFFFF

    def api_SetCursor(self, c):
        return 0

    def api_LoadCursorFromFileA(self, p):
        self.log("LoadCursorFromFileA(%r)" % self.cstr(p))
        return 0x00050001

    def api_DestroyCursor(self, c):
        return 1

    def api_GetCursorPos(self, p):
        self.wb(p, struct.pack("<ii", *self.mouse))
        return 1

    def api_SetCursorPos(self, x, y):
        self.set_mouse(x, y)
        return 1

    def api_GetKeyState(self, k):
        return 0xFFFF8000 if self.sync_keys[k & 0xFF] else 0

    # ---- user32: messages ----------------------------------------------------------------------------------
    def api_PeekMessageA(self, p, h, lo, hi, remove):
        for i, m in enumerate(self.msgs):
            if (lo == 0 and hi == 0) or lo <= m[1] <= hi:
                break
        else:
            if remove & 1 and lo == 0 and hi == 0:
                self.input_idle()
            return 0
        self.write_msg(p, m)
        if self.trace or (self.args.trace_from and self.serial >= self.args.trace_from):
            self.log("message %#x wp %#x lp %#x%s" % (m[1], m[2], m[3], "" if remove & 1 else " (peek)"))
        if remove & 1:
            del self.msgs[i]
            self.spin = {}
            if m[1] in (WM_KEYDOWN, WM_SYSKEYDOWN):
                self.sync_keys[m[2]] = True
            elif m[1] in (WM_KEYUP, WM_SYSKEYUP):
                self.sync_keys[m[2]] = False
        return 1

    def api_TranslateMessage(self, p):
        _, msg, wp, lp = struct.unpack("<IIII", self.rb(p, 16))
        if msg not in (WM_KEYDOWN, WM_SYSKEYDOWN):
            return 0
        shift = self.sync_keys[VK_SHIFT]
        if 0x41 <= wp <= 0x5A:
            ch = wp if shift else wp + 0x20
        elif 0x30 <= wp <= 0x39 or wp in (0x0D, 0x1B, 0x20, 0x08, 0x09):
            ch = wp
        else:
            return 0
        self.msgs.appendleft([HWND_MAIN, WM_CHAR if msg == WM_KEYDOWN else WM_SYSCHAR, ch, lp, self.now,
                              self.mouse[0], self.mouse[1]])
        return 1

    def api_DispatchMessageA(self, p):
        hwnd, msg, wp, lp = struct.unpack("<IIII", self.rb(p, 16))
        if not self.wndproc:
            return 0
        return ("call", self.wndproc, [hwnd, msg, wp, lp], None)

    def api_wsprintfA(self, *_):
        esp = self.uc.reg_read(UC_X86_REG_ESP)
        buf, fmt = self.r32(esp + 4), self.r32(esp + 8)
        args = iter(range(esp + 12, esp + 12 + 4 * 64, 4))
        s = self.format(self.cstr(fmt), lambda: self.r32(next(args)))[:1023]
        self.wb(buf, s + b"\0")
        return len(s)

    api_wsprintfA.cdecl = True

    def api_wvsprintfA(self, buf, fmt, va):
        args = iter(range(va, va + 4 * 64, 4))
        s = self.format(self.cstr(fmt), lambda: self.r32(next(args)))[:1023]
        self.wb(buf, s + b"\0")
        return len(s)

    def format(self, fmt, next_arg):
        out = bytearray()
        i = 0
        while i < len(fmt):
            c = fmt[i]
            i += 1
            if c != 0x25:
                out.append(c)
                continue
            if i < len(fmt) and fmt[i] == 0x25:
                out.append(0x25)
                i += 1
                continue
            flags = ""
            while i < len(fmt) and chr(fmt[i]) in "-#0+ ":
                flags += chr(fmt[i])
                i += 1
            width = ""
            while i < len(fmt) and chr(fmt[i]).isdigit():
                width += chr(fmt[i])
                i += 1
            prec = None
            if i < len(fmt) and fmt[i] == 0x2E:
                i += 1
                p = ""
                while i < len(fmt) and chr(fmt[i]).isdigit():
                    p += chr(fmt[i])
                    i += 1
                prec = int(p or "0")
            while i < len(fmt) and chr(fmt[i]) in "lhwI":
                i += 1
            if i >= len(fmt):
                break
            conv = chr(fmt[i])
            i += 1
            if conv in "sS":
                s = self.cstr(next_arg())
                if prec is not None:
                    s = s[:prec]
            elif conv == "c":
                s = bytes([next_arg() & 0xFF])
            elif conv in "di":
                v = next_arg()
                v = v - (1 << 32) if v & 0x80000000 else v
                s = ("%" + flags.replace("#", "") + width + ("." + str(prec) if prec is not None else "") + "d"
                     ) % v
                s, width = s.encode(), ""
            elif conv in "uxXop":
                v = next_arg()
                spec = {"u": "d", "p": "X"}.get(conv, conv)
                s = ("%" + flags + width + ("." + str(prec) if prec is not None else "") + spec) % v
                s, width = s.encode(), ""
            else:
                s = b""
            if width and len(s) < int(width):
                pad = int(width) - len(s)
                s = s + b" " * pad if "-" in flags else (b"0" if "0" in flags else b" ") * pad + s
            out += s
        return bytes(out)

    # ---- gdi32 ---------------------------------------------------------------------------------------------
    def dc_surface(self, dc):
        return self.dcs.get(dc)

    def api_GetPixel(self, dc, x, y):
        s = self.dc_surface(dc)
        if s is None or not (0 <= x < s.w and 0 <= y < s.h):
            return 0xFFFFFFFF
        v = self.r16(s.mem + y * s.pitch + 2 * x)
        r, g, b = (v >> 11) & 31, (v >> 5) & 63, v & 31
        return (r << 3 | r >> 2) | (g << 2 | g >> 4) << 8 | (b << 3 | b >> 2) << 16

    def api_SetPixel(self, dc, x, y, color):
        s = self.dc_surface(dc)
        if s is None or not (0 <= x < s.w and 0 <= y < s.h):
            return 0xFFFFFFFF
        r, g, b = color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF
        self.w16(s.mem + y * s.pitch + 2 * x, (r >> 3) << 11 | (g >> 2) << 5 | b >> 3)
        return color

    def api_SetStretchBltMode(self, dc, mode):
        return 1

    def api_StretchDIBits(self, dc, dx, dy, dw, dh, sx, sy, sw, sh, bits, bmi, usage, rop):
        """Unstretched SRCCOPY of a BI_RGB DIB into a surface DC (the pixel-format probe)."""
        s = self.dc_surface(dc)
        _, w, h, _, bpp, comp = struct.unpack("<IiiHHI", self.rb(bmi, 20))
        if s is None or (dw, dh) != (sw, sh) or rop != 0xCC0020 or comp != 0 or bpp != 16:
            return self.fail("StretchDIBits(%d bpp, %dx%d -> %dx%d, rop %#x)" % (bpp, sw, sh, dw, dh, rop))
        stride = (w * 2 + 3) & ~3
        for y in range(sh):
            row_index = sy + y if h < 0 else abs(h) - 1 - (sy + y)
            src = array.array("H", self.rb(bits + row_index * stride + 2 * sx, 2 * sw))
            out = array.array("H", ((v >> 10 & 31) << 11 | ((v >> 5 & 31) << 1 | (v >> 9 & 1)) << 5 | (v & 31)
                                    for v in src))
            if 0 <= dy + y < s.h:
                self.wb(s.mem + (dy + y) * s.pitch + 2 * dx, out.tobytes()[:2 * max(0, s.w - dx)])
        return sh

    def api_SelectObject(self, dc, o):
        return 0x00030000

    def api_DeleteObject(self, o):
        return 1

    def api_CreatePen(self, style, w, color):
        return 0x00060001

    def api_MoveToEx(self, dc, x, y, p):
        return 1

    def api_LineTo(self, dc, x, y):
        return 1

    # ---- mss32 (Miles): silent success ---------------------------------------------------------------------
    def ail_default(self, name, *a):
        if name == "AIL_mem_alloc_lock":
            return self.alloc(a[0])
        if name == "AIL_mem_free_lock":
            self.free(a[0])
            return 0
        if name in ("AIL_waveOutOpen", "AIL_midiOutOpen"):
            if a[0]:
                self.w32(a[0], self.alloc(0x400, zero=True))
            if a[1]:
                self.w32(a[1], 0x00070001)
            return 0
        if name in ("AIL_allocate_sample_handle", "AIL_allocate_sequence_handle"):
            return self.alloc(0x400, zero=True)
        if name == "AIL_sample_status":
            return 2  # SMP_DONE
        if name in ("AIL_set_sample_file", "AIL_init_sequence"):
            return 1
        if name in ("AIL_redbook_open", "AIL_redbook_open_drive"):
            return 0  # no CD drive
        if name == "AIL_digital_configuration":
            if a[1]:
                self.w32(a[1], 22050)
            if a[2]:
                self.w32(a[2], 1)
            if a[3]:
                self.wb(a[3], b"oracle\0")
            return 0
        return 0

    # ---- ddraw ---------------------------------------------------------------------------------------------
    DD7 = ["QueryInterface", "AddRef", "Release", "Compact", "CreateClipper", "CreatePalette", "CreateSurface",
           "DuplicateSurface", "EnumDisplayModes", "EnumSurfaces", "FlipToGDISurface", "GetCaps", "GetDisplayMode",
           "GetFourCCCodes", "GetGDISurface", "GetMonitorFrequency", "GetScanLine", "GetVerticalBlankStatus",
           "Initialize", "RestoreDisplayMode", "SetCooperativeLevel", "SetDisplayMode", "WaitForVerticalBlank",
           "GetAvailableVidMem", "GetSurfaceFromDC", "RestoreAllSurfaces", "TestCooperativeLevel",
           "GetDeviceIdentifier", "StartModeTest", "EvaluateMode"]
    SURF7 = ["QueryInterface", "AddRef", "Release", "AddAttachedSurface", "AddOverlayDirtyRect", "Blt", "BltBatch",
             "BltFast", "DeleteAttachedSurface", "EnumAttachedSurfaces", "EnumOverlayZOrders", "Flip",
             "GetAttachedSurface", "GetBltStatus", "GetCaps", "GetClipper", "GetColorKey", "GetDC", "GetFlipStatus",
             "GetOverlayPosition", "GetPalette", "GetPixelFormat", "GetSurfaceDesc", "Initialize", "IsLost", "Lock",
             "ReleaseDC", "Restore", "SetClipper", "SetColorKey", "SetOverlayPosition", "SetPalette", "Unlock",
             "UpdateOverlay", "UpdateOverlayDisplay", "UpdateOverlayZOrder", "GetDDInterface", "PageLock",
             "PageUnlock", "SetSurfaceDesc", "SetPrivateData", "GetPrivateData", "FreePrivateData",
             "GetUniquenessValue", "ChangeUniquenessValue", "SetPriority", "GetPriority", "SetLOD", "GetLOD"]
    CLIP = ["QueryInterface", "AddRef", "Release", "GetClipList", "GetHWnd", "Initialize", "IsClipListChanged",
            "SetClipList", "SetHWnd"]

    def vtable(self, iface, methods, prefix):
        key = "_vt_" + iface
        if not hasattr(self, key):
            stubs = []
            for m in methods:
                h = getattr(self, prefix + m, None)
                n = len(inspect.signature(h).parameters) if h else 0
                stubs.append(self.make_stub("%s::%s" % (iface, m), h, n))
            vt = self.com_next
            self.com_next += 4 * len(stubs)
            self.wb(vt, b"".join(struct.pack("<I", s) for s in stubs))
            setattr(self, key, vt)
        return getattr(self, key)

    def com_object(self, iface, methods, prefix, obj, size=0x40):
        a = self.com_next
        self.com_next += size
        self.w32(a, self.vtable(iface, methods, prefix))
        self.com[a] = obj
        return a

    def api_DirectDrawCreateEx(self, guid, out, iid, outer):
        a = self.com_object("IDirectDraw7", self.DD7, "dd_", "ddraw")
        self.w32(out, a)
        self.log("DirectDrawCreateEx -> %#x" % a)
        return 0

    def dd_QueryInterface(self, this, iid, out):
        self.w32(out, this)
        return 0

    def dd_AddRef(self, this):
        return 2

    def dd_Release(self, this):
        return 0

    def dd_GetCaps(self, this, drv, hel):
        for p in (drv, hel):
            if p:
                n = self.r32(p)
                self.wb(p + 4, bytes(max(n - 4, 0)))
        return 0

    def dd_SetCooperativeLevel(self, this, hwnd, flags):
        self.log("SetCooperativeLevel(%#x)" % flags)
        self.coop = flags
        return 0

    def dd_SetDisplayMode(self, this, w, h, bpp, refresh, flags):
        self.log("SetDisplayMode(%d, %d, %d)" % (w, h, bpp))
        if (w, h, bpp) != (SCREEN_W, SCREEN_H, 16):
            self.fail("display mode %dx%dx%d not emulated" % (w, h, bpp))
        return 0

    def dd_RestoreDisplayMode(self, this):
        return 0

    def dd_WaitForVerticalBlank(self, this, flags, ev):
        return 0

    def dd_CreateClipper(self, this, flags, out, outer):
        self.w32(out, self.com_object("IDirectDrawClipper", self.CLIP, "clip_", "clipper"))
        return 0

    def clip_SetHWnd(self, this, flags, hwnd):
        return 0

    def clip_Release(self, this):
        return 0

    def clip_AddRef(self, this):
        return 2

    def new_surface(self, w, h, caps):
        a = self.com_object("IDirectDrawSurface7", self.SURF7, "surf_", None)
        s = Surface(self, w, h, caps, a)
        self.com[a] = s
        self.surfaces.append(s)
        return s

    def dd_CreateSurface(self, this, desc, out, outer):
        flags, h, w = struct.unpack("<III", self.rb(desc + 4, 12))
        backs = self.r32(desc + 0x14)
        caps = self.r32(desc + 0x68)
        if flags & 0x1000:  # DDSD_PIXELFORMAT
            bits = self.r32(desc + 0x48 + 0xC)
            if bits != 16:
                return self.fail("CreateSurface with %d-bit pixel format" % bits)
        if caps & 0x200:  # primary: the screen
            w, h = SCREEN_W, SCREEN_H
        s = self.new_surface(w, h, caps)
        if caps & 0x200 and flags & 0x20 and backs:
            s.back = self.new_surface(w, h, (caps & ~0x200) | 0x4)
        self.log("CreateSurface(%dx%d caps %#x backs %d) -> %#x" % (w, h, caps, backs if flags & 0x20 else 0,
                                                                     s.addr))
        self.w32(out, s.addr)
        return 0

    def surf(self, a):
        s = self.com.get(a)
        if not isinstance(s, Surface):
            raise RuntimeError("not a surface: %#x" % a)
        return s

    def surf_AddRef(self, this):
        s = self.surf(this)
        s.refs += 1
        return s.refs

    def surf_Release(self, this):
        s = self.surf(this)
        s.refs -= 1
        return max(s.refs, 0)

    def surf_QueryInterface(self, this, iid, out):
        self.w32(out, this)
        return 0

    def surf_GetAttachedSurface(self, this, caps, out):
        s = self.surf(this)
        if s.back is None:
            self.w32(out, 0)
            return 0x887600FF  # DDERR_NOTFOUND
        self.w32(out, s.back.addr)
        return 0

    def surf_SetClipper(self, this, clip):
        self.surf(this).clipper = clip
        return 0

    def surf_IsLost(self, this):
        return 0

    def surf_Restore(self, this):
        return 0

    def surf_SetColorKey(self, this, flags, ck):
        s = self.surf(this)
        if ck and flags & 8:  # DDCKEY_SRCBLT
            s.ckey = self.r32(ck) & 0xFFFF
        return 0

    def surf_SetPalette(self, this, pal):
        return 0

    def pixel_format(self, p):
        self.wb(p, struct.pack("<IIIIIIII", 32, 0x40, 0, 16, 0xF800, 0x07E0, 0x001F, 0))

    def surface_desc(self, s, p, mem):
        self.wb(p, struct.pack("<IIIIi", 0x7C, 0x100F, s.h, s.w, s.pitch) + bytes(0x24 - 0x14))
        self.w32(p + 0x24, mem)
        self.pixel_format(p + 0x48)
        self.wb(p + 0x68, struct.pack("<IIII", s.caps, 0, 0, 0))

    def surf_GetSurfaceDesc(self, this, p):
        s = self.surf(this)
        self.surface_desc(s, p, s.mem)
        return 0

    def surf_GetPixelFormat(self, this, p):
        self.pixel_format(p)
        return 0

    def surf_Lock(self, this, rect, p, flags, ev):
        s = self.surf(this)
        mem = s.mem
        if rect:
            l, t, _, _ = struct.unpack("<iiii", self.rb(rect, 16))
            mem += t * s.pitch + 2 * l
        self.surface_desc(s, p, mem)
        return 0

    def surf_Unlock(self, this, rect):
        return 0

    def surf_GetDC(self, this, out):
        dc = 0x00080000 + len(self.dcs) + 1
        self.dcs[dc] = self.surf(this)
        self.w32(out, dc)
        return 0

    def surf_ReleaseDC(self, this, dc):
        self.dcs.pop(dc, None)
        return 0

    def read_rect(self, p, s):
        if not p:
            return 0, 0, s.w, s.h
        return struct.unpack("<iiii", self.rb(p, 16))

    def copy(self, dst, dl, dt, src, sl, st, w, h, key):
        """Unstretched copy of a w x h block, clipped to both surfaces."""
        if dl < 0:
            sl, w, dl = sl - dl, w + dl, 0
        if dt < 0:
            st, h, dt = st - dt, h + dt, 0
        w = min(w, dst.w - dl, src.w - sl)
        h = min(h, dst.h - dt, src.h - st)
        if w <= 0 or h <= 0:
            return
        if key is None and w == src.w == dst.w and src.pitch == dst.pitch:
            self.wb(dst.mem + dt * dst.pitch, self.rb(src.mem + st * src.pitch, h * src.pitch))
            return
        for y in range(h):
            row = self.rb(src.mem + (st + y) * src.pitch + 2 * sl, 2 * w)
            if key is not None:
                old = array.array("H", self.rb(dst.mem + (dt + y) * dst.pitch + 2 * dl, 2 * w))
                new = array.array("H", row)
                for i, v in enumerate(new):
                    if v != key:
                        old[i] = v
                row = old.tobytes()
            self.wb(dst.mem + (dt + y) * dst.pitch + 2 * dl, row)

    def surf_Blt(self, this, drect, src, srect, flags, fx):
        d = self.surf(this)
        dl, dt, dr, db = self.read_rect(drect, d)
        if flags & 0x400:  # DDBLT_COLORFILL (clearing a surface is not a present)
            color = self.r32(fx + 0x50) & 0xFFFF
            row = struct.pack("<H", color) * max(dr - dl, 0)
            for y in range(max(dt, 0), min(db, d.h)):
                self.wb(d.mem + y * d.pitch + 2 * max(dl, 0), row[:2 * (min(dr, d.w) - max(dl, 0))])
            return 0
        elif src:
            s = self.surf(src)
            sl, st, sr, sb = self.read_rect(srect, s)
            if (sr - sl, sb - st) != (dr - dl, db - dt):
                return self.fail("stretching Blt %r -> %r" % ((sl, st, sr, sb), (dl, dt, dr, db)))
            key = s.ckey if flags & 0x8000 else None
            self.copy(d, dl, dt, s, sl, st, sr - sl, sb - st, key)
        else:
            return self.fail("Blt flags %#x" % flags)
        if d.primary:
            self.present(d)
        return 0

    def surf_BltFast(self, this, x, y, src, srect, trans):
        d, s = self.surf(this), self.surf(src)
        sl, st, sr, sb = self.read_rect(srect, s)
        key = s.ckey if trans & 1 else None
        self.copy(d, x, y, s, sl, st, sr - sl, sb - st, key)
        if d.primary:
            self.present(d)
        return 0

    def surf_Flip(self, this, override, flags):
        f = self.surf(this)
        b = f.back
        if b is None:
            return 0x88760091  # DDERR_NOTFLIPPABLE
        f.mem, b.mem = b.mem, f.mem
        self.present(f)
        return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", default=os.environ.get("S2_GAME_DIR"))
    ap.add_argument("--out", default=".")
    ap.add_argument("--exe", default=DEFAULT_EXE)
    ap.add_argument("--script", default="")
    ap.add_argument("--script-file")
    ap.add_argument("--saves", help="start with this folder's files as SAVE\\ (default: the game's SAVE)")
    ap.add_argument("--time-seed", type=lambda s: int(s, 0), default=0)
    ap.add_argument("--trace", action="store_true", help="log every API call to OUT/oracle.log")
    ap.add_argument("--trace-from", type=int, default=0, help="like --trace, from this frame on")
    ap.add_argument("--max-frames", type=int, default=0)
    ap.add_argument("--wall-limit", type=float, default=1800.0, help="seconds")
    ap.add_argument("--peek", type=lambda s: [(int(a, 16), int(n, 0)) for a, n in (x.split(":") for x in s.split(","))],
                    default=[], help="ADDR:LEN,... hex dumps per shot in NAME.peek.txt (ADDR with bit 31 set: "
                                     "dump at the pointer stored there)")
    ap.add_argument("--dump-units", action="store_true", help="also write NAME.units.txt (both armies) per shot")
    ap.add_argument("--frame-log", help="write 'serial ms' (+ ' P' for a present) per frame, as s2run --frame-log")
    ap.add_argument("--watch-write", type=lambda s: [int(x, 16) for x in s.split(",")], default=[],
                    help="comma-separated hex addresses: log every write to them")
    ap.add_argument("--log-presents", action="store_true", help="log each present with its call chain")
    ap.add_argument("--watch", type=lambda s: [int(x, 16) for x in s.split(",")], default=[],
                    help="comma-separated hex addresses: log each time execution reaches them")
    args = ap.parse_args()
    if not args.game:
        ap.error("--game DIR (or S2_GAME_DIR) is required")
    if args.script_file:
        with open(args.script_file) as f:
            args.script = f.read()
    o = Oracle(args)
    code = o.run()
    if o.stop_reason not in ("script done", "quit"):
        print("oracle: stopped: %s (see %s)" % (o.stop_reason, os.path.join(o.out, "oracle.log")), file=sys.stderr)
    sys.exit(code)


if __name__ == "__main__":
    main()
