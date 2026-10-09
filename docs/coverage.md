# Coverage audit, 0x401000–0x45ffff

Functions of game code that no crate or doc cited (docs/coverage-todo.tsv, the part below
0x460000; GRAPHICS audits the rest). Each was read, its callers checked in the call graph
and by scanning the exe image for absolute references (vtables, function-pointer tables),
and classified. Citing them here (and, for FLOW-owned code, in the doc comment of the port)
makes them covered.

Counts: MISSING→PORTED 2, MISSING→ROUTED 3, EQUIVALENT 64, PLATFORM 124, DEAD 42 (total 235).

- **PLATFORM**: CRT / C++ static-init glue, heap and resource release, Miles (AIL) audio and CD
  Redbook handling, sprite caches — replaced by Rust ownership, the frontend and the audio backend.
- **DEAD**: no caller in the call graph and no absolute reference anywhere in the image.
- **EQUIVALENT**: the behaviour exists in our code (where = the files porting its callers or the
  named place); trivial getters/setters and helpers are inlined in those ports.
- **MISSING→PORTED**: game behaviour we lacked, now ported (with tests).
- **MISSING→ROUTED**: lacking in BATTLE-owned code; described here for the BATTLE port.

## MISSING→PORTED (2)

| addr | size | notes | where |
|---|---|---|---|
| `FUN_0044414b` | 824 | end-of-stage army cleanup: NPC/summon leavers, revive by item 0x18, FUN_004585cc reset, keep_deployed = flow/armies.rs stage_end_cleanup (called from town::between_stages, stage::cave_stage_setup, ending) |  |
| `FUN_0045cd5b` | 350 | cave: deployed members take the turn-1 cells = flow/armies.rs place_in_cave (stage::cave_stage_setup) |  |

## MISSING→ROUTED (3)

| addr | size | notes | where |
|---|---|---|---|
| `FUN_00426056` | 359 | after a keyboard cursor move (FUN_0042b72e → FUN_0042bfd6): -6 when the cell is in the 10-column x 30-row window, -5 (scroll, FUN_00432ecf(4)) when not; a cursor above row 1 after the height offset reverts to the last cell (DAT_00497580/84/88). battle::menu::input returns -6 for every key move |  |
| `FUN_0042bfd6` | 20 | FUN_0042bfea + FUN_00426056 (see above); return value is the input code |  |
| `FUN_0042c38a` | 697 | aiming (FUN_00401634 / FUN_004052e3): after the range is shown, a cursor outside the window is clamped to its edge (codes -9..-0x10 pick the side), then FUN_00426056 |  |

## EQUIVALENT (64)

