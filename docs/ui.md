# Display, input, timing and the UI toolkit

Companion to `flow.md`. Function addresses refer to the decompilation with
recovered switches (`~/games/super2sp/re/flow/decomp_full.c`, `tools/fn.sh`).
Colors are palette indices. "(guess)" marks unconfirmed inferences.

## 1. Screen and pages — object `DAT_004947c8`

```
+0x00 i32 width (640)      +0x04 i32 height (480)
+0x08 i32 current page     +0x0c i32 page count (3)
+0x10 u8* page[4]          +0x20 i32* row offset table (y*640), nonzero = initialised
```
Allocated by `FUN_0046e998(screen, 3)` → `FUN_0046e9f2` (one block, pages contiguous).
A second object of the same type, `DAT_004947f0`, is the battle-map surface
(`FUN_0046e957(obj, mapw*64, maph*32, 1)`), so map tiles are 64×32.

All drawing targets the **current page**. Pages are plain 8-bit buffers; the
"screen" is a separate DirectDraw surface object `DAT_004a2b00`.

| Function | Meaning |
|---|---|
| `FUN_0046eb02(scr, p)` | select current page (ignored if p ≥ count) |
| `FUN_0046ec3c(scr)` | pointer to current page |
| `FUN_0046ec54(scr, p)` | pointer to page p |
| `FUN_0046ec6b(scr, src, dst)` | copy page src → page dst (whole screen; `memmove(page[dst], page[src])`) |
| `FUN_0046ecc2(scr)` / `FUN_0046ece7(scr, p)` | clear current page / page p to 0 |
| `FUN_0046eb48(scr)` | **present** the current page: hide OS cursor, copy page → screen surface, show OS cursor |
| `FUN_0046eb8a(scr, p)` | present page p without changing the current page |
| `FUN_0046eb25(scr)` | present without cursor bracketing |

Port model: `pages: [[u8; 640*480]; 3]`, `current`, and `present(p)` = copy
page p into `Sys::fb` + `Sys::flip()`. The real program presents synchronously
(DirectDraw blit); assume one present ≈ one display frame.

### Drawing primitives (all clip to 640×480, draw into the current page)

| Function | Signature | Behaviour |
|---|---|---|
| `FUN_0046f629` | `(scr, x, y, raw, opaque)` | raw image `i16 w-1, i16 h-1, w*h bytes`; opaque=1 memcpy rows, 0 skips index 0 |
| `FUN_0046f02f` | `(scr, x, y, rle, opaque)` | RLE image (formats.md); runs never cross rows here; opaque=1 also writes the transparent `0x00` codes and zero-valued fill runs as 0 |
| `FUN_0046ed81` | `(scr, x, y, tagged, opaque)` | tagged image: byte 0 = 0 → RLE at +1, else raw at +1 |
| `FUN_0046ed32` | same | tagged variant using `FUN_0046edd0`/`FUN_0046f50a` (mirrored or remapped blits, guess — not on the critical path) |
| `FUN_0046fad2` | `(scr, x, y, c)` | put pixel |
| `FUN_0046fb13` | `(scr, y, x0, x1, c)` | horizontal line (note y first) |
| `FUN_0046fb6c` | `(scr, x, y0, y1, c)` | vertical line |
| `FUN_0046fbd3` | `(scr, x0, y0, x1, y1, c, filled)` | rectangle outline (filled=0) or fill |
| `FUN_0046f907` | `(scr, x0, y0, x1, y1, out_raw)` | grab rect into a raw image (out-of-screen parts zeroed) |
| `FUN_0046f839` | same | grab without clipping (guess) |
| `FUN_0046e5c1` | `(front, x, y, raw, opaque)` | blit a raw image **directly to the visible screen** (`DAT_004a2b00`); used by text with target=0 |
| `FUN_00420710` | `(buf, lut, n)` | remap n bytes through LUT `lut` (§2.4), e.g. `(page, 6, 640*480)` darkens the whole page |
| `FUN_00420bf5` | `(x, y, img, 1)` | window drop shadow: remaps an 8-px band left of and below the image rect (x-8.., y+8..) through LUT 6; skipped if it would leave the screen |
| `FUN_0040f08e` | `(x, y, w, h)` | bevel box: fill 0xf0; inner top/left lines 0xfe; inner bottom/right 0xed; outer bottom/right 0x7d |

