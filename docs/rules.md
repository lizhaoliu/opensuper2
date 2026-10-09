# Battle rules and formulas

Transcribed from the decompiled exe. All arithmetic is C `int` (truncating
division, applied in the order written). The executable transcription is
`crates/s2engine/src/battle/rules.rs` (+ `rng.rs`); where this text and that
code differ, the code (checked against the decompilation and disassembly) wins. `P[i]` is the signed byte `i` of the
BIN 10 constants block (`FormulaParams::p`); the shipped values are in
`s2tables params`. Unit fields use the offsets of the unit struct
(docs/formats-tables.md); `cls(u)` is the unit's class record.

## Random numbers

- `rand()` is MSVC `rand`: `seed = seed*214013 + 2531011; return (seed>>16) & 0x7fff`
  (`FUN_0047abfd`). The game reseeds from the clock (`FUN_0047ac1b`) every 200
  draws (`DAT_004a2af8` counter in `FUN_0046d776`/`FUN_0046d7ca`).
- `rnd(n)` = `n == 0 ? 0 : rand() % n` (`FUN_0046d7ca`).
- `pct()` = `T[rand() % 100]` where `T` is a permutation of 0..99 built at
  start-up by 600 random swaps of 0..99 (`FUN_0046d6b5`, `FUN_0046d776`).
  "Roll succeeds" means `pct() <= chance`.

Exact RNG fidelity requires drawing in the same order as below.

## Terrain and movement (`FUN_00427e1f`)

For a unit of class `c` on cell `(ground g, object o)`:

```
ga = attr_ground[g]; if g == 0 or ga == 0: blocked
gp = ga >> 8;  gt = (ga & 0xff) - 1 (0 if low byte 0)
e  = terrain[c][gt]                          # (cost, atk, def)
if gp == 5 or e.cost == -1: blocked
lev = FUN_0043aa26(unit)                     # flying/levitating state
switch cls.move_type:
  1..5, 10: walk if gp in {0,1,2,4} else (lev ? fly : blocked)
  6..8:     fly  if gp in {2,3}     else walk
  9:        walk if gp in {1,2}     else (lev ? fly : blocked)
  other:    blocked
if o != 0:
  oa = attr_object[o]; op = oa >> 8; ot = oa & 0xff
  if ot > 30: blocked
  e2 = terrain[c][24 + ot - 1]
  if op == 1 or e2.cost == -1: blocked
  if op == 2: fly;  if op == 10: (lev ? fly : blocked)
  e += e2                                    # costs and modifiers add up
```
`walk`/`fly` = return 1/2. The `(atk, def)` bytes feed the damage formula as
the attacker's `T_atk` and the defender's `T_def`. In "simple" mode
(`DAT_00494684 == 1`) terrain and height are ignored.

Height: `h(u)` = cell byte 0 of the unit's cell. `dh = h(attacker) - h(defender)`.

Direction (`ctx+0x40`, `FUN_004082bd`): 3 = from behind, 2 = from the side,
else front. `dirH = {3:5, 2:3, else 0}`, `dirD = {3:6, 2:3, else 0}`.

## Physical attack

