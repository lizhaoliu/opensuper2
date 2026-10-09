# Battle system specification

Port-ready description of the tactical battle of 超時空英雄傳說2 北方密使,
reverse-engineered from the unpacked `WSUPERSP.EXE`. Struct layouts and field
names are in `structs.md`; enemy AI is in `ai.md`. Every rule cites the
function it comes from. Integer arithmetic is C `int` (32-bit, division
truncates toward zero) unless stated. `rules[n]` = signed byte n of the rules
constants table (`DAT_0049456c`, BIN entry 10). "Guess" marks inference.

## 1. Time, RNG, presentation

**Clock.** `FUN_0046d5d6` returns centiseconds: `(minute*60 + second)*100 +
ms/10` from `GetLocalTime` (wraps hourly). `FUN_0046d626(t0, n)` busy-waits
until `|t0 − now| ≥ n`. All animation delays in the battle code are in
centiseconds (e.g. walking: 8 per step, hit flash: 12, damage popup: 3/2/1).

**RNG** (`FUN_0046d6b5`, `FUN_0046d776`, `FUN_0046d7ca`, MSVC `rand`
`FUN_0047abfd`):

```text
rand():            state = state*0x343fd + 0x269ec3; return (state >> 16) & 0x7fff
init (FUN_0046d6b5): perm[r*10 + c] = c*10 + r ... i.e. perm[i] = column-major 0..99,
                     then 600× swap(perm[randn(100)], perm[randn(100)])
reseed:            a shared counter is incremented by every rand100/randn call; when it
                   exceeds 199 it is reset to 0 and srand(time(NULL)) is called first.
rand100():         perm[rand() % 100]          // 0..99, FUN_0046d776
randn(n):          n == 0 ? 0 : rand() % n     // FUN_0046d7ca (n==0 does not tick the counter)
```

Because of the time reseed the original is not reproducible; a port should keep
the same call *sites and order* (so distributions match) and may seed
deterministically for tests.

**Presentation.** There is no separate battle screen: attacks animate on the map
(attacker swing frames, target flash, floating numbers) and spells play the
CEL entry 1 frame sets (`FUN_00466507`).

## 2. Stage loop and turn structure (`FUN_0040d108`)

```text
stage_loop(entry):
  init stage (script, armies, map) ...
  loop:
    if turn == 1 (or entry 7/8 = resumed):
        first-turn setup, music, show objectives once (FUN_00432054)
    else:
        upkeep_both()                     // FUN_00433831, §11
        r = check_end(); if r: goto finish
    script.turn_start()                   // FUN_00417a1d (TEXT sub-task)
    r = check_end(); if r: goto finish    // FUN_0043311d
    autosave slot 0x34                    // FUN_0043117c → FUN_004312c4 (not in VS)
    set combat context for player         // FUN_004411cf
    phase banner 0                        // FUN_00433453 (palette dim + sliding text)
    ai_wake_triggers(player)              // FUN_004447e6, ai.md §1
    r = VS ? vs_phase(0) : player_phase() // FUN_0043297f / FUN_004323a3
    phase_reset(player)                   // FUN_00440e71, §11
    if r in {victory, defeat, quit, load}: goto finish
    r = end_of_phase_terrain(player)      // FUN_0045b29d, §11 (also re-checks end)
    if r ...: goto finish                 // victory (1) first runs script hook FUN_00417b74
    script.phase_end()                    // FUN_00417cbd
    set combat context for enemy; banner 1; ai_wake_triggers(enemy)
    r = VS ? vs_phase(1) : enemy_phase()  // FUN_0043310e = ai_phase(enemy, 1)
    phase_reset(enemy); r = end_of_phase_terrain(enemy) ...
    script.phase_end()
    turn += 1
finish:
  VS: show winner, return 0x14
  1 (victory): victory anim (rules[0xfc]), rewards (FUN_0045cfad), survivors +30 EXP (FUN_0045d713)
  2 (defeat):  FUN_0041a765
```

`check_end()` (`FUN_0043311d`) = script check (`FUN_0041be4c(0)`), else 2 if
`defeat()` holds, else 1 if `victory()` holds, else 0.

### Defeat (`FUN_00433161`)

- VS: player side has no deployed living unit.
- turn ≥ 100, or script flag `DAT_0049450c`.
- Any of the stage's defeat conditions (`DAT_00494754[i]`, count at stage header +0xc):
  - `0` → no deployed living NPC ally (`npc == 1`) remains in the player army (`FUN_0044e99d(-1)`)
  - `1..0x2b` → the unit with that char id is dead
  - `101..190` → `cond − 100 ≤ turn` (turn limit)

### Victory (`FUN_00433257`)

Script flag `DAT_0049450b` wins immediately. Otherwise two condition groups
(counts at stage header +8 and +10); a group wins when **all** its conditions
hold; `DAT_004952cc` = winning group + 1 (selects the story route).

| cond | holds when |
|---|---|
| 0 | every enemy deployed unit is dead |
| 1..20 | event point *cond* has been reached (`DAT_00494598[(cond−1)*4]` != 0, needs `cond ≤ DAT_0049478c`) |
| 0x1f | the protagonist (char 0xb, or 1 if `DAT_00497b54`) stands on overlay class 6 and `DAT_00494508` is set |
| 0x20 | any player non-NPC, non-summon unit stands on overlay class 6 |
| 0x21 | every enemy boss (`rank == 1`) is dead |
| 0x22 | every enemy non-boss is dead |
| other | never |

## 3. Player phase (`FUN_004323a3`)

Event loop: `FUN_0042b72e` turns input into a code. Mouse: moving to the
screen edge (10 px) scrolls (codes −9..−0x10); left click on a cell = −4 /
select; right click = −3. Keyboard (scan codes, `FUN_0042b9f8` jump table):