## 2. Palette

### 2.1 Master palette
`DAT_004a0b08` = first 768 bytes of SUPERSP.PAL (6-bit), entry 0 forced to 0,0,0 (`FUN_004217b9`).
PCX pictures carry their own 8-bit palettes, converted to 6-bit (`>>2`) by `FUN_00420fab`.

### 2.2 Set and fade — `FUN_0046d390(pal)`, `FUN_0046d442(pal, dir, speed)`
- `FUN_0046d390(pal)`: set all 256 entries (6-bit → `<<2`) and re-present.
- `FUN_0046d442(pal, 0, speed)`: **fade in** from black to `pal`; `FUN_0046d442(pal, 1, speed)`: **fade out** from the current palette to black.
  - step = `|speed|` if speed < 0, else 1; levels `l = 0,step,…,63` (in) or `63,…,0` (out).
  - each level sets entries 1..255 to `c*(l*4)/255` per channel (entry 0 untouched; `FUN_004758c3`) and re-presents.
  - speed > 0 calls a no-op hook between steps; so speed 0 or 1 = 64 steps, -4 = 16 steps. There is no timer: one step per presented frame. Port: one step per 60 Hz frame (assumption).
- v1.4 renders through a 16-bit surface; palette changes re-convert the last presented page (`FUN_00475a50`). Port: the palette is simply part of the presented frame.

### 2.3 Palette cycling — `FUN_0046d2da` (called every loop iteration on most screens)
Table at `0x48f128`, 3 entries `{start, count, period_cs, last}`:

| start | count | period |
|---|---|---|
| 3 | 8 | 16 cs |
| 11 | 8 | 9 cs |
| 19 | 13 | 9 cs |

When `|now - last| > period`: rotate entries [start, start+count) by one toward higher indices (last → first; `FUN_00475780` dir 1), `last = now`; if any rotated, re-present the palette.

### 2.4 Remap LUTs — `FUN_0045e3c7`
13 tables of 256 bytes at `DAT_0049fe08 + k*256`. Table k maps color c to the
palette index nearest to `mix(pal[target], pal[c], pct)` = `(target*pct + c*(100-pct))/100` per 6-bit channel:

| k | target | pct | k | target | pct |
|---|---|---|---|---|---|
| 0 | 0x21 | 0 (identity) | 7 | 0xea | 65 |
| 1 | 0x21 | 10 | 8 | 0x20 | 25 |
| 2 | 0x21 | 20 | 9 | 0xbd | 60 |
| 3 | 0x21 | 30 | 10 | 0x24 | 15 |
| 4 | 0x21 | 40 | 11 | 0x3c | 15 |
| 5 | 0x20 | 60 | 12 | 0x44 | 80 |
| 6 | 0x20 | 50 (= `DAT_004a0408`, shadows) | | | |

pct ≥ 100 fills the table with `target`. Nearest-color search: k-d tree
(`FUN_0045daaf` build, `FUN_0045dc15` insert, `FUN_0045dd6e` query) over
palette indices 0x20..0xfe only, search box ±32 per channel, squared
Euclidean distance, strict `<` (first found wins ties). For bit-exact tables
port the tree as is (insertion order: from the index nearest (31,31,31)
down to 0, then upward).

## 3. Timing