Context (`FUN_00408078` → `FUN_00409488` + `FUN_00409642`): attacker `A`,
defender `D`, attacker action `a`, defender counter action `d` (chosen at
random among D's actions that can reach A, `FUN_00409488`; none → no counter).

Proficiency `prof(u, act)`: for action kind < 21: `u.prof[weapon.kind - 1]`
of the equipped weapon (0 if unarmed); otherwise `u.prof[17]` (0xf9).

Hit modifiers `hitmods(X, Y, act, h)` (`FUN_00409f0b`), applied to `h`:
```
if act.kind < 21: h += (X.weapon ? X.weapon.hit : 90) * act.hit / 100
else:             h += act.hit
for each equipped item of Y: h -= item.evasion
if Y.skill & 2: h -= 30
if Y.status & 4 or ability(Y, 0x10): h -= h / 10
clamp h to 1..100
```

Desperation (`FUN_00409e98(v, lost%)`, `lost% = (maxHP - HP)*100/maxHP`):
```
71..90:  pct() < 90 ? v*13/10 : v*3/10
> 90:    pct() < 95 ? v + v/2  : v/2
```

### Attacker strike (`FUN_0040881a`)

```
if forecast hit and damage are both 0: return 0
S   = desperation(A.戰技(0x138), A.lost%)          # draws
hit = S * (dh*1000 + 8000 + dirH*800 + prof(A,a)) / 8000 - D.反應(0xc1)
hit = hitmods(A, D, a, hit)
hit = hit * 12 / 10
hit = hit * (P[2] + rnd(P[3] + 3)) / 10;  if enemy army and not 2-player: hit = hit*(100 + P[0xf3+difficulty])/100;  clamp 1..99
if D.buffs & 0x200 or D.status & 0x2a000: hit = 99           # disabled defender
if pct() > hit: miss (0)
if D.status & 0x800: 0                                         # immune
div = variance()                                               # see below
c   = clamp(dh, -8, 4);  k = c < 0 ? 125 : 250
DEF = (A.status & 0x80 or A.skill & 0x40000000) and D.boss != 1 ? 0 : D.防禦(0x136)
dmg = (A.攻擊(0x134) - DEF) * (c*k + 1000 + (T_atk(A) - T_def(D))*10 + prof(A,a)) / 10 / div
dmg = modify(dmg, dirD, A, D, a)
if dh > 0: dmg += dh
dmg = dmg * (P[0] + rnd(P[1] + 3)) / 10;  difficulty scaling as for hit;  if dmg < 1: dmg = 1
if dmg < 3:  r = pct(); dmg = r < 1 ? 5 : r < 6 ? 4 : r < 16 ? 3 : r < 46 ? 2 : 1
```

`variance()` (`FUN_0040a2ed`): `r = pct(); v = rnd(10)`;
r < 2: `v < 5 ? 100+v : 140+v`; r < 10: `105+v / 135+v`; r < 25:
`110+v / 130+v`; r < 50: `115+v / 125+v`; else `120+v`.

`modify(x, dir, X, Y, act)` (`FUN_0040a080`):
```
adv = type_matrix[type(X)-1][type(Y)-1];  if adv > 0 and Y.skill & 0x800: adv -= adv/2
x = x * (dir*10 + 100 + adv) / 100 * act.power / 10
soft cap: 100<x<=200: (x-100)*90/100+100; <=300: (x-200)*80/100+190;
          <=400: (x-300)*70/100+270; >400: (x-400)*60/100+340
x += min(15, rating(X, 0, 0) / 1000)
if Y.status & 0x4000000 or ability(Y, 0xd): x -= x/3
if Y.status & 0x20: x = x < 151 ? 1 : x - 150
if Y.status & 0x40: x = x < 181 ? 1 : x - 180
if X.status & 0x40000: x /= 2
if Y.buffs & 0x1000: x = x*80/100;  if Y.buffs & 0x2000: x = x*70/100
```

### Counterattack (`FUN_00408bae`)

Same structure with roles swapped (`dh' = -dh`, direction terms negated:
`dirH' = -dirH`, `dirD' = -dirD`), and:
- returns 0 at once when there is no counter action or the counter forecast
  hit and damage are both 0;
- the defender's 戰技 is the desperation value rolled by the preceding
  attacker strike (`FUN_0040881a` rolls desperation for **both** sides first);
- `if D.status & 0x1000 (blind): hit /= 10`; then `hit*12/10`, then
  `hit = (rnd(4) + 3) * hit / 10` before the usual hit scaling; the hit also
  misses outright when `A.status & 0x800`;
- after `modify`: `dmg = (rnd(4) + 2) * dmg / 10 + dh'`;
  `if D.skill & 0x400: dmg = D.攻擊 base (0xc3)`; `if ability(D, 0x1e): dmg *= 3`;
  then damage scaling; `dmg < 1` → the same small-damage table.

The forecast shown before attacking (`FUN_00409642`) uses expected values:
`(P[3]+3)/2` instead of `rnd`, damage `((ATK-DEF)*factor/1000)*100/125`,
counter hit `x45/100`, counter damage `x35/100`, no 1.2 hit factor.

## Spells (`FUN_00467f96`: chance `FUN_00467fe9`, value `FUN_00468b0d`)

Inputs: caster `C`, optional combo partner `Q` (spells with `combo & 7`),
target `T`, spell `S`. `school_slot = S.school + 5` indexes the proficiency
array (school EXP); `prof/10` is the school level.

### Power (`FUN_0046932f`)

```
X = 0, nx = 0, Y = 0, ny = 0
if S.stat & 1: X = C.魔力cur (+X/5 with item ability 0x12); nx = 1
               combo: X += Q.魔力cur (+Q.魔力cur/5 with 0x12); nx = 2
               Y = T.魔力cur (+Y/5 with 0x12); ny = 1
if S.stat & 2: X += C.劍術; nx += 1; combo: X += Q.劍術; nx += 1;  Y += T.劍術; ny += 1
if nx == 0 or ny == 0: value 0
X = max(X/nx, 1); Y = max(Y/ny, 1)
A = T.alignment (unit+0x123) + 10 with item ability 0x40 on T
L = C.prof[school_slot]/10, combo: (Q.prof/10 + L)/2
```
`L1(v, a) = v*P[a]/P[a+1] + power`, `L2(a) = X*P[a]/P[a+1] − Y*P[a+2]/P[a+3] + power`.

| formula | value |
|---------|-------|
| 0, > 0x4a | power |
| 1..4 | L1(X, 0x12 + 2(f−1)) |
| 5 | L2(0x1a) |
| 6 | L1(X, 0x1e) |
| 7..0xc | L2(0x20 + 4(f−7)) |
| 0xd, 0xe | L1(X, 0x38), L1(X, 0x3a) |
| 0xf | L2(0x3c) |
| 0x10 | L1(X, 0x40) |
| 0x11..0x15 | L2(0x42 + 4(f−0x11)) |
| 0x16 | L1(X, 0x56) |
| 0x17 | L1(C.HP, 0x58) |
| 0x18 | L2(0x5a) |
| 0x19 | L1(X, 0x5e) |
| 0x1a | L2(0x60) |
| 0x1b | L1(rating(C, 0, part 1 = magic), 0x64) |
| 0x1c, 0x1d | L1(X, 0x66), L1(X, 0x68) |
| 0x1e, 0x1f | L2(0x6a), L2(0x6e) |
| 0x20 | L1(X, 0x72) |
| 0x21 | L2(0x74) |
| 0x22 | T.HP |
| 0x23 | (X*X/Y)*P[0x76]/P[0x77] + power (reuses 0x21's last pair) |
| 0x24..0x2a | L2(0x7a + 4(f−0x24)) |
| 0x2b | L1(C.HP, 0x96) |
| 0x2c..0x41 | (L + 1000)*X/1000 − Y*P[a]/P[a+1] − T.防禦cur + power, a = 0x98 + 2(f−0x2c) (`FUN_0046adeb`) |
| 0x42 | (X*X/Y)*P[0xc4]/P[0xc5] |
| 0x43 / 0x44 / 0x45 | rating(C,0,0) / rating(T,0,0) / rating(C,0,3=body) × P[0xc6/0xc8/0xca]/P[+1] (no power) |
| 0x46 / 0x47 / 0x48 | L1(A, 0xcc) / L1(100 − A, 0xce) / L1(T.kills − A + 100, 0xd0) |
| 0x49, 0x4a | L1(T.HP, 0xd2), L1(T.HP, 0xd4) |

### Value by effect (`FUN_00468b0d`)

- damage (1, 0x24, 0x2a): `damage_mods` below, then 0 if `T.status & 0x800` (無我);
- support (2, 6, 7, 9, 0xc, 0x29, 0x31): `vary(pair 8/9)`;
- 0x30: −1/3 if `T.status & 0x4000000` or item 0xd, then `vary(pair 8)`;
- heal (0x12, 0x5a): 0 if `T.status & 0x10000` (cursed); ×2 with item 0x13 on T; `vary(pair 0xc)`;
- 0x23, 0x2b: the formula value unchanged; every other effect: `power`.

`damage_mods` (`FUN_00468d53`), skipped entirely when `S.flags & 0x40`:
```
mt = move_type(T)
if S.stat & 0x70 and difficulty != 1 and !(T.abilities & 0x2000):
    col = stat&0x10 ? 0 : stat&0x20 ? 1 : stat&0x40 ? 2 : 0
    facing(C → T) == 3: v += v*{40,45,50}[col]/100;  == 2: v += v*{20,25,30}[col]/100
if T.status & 0x100 (靈盾) and school 5: v /= 2
if class(T).resist & S.elements: v /= 10;  if class(T).weak & S.elements: v *= 3
if mt == 0 and S.flags & 4: v += v/5
if mt in {3,4} and S.flags & 8: v += v/5;  if mt in {3,4} and S.flags & 0x10: v /= 10
if T.abilities & 0x10 and S.stat & 1: v /= 2
if C.abilities & 0x400000 and S.stat & 1: v = v*125/100
if T.abilities & 0x2000000 and school in {2,4}: v /= 2
if item(T, 0xb) and school in 3..=6: v /= 2
if T.status & 0x4000000 or item(T, 0xd): v = (v<<1)/3
if T.buffs & 0x1000: v = v*80/100;  if T.buffs & 0x2000: v = v*70/100
if !(stat&4) and !(stat&1) and stat&2 and combo&7 and item(C,2) and item(Q,2): v += v/2
v = vary(v, pair 4/5, caster side)
if T.status & 0x40 (神佑): v = v < 181 ? 1 : v − 180
```

### Chance (`FUN_00467fe9`)

```
0  if move_type(T) != 0 and S.flags & 1;  if T.rank == 1 and flags & 2;  if T.rank == 2 and flags & 0x20
S.hit if S.target == 0 (self)
0  if effect 0x10 and T.abilities & 0x5000;  99 if effect 1 and (T.buffs & 0x200 or T.status & 0x22000)
0  if effect 0x12 and T.status & 0x10000
c = 0, n = 0
if stat & 4: c = S.hit; n = 1
if stat & 1: (kind, v) = magic_vs(C, T)            # FUN_004683f2
    kind 1 (raw): v = vary(v, pair 0xa); c = clamp(c, 5, 99)
    kind 2 (effect 1): c = vary(c, pair 6) (0 → 1)
    kind 3: c = vary(c, pair 0xa) (0 → 1)
    kind 0: v = S.hit (the asm returns +0x17; Ghidra shows an uninitialised value)
    c += v; n += 1
if stat & 2: v = tech_vs(C, T); c = vary(c, pair 6); c += v; n += 1
c = n ? c/n : 0;  if stat & 0x80 and difficulty != 1: c += c*20*clamp(h(C) − h(T), −5, 5)/100
clamp 1..99
```
`magic_vs`: effect 1 → `p = max(C.prof[slot]/10, 10)` used for **both**
sides, `m = clamp(魔力cur, 10, 99)`, `v = (mC*(p+500) − mT*(p+500))/500`,
curve, `× S.hit/100` → kind 2. Effects 0xe, 0x12, 0x5a or a target in the
caster's own army → kind 0. 0x22: pick one equipped armour slot 1..5 with
`rnd(count)` (also in AI evaluation), `v = (C.魔力 − armour.DEF)/2` → kind 1;
0x26: `v = (C.魔力 − weapon.ATK)/2` → kind 1 (chosen slot → SpellCaster+0x600).
Otherwise `v = C.魔力 − T.魔力 − 100 + S.hit`, curve → kind 3. Effect 0xd vs
important targets (`rank & 3`): ×7/10.
`tech_vs` (`FUN_00468822`): `a = 劍術 * (prof/10 + 1000) * (terrain atk + 100)`
(averaged with the partner for combos), `b = ((T.戰技cur + T.劍術)/2) *
(T.prof/10 + 1000) * (T terrain def + 100)`; `v = (a − b)/100000`, curve, `× S.hit/100`.
Curve: `v ≥ 43 → 99; 10..42 → (v−10)/4 + 90; 1..9 → v + 80; 0 → 80;
−39..−1 → v/2 + 80; −79..−40 → v + 100; −119..−80 → (v+80)/2 + 21; else 1`.

### Cost, duration

Cost (`FUN_0045f78b`): AP `P[0xef]`, plus `P[0xf3]` if the unit already used
this kind of action this turn (turn flag 4 magic / 8 technique); unless the
BIN 10 per-spell byte (`DAT_004a1f54`) is 2: MP `cost` (half with ability
0x20) for `stat & 1`, ST `cost` for `stat & 2`, HP `cost` (not below 1) for
`stat & 4`; a combo partner pays the same.

Duration (`FUN_0046ae25`): `< 101` → value; `101..200` → `C.魔力cur/(v−100)`
(≥1); `201..254` → `(C.魔力cur − T.魔力cur)/(v−200)` (≥1); else −1
(permanent). All three callers add 1 when the caster's army is the enemy
side — which also turns an enemy's permanent −1 into 0.

## EXP and levels

## Criticals and instant death

Critical chance (`FUN_0040927b`, attack `FUN_004091ab` with facing bonus
`back 10 / side 5`, counter `FUN_00409213` with `−10 / −5`), striker X, struck Y:
```
0 if the duel's attack forecast hit and damage are both 0 (also for counters)
0 if Y.abilities & 4, or the duel's defender army has flag 2 (also for counters)
0 if Y.status & 0x4000000 or item(Y, 0xd)
c = Σ X.equip crit (item +0x14) + X.戰技cur + facing − Y.反應 − Σ Y.equip evasion
+30 if X.abilities & 1;  −30 if Y.abilities & 2;  −c/10 if Y.status & 4 or item(Y, 0x10)
clamp 1..50;  critical when pct() < c
```
Instant death (`FUN_00409094`): never if the forecast is 0/0, the defender
is character 1 or 0xb, or has status 0x4000000 / item 0xd. Chance 50 if the
defender's unit type is 0xe (undead), it is not a boss (rank 1) and the
attacker has item ability 0x3e; else 5 if the attacker has ability 0x200 and
`defender.rank & 3 == 0`; else 0. `pct()` is drawn even when the chance is 0.

## EXP per hit

Per target hit (`FUN_00408fc9`): `Ra = rating(A, 1, 0)`, `Rd = rating(D, 1, 0)`
(rating with HP before the hit):
```
if damage == 0 and D alive: exp = 1
else: exp = max(1, damage * Rd * P[0xe5] / (Rd + Ra) / P[0xe1])
      if D.HP == 0: exp += 3 + Rd * P[0xe2] / Ra
```
Defeating a unit also adds its `kill_exp` (unit+0x119) and its drops
(`FUN_00453608`). Award (`FUN_00452e38`): `+1/3` with skill 0x40; then
`exp = exp * (P[0x10] + 3 + P[0x11]) / 10` clamped 1..999 (`FUN_00407880`);
player army outside 2-player mode: difficulty 1 → x3, 2 → x2. EXP
(unit+0x113) caps at 999.

Level up (`FUN_00453725`/`FUN_004537da`) while `exp >= exp_per_level` and
level < 99: `level += 1; exp -= exp_per_level;` gains =
`growth(class, level)` (below), each `+1` if unit flag `0x30 & 1`; HP capped
0..5000, other stats 0..999.

`growth(class, L)` (`FUN_0042345b`, record cached per class): for each stat
descriptor `d`: `0 if d1 == 0 or d3 == 0` else
`r1 = rnd(d4+1); r2 = rnd(d5+1) + 1;  trunc(sqrt(d0/d1 * L) + d2/d3 + r1/r2 + 0.5)`.
The game computes the HP gain twice and keeps the second (two extra RNG
draws), then MP, ST, 攻擊, 防禦, 反應, 戰技, 魔力, 劍術.

Weapon proficiency: +1 per use up to 1500 (`FUN_00452..`, action kinds 2..7
→ prof index kind−2, specials → 17). AI units get a floor
`P[0xe0]*difficulty*cls.0x14/10 + level*cls.0x14/20` (cap 999,
`FUN_00452af0`). School EXP: casting adds the spell's school EXP (halved for
combos), cap 15000; spells with `level <= schoolEXP/10` are learned
automatically (`FUN_00460...` around line 44200).

## Class change (轉職, `FUN_00455d95`)

Allowed targets: `cls.promotions`; requirement (`FUN_00455c85`):
`(change_flags == 0 or unit.skill & change_flags)` and
`(change_item == 0 or the item is equipped)`; the item is unequipped/consumed.
Effects: all equipment removed; `alignment = min(max(al − old.0x1a, 0) + new.0x1a, 110)`; stat gains = `growth(new, 1)` plus, for each stat below the new
class base (BIN 8 base record), half the difference; AP += `max(0, new.base_ap - unit.AP)`
(−4 when leaving a non-type-5 class into type 5); innate skill flag set;
`level = 1, exp = 0, exp_per_level = new.0x0e`; optional chosen school
gets +300 school EXP (cap 15000) and learns the spells that unlocks.

## Strength rating (強度, `FUN_0045781c(u, src, part)`)

`src`: 0 current stats, 1 current with HP-before-hit, 2 maximum stats.
```
mag   = (nSchoolsKnown(first P-limited schools) + 10) * 魔力 (+1/2 with ability 0x12) * (MP + 1) / (P[0xd6]*10*P[0xd7])
sword = (nSwordSchools + 10) * 劍術 * (ST + 1) / (P[0xd8]*10*P[0xd9])
phys  = HP / P[0xda] + ((攻擊 + 防禦) * (戰技 + 反應) / P[0xdb]) / P[0xdc]
skill = trunc(sqrt(n) * 400 * n)   n = set skill flags excluding 0x40, 0x20000, 0x40000, 0x200000, 0x4000000, 0x8000000, 0x80000000
total = max(1, (mag + sword + phys + skill) * AP / P[0xdf] * cls.rating_mult / 10)
```
`part` 1..4 return a single component (5..8 return the cached ones).

## Difficulty

`difficulty = DAT_00494684`, 1..=5 (1 easiest; new game sets 3), saved in
globals block C at +0x1c. The eight variance helpers `v*(P[2k] + rnd(P[2k+1]+3))/10`
(expected value `P[2k] + (P[2k+1]+3)/2`; k = 0 physical damage, 1 physical
hit, 2 spell damage, 3 technique hit, 4 support amount, 5 magic hit, 6 heal,
7 an unused chance pair; the EXP scale uses `P[0x10] + 3 + P[0x11]` without
randomness) end with `FUN_00407d91`: values of the enemy side are multiplied by
`(100 + P[0xf3+difficulty])/100` (P[0xf4..0xf8]; never in 2-player mode).
Chance pairs then clamp to 1..99, amount pairs to ≥ 1. Difficulty 1 also
zeroes terrain modifiers and height differences in combat and spell
facing/height terms. Enemy units get stat scaling `army.scale%`
(`FUN_00457d3c`). Player EXP ×3 / ×2 at difficulty 1 / 2.

## Stage end

Rewards from the stage block (docs/formats-tables.md); hidden treasure needs
army flag 0x40. Units with `0x30 & 0x20000000` are dead.

## Where the ports live

`crates/s2engine/src/battle/`: `rng.rs` (RNG), `rules.rs` (every formula
above; "RULES API" block at the top), `spells.rs` (cast dispatcher
`FUN_0045ff9e`, per-target effects `FUN_00460c85` / `FUN_004606d3` /
`FUN_0046136c` / `FUN_00462144` / `FUN_00462c42`, combos `FUN_0046552e`,
learning, costs), `items.rs` (`FUN_0043a5d3`, `FUN_00446d1a`,
`FUN_004470e7`, `FUN_0044bd12`, consumables `FUN_0044a619`),
`classchange.rs` (`FUN_0043a522`, `FUN_00455c85`, `FUN_00455d95`).

Quirks kept on purpose: unequipping an item whose 魔力 bonus drives 魔力
below 0 zeroes 防禦 instead; the AP cap in `FUN_00446d1a` is `999 −
ap_max`; ally support spells only change HP for effects 0x31 and 0x3c (the
others' amount is a display value); blood-price spells (0x29) can push the
caster's HP below 0; a couple-combo partner with character id 41 reads one
byte past the affinity table (treated as 0); an enemy's permanent status
duration (−1) becomes 0.

## Open items

- Spell area shapes (`spell.shape`, `spell.range`) not transcribed.
- AP: 0xbf AP max, 0x12c AP refilled each turn (incl. equipment/buffs),
  0x12e current AP, 0x130 AP carried to the next turn (docs/structs.md).
- `FUN_004082bd` direction rules; counter-action selection range test
  `FUN_004072f1`.
- Status effect ids (0x38/0x3c bits) ↔ status names mapping.
