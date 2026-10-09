"""Unpack a PECompact2-packed PE32 by emulating its stub in Unicorn.

Usage: unpack_pec2.py <packed.exe> <out.exe>

The stub is run until execution reaches the original code range; the
memory image is then dumped and a fresh import directory is rebuilt from
the IAT slots the stub filled with our fake API addresses.
"""
import struct
import sys

import pefile
from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UC_HOOK_MEM_INVALID
from unicorn.x86_const import *

STACK_BASE, STACK_SIZE = 0x00100000, 0x00100000
TEB = 0x7FFDF000
API_BASE, API_SIZE = 0x70000000, 0x00100000
MOD_BASE = 0x60000000
HEAP_BASE = 0x10000000
SCRATCH = 0x00300000  # exception record / context for SEH emulation


class Emu:
    def __init__(self, path):
        self.pe = pefile.PE(path)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.size = self.pe.OPTIONAL_HEADER.SizeOfImage
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc = self.uc
        uc.mem_map(self.base, (self.size + 0xFFF) & ~0xFFF)
        uc.mem_write(self.base, bytes(self.pe.get_memory_mapped_image()))
        uc.mem_map(STACK_BASE, STACK_SIZE)
        uc.mem_map(TEB, 0x2000)
        uc.mem_map(API_BASE, API_SIZE)
        uc.mem_map(SCRATCH, 0x10000)
        uc.mem_write(API_BASE, b"\xc3" * API_SIZE)
        self.heap_next = HEAP_BASE
        self.modules = {}  # handle -> dll name
        self.apis = {}  # fake addr -> (dll, name)
        self.api_by_name = {}
        self.code_lo = self.base + 0x1000
        self.code_hi = self.base + self.pe.sections[0].VirtualAddress + self.pe.sections[0].Misc_VirtualSize
        self.seh_done = False
        self.oep = None
        # TEB: fs:[0] = SEH list end, fs:[0x18] = self, fs:[0x30] = PEB
        uc.mem_write(TEB, struct.pack("<I", 0xFFFFFFFF))
        uc.mem_write(TEB + 0x18, struct.pack("<I", TEB))
        uc.mem_write(TEB + 0x30, struct.pack("<I", TEB + 0x1000))
        uc.mem_write(TEB + 0x1000 + 8, struct.pack("<I", self.base))  # PEB.ImageBaseAddress
        self.setup_gdt()
        esp = STACK_BASE + STACK_SIZE - 0x100
        uc.reg_write(UC_X86_REG_ESP, esp)
        uc.mem_write(esp, struct.pack("<I", API_BASE + API_SIZE - 0x10))  # return addr -> exit
        # The kernel32 imports of the packed file itself.
        for imp in self.pe.DIRECTORY_ENTRY_IMPORT:
            for f in imp.imports:
                addr = self.fake_api(imp.dll.decode().lower(), f.name.decode())
                uc.mem_write(f.address, struct.pack("<I", addr))
        uc.hook_add(UC_HOOK_CODE, self.on_api, begin=API_BASE, end=API_BASE + API_SIZE - 1)
        uc.hook_add(UC_HOOK_CODE, self.on_code, begin=self.code_lo, end=self.code_hi - 1)
        uc.hook_add(UC_HOOK_MEM_UNMAPPED | UC_HOOK_MEM_INVALID, self.on_fault)
        self.pending_seh = False

    def setup_gdt(self):
        uc = self.uc
        gdt = 0x00400000 - 0x10000 if False else 0x00010000
        uc.mem_map(gdt, 0x1000)

        def desc(base, limit, access, flags):
            d = limit & 0xFFFF
            d |= (base & 0xFFFFFF) << 16
            d |= access << 40
            d |= ((limit >> 16) & 0xF) << 48
            d |= flags << 52
            d |= ((base >> 24) & 0xFF) << 56
            return struct.pack("<Q", d)

        # 1: code, 2: data, 3: fs (TEB), 4: stack segment ring0
        uc.mem_write(gdt + 8, desc(0, 0xFFFFF, 0x9A | 0x60, 0xC))
        uc.mem_write(gdt + 16, desc(0, 0xFFFFF, 0x92 | 0x60, 0xC))
        uc.mem_write(gdt + 24, desc(TEB, 0xFFF, 0x92 | 0x60, 0x4))
        uc.mem_write(gdt + 32, desc(0, 0xFFFFF, 0x92, 0xC))
        uc.reg_write(UC_X86_REG_GDTR, (0, gdt, 0x1000, 0))
        uc.reg_write(UC_X86_REG_SS, (4 << 3))  # ring0 stack seg
        uc.reg_write(UC_X86_REG_DS, (2 << 3) | 3)
        uc.reg_write(UC_X86_REG_ES, (2 << 3) | 3)
        uc.reg_write(UC_X86_REG_FS, (3 << 3) | 3)

    def fake_api(self, dll, name):
        key = (dll, name)
        if key in self.api_by_name:
            return self.api_by_name[key]
        addr = API_BASE + 0x10 * (len(self.apis) + 1)
        self.apis[addr] = key
        self.api_by_name[key] = addr
        return addr

    def rd(self, a, n=4):
        return struct.unpack("<I", self.uc.mem_read(a, 4))[0] if n == 4 else bytes(self.uc.mem_read(a, n))

    def cstr(self, a):
        out = b""
        while True:
            c = bytes(self.uc.mem_read(a, 1))
            if c == b"\0":
                return out.decode("latin1")
            out += c
            a += 1

    def ret(self, value, nargs):
        uc = self.uc
        esp = uc.reg_read(UC_X86_REG_ESP)
        ra = self.rd(esp)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(UC_X86_REG_EIP, ra)

    def arg(self, i):
        return self.rd(self.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * i)

    def on_api(self, uc, addr, size, _):
        if addr == API_BASE + API_SIZE - 0x10:
            print("stub returned to exit stub")
            uc.emu_stop()
            return
        if addr not in self.apis:
            return
        dll, name = self.apis[addr]
        if name == "LoadLibraryA" or name == "GetModuleHandleA":
            p = self.arg(0)
            if p == 0:
                return self.ret(self.base, 1)
            lib = self.cstr(p).lower()
            if not lib.endswith(".dll"):
                lib += ".dll"
            for h, n in self.modules.items():
                if n == lib:
                    return self.ret(h, 1)
            h = MOD_BASE + 0x10000 * (len(self.modules) + 1)
            self.modules[h] = lib
            return self.ret(h, 1)
        if name == "GetProcAddress":
            h, p = self.arg(0), self.arg(1)
            lib = self.modules.get(h, "kernel32.dll")
            fn = ("#%d" % p) if p < 0x10000 else self.cstr(p)
            return self.ret(self.fake_api(lib, fn), 2)
        if name == "VirtualAlloc":
            addr_req, size_req = self.arg(0), self.arg(1)
            size_req = (size_req + 0xFFFF) & ~0xFFFF
            if addr_req:
                return self.ret(addr_req, 4)
            a = self.heap_next
            uc.mem_map(a, size_req)
            self.heap_next += size_req + 0x10000
            return self.ret(a, 4)
        if name == "VirtualFree":
            return self.ret(1, 3)
        if name == "VirtualProtect":
            old = self.arg(3)
            if old:
                uc.mem_write(old, struct.pack("<I", 0x40))
            return self.ret(1, 4)
        simple = {"IsDebuggerPresent": (0, 0), "GetTickCount": (0, 0x10000), "GetVersion": (0, 0x0A280105),
                  "GetCurrentProcess": (0, 0xFFFFFFFF), "GetCurrentThread": (0, 0xFFFFFFFE),
                  "FlushInstructionCache": (3, 1), "SetErrorMode": (1, 0), "GetCurrentProcessId": (0, 0x100)}
        if name in simple:
            n, v = simple[name]
            return self.ret(v, n)
        print("unhandled API", dll, name)
        uc.emu_stop()

    def on_code(self, uc, addr, size, _):
        if self.seh_done and self.oep is None and addr >= self.code_lo + 0x30:
            self.oep = addr
            print("reached OEP %#x" % addr)
            uc.emu_stop()

    def on_fault(self, uc, access, addr, size, value, _):
        eip = uc.reg_read(UC_X86_REG_EIP)
        if addr < 0x1000 and not self.seh_done:
            # Deliberate null write: dispatch to the registered SEH handler.
            self.pending_seh = eip
            uc.emu_stop()
            return False
        print("fault at eip=%#x addr=%#x" % (eip, addr))
        return False

    def dispatch_seh(self, fault_eip):
        uc = self.uc
        frame = self.rd(TEB)
        handler = self.rd(frame + 4)
        er, ctx = SCRATCH, SCRATCH + 0x100
        uc.mem_write(er, struct.pack("<IIIII", 0xC0000005, 0, 0, fault_eip, 0))
        regs = [UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ECX, UC_X86_REG_EAX,
                UC_X86_REG_EBP]
        # CONTEXT: Edi at 0x9c .. Ebp at 0xb4, Eip 0xb8, SegCs 0xbc, EFlags 0xc0, Esp 0xc4
        uc.mem_write(ctx, b"\0" * 0x2CC)
        uc.mem_write(ctx, struct.pack("<I", 0x10007))
        for i, r in enumerate(regs):
            uc.mem_write(ctx + 0x9C + 4 * i, struct.pack("<I", uc.reg_read(r)))
        uc.mem_write(ctx + 0xB8, struct.pack("<I", fault_eip))
        uc.mem_write(ctx + 0xC0, struct.pack("<I", uc.reg_read(UC_X86_REG_EFLAGS)))
        uc.mem_write(ctx + 0xC4, struct.pack("<I", uc.reg_read(UC_X86_REG_ESP)))
        saved_esp = uc.reg_read(UC_X86_REG_ESP)
        esp = saved_esp - 0x400
        ret_marker = API_BASE + API_SIZE - 0x20
        uc.mem_write(esp, struct.pack("<IIIII", ret_marker, er, frame, ctx, 0))
        uc.reg_write(UC_X86_REG_ESP, esp)
        uc.emu_start(handler, ret_marker)
        for i, r in enumerate(regs):
            uc.reg_write(r, self.rd(ctx + 0x9C + 4 * i))
        uc.reg_write(UC_X86_REG_ESP, self.rd(ctx + 0xC4))
        new_eip = self.rd(ctx + 0xB8)
        print("SEH handler %#x resumed at %#x" % (handler, new_eip))
        return new_eip

    def run(self):
        ep = self.base + self.pe.OPTIONAL_HEADER.AddressOfEntryPoint
        start = ep
        # Entry itself is inside the code range; only start watching after SEH.
        while True:
            self.pending_seh = False
            try:
                self.uc.emu_start(start, 0, count=500_000_000)
            except UcError as e:
                if not self.pending_seh:
                    raise
            if self.pending_seh:
                start = self.dispatch_seh(self.pending_seh)
                self.seh_done = True
                continue
            break
        return self.oep