| addr | size | notes | where |
|---|---|---|---|
| `FUN_0040103f` | 124 | targeting object ctor: cast panel (0xa6, 300) = Battle::default cast_panel (battle/mod.rs) |  |
| `FUN_004071ef` | 29 | helper of ported callers, inlined there | s2engine/src/battle/combat.rs |
| `FUN_00407353` | 30 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/menu.rs, s2engine/src/battle/view.rs |
| `FUN_00407371` | 17 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/ai.rs, s2engine/src/battle/mod.rs |
| `FUN_00407472` | 30 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/ai.rs, s2engine/src/battle/mod.rs |
| `FUN_00408332` | 17 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_panel.rs |
| `FUN_00408343` | 17 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_panel.rs |
| `FUN_004083a2` | 17 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_panel.rs |
| `FUN_004083b3` | 17 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_panel.rs |
| `FUN_00409466` | 17 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/ai.rs, s2engine/src/battle/mod.rs |
| `FUN_00409477` | 17 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/mod.rs |
| `FUN_0040a425` | 30 | helper of ported callers, inlined there | s2engine/src/battle/mod.rs, s2engine/src/battle/turn.rs, s2engine/src/event/battle_hooks.rs |
| `FUN_0040a8e7` | 19 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/turn.rs, s2engine/src/battle/view.rs |
| `FUN_0040c40f` | 21 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/turn.rs, s2engine/src/battle/view.rs |
| `FUN_0040cd17` | 19 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/turn.rs, s2engine/src/battle/view.rs |
| `FUN_0040cff0` | 20 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_motion.rs |
| `FUN_0041ec63` | 46 | font glyph fetch (16 px / 24 px bank, mainland picks 16 px) = s2data font draw_glyph; mainland layout now in text.rs |  |
| `FUN_0041ec91` | 69 | font glyph fetch (16 px / 24 px bank, mainland picks 16 px) = s2data font draw_glyph; mainland layout now in text.rs |  |
| `FUN_0041ee1a` | 78 | font glyph fetch (16 px / 24 px bank, mainland picks 16 px) = s2data font draw_glyph; mainland layout now in text.rs |  |
| `FUN_0041ee68` | 78 | font glyph fetch (16 px / 24 px bank, mainland picks 16 px) = s2data font draw_glyph; mainland layout now in text.rs |  |
| `FUN_0042023a` | 611 | helper of ported callers, inlined there | s2engine/src/battle/view_unit.rs |
| `FUN_0042049d` | 369 | helper of ported callers, inlined there | s2engine/src/battle/cast.rs, s2engine/src/battle/menu.rs, s2engine/src/battle/targeting.rs |
| `FUN_00422335` | 30 | helper of ported callers, inlined there | s2engine/src/flow/town.rs |
| `FUN_004249fe` | 289 | map object ctor: scroll (1,1), building flag = MapView/Map defaults (battle/view_map.rs, s2data gfx_map) |  |
| `FUN_0042532b` | 135 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/menu.rs, s2engine/src/battle/view.rs |
| `FUN_004253b2` | 79 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/cast.rs, s2engine/src/battle/view_unit.rs |
| `FUN_00425401` | 109 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/hooks.rs, s2engine/src/battle/menu.rs |
| `FUN_0042546e` | 125 | helper of ported callers, inlined there | s2data/src/gfx_map.rs |
| `FUN_0042552d` | 36 | helper of ported callers, inlined there | s2engine/src/battle/menu.rs, s2engine/src/battle/targeting.rs |
| `FUN_0042572f` | 29 | helper of ported callers, inlined there | s2data/src/gfx_map.rs |
| `FUN_004267d3` | 75 | helper of ported callers, inlined there | s2engine/src/battle/view.rs |
| `FUN_004282be` | 40 | helper of ported callers, inlined there | s2engine/src/battle/targeting.rs |
| `FUN_00428b6a` | 1286 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/view.rs, s2engine/src/battle/view_map.rs |
| `FUN_00429cf9` | 152 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/map.rs, s2engine/src/battle/view_map.rs |
| `FUN_00429d91` | 158 | helper of ported callers, inlined there | s2data/src/gfx_map.rs, s2engine/src/battle/map.rs, s2engine/src/battle/view_map.rs |
| `FUN_0042b3f7` | 5 | empty input hook (no-op) |  |
| `FUN_0043b79c` | 611 | summon growth with caps 5000/999 = Battle::create_summon (battle/deploy.rs) |  |
| `FUN_0043c7e3` | 75 | helper of ported callers, inlined there | s2engine/src/battle/unit.rs |
| `FUN_0043c82e` | 75 | helper of ported callers, inlined there | s2data/src/tables.rs, s2engine/src/battle/spells.rs |
| `FUN_0043c96c` | 38 | helper of ported callers, inlined there | s2engine/src/battle/turn.rs |
| `FUN_00440059` | 33 | helper of ported callers, inlined there | s2engine/src/battle/view.rs, s2engine/src/battle/view_panel.rs |
| `FUN_0044007a` | 237 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/cast.rs, s2engine/src/battle/mod.rs |
| `FUN_00441b15` | 246 | helper of ported callers, inlined there | s2engine/src/battle/ai.rs, s2engine/src/battle/hooks.rs, s2engine/src/event/battle_hooks.rs |
| `FUN_00441d14` | 245 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/view_cast.rs |
| `FUN_00442238` | 411 | helper of ported callers, inlined there | s2engine/src/battle/deploy.rs, s2engine/src/battle/hooks.rs, s2engine/src/battle/spells.rs |
| `FUN_00442ecb` | 25 | helper of ported callers, inlined there | s2engine/src/flow/march.rs, s2engine/src/flow/shop.rs |
| `FUN_00442ee4` | 20 | helper of ported callers, inlined there | s2data/src/tables.rs, s2engine/src/battle/ai.rs, s2engine/src/battle/mod.rs |
| `FUN_00444483` | 19 | enemy army clear (FUN_0043deeb) = Army::clear (battle/army.rs) |  |
| `FUN_00444ba6` | 20 | helper of ported callers, inlined there | s2engine/src/flow/ending.rs, s2tool/src/bin/s2audio.rs |
| `FUN_0044627c` | 23 | FUN_00416ffb(0, 0) = deploy::place_on_map(None) (flow/deploy.rs) |  |
| `FUN_00446abf` | 106 | helper of ported callers, inlined there | s2engine/src/battle/cast.rs, s2engine/src/flow/spellview.rs |
| `FUN_00446b29` | 33 | helper of ported callers, inlined there | s2engine/src/battle/targeting.rs |
| `FUN_00446b70` | 20 | helper of ported callers, inlined there | s2engine/src/flow/shop.rs |
| `FUN_004574d1` | 49 | helper of ported callers, inlined there | s2engine/src/battle/actions.rs, s2engine/src/battle/ai.rs, s2engine/src/battle/cast.rs |
| `FUN_00457502` | 118 | helper of ported callers, inlined there | s2engine/src/battle/spells.rs |
| `FUN_0045a355` | 257 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045a456` | 257 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045a557` | 547 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045a77a` | 701 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045aa37` | 547 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045acec` | 157 | FLC chunk decoders and 320x200 doubling = s2data/src/anim.rs |  |
| `FUN_0045e041` | 185 | nearest-colour search over the palette = s2data gfx_palette ColorTree |  |
| `FUN_0045e980` | 30 | helper of ported callers, inlined there | s2engine/src/battle/mod.rs, s2engine/src/battle/turn.rs, s2engine/src/event/battle_hooks.rs |
| `FUN_0045e99e` | 35 | helper of ported callers, inlined there | s2engine/src/event/battle_hooks.rs |

## PLATFORM (124)

| addr | size | notes | where |
|---|---|---|---|
| `FUN_00401000` | 15 | static initializer (CRT init table 48e00c) |  |
| `FUN_0040100f` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0040101e` | 18 | static dtor registration (atexit) |  |
| `FUN_00401030` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_004010bb` | 19 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_00406a23` | 19 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00406ca9` | 99 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0040751c` | 80 | CD-ROM wait / Redbook status (CD audio is replaced by the audio backend) |  |
| `FUN_0040756c` | 124 | Miles audio (AIL) |  |
| `FUN_004075e8` | 49 | Miles audio (AIL) |  |
| `FUN_00407619` | 95 | CD-ROM wait / Redbook status (CD audio is replaced by the audio backend) |  |
| `FUN_004076da` | 59 | Miles audio (AIL) |  |
| `FUN_0040771a` | 64 | Miles audio (AIL) |  |
| `FUN_00407764` | 30 | Miles audio (AIL) |  |
| `FUN_004077cb` | 75 | Miles audio (AIL) |  |
| `FUN_00407816` | 97 | Miles audio (AIL) |  |
| `FUN_0040801b` | 15 | static initializer (CRT init table 48e010) |  |
| `FUN_0040802a` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_00408039` | 18 | static dtor registration (atexit) |  |
| `FUN_0040804b` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0040a3e6` | 15 | static initializer (CRT init table 48e014) |  |
| `FUN_0040a3f5` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0040a404` | 18 | static dtor registration (atexit) |  |
| `FUN_0040a416` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0040cfe0` | 11 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0040fca9` | 147 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_004172b0` | 15 | static initializer (CRT init table 48e018) |  |
| `FUN_004172bf` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_004172ce` | 18 | static dtor registration (atexit) |  |
| `FUN_004172e0` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_004172ef` | 74 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_00417339` | 19 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_00418859` | 113 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0041989a` | 10 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00419a98` | 28 | Miles audio (AIL) |  |
| `FUN_00419ad1` | 55 | Miles audio (AIL) |  |
| `FUN_00419ee4` | 181 | Miles audio (AIL) |  |
| `FUN_00419f99` | 131 | Miles audio (AIL) |  |
| `FUN_0041a282` | 235 | Miles audio (AIL) |  |
| `FUN_0041dd30` | 46 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0041dd60` | 46 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0041dd90` | 15 | static initializer (CRT init table 48e01c) |  |
| `FUN_0041dd9f` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0041ddae` | 18 | static dtor registration (atexit) |  |
| `FUN_0041ddc0` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0041ddcf` | 64 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_0041de0f` | 19 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0041de3d` | 27 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0041de87` | 79 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0041ebf2` | 113 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0042080b` | 64 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0042084b` | 38 | frees a global buffer |  |
| `FUN_00420871` | 37 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00420d56` | 262 | image RLE packing for FUN_0046e538 (GRAPHICS range); images stay unpacked |  |
| `FUN_00421c71` | 38 | frees a global buffer |  |
| `FUN_00422256` | 38 | frees a global buffer |  |
| `FUN_0042318e` | 48 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00423728` | 15 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0042395f` | 106 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00423a28` | 48 | frees a global buffer |  |
| `FUN_00423aae` | 38 | frees a global buffer |  |
| `FUN_004241e3` | 72 | frees a global buffer |  |
| `FUN_004242cc` | 38 | frees a global buffer |  |
| `FUN_004243d5` | 72 | frees a global buffer |  |
| `FUN_00424980` | 15 | static initializer (CRT init table 48e020) |  |
| `FUN_0042498f` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0042499e` | 18 | static dtor registration (atexit) |  |
| `FUN_004249b0` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_004249bf` | 15 | static initializer (CRT init table 48e024) |  |
| `FUN_004249ce` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_004249dd` | 18 | static dtor registration (atexit) |  |
| `FUN_004249ef` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_00424ee0` | 405 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00428ad7` | 147 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0042b330` | 38 | heap allocation wrappers (calloc/free with the out-of-memory abort) |  |
| `FUN_0042b356` | 17 | heap allocation wrappers (calloc/free with the out-of-memory abort) |  |
| `FUN_0042b367` | 7 | heap allocation wrappers (calloc/free with the out-of-memory abort) |  |
| `FUN_0043395f` | 38 | frees a global buffer |  |
| `FUN_0043a254` | 251 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043a8e4` | 134 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043d0a0` | 15 | static initializer (CRT init table 48e028) |  |
| `FUN_0043d0af` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d0be` | 18 | static dtor registration (atexit) |  |
| `FUN_0043d0d0` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d0df` | 15 | static initializer (CRT init table 48e02c) |  |
| `FUN_0043d0ee` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d0fd` | 18 | static dtor registration (atexit) |  |
| `FUN_0043d10f` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d11e` | 15 | static initializer (CRT init table 48e030) |  |
| `FUN_0043d12d` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d13c` | 18 | static dtor registration (atexit) |  |
| `FUN_0043d14e` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d15d` | 296 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_0043d285` | 19 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0043d3e2` | 70 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0043d739` | 78 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0043ee88` | 110 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0043f1f9` | 67 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0043f2ef` | 96 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043f34f` | 221 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043f5de` | 181 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043f96a` | 115 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_0043fe07` | 80 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0043fe57` | 78 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_00443c84` | 485 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_00443e69` | 612 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_004440cd` | 126 | sprite / portrait cache management (DAT_00497b68 type table, unit+0xa0/+0xad buffers); the view decodes frames per unit (battle/view*.rs) |  |
| `FUN_00446b50` | 19 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_00459340` | 71 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_00459387` | 25 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_00459549` | 29 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0045da70` | 10 | static initializer (CRT init table 48e034) |  |
| `FUN_0045da7a` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0045da89` | 38 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_0045e490` | 15 | static initializer (CRT init table 48e038) |  |
| `FUN_0045e49f` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0045e4ae` | 18 | static dtor registration (atexit) |  |
| `FUN_0045e4c0` | 15 | C++ static initializer chain (CRT init table / atexit); the global objects are plain Rust values |  |
| `FUN_0045e65d` | 72 | frees a global buffer |  |
| `FUN_0045e75a` | 256 | constructors that only zero fields (the Rust Default impls) |  |
| `FUN_0045e85a` | 88 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0045ec90` | 140 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0045eed5` | 54 | memory release (heap buffers / object teardown); Rust drops them |  |
| `FUN_0045f0d8` | 54 | memory release (heap buffers / object teardown); Rust drops them |  |

## DEAD (42)

| addr | size | notes | where |
|---|---|---|---|
| `FUN_00407715` | 5 | no callers or references |  |
| `FUN_0040775a` | 5 | no callers or references |  |
| `FUN_0040775f` | 5 | no callers or references |  |
| `FUN_00419ab4` | 12 | no callers or references |  |
| `FUN_00419ac0` | 12 | no callers or references |  |
| `FUN_00419acc` | 5 | no callers or references |  |
| `FUN_0041e1f6` | 234 | no callers or references |  |
| `FUN_0041eaec` | 168 | no callers or references |  |
| `FUN_00420034` | 518 | only caller FUN_00440167 is unreferenced |  |
| `FUN_00420977` | 92 | no callers or references |  |
| `FUN_004209d3` | 132 | only caller FUN_0041eaec is unreferenced |  |
| `FUN_00420a57` | 122 | only caller FUN_0041eaec is unreferenced |  |
| `FUN_00420ad1` | 94 | no callers or references |  |
| `FUN_004213e1` | 959 | no callers or references |  |
| `FUN_00421dcf` | 325 | no callers or references |  |
| `FUN_004252a0` | 41 | no callers or references |  |
| `FUN_004252f4` | 55 | no callers or references |  |
| `FUN_00425551` | 17 | no callers or references |  |
| `FUN_00425644` | 61 | no callers or references |  |
| `FUN_00425681` | 61 | no callers or references |  |
| `FUN_004256be` | 29 | no callers or references |  |
| `FUN_00425712` | 29 | no callers or references |  |
| `FUN_00426dda` | 513 | no callers or references |  |
| `FUN_004272df` | 261 | no callers or references |  |
| `FUN_0042a97b` | 259 | no callers or references |  |
| `FUN_0042b36e` | 34 | no callers or references |  |
| `FUN_00433870` | 10 | no callers or references |  |
| `FUN_00433a61` | 10 | no callers or references |  |
| `FUN_00440167` | 253 | no callers or references |  |
| `FUN_00440bc1` | 577 | no callers or references |  |
| `FUN_00440e02` | 58 | no callers or references |  |
| `FUN_00440e3c` | 53 | no callers or references |  |
| `FUN_004418e2` | 234 | no callers or references |  |
| `FUN_00442959` | 707 | no callers or references |  |
| `FUN_00444bba` | 21 | no callers or references |  |
| `FUN_00444bcf` | 27 | no callers or references |  |
| `FUN_004469e7` | 216 | no callers or references |  |
| `FUN_004528e1` | 73 | no callers or references |  |
| `FUN_00459daa` | 1399 | no callers or references |  |
| `FUN_0045cd1f` | 60 | no callers or references |  |
| `FUN_0045ed1c` | 441 | no callers or references |  |
| `FUN_0045f10e` | 55 | no callers or references |  |

## Related fixes found during the audit

- Saves loaded inside a stage now get the stage's placement records (`FUN_00423033`'s record
  split runs in every mode; `armies::set_records`), so later-turn reinforcements appear after a load.
- `Battle::start_stage` no longer places the party (BATTLE; `start_stage_placed` is the
  save-load / VS path, `FUN_0040f67f`), so the DEPLOYED-bit restore in `flow::stage::load_stage`
  was removed. Campaign stages start with the party as `FUN_0044414b` left it.