| key | code | action |
|---|---|---|
| Esc, `1` | −1 | system menu (`FUN_0042cc2c`) |
| arrows (extended) | −0x10..−0xa | scroll the map |
| numpad 8 / − / 2 (`0x48/0x4a/0x50`) | −6 | cursor up-left / up-right / down-left (the down-right handler at 0x42be27 is unreachable from the table) |
| numpad 9 / 3 (`0x49/0x51`) | −6 | cursor straight up / down (y∓2) |
| numpad 5 / + (`0x4c/0x4e`) | −6 | cursor x−1 / x+1 |
| Enter/Ctrl (`0x1d`), CapsLock (`0x3a`) | 0x1c / −8 | act on cursor cell |
| Q (`0x10`) | −0x15 | jump to next ready unit (`FUN_00433b80`) |
| W / E (`0x11/0x12`) | −0x1f / −0x20 | select unit at cursor (then open 行動 / 移動 directly, `FUN_00436186`) |
| F3 | −0x1c | ? (`FUN_0043225f`) |
| F4 | −0x1b | save/load menu (`FUN_004322bc`) |
| F5 | −0x1a | **auto battle**: AI plays all player units (`FUN_00433d8f`) |
| F6 | −0x19 | **end turn**: every remaining unit rests (`FUN_00441038`) |
| F7 | −0x18 | toggle `DAT_0049464d` (status window) |
| F8 | −0x17 | unit info at cursor (`FUN_0044fe15`) |
| `,` | −0x16 | objectives (`FUN_004301ae`) |
| `/` `.` | −0x14/−0x13 | music+sfx volume ±1 |
| numpad 1/4 (`0x4f/0x4b`) | −0x1e/−0x1d | message speed ±1 (`DAT_00494648`) |

Cursor movement on the staggered grid (the four edge-adjacent neighbours of
(x, y)): up-left `(x − [y even], y−1)`, up-right `(x + [y odd], y−1)`,
down-left `(x − [y even], y+1)`, down-right `(x + [y odd], y+1)`.

Selecting (`FUN_004346cb(army, x, y)`): own unit at the cell → if it has
`act == 2`, or is an NPC (`npc == 1`) without army flag 0x20, or `buffs & 2`
(berserk), or cannot act → info only (returns 2). Fear (`status & 0x80000`):
`rand100() ≥ 51` → the unit loses its turn (`act = 2`). Otherwise
`act = 1` and the command loop `FUN_00434893` runs. Enemy unit → info.
Empty cell → system menu.

The phase ends when the "next ready unit" cycle (`FUN_00433b80`) finds nobody or
`FUN_00440fa1` reports that no deployed unit with `act == 0` that can act and is
player-controllable (not an NPC, unless army flag 0x20) remains. Then
`ai_phase(player, 1)` (`FUN_00433da4`) lets NPC allies and berserk units act
(`ai.md`).

### 3.1 Command menu (`FUN_00435a82`, `FUN_00434893`)

AP (行動點) is the central resource: every command costs AP from `unit.ap`
and a unit can keep issuing commands until it waits/rests or runs out.

Main menu entries (icon id → condition → effect):

| id | label | shown when | effect |
|---|---|---|---|
| 0x9e | 行動 | any action-submenu entry is available | opens the action submenu |
| 0xa0 | 移動 | not moved yet and `FUN_0043a7ea` | §4; once per turn; cancel undoes it |
| 0xa2 | 道具 | not NPC, not cursed, `ap ≥ rules[0xf1]` or `ap ≥ rules[0xf2]` | submenu: 0xb8 使用物品, 0xba 裝備物品 |
| 0xa4 | 狀態 | always | status screen |
| 0xbc | 轉職 | not summon/NPC, `FUN_0043a522` | class change §10 |
| 0xa6 | 休息 | always (greyed if `ap < rules[0xf0]`) | choose facing; if `ap ≥ rules[0xf0]`: `ap −= rules[0xf0]` and rest-heal (§11); unit done |

Action submenu (`DAT_0049f9e8`), each entry bound to one AtkSkill of the class
skill list for which a target is in range from the current cell
(`FUN_00442c1c`) and `ap ≥ attack_cost`:

| id | label | AtkSkill type |
|---|---|---|
| 0xa8 | 攻擊 | 1..7 except 5 (name = weapon name if armed) |
| 0xaa | 射擊 | 5 (bows) |
| 0xac | 魔法 | magic: knows a magic category, not 禁錮, not sealed, `ap ≥ spell_cost` |
| 0xae | 招式 | technique: knows a technique category, not sealed, `ap ≥ spell_cost` |
| 0xb0 | 魔擊 | 0x24 |
| 0xb2 | 魔法劍 | 0x20 |
| 0xb4 | 鷹襲 | 0x17 |
| 0xb6 | (skill name) | any other type except 0xb/0xc |

AP costs:

```text
attack_cost = rules[0xee] − (ability & 0x20000000 ? 4 : 0)
              + (turn_flags & 2 && !(ability & 0x8000) ? rules[0xf3] : 0)
spell_cost  = rules[0xef] + (turn_flags & 0xc ? rules[0xf3] : 0)
```

After an attack: `ap −= rules[0xee]` (+4 back with ability 0x20000000);
the first attack sets `turn_flags |= 2`, later ones also pay `rules[0xf3]`
unless ability 0x8000; `ap_carry = min(ap_carry, ap)`. The unit may act again
unless it became incapacitated. Magic: `FUN_0045f78b` (§8.1). Use item:
`ap −= rules[0xf1]`, `turn_flags |= 0x10`, then the unit rests/ends
(`FUN_0044d7c1(..,2)`). Equip: `ap −= rules[0xf2]`, `turn_flags |= 0x20`,
continue.

## 4. Movement

### 4.1 Reachable cells (`FUN_004709dc`, `FUN_00471bda`, `FUN_00471e51`)

Depth-first flood fill with improvement over the 4 neighbours, AP as budget.

