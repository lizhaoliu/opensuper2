import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from s2 import *
out = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
os.makedirs(out, exist_ok=True)
pal, t = load_index()
for i in range(1, 19):
    open(out + 'bin_%02d' % i, 'wb').write(entry('bin', i))
for i in list(range(1, 11)):
    open(out + 'dat_%02d' % i, 'wb').write(entry('dat2', i))