def rebuild(emu, out_path):
    uc, pe = emu.uc, emu.pe
    img = bytearray(uc.mem_read(emu.base, emu.size))
    # Find IAT slots holding fake API addresses.
    slots = []
    for off in range(0, len(img) - 3, 4):
        v = struct.unpack_from("<I", img, off)[0]
        if v in emu.apis and off < 0xDE000:
            slots.append((off, v))
    # Group consecutive slots by dll.
    groups = []
    for off, v in slots:
        dll = emu.apis[v][0]
        if groups and groups[-1][0] == dll and groups[-1][1][-1][0] == off - 4:
            groups[-1][1].append((off, v))
        else:
            groups.append((dll, [(off, v)]))
    print("IAT groups:")
    for dll, g in groups:
        print("  %-14s rva=%#x n=%d" % (dll, g[0][0], len(g)))
    # Build new .idata section after the image.
    sec_rva = (emu.size + 0xFFF) & ~0xFFF
    idata = bytearray()
    desc_size = 20 * (len(groups) + 1)
    strings = bytearray()
    thunks = bytearray()
    str_base = desc_size
    # Layout: descriptors | INT thunks | strings
    int_tables = []
    for dll, g in groups:
        int_tables.append(len(thunks))
        thunks += b"\0" * (4 * (len(g) + 1))
    thunk_base = desc_size
    str_base = thunk_base + len(thunks)
    descs = bytearray()
    for (dll, g), int_off in zip(groups, int_tables):
        name_rva = sec_rva + str_base + len(strings)
        strings += dll.encode() + b"\0"
        for i, (off, v) in enumerate(g):
            fn = emu.apis[v][1]
            if fn.startswith("#"):
                ent = 0x80000000 | int(fn[1:])
            else:
                if len(strings) % 2:
                    strings += b"\0"
                ent = sec_rva + str_base + len(strings)
                strings += b"\0\0" + fn.encode() + b"\0"
            struct.pack_into("<I", thunks, int_off + 4 * i, ent)
            struct.pack_into("<I", img, off, ent)  # IAT holds same as INT on disk
        descs += struct.pack("<IIIII", sec_rva + thunk_base + int_off, 0, 0, name_rva, g[0][0])
    descs += b"\0" * 20
    idata = descs + thunks + strings
    idata += b"\0" * ((-len(idata)) % 0x1000)
    # Write a flat PE: headers, one big RWX section covering the image, plus .idata.
    hdr = bytearray(img[:0x1000])
    pe2 = pefile.PE(data=bytes(hdr) + bytes(img[0x1000:]) + bytes(idata), fast_load=True)
    data = bytearray(bytes(img) + bytes(idata))
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    nsec_off = e_lfanew + 6
    opt_off = e_lfanew + 24
    sect_off = opt_off + struct.unpack_from("<H", data, e_lfanew + 20)[0]
    # sections: keep original headers but make raw == virtual
    nsec = struct.unpack_from("<H", data, nsec_off)[0]
    names = []
    for i in range(nsec):
        so = sect_off + 40 * i
        va, vs = struct.unpack_from("<II", data, so + 12)[0], struct.unpack_from("<I", data, so + 8)[0]
        struct.pack_into("<II", data, so + 16, vs, va)  # SizeOfRawData, PointerToRawData
        names.append(data[so:so + 8])
    so = sect_off + 40 * nsec
    struct.pack_into("<8sIIIIIIHHI", data, so, b".idata2", len(idata), sec_rva, len(idata), sec_rva, 0, 0, 0, 0,
                     0xC0000040)
    struct.pack_into("<H", data, nsec_off, nsec + 1)
    struct.pack_into("<I", data, opt_off + 16, emu.oep - emu.base)  # AddressOfEntryPoint
    struct.pack_into("<I", data, opt_off + 56, sec_rva + len(idata))  # SizeOfImage
    struct.pack_into("<I", data, opt_off + 60, 0x1000)  # SizeOfHeaders
    dd = opt_off + 96
    struct.pack_into("<II", data, dd + 8, sec_rva, len(descs))  # import dir
    struct.pack_into("<II", data, dd + 8 * 12, 0, 0)  # IAT dir
    with open(out_path, "wb") as f:
        f.write(data)
    with open(out_path + ".imports.txt", "w") as f:
        for off, v in slots:
            f.write("%08x %s!%s\n" % (emu.base + off, *emu.apis[v]))


if __name__ == "__main__":
    emu = Emu(sys.argv[1])
    oep = emu.run()
    if oep is None:
        sys.exit("OEP not reached")
    print("modules:", sorted(set(d for d, _ in emu.apis.values())))
    rebuild(emu, sys.argv[2])