```text
root: (unit.x, unit.y), budget = min(unit.ap − rules[0xe8], 110), can_stop = 1, height = h(root)
expand(node): for dir in [up-right, down-left, up-left, down-right]:
    (nx, ny) = neighbour(node, dir); skip if it is node's parent cell
    child = try_step(node, nx, ny); if child: expand(child)

try_step(n, x, y):
  if n.budget < 1 or (x,y) == root: no
  cell = map cell (rows 1..2h−2, even-row columns 1..w−2); none → no
  if (y − cell.height)*16 < 0: no                      // above the top of the map
  dh = n.height − cell.height                           // >0 = going down
  if |dh| ≥ 11: no
  if !(ability & 0x10000) && !(status & 0x400 /*輕功*/):
      if (dh > 4 || dh < −3) && move_type == 0: no
      climb = dh < 1 ? −dh : 0
  else climb = 0
  can_stop = 1
  if !(flags & 0x8000):
      if move_type != 2 && n is not root:
          adj = enemies adjacent to n (FUN_004414df: 4 neighbours, height diff in −4..3)
          if adj not empty: if move_type == 0 or any adj has move_type != 0: no   // zone of control
      if enemy at (x,y): if move_type == 0 or that enemy's move_type != 0: no; else can_stop = 0
      if own unit at (x,y): can_stop = 0
  t = terrain(x, y, unit)                               // §4.3
  if t == impassable: no;  if t == pass_only: can_stop = 0
  if map object at (x,y) with code & 0x8000: no
  if move_type == 5: climb = 0
  budget' = move_type == 0 ? n.budget − cost − climb*rules[0xeb]
                           : n.budget − climb*rules[0xea] − rules[0xe9]
  if budget' < 0: no
  if (x,y) already in the tree: if existing.budget ≥ budget': no
                                else delete existing subtree (FUN_00472403) and continue
  add node {x, y, budget', cell.height, can_stop, parent = n}
  mark = 2 if (flags & 2) && budget' ≥ spell_cost
         else 1 if (flags & 1) && budget' ≥ attack_cost else 0
```

(Exact rule for `mark` when `flags & 2` is set but `budget' < spell_cost`: falls
back to the attack test.) The tree holds at most 2522 nodes (`FUN_0046cf8f(5)` if exceeded).

Destination cells offered to the player are nodes with `can_stop == 1`
(`FUN_00470c27`); the cursor icon shows `mark` (0 none / 1 attack / 2 spell).

### 4.2 Executing a move (`FUN_0044cccd`, `FUN_0044c331`)

`act = 3`; the path is rebuilt from the destination's parent chain; the unit
steps cell by cell (8 cs per step) with a footstep sound chosen by terrain;
`unit.ap = destination.budget` (so a move costs `rules[0xe8]` + step costs);
`ap_carry = min(ap_carry, ap)`; `act = 4`; `turn_flags |= 1`. Adjacent
invisible/hidden enemies are revealed (`FUN_0043c122`). The unit then auto-faces
the nearest enemy that could reach it (`FUN_0044e37d`). Flyers/levitators use
separate animation routines (`FUN_0044c631`, `FUN_0044c963`).

### 4.3 Terrain (`FUN_00427e1f`)

```text
tile = cell.tile; if tile == 0 → impassable
code = map.tile_terrain[tile−1]; if code == 0 → impassable
class = code >> 8; type = code & 0xff
(cost, atk, def) = terrain_tbl[(unit.kind−1) * stride + max(type−1, 0)]   // per-CLASS rows (DAT 6)
if class == 5 or cost == −1 → impassable
mc = ClassRec.move_class; mt = move_type(unit)
mc 1..5, 10: class ∈ {0,1,2,4} → ok; else (water etc.) mt != 0 ? pass_only : impassable
mc 6..8:     class ∈ {2,3} → pass_only else ok
mc 9:        class ∈ {1,2} → ok; else mt != 0 ? pass_only : impassable
other mc → impassable
if cell.overlay: oc = overlay_terrain[overlay−1]; oclass = oc >> 8; otype = oc & 0xff
    otype > overlay_count → impassable; oclass == 1 or entry cost == −1 → impassable
    oclass == 2 → pass_only; oclass == 10 → mt != 0 ? pass_only : impassable
    (cost, atk, def) += overlay_tbl[otype−1 + overlay_base]
```

`atk`/`def` feed the damage formula (§5.4). With difficulty 1 terrain and height
are ignored in combat (`DAT_00494684 == 1`).

## 5. Physical combat

### 5.1 Attack range (`FUN_00406a36`, `FUN_0043b464`)

```text
(min, max, shape):
  if AtkSkill.type ∉ 1..10 or no weapon: (skill.min, skill.max, skill.shape)
  else code = weapon.range; max = code % 10; min = code == 1 ? 1 : code < 10 ? 2 : 1
       shape = weapon.shape > 0 ? weapon.shape : skill.shape
  if min == 0: min = max = 1;  if max < min: max = min
```

- `shape == 2` → **ring** (`FUN_00406f27`): for c = 1..max (c ≥ min): the
  staggered "ring" at distance c around the unit (top/bottom rows at y∓c,
  columns x0..x0+c; side cells every other row). Cells must be valid and not
  blocked by a 0x8000 object.
- otherwise → **four rays** (`FUN_00406d0c`) along up-right, down-left, up-left,
  down-right; steps 1..max, cells kept from step `min`; a ray stops at an
  invalid/blocked cell or where the height difference to the attacker exceeds
  +2 (target higher) or −3 (lower). `shape == 3` additionally hits every enemy
  on the ray up to the chosen target (piercing).

Targets = cells holding a visible enemy (`FUN_00440b16(.., 1)`); each target
stores its facing toward the attacker. Grid distance (`FUN_00472d46`) is the
ring index of the same ring construction (≤ 80).

### 5.2 Duel setup (`FUN_00408078` → `FUN_00409488`, `FUN_00409642`)

- **Counter skill**: if the defender is not incapacitated (`buffs & 0x200`,
  `status & 0x2b000`), collect every AtkSkill of its class (not types 0xb/0xc)
  usable with its weapon (`FUN_0043ae12`) whose target list from the defender's
  cell contains the attacker; pick one with `randn(count)`. None → −1. (This
  consumes RNG even for forecasts.)
- **Facing** (`FUN_004080b0`): 1 front / 2 side / 3 back, from the attacker's
  cell relative to the defender and the defender's facing; ability 0x2000 on the
  defender → always 1; difficulty 1 → always 1.
- **Proficiency** `prof`: AtkSkill.type < 0x15 → weapon type proficiency
  `prof[weapon.type − 1]` (0 if unarmed); else `prof[17]`.
- **Terrain triples** for both sides (§4.3).
- `hp_lost% = (hp_max − hp) * 100 / hp_max` for both.

### 5.3 Hit chance