- `FUN_0046d5d6()` → **centiseconds**: `(minute*60 + second)*100 + ms/10` from `GetLocalTime` (wraps hourly; the original then waits too long once — don't replicate).
- `FUN_0046d688(t0, n)` → `|t0 - now| < n` (still waiting).
- `FUN_0046d626(t0, n)` → busy-wait until `|t0 - now| ≥ n`. `FUN_0046d652(n)` → wait n cs from now.
- All delays in code and data are centiseconds (e.g. frame-set delay 5 = 50 ms, text speed default 10 cs/char).

Port: `ticks_cs = Sys::ticks()/10`.

## 4. Input

### 4.1 Event record (36 bytes; global copy at `DAT_004952d8`)
Built by the message pump `FUN_004751a0` from Windows messages:

| Offset | Global | Field |
|---|---|---|
| +0x04 | `DAT_004952dc` | type: 1 key down, 2 key auto-repeat, 4 key up, 8 mouse down, 0x10 mouse up, 0x20 mouse move, 0x40 timer |
| +0x08 | `DAT_004952e0` | message time (ms) |
| +0x0c | `DAT_004952e4` | mouse x (client coords) |
| +0x10 | `DAT_004952e8` | mouse y |
| +0x14 | `DAT_004952ec` | keys: low byte = character (WM_CHAR/WM_SYSCHAR, 0 for non-character keys), bits 8..15 = PC set-1 scan code; repeat events add the repeat count in bits 16..31. Mouse: 1 = left, 4 = right |
| +0x18 | `DAT_004952f0` | modifiers: 1 left button held, 2 right held, 0x18 Shift, 0x20 Ctrl, 0x40 Alt, 0x8000 double click |

Key-up events carry the code that the matching key-down stored (table indexed by scan code).

### 4.2 Queue (`FUN_00474aac` and friends; max 100 events)
- Every queue call first pumps **one** Windows message.
- Consecutive mouse moves are merged into the pending move event.
- `FUN_00472f69(ev, mask)` get: remove and return the first queued event with `type & mask`; others stay. Screens use mask 0xffff.
- `FUN_00472f86(ev, mask)` peek. `FUN_00472f9e(mask)` drain all events matching mask (0x1f = everything but moves/timer; 7 = keys; 0x18 = mouse buttons).
- `FUN_00472fb2(ev, mask)` block until an event matching mask arrives (used after Enter/click to wait for the key-up 4 or button-up 0x10).

### 4.3 Scan codes used by the screens
0x01 Esc, 0x0f Tab, 0x1c Enter, 0x38 Alt, 0x39 Space, 0x48 Up, 0x49 PgUp,
0x4b Left, 0x4d Right, 0x50 Down, 0x51 PgDn. Screens switch on the scan code
(`(key>>8)&0xff`); the save screen switches on the character (0x0d / 0x20 select, 0x1b cancel, 0 → scan-code arrows).

Port: extend `s2engine::sys::InputEvent` to carry (type, char, scan, x, y,
mods) and map winit `KeyCode` → set-1 scan codes and characters; mouse moves
must become events (the original hit-tests on move events).

### 4.4 Per-iteration hook
Every screen loop calls `FUN_004198a4()` (music keep-alive: re-plays CD audio when needed; AUDIO doc) and usually `FUN_0046d2da()` (palette cycling).

## 5. Cursor (software)

- Shapes: `sysimg[n]` raw images; current shape `DAT_0048ec00` (`FUN_00433985`). 0xb = default pointer (hand), 0xc = alternative used on title / town (looks like the same hand art); 0..7 eight scroll arrows, 9 axe, 10 sword, 15 forbidden, 16 hourglass, 17/18 boots (move) (identified visually, guess for usage).
- Save-under buffer `DAT_004975a0` (34×34 raw + header). Position `DAT_0048ec04/08`, read from the OS cursor each time (`FUN_00473042` = GetCursorPos in client coords).
- `FUN_00433ac7(restore)`: if `restore` restore the old save-under; read the cursor position; save-under 34×34 at it; draw the shape transparent. Into the current page.
- `FUN_004339cc()`: capture the save-under only. `FUN_00433a1f()`: restore the save-under (erase the cursor).
- `FUN_0043399f(shape, restore)`: optional set shape, `FUN_00433ac7(restore)`, present. This is the usual "frame end" of a screen.
- `FUN_00433a6b(on)`: software-cursor enable counter (`DAT_004975a4`). `FUN_0043387a(on)`: OS cursor show counter around presents.
- `FUN_004338f1(x, y)`: warp the cursor (SetCursorPos). Screens warp it onto the selected item.

Port: draw the cursor into the frame exactly like the original (save-under included), so screens that present without redrawing keep the original look; warp = set the game-side mouse position and ask the frontend to move the OS pointer (optional).

## 6. UI resources

- **sysimg** `(&DAT_00493848)[i]`, loaded by `FUN_00421f14`: i = 0..141 → DAT entry 5 sub-image i (raw images); i = 142..199 → CEL entry 2 sub-image i-142 (RLE). Globals named `DAT_00493xxx` in that range are slots: `i = (addr - 0x493848)/4`.
  Known slots: 11/12 cursor, 13/14 click-to-continue blink frames, 20/21 unit shadow ellipses, 25 tile cursor, 33..37 colored range diamonds (64×32), 145/146 scroll arrows, 147 scroll thumb, 149 敵方行動, 150 我方行動, 151 回合, 154 message-box parchment (286×66), 155/156/157 banner left/middle/right, 194 map-menu panel (392×409), 195/196 buttons, 197 yes/no bar (249×38), 198 ○ pressed, 199 × pressed.
- **CEL2 on demand**: `FUN_00422878(id, may_fail)` loads CEL2 sub-image `id - 0x8e` (ids continue the sysimg numbering); `FUN_00422897(out[], n, ids[])` loads several. Used: 0xc9 overview panel, 0xca settings panel, 0xcb world-map banner, 0xcc save/load panel, 0xe9/0xea/0xeb/0xed/0xee map dialogue pieces.
- **DAT 7 images**: `FUN_00422551(id)` / `FUN_0042267c(out[], n, ids[])` (1-based, tagged images). Used: 3/4 difficulty screen, 5/6 world map.
- Generic sub-entry loader: `FUN_00421c97(kind, entry, index, may_fail)`.
- **PCX pictures** `FUN_00420fab(id, pal_out, kind, may_fail)`: kind 0..4 with bases {1,1,1,0x1a,9}; `id<0` → entry `|id|`, else `base-1+id`; kind 4 means PRG with base 9. Decodes standard PCX (0x80 header, 0xC0 RLE, 768-byte 8-bit palette at the end). DAT entries: 11 world map 1280×907, 12 town, 13 inn, 14 shop, 15 church, 16 dimmed inn, 17 dimmed church, 18 world map (travel), 19 title, 20 dimmed title, 26..39 ending/CG pictures (640×N), 40 ending backdrop.
- **UI strings** `DAT_00494768[i]` from BIN 5 (BIN 6 mainland): `i16 n; (u32 off, u32 size)[n]; strings` (`FUN_00424122`).

## 7. Text API — renderer object `DAT_00492d38`

Font decoding belongs to the TEXT sub-task; these are the call conventions.

`+0x00 screen*, +0x04 fg, +0x08 c2, +0x0c target, +0x10 style`

- **Small font** `FUN_0041df60(r, x, y, str, fg, c2, target, style)`: ASCII advance 8 px, Big5 16 px (16×16 glyphs). Stops at NUL or `\n`; chars < 0x1e drawn as 0x1e.
- **Large font** `FUN_0041f748(...)`: ASCII advance 16 px (drawn at x+2), Big5 advance 24 px (24×24 glyphs; mainland text uses 16×16 glyphs offset +4,+4).
- `target`: 0 = draw directly on the visible screen (`FUN_0046e5c1`), 1 = current page.
- `style`: 2 = plain fg; 1 = drop shadow (c2 at +1,+1, then fg); other (0/3) = 1-px outline in c2 around fg.
- Centering idiom: large font `x = cx - strlen(s)*6`, small font `x = cx - strlen(s)*4` (strlen in bytes).
- `FUN_0041deeb(r, x, y, value, fg, c2, target, style, fmt)` numbers with `fmt` ∈ {"%d","%1d","%2d","%3d","%4d","%5d"} in the small font.
- `FUN_0041f87c(r, x, y, str, fg, c2, target, style, rows, cols, line_h)` large-font text block, `*` = line break (TEXT doc for full markup).
- `FUN_0041e2e0(r, fg, c2, target)` set defaults (used by the narration typer).

## 8. Widgets

### 8.1 Screen loop idiom (every menu)
```
loop {
  FUN_004198a4(); FUN_0046d2da();                // music keep-alive, palette cycling
  if full_redraw { compose into page 1 (or 2) }  // background + static text
  if selection_changed {
     copy page 1 → page 0; draw highlight;      // page 0 = what gets presented
     if first { capture save-under }
     FUN_0043399f(-1, 0) or FUN_00433ac7(0)+present
     if first { fade in }
  } else if mouse_moved { FUN_0043399f(-1, 1) }  // cursor-only redraw
  if FUN_00472f69(&ev, 0xffff) { dispatch on ev.type then scan code / mouse rect }
}
FUN_00472f9e(0x1f)                               // flush on exit
```
Keyboard confirm waits for key-up (`FUN_00472fb2(ev,4)`), mouse confirm for button-up (`…,0x10`). Mouse hover moves the highlight; clicking the highlighted row confirms. Right click = cancel on most screens.

### 8.2 Message box — `FUN_00431f6d(x, y, text, wait)`
x clamped ≥ 16. Restore cursor, shadow (§1), sysimg 154 RLE at (x,y), text large font color 0x2a plain centred: `(x + w/2 - len*6, y - 12 + h/2)`, present. wait: -1 → warp cursor to (320,400) and return; -2 → return; else click-to-continue with timeout `wait` (0 = none).

### 8.3 Banner — `FUN_00432054(cx, cy, text, wait)`
Left cap sysimg 155, `len/2` middle pieces sysimg 156, right cap 157, centred on cx; text large font color 0x2a at `(cx - len*6, cy - 12)`; present; wait as 8.2 (no -2 case).

### 8.4 Click-to-continue — `FUN_0046d8a8(timeout_cs)`
Blinking icon (sysimg 13/14, toggles every 30 cs) drawn at the mouse position with its own save-under; follows mouse moves. Ends on: key down while Shift not held (waits for key-up unless the key is Alt, scan 0x38), mouse button up, or timeout (>0). Flushes input.

### 8.5 Wait for key — `FUN_0046d81f(timeout_cs)`
Ends on key down or mouse down, or after timeout (≤0 = never). Flushes.

### 8.6 Yes/No — `FUN_0042c663(x, y, start_yes, text, force, set_pal)` → 1 yes / 0 no
- If 確定詢問 is off and `force == 0`: return 1 without showing.
- Shadow + sysimg 197 at (x,y) opaque. Text color 0x2a plain: < 15 bytes → large font at `(x+0x58-len*6, y+8)`, else small font `(x+0x58-len*4, y+8)`.
- Selection outline color 0x3c at `(x+0xa9+31*sel, y+5)-(x+0xc4+31*sel, y+0x21)`; sel 0 = 是, 1 = 否; start sel = `start_yes==1 ? 0 : 1`; cursor warped to `(x+0xb8+31*sel, y+0x14)`.
- `set_pal` → master palette applied once after the first draw.
- Keys: Esc → no; Enter/Space → current; Tab/Left/Right toggle (sfx 100). Mouse: hover in y∈[y+4,y+0x20] over x∈[x+0xa9,x+0xc4] (是) / [x+200,x+0xe3] (否) moves selection; left click on 是/否 chooses; right click → no.
- Result: draw sysimg 198 at (x+0xa9,y+5) for yes or 199 at (x+200,y+5) for no; sfx 100 on keyboard/mouse confirm.

### 8.7 Hotspot picture menu — `FUN_00410060(rects, n, msgs, pic, flags, cancellable)` → 1..n or 0
- `rects[i] = (x0,y0,x1,y1)` i16; `msgs[0]` title (unused here), `msgs[1..n]` labels; `pic` indexes the loaded scene pictures (0 town, 1 inn, 2 shop, 3 church; `DAT_00492b8c[]`), palettes at `DAT_00492b78 + pic*0x300`.
- flags: 1 fade in on first frame, 2 fade out on exit, 4 palette cycling, 8 fast fades (16 steps).
- Starts with hotspot 1 selected and the cursor warped to its centre. Draws the picture opaque into page 1 on the first frame.
- Hover: label of the hotspot under the mouse in the small font (fg 3, outline 2, target 1, style 0) at `(mx - len*4, my - 30)` (y ≥ 30, x clamped to [0, 639-len*8]); leaving all rects clears the selection. Left click outside all rects toggles "show all labels" (each centred on its rect). Left click inside a rect returns it.
- Keys: Esc → 0 if cancellable; Enter/Space → current; Tab/Left/Right → next hotspot (wraps) and warps the cursor.
- Right click → 0 if cancellable.

### 8.8 Vertical list menus (title, map menu, system menu, overview)
Rows at a fixed pitch (title 30 px, others 31 px), normal/highlight colors per screen (title fg 0xf/sh 3 → fg 2; map menu 0xe9 → 0x77), keyboard Up/Down, mouse hover/click by row arithmetic, confirm draws a pressed-button image (sysimg 196 at panel+0x1d1) and plays sfx 100.
- Map menu `FUN_0042cc2c` (BATTLE calls it): panel sysimg 194 centred at (320,240); rows centred on x=320 at y = top+0x17c+31i (top = 240-h/2); sub-panels open at `DAT_00494868/c = (0x1c7-w/2, 0x14e-h/2)`. Items msg 0x79..0x7e (勝利條件, 總檢表, 系統設定, 自動作戰, 回合結束, 撤離戰場; the last only on type-2 stages). Results: 6 auto battle (confirm msg 0x2d), 3 end turn (msg 0x2c), 0xf retreat (msg 0x2e, sfx 0x82), or the sub-screen's code.
- System menu `FUN_0042d7cc`: msgs 讀取存檔 / 儲存檔案 / 設定 / 離開遊戲 (save panel 0xcc, settings panel 0xca). Load → 7/8, quit → 0xb.
- Overview `FUN_0042f033`: panel 0xc9; 檢閱人物 (`FUN_0042f620(0)`), 檢閱地圖 (`FUN_004301ae`).

### 8.9 Save/Load — `FUN_00430405(panel, mode, fade_in)`
- mode 0 load (title msg 0x80), 1 save (0x81), 2 blue-army pick (0x10f), 3 red-army pick (0x110). In 2P mode modes 0/1 do nothing.
- Panel = CEL2 0xcc RLE centred at `(320-w/2, 240-h/2)` = (px,py). Title big font fg 0x49 outline 0x37 at (px+0xb4, py+0xc).
- 13 rows: rows 0..9 = slot `page*10 + r` at y = py+0x2e+30r; rows 10..12 = slots 50..52 at y = py+0x16e+30(r-10). Slot number (`"%d"`, slot+1) at px+0x4a, description (50-byte header text, or msg 0x91 尚未使用) at px+0x60; normal fg 0xe5, selected 0xaf.
- Footer y = py+0x155 (fg 0xaf): 上一頁 msg 0x9a at px+0x56 when page > 0, 下一頁 0x9b at px+0x96 when page < 4, `"%d/%d"` at px+0x13c. Page `DAT_00497594` (0..4) persists between visits.
- Keys: Up/Down move (load: skip empty slots, wraps over 13 rows; save: rows 0..9 only, wraps), PgUp/PgDn page, Enter/Space choose, Esc cancel. Mouse: rows by y; footer click on 上一頁 (x px+0x54..px+0x88) / 下一頁 (px+0x96..px+200); right click cancel.
- Load: occupied slot → `FUN_00431845(path)`, cursor 0xb, return 8 for slot 50 else 7. Save: occupied → yes/no msg 0xfd 是否覆蓋存檔 at (176, 76+30·row); then `FUN_004312c4(slot+1)`, message box msg 0x34 存檔完畢 at (176,80) for 50 cs, return 9. Army pick: return slot+1.
- Save headers are scanned by `FUN_00430244` into `DAT_00494870` (53 × 50 bytes). Save file format: TABLES doc.

### 8.10 Settings — `FUN_0042deda(panel, in_battle)` (entry `FUN_0042efcd(where)`, panel CEL2 0xca)
- Panel centred at (320,240): origin `(320-w/2, 240-h/2)`; coordinates below are relative to `(-w/2, -h/2)` as in the code (add 0x140/0xf0 to get screen).
- Title msg 0x82 其它設定 at (+0x246,+0xfc) big font fg 0x49 outline 0x37.
- Four sliders (10 steps, 24 px apart, marker = filled rect color 0xbd 14×8 at x = +0x237+24·pos, rows y = +0x124, +0x143, +0x162, +0x181): 畫面速度 pos = 9 - min(9, v/3); 訊息速度 pos = 9 - min(9, v/5); 音樂聲量 pos = min(9, v/11) of MIDI or CD volume (CD when CD音軌 on); 音效聲量 pos = min(9, v/11). Labels msg 0x84..0x87, end labels 慢/快 (0x92/0x93), 小/大 (0x94/0x95) at +0x212/+0x338.
- Eight toggles in two columns (labels msg 0x88..0x8f, value 開啟/關閉 0x96/0x97 at x +0x23a / +0x32a, rows +0x1a1, +0x1c0, +0x1df, +0x1fe):
  left: 確定詢問 `64c`, 狀況視窗 `64d`, 快速切換 `64e`, 自動切換 `64f`; right: 遮掩網點 `652`, 動畫播放 `651`, CD音軌 `650`, 難度調整 (易/難 msg 0x98/0x99 with the difficulty number at +0x322).
- Toggling CD音軌 restarts the current music cue. Leaving the screen in battle after changing 遮掩網點 redraws both armies. `FUN_0042efcd(where)`: where 0 = title (writes SUPERSP.CFG via `FUN_00423eb9` afterwards), 1 = battle (darkens the page with LUT 6 first), 2 = inn.

### 8.11 Narration typer — `FUN_00431ca5(mode)`
Used for stage intro text (`DAT_00494744`). mode 1: clear + fade in; mode 0: darken the current page with LUT 6; mode 2: world-map travel (uses text color 10, waits for a key at the end instead of click-to-continue + fade). Text at x = 0x3a (0x9e mainland), y = 0x96, one character every 訊息速度 cs (the user can skip ahead, guess), via `FUN_0041fa6c` (TEXT markup). Ends with click-to-continue and fade out (modes 0/1).

### 8.12 Map dialogue box — `FUN_004188ca(script, no_anim)`
For each line of the current event (`script+0x18` event table, `+0x1c` 23-byte line records):
- pieces: CEL2 0xe9, 0xea (open/close frames), 0xeb (box), 0xed (tail, box above unit), 0xee (tail, box below unit).
- speaker = unit id at line+0; position from the player or enemy army (`FUN_00442caa`); not on the map → box at (6, 0x7d) without tail.
- speaker screen y < 0x137 → box below: `by = uy - 2` (clamped so the box fits above 0x1df), tail 0xee at `(ux+0x21, by-0x20)`; else box above: `by = uy - 100 - h` (≥ 0x20), tail 0xed at `(ux+0xc, by+0xd7)`. Tail x clamped to [0, 0x24f].
- open animation: redraw the map (`FUN_00424c5f`), frame 0xe9 then 0xea centred at (320, by + h/2), present each, 4 cs apart unless input is pending.
- box opaque at (`DAT_0048e294`, by) (x is a constant 6; the global's initial value is (6,125) and it is only ever set to 6), tail transparent; portrait (tagged image from the script's face table) at `(bx+0x4a - w/2, by+0x1d)`; speaker name large font fg 0x36 at `(bx+0x4a - len*6, by+0xa8)`.
- per-line music cue before (line+2) and after (line+4); text via `FUN_00418e73(text)` (TEXT doc: `D%02x`, `W%02x,%02x` codes).
- close animation (0xea then 0xe9, 3 cs) unless it is the last line and story flag 2 (`DAT_00494509`) is set, or mode 1.
- lines whose speaker is in neither army (`FUN_0043a860`) are skipped entirely.
- text markup `@@Wcc,ss@@` colours, `@@Ddd@@` delay in **centiseconds** per character (compared with `FUN_0046d688`), `@@RW@@` / `@@RD@@` reset; Enter / Space / Alt / click finish the page; a full page shows the 0xec arrow at (bx+0x244, by+0xb7) and waits (click-to-continue, 40 s).

## 9. Sound effects used by the UI
`FUN_00419b1d(_, id, 0)`: 100 = menu confirm/toggle, 0x82 = retreat. Music: `FUN_004198d4(cue, -1, 1)`, stop `FUN_00419a7b()` (AUDIO doc). Cues seen: 0x19 title/difficulty, 4 town, 0x25 church.

## 10. Open questions
- Exact meaning of SET fields 自動切換 / 快速切換 / 狀況視窗 / 畫面速度 at their use sites (BATTLE).
- Cursor shapes 0xb vs 0xc (both look like the same hand).
- `FUN_0046ed32`'s blit variants; `FUN_0046f839` clipping.
- Settings changed in battle or at the inn are not written to SUPERSP.CFG by `FUN_0042efcd` (only where=0); check whether another path saves them (e.g. with the save file).
