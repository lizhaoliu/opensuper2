#!/bin/bash
# Print decompiled function(s) by address: fn.sh 0040d7e1 [more...]
for a in "$@"; do
awk -v A="$a" '/^\/\/ ==== /{p=($0 ~ ("@ " A "$"))} p' ${DECOMP:-/home/lizhaoliu/games/super2sp/re/flow/decomp_full.c}
done