```text
hit_attack(duel, random):                   // FUN_0040881a first half
  hd = height(att) − height(def)            // 0 on difficulty 1
  acc = random ? desperation(att.skill_cur, att_hp_lost) : att.skill_cur
  fb = {3: 5, 2: 3}[facing] or 0
  hit = acc * (hd*1000 + 8000 + fb*800 + att_prof) / 8000 − def.react
  hit_mods(att, def, &hit, skill)           // FUN_00409f0b
  hit = hit * 12 / 10                       // actual roll only (forecast omits ×1.2)
  hit = vary(hit, pair 2, att_army, random) clamp 1..99     // FUN_004078df
  if def.buffs & 0x200 or def.status & 0x2a000: hit = 99
desperation(v, lost) (FUN_00409e98):        // 2 rand100 calls max
  71 ≤ lost ≤ 90: rand100 < 90 ? v*13/10 : v*3/10
  lost > 90:      rand100 < 95 ? v + v/2 : v/2      (v>>1 arithmetic shift)
  else v
hit_mods (FUN_00409f0b):
  if AtkSkill.type < 0x15: hit += (weapon ? weapon.hit : 90) * skill.hit_bonus / 100
  else hit += skill.hit_bonus
  hit −= Σ def.equip[i].evasion
  if def.abilities & 2: hit −= 30
  if def.status & 4 or def has item ability 0x10: hit −= hit/10
  clamp 1..100
vary(v, pair k, army, random):
  m = random ? rules[2k] + randn(rules[2k+1] + 3) : rules[2k] + (rules[2k+1] + 3)/2
  v = v * m / 10
  if !VS and army != player: v = v * (rules[0xf3 + difficulty] + 100) / 100
```

The roll hits when `hit ≥ rand100()`.

### 5.4 Damage

```text
damage_attack (FUN_0040881a second half, only when hit):
  if def.status & 0x800 (無我): 0
  div = random_divisor()                    // FUN_0040a2ed, ≈100..149
  hdc = clamp(hd, −8, 4); k = hdc < 0 ? 125 : 250
  fd = {3: 6, 2: 3}[facing] or 0
  pierce = (att.status & 0x80 || att.abilities & 0x40000000) && def.rank != 1
  d = ((att.atk_cur − (pierce ? 0 : def.def_cur))
        * (hdc*k + 1000 + (att_terrain.atk − def_terrain.def)*10 + att_prof) / 10) / div
  d = damage_mods(d, fd, att, def, skill)   // FUN_0040a080
  if hd > 0: d += hd
  d = vary(d, pair 0, att_army, true) (min 1, no max)       // FUN_0040797f
  if d < 3: r = rand100(); d = r<1 ? 5 : r<6 ? 4 : r<16 ? 3 : r<46 ? 2 : 1
random_divisor():  r = rand100(); n = randn(10)
  r<2: n<5 ? 100+n : 140+n;  r<10: n<5 ? 105+n : 135+n;  r<25: n<5 ? 110+n : 130+n
  r<50: n<5 ? 115+n : 125+n;  else 120+n
damage_mods(d, fd, att, def, skill):
  aff = affinity[att.class_type][def.class_type]; if aff > 0 && def.abilities & 0x800: aff −= aff/2
  d = (d * (fd*10 + 100 + aff) / 100) * AtkSkill.power / 10
  soft cap: 100<d≤200: 100 + (d−100)*90/100;  ≤300: 190 + (d−200)*80/100
            ≤400: 270 + (d−300)*70/100;       >400: 340 + (d−400)*60/100
  d += min(power(att, 0, 0) / 1000, 15)
  if def.status & 0x4000000 or def has item ability 0xd: d −= d/3
  if def.status & 0x20: d = d < 151 ? 1 : d − 150
  if def.status & 0x40: d = d < 181 ? 1 : d − 180
  if att.status & 0x40000: d /= 2
  if def.buffs & 0x1000: d = d*80/100;  if def.buffs & 0x2000: d = d*70/100
```

The **forecast** (`FUN_00409642`, shown to the player and used by the AI) uses
the same formulas without random parts: `acc = skill_cur`, no ×1.2, deterministic
`vary`, damage `((atk−def)*(...)/1000)*100/125` instead of `/div`; and sets
hit and damage to 0 if the defender has ability 0x1000000 or status 0x800.
If the forecast hit and damage are both 0 the real roll returns 0 too.

### 5.5 Counter-attack values

```text
hit_counter (FUN_00408bae): if counter_skill == −1 or both forecast counter values 0: 0
  hd2 = height(def) − height(att); fb = {3: −5, 2: −3}[facing] or 0
  c = def_acc * (hd2*1000 + 8000 + fb*800 + def_prof) / 8000 − att.react
      // def_acc was rolled by the preceding hit_attack call
  hit_mods(def, att, &c, counter_skill)
  if def.status & 0x1000 (blind): c /= 10
  c = c*12/10;  c = (randn(4)+3) * c / 10;  c = vary(c, pair 2, def_army, true)
  miss if c < rand100(); 0 if att.status & 0x800
  damage as §5.4 with roles swapped, fd = {3: −6, 2: −3}, then
  d = (randn(4)+2) * d / 10 + hd2
  if def.abilities & 0x400: d = def.atk          (fixed counter)
  if def has item ability 0x1e: d *= 3
  vary(d, pair 0, def_army, true); if d < 1: same 1..5 table as §5.4
```

Forecast counter: hit ×45/100, damage ×35/100 (`FUN_00409642`).

### 5.6 Criticals, instant death, on-hit effects

- **Critical** (`FUN_004091ab` / counter `FUN_00409213`): `chance = Σ att equip
  crit + att.skill_cur + fb − def.react − Σ def equip evasion` (fb = 10 back, 5
  side; −10/−5 for counters); +30 if attacker ability 1; −30 if defender ability
  2; −chance/10 if defender has status 4 or item ability 0x10; clamp 1..50;
  0 if forecast is 0/0, defender ability 4, defender army flag 2, defender
  status 0x4000000 or item ability 0xd. Critical when `rand100() < chance`:
  the attack is **rolled again and added** (a second full hit+damage roll).
