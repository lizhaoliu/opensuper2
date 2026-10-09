import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
for k in ['cel','prg']:
    for i,(o,s) in enumerate(tables[k]):
        e = entry(k, i+1)
        print(k, i+1, hex(o), s, e[:48].hex(' '))