- **Instant death** (`FUN_00409094`): never on char ids 1 and 0xb or defenders
  with status 0x4000000 / item ability 0xd; chance 50 if defender class type
  0xe and not boss and attacker has item ability 0x3e, else 5 if attacker
  ability 0x200 and `def.rank & 3 == 0`, else 0. Success (`rand100() <
  chance`) sets damage = current HP and cancels the crit.
- **On-hit effects** (`FUN_00438c92`, for each target with damage > 0, alive):
  wake from sleep (clear 0x20000); paralysis (0x8000) cleared on `rand100() <
  71`. Unless the target has 屏障 (0x100000) or ability 0x4000, with physical
  skills (type < 0xb): weapon item ability 3 or attacker ability 0x80000 →
  poison (3 turns) at 30% (15% if target has item ability 0xe), not on bosses or
  ability-0x1000 targets; item ability 7 → paralysis 3 turns 30%/15%; attacker
  ability 0x100000 → 禁錮 3 turns 30%/15% (not on important units); attacker
  ability 0x800000 → **drain** (damage negated = heals attacker by the amount);
  attacker ability 0x10000000 → steals 25% of the target's MP and ST (if > 4).
  Then the skill's own effect (`AtkSkill.+0x11`) with chance `+0x12` (halved by
  item ability 0xe; ×0.7 for type 8 on important units), duration `+0x13`:
  1 poison, 2 paralysis, 3 petrify (not important), 4 blind (not important),
  5 berserk (+ATK/2), 6 curse, 7 drain, 8 freeze, 9 kill undead (class type
  0xe, not boss), 10 kill (not important), 11 weak.

### 5.7 Attack sequence (`FUN_00436e46`, counters `FUN_00437b9c`)

```text
attack(att, target_list, main_target):
  script.on_attack(att, main_target.char_id)          // FUN_00417d8e
  targets = [main]  (+ for piercing shape: every enemy on the ray before main, each with its own Duel)
  for t in targets: dmg[t] = hit_attack+damage; crit[t] = crit roll; instant death roll;
                    if crit: dmg[t] += second roll; if dmg[t] == 0: crit = 0
  animate (twice if any crit)
  on_hit_effects(att, targets, dmg, skill)
  for t in targets:
      if t.ai_mode == 2: t.ai_mode = 1, t.ai_sub = 0                  // provoked
      dmg == 0 → "miss" popup; else apply (FUN_00439e2a): HP drops in 1,2,4,8.. chunks;
          negative dmg (drain) heals the attacker by the same amount
      if t.hp == 0:
          item ability 0x19 → revive to full; 0x1c → revive to full and consume that item
          else die(t) (§9); att.kills += 1
      elif item ability 0x1d and rand100() < 30: t heals 2×dmg
  counters(...)                                      // below
  if att alive: exp = Σ exp_gain(duel_t, dmg[t]); drops; proficiency +1 (FUN_00452da5);
                award_exp(att, exp, items) (§9); level-up check (FUN_00453725)

counters (FUN_00437b9c): for each target t (in order) that is alive, not incapacitated
  and has counter_skill != −1:
    t attacks back with counter_skill: piercing shape also hits other units of the
    attacker's side on the ray before the attacker (normal formulas); the attacker
    takes hit_counter/crit_counter values; on-hit effects; deaths as above;
    EXP for t from every unit it damaged; stop all counters once the attacker dies.
  then award EXP/items to each counter-attacker.
```

## 6. Power rating (`FUN_0045781c(u, mode, part)`)

Used for EXP, AI and some spell formulas. `mode 2` uses max stats, `mode 1`
uses `hp_before_hit`, `mode 0` current values.

```text
P1 (magic)   = ((#magic categories known + 10) * MAG' * (MP + 1)) / (rules[0xd6]*10*rules[0xd7])
               MAG' = MAG * 1.5 if item ability 0x12
P2 (tech)    = ((#technique categories known + 10) * sword * (ST + 1)) / (rules[0xd8]*10*rules[0xd9])
P3 (body)    = HP / rules[0xda] + ((atk_cur + def_cur) * (skill_cur + react) / rules[0xdb]) / rules[0xdc]
P4 (skills)  = trunc(sqrt(n) * 400.0 * n), n = popcount(abilities & !0x8c260040)
               (excluded bits: 0x40, 0x20000, 0x40000, 0x200000, 0x4000000, 0x8000000, 0x80000000)
part 0       = max(1, ((P1+P2+P3+P4) * ap_max / rules[0xdf]) * ClassRec.+0x21 / 10)
parts 1..4 return P1..P4 (and cache them); parts 5,6,7,8 return cached P1,P2,P3,P4.
```

## 7. EXP, level up, proficiency

```text
exp_gain(duel, dmg) (FUN_00408fc9):   // attacker = duel.att, victim = duel.def (FUN_00408fa2 swaps)
  if dmg == 0 and victim alive: 1
  pa = power(att, 1, 0); pv = power(victim, 1, 0)
  e = max(1, ((dmg * pv * rules[0xe5]) / (pv + pa)) / rules[0xe1])
  if victim.hp == 0: e += 3 + pv * rules[0xe2] / pa
kill bonus (FUN_00453608): e += victim.bounty; items: VS → victim's equipment,
  else (if killer is enemy-side or not an NPC) victim.drops[0..4]
award_exp(u, e, items) (FUN_00452e38):
  if e: if u.abilities & 0x40: e += e/3
        e = clamp(e * (rules[0x10] + 3 + rules[0x11]) / 10, 1, 999)
        player side, not VS: difficulty 1 → ×3, 2 → ×2
  u.exp = min(u.exp + e, 999); items → army inventory (cap 99 each); popups for the player side
level_up (FUN_00453725, FUN_004537da): while exp ≥ exp_next and level < 99:
  level += 1; exp −= exp_next; g = growth(kind, level); apply_growth(u, g)
growth(kind, level) (FUN_0042345b, FUN_00423607): 9 records (a,b,c,d,e,f) per class in BIN 8
  value = (b == 0 || d == 0) ? 0
          : trunc(sqrt(a/b * level) + c/d + randn(e+1) / (randn(f+1)+1) + 0.5)   (doubles)
  order of evaluation: HP, MP, ST, HP (re-rolled, overwrites), ATK, DEF, 反應, 戰技, MAG, 劍術
apply_growth (FUN_00456c88): flags bit 1 → +1 each; HP max&cur (≤5000), MP, ST, ATK (0xc3 & 0x134),
  DEF (0xc5 & 0x136), 反應, 戰技 (0xc9 & 0x138), MAG (0xbd & 0x12a), 劍術; others ≤ 999
weapon proficiency (FUN_00452da5): AtkSkill type 2..7 → prof[type−2] += 1 (cap 1500); type ≥ 0x15
  and not 0xb/0xc → prof[17] += 1; others nothing
```

Victory: every surviving deployed player unit gets +30 EXP then levels up.

## 8. Magic, techniques, summons

Menu (`FUN_0045f212` magic / `FUN_0045f3b0` technique): one page per known
category, entries = learned skills passing `FUN_0045f145` (combo skills need a
partner `FUN_0046552e`; summon skills (effect 0x15..0x1f) need no active summon:
"只能召喚一個") and `FUN_0046586d` (enough MP/ST/HP/AP).

### 8.1 Cost (`FUN_0045f78b`)

`ap −= rules[0xef]` (+`rules[0xf3]` if this kind — 4 magic, 8 technique — was
already used this turn); clamp carry. Unless `DAT_004a1f54[id−1] == 2`:
cost flag 1 → MP −= cost (half with ability 0x20); 2 → ST −= cost; 4 → HP −=
cost (min 1). Combo partners pay the same AP and cost.

### 8.2 Combos (合擊, 情侶合技) (`FUN_0046552e`, `FUN_0046570c`)

A skill with `MagicRec.+0x20 & 7` needs a partner: an adjacent (4-neighbour)
ally with `act == 0` that also knows the skill, standing within height 3.
Couple combos (`& 6`) additionally need `caster.affinity[partner.char_id−1] > 69`
and vice versa. Power uses the average of both casters' stats and proficiency.

### 8.3 Hit chance (`FUN_00467fe9`)

```text
0 if the skill is ground-only (+0x1d & 1) and target move_type != 0
0 if boss and +0x1d & 2; 0 if sub-boss and +0x1d & 0x20
target side 0 (self): base chance (+0x17)
effect 0x10 vs ability 0x5000: 0;  effect 1 vs petrified/frozen/sleep: 99;  effect 0x12 vs cursed: 0
else c = 0; n = 0
  flag 4: c = base; n++
  flag 1: (v, kind) = magic_vs(caster, target)            // FUN_004683f2
           kind 1: v = vary(v, pair 10); c = clamp(c, 5, 99)
           kind 2: c = vary(c, pair 6), 0 → 1;  kind 3: c = vary(c, pair 10), 0 → 1
           c += v; n++                                   (vary = FUN_00407b37 / FUN_00407a0a)
  flag 2: c = vary(c, pair 6) + tech_vs(caster, target); n++       // FUN_00468822
  c = n ? c / n : 0
  flag 0x80 and difficulty != 1: c += c*20*clamp(h(caster) − h(target), −5, 5)/100
  clamp 1..99
magic_vs (FUN_004683f2):
  effect 1: p = max(caster prof_cat/10, 10) (the original uses the caster's proficiency for
            both sides); m1 = clamp(caster MAG, 10, 99), m2 = clamp(target MAG, 10, 99)
            v = (m1*(p+500) − m2*(p+500)) / 500                         → kind 2
  effects 0xe, 0x12, 0x5a or caster's own army: kind 0, value = base chance (+0x17; the asm returns it, Ghidra shows garbage) → c += base
  effect 0x22: v = (caster.MAG − random equipped armour DEF) / 2       → kind 1 (remembers the slot)
  effect 0x26: v = (caster.MAG − weapon ATK) / 2                       → kind 1
  else: v = caster.MAG − target.MAG − 100 + base_chance                  → kind 3
  map v → %: v ≥ 43: 99;  10..42: (v−10)/4 + 90;  1..9: v + 80;  0: 80;
             −39..−1: v/2 + 80;  −79..−40: v + 100;  −119..−80: (v+80)/2 + 21;  < −119: 1
  effect 1 only: result = result * base_chance / 100   (the generic path already added it)
  effect 0xd vs important targets: ×0.7
tech_vs (FUN_00468822): power side = (sword(s) avg) * (prof/10 + 1000) * (terrain atk + 100);
  resist = ((target.skill_cur + target.sword)/2) * (target prof/10 + 1000) * (target terrain def + 100)
  v = (power − resist) / 100000 → same % mapping → × base_chance / 100
```

### 8.4 Power (`FUN_00468b0d` → `FUN_0046932f`, mods `FUN_00468d53`)

`X` = caster stat (flag 1: MAG (+1/5 with item ability 0x12, averaged with a
combo partner); flag 2: 劍術 added; divided by number of contributors, ≥1),
`Y` = the same for the target, `P` = caster category proficiency/10 (averaged
for combos), `B` = MagicRec base power (+0x15). Formula id `+0x14`:

| id | value |
|---|---|
| 0 | B |
| 1–4, 6, 0xd, 0xe, 0x10, 0x16, 0x19, 0x1c, 0x1d, 0x20 | `X*r[a]/r[a+1] + B` |
| 5, 7–0xc, 0xf, 0x11–0x15, 0x18, 0x1a, 0x1e, 0x1f, 0x21, 0x24–0x2a | `X*r[a]/r[a+1] − Y*r[a+2]/r[a+3] + B` |
| 0x17, 0x2b | `caster.hp*r[a]/r[a+1] + B` |
| 0x1b | `P1(caster)*r[0x64]/r[0x65] + B` |
| 0x22 | target current HP |
| 0x23 | `(X*X/Y)*r[0x76]/r[0x77] + B` |
| 0x2c–0x41 | `(P+1000)*X/1000 − Y*r[a]/r[a+1] − target.def_cur + B` |
| 0x42 | `(X*X/Y)*r[0xc4]/r[0xc5]` |
| 0x43 / 0x44 / 0x45 | `power(caster,0,0)` / `power(target,0,0)` / `P3(caster)` × r[a]/r[a+1] |
| 0x46 / 0x47 / 0x48 | target charm / `100−charm` / `kills − charm + 100` × r[a]/r[a+1] + B |
| 0x49, 0x4a | target HP × r[a]/r[a+1] + B |
| other | B |

`a` is the formula's own offset into the rules table: the constants are packed
in id order starting at 0x12 (2 bytes for the 1-term forms, 4 for the 2-term
forms); see `FUN_0046932f` for the exact offset of each id.

Then per effect: damage effects (1, 0x24, 0x2a) → `FUN_00468d53`: facing bonus
(flags 0x10/0x20/0x40 select +20/25/30% side or +40/45/50% back) unless
ability 0x2000 or difficulty 1; ×½ vs 靈盾 for category-5 skills; element mask
vs ClassRec resist (÷10) / weakness (×3); ±20% / ÷10 vs ground/flyers by flags;
½ vs ability 0x10 (magic); ×1.25 with caster ability 0x400000; ½ vs ability
0x2000000 for categories 2/4; ½ with item ability 0xb for categories 3–6; ×2/3
vs 光盾/item 0xd; ×0.8 運功, ×0.7 發功; combos with both item ability 2 ×1.5;
`vary(pair 4)`; 神佑 −180; 0 vs 無我. Support amounts (2, 6, 7, 9, 0xc, 0x29,
0x30, 0x31) → `vary(pair 8)` (0x30: −1/3 vs 光盾); heals (0x12, 0x5a) → ×2
with item ability 0x13, `vary(pair 0xc)`, 0 if cursed.

### 8.5 Effects (`FUN_0045ff9e`)

Every cell of the chosen area (`DAT_00492a68`) is processed; a failed chance roll
shows "miss". Duration `FUN_0046ae25`: `+0x19 < 101` → that many turns;
`101..200` → `MAG/(d−100)`; `201..254` → `(MAG − target MAG)/(d−200)`; 255 →
permanent; +1 when cast by the enemy side (which turns an enemy's permanent −1 into 0).

- **Damage** (effect 1, `FUN_00460b73`): enemies (and allies if side 3) take
  `min(power, hp)` (≥1); deaths as §5.7 (summoner effects with base 5 or
  `DAT_004a1f54 == 2` use death mode 2); kill bonus EXP; proficiency gain
  `((hits + kills_now − kills_before + 6 − difficulty) * 10) / 3 + 10`.
- **Heal** (0x12, 0x5a; 0x5a also cleanses) (`FUN_004606d3`): allies, capped at
  missing HP, gain `((healed_units + 6 − difficulty) * 5 + 10)`.
- **Summon** (0x15..0x1f, `FUN_004608e2`): creates a unit of summon kind
  `id − first_summon − 1` at the target cell with level `clamp(MAG/20 + 1, 1,
  10)`, `act = 2`; NPC if the caster is.
- **Ally buffs** (`FUN_0046136c`), **enemy debuffs** (`FUN_00462144`), **all units**
  (`FUN_00462c42`): switch on effect type, e.g. 2 護甲 DEF+, 6 嗜血 ATK+, 9 魔強,
  10 加速 (AP+, AP ≤ 110), 0xb flying, 0xc 激勵, 0xe un-petrify, 0x13 blessing,
  0x14 cleanse, 0x20 barrier, 0x21 invisibility, 0x25 levitate, 0x2c 神護,
  0x31 all shields, 0x32..0x39 elemental shields, 0x3c cure paralysis/poison
  (+heal), 0x3f 附體, 0x41 神佑, 0x46 靈擊, 0x48 靈盾, 0x4a clear all, 0x4c regen,
  0x54 運功, 0x55 輕功, 0x56 發功, 0x57 無我, 0x58 激勵+blessing+regen,
  0x5b **refresh** (another unit that already acted gets `act = 0` and AP refilled);
  debuffs 3 kill, 5 petrify, 7 防降, 8 blind, 0xd freeze, 0xf 衰竭, 0x10 poison,
  0x11 禁錮, 0x22/0x26 destroy equipment, 0x2b/0x3b paralysis, 0x2e slow,
  0x35 暗盾?, 0x37 curse, 0x38 multi-ailment (5 rolls), 0x3d sleep, 0x43 fear,
  0x45 weak; all-units 0x23 (`FUN_0046313d`), 0x28 berserk, 0x2f dispel, 0x3a 噁心.
  Enemy/all-unit effects with power > 0 and without `+0x1d & 0x80` also deal
  that much damage (capped at HP); ally effects that applied heal `power` HP
  (0x3c negates the sign, i.e. damages).
  Other targeted effects: 4 teleport (`FUN_00458334`), 0x27 (`FUN_0045844b`),
  0x2d reveal (army flag 1).
- **Learning** after a cast: category proficiency `prof[cat+5] += gain` (cap
  15000); every not-yet-learned skill of that category with `required ≤ prof/10`
  (and, if `+0x20 & 0x80`, `FUN_00457301`) is learned ("習得 …"). Same for a
  combo partner. EXP for spells comes from the accumulated damage/heal values
  (`SpellCaster+0x1c/+0x20`; combos halve it, scrolls divide by difficulty).

## 9. Death, retreat, results

`die(u, mode)` (`FUN_0044d14b`): `deaths += 1`; script hook
`FUN_00417f26(u)` may return 0x11 (special removal) or 0x13 (retreat
animation, unit leaves: `flags = flags & ~0x80000000 | 0x20000000`); hidden
reserves just leave the map; otherwise death sound 0x8d, death animation,
`flags |= 0x20000000`. A dying summon frees its army summon slot. Army flags are
recomputed.

## 10. Class change (轉職) (`FUN_00453986`, `FUN_00455c85`, `FUN_00455d95`)

Available when `FUN_0043a522` (level ≥ 11, or level 10 with full EXP) and the
class has targets (`DAT_00494660[kind]`, up to 8). A target is offered if its
`+0x1d` ability mask is 0 or matches an ability the unit has, and its `+0x1b`
item is 0 or equipped (hidden classes). The player also picks a skill category
to boost. Applying:

```text
consume the required item; unequip all slots to the inventory
charm = min(max(charm − old.charm, 0) + new.charm, 110)
g = growth(new, 1); for each of the 9 stats: if new base > current: g += (base − current)/2
apply_growth(u, g)
d = max(new.ap − ap_max, 0); if old class type == 5 and new != 5: d = −4
if ap_max + d < 110: ap_max += d, ap_base += d;  if keep_ap: ap += d;  if ap < 1: ap = carry = 0
abilities |= bit(new.ability − 1) if new.ability
level = 1; exp = 0; exp_next = new.exp_next
chosen category: prof += 300 (cap 15000), learn all reachable skills (not +0x20&0x80)
kind = new; reload sprite (FUN_0045721e, FUN_00443c4c)
if exp ≥ exp_next: exp = min(exp, 3*exp_next); level_up loop
```

## 11. Upkeep, resting and terrain effects

- **Start of turn** (turn ≥ 2, both armies, `FUN_0045ad90`): every buff/status
  with a positive counter counts down and is removed at 0; stats recomputed;
  regen (ability 0x100, status 0x200 or item ability 0x1a, not cursed): `+max(15%
  HP, 10)` up to max; item ability 0xc: +10 MP; poison: `−min(HP/5, 200, hp)`
  (can kill); invisible units are redrawn when the opponent has flag 1. The
  opponent's flag 1 is cleared.
- **End of a side's phase** (`FUN_00440e71`): each unit with `act == 2`:
  `act = 0`, `turn_flags = 0`, `ap_carry = min(ap, rules[0xed])` (0 if
  `ap_base + carry > 109`), `ap = ap_base + ap_carry`. Units that never acted
  keep their AP unchanged.
- **Rest** (`FUN_0044e460`, used by 休息, end turn and the AI's end of action
  `FUN_0044e210` when `ap ≥ rules[0xf0]`): not cursed; `r = randn(11) + 10`;
  HP += clamp(hp_max*r/100, ≥5, ≤ missing, ≤ 300); MP and ST likewise
  (≤ 100; MP ×2 with item ability 0x3f). Costs `rules[0xf0]` AP.
- **End-of-phase terrain** (`FUN_0045b29d`, overlay class of the unit's cell):
  0xb/0x11 −20% HP, 0x10 −10%, 0x12 −30%, 0x13 −20, 0x14 −50, 0x15 −100 (≥1, can
  kill); 0x16/0x17/0x18 refill HP/MP/ST, 0x19 all, 0x1a all + cleanse, 0x1b
  cleanse, 0x1c dispel, 0x1d clear all statuses, 0x1e +10 EXP, 0x1f 神佑 3 turns,
  0x20 berserk (+30% ATK, 3), 0x21 poison 3, 0x22 輕功 3, 0x23 random teleport,
  0x24 `FUN_0044cf97`.

## 12. Items in battle (`FUN_00447490`)

Equip UI `FUN_00447d3f`, use UI `FUN_0044957b` (targets: own units, incl. NPCs
for use). Consumable effects by item ability code (`FUN_0044a619`), duration
`randn(6 − difficulty) + 1` where relevant: 4 reflect, 5 full HP, 6 +60 MP,
9 barrier, 10 +30 MP, 0x14 levitate, 0x1b `FUN_0044b41b`, 0x20 `FUN_0044b515`,
0x21 +amount HP, 0x22 +amount MP, 0x23 cleanse + amount to HP/MP/ST,
0x24 cure poison, 0x25 un-petrify, 0x26/0x27 cleanse, 0x28 amount, 0x2b berserk,
0x2c–0x31 permanent +amount to ATK/DEF/反應/MAG/戰技/劍術 (cap 999),
0x32 +max HP (cap 5000), 0x33 +max MP, 0x34 +AP max (cap 110), 0x35 invisible,
0x3b cure paralysis, 0x3c +amount ST, 0x3d +100 max HP & `FUN_0044b725`,
0x41/0x42 `FUN_0044bfb2` (learn skills?), 0x43 EXP +100 (or +999). Not cursed
for the healing ones. Equipped-item passive abilities (`FUN_0043ab32`) used by
the battle code: 1 (with 0xda==0: decrements `DAT_00497b48`), 3 poison weapon,
7 paralysis weapon, 0xb, 0xc MP regen, 0xd damage −1/3 & crit/death immune,
0xe status resist, 0xf/0x11 movement types, 0x10 evasion, 0x12 MAG ×1.5, 0x13
heal ×2, 0x19/0x1c revive, 0x1a regen, 0x1d counter-heal, 0x1e counter ×3,
0x1f/0x3a army flags, 0x3e undead slayer, 0x3f rest MP ×2, 0x40 charm +10.

## 13. Two-player VS (`FUN_0043297f`)

Ported in `battle/vs.rs` (VS API block there). Setup `FUN_0040e5db` picks a
battlefield of `FSTAGE.BIN` (10, none with script events) and two save
slots; `FUN_0040f67f` sets turn 1, difficulty 5, no next stage, loads the
battlefield (`FUN_0040f418`, out of range → 1), and places each save's
*player* army (`FUN_00443482`) on the blue records (before the first
side-2/3/8 record) and the red ones, then shows both sides' save difficulty,
power sum, best power and unit count. In the stage loop no per-turn
deployment or autosave runs; each side plays `FUN_0043297f(side)` — the
player phase loop for that army without save/load/retreat, the "actions"
event poll or the difficulty-5 range rule — and `DAT_004944e0` (side to
move: hidden units are hidden from the other) flips after each. Victory =
the opponent army wiped out; the loop ends with 藍軍勝利/紅軍勝利 and code
0x14. Rules: no difficulty scaling or EXP multiplier, kills drop the
victim's equipment, no class change, no caves/treasure/tile flags.

## Open questions

- Exact semantics of a few fields (army +0xd4/+0xd6/+0xe2, overlay
  class meanings beyond those listed) need the TABLES/SCRIPT data to confirm.
- Spell targeting/area shapes (`DAT_00492a68`, 0x401000–0x406000) and the AI's
  spell scoring (`FUN_0040576b`, `FUN_0046be92`) are ported in
  `battle/targeting.rs` (shapes listed in its header) and `battle/ai.rs`.
- `FUN_0041be4c` / script hooks are interfaces only (TEXT/SCRIPT sub-task).
