# Enemy / NPC AI

How computer-controlled units act. Used for the enemy phase, NPC allies and
berserk units after the player's phase, and the player's auto battle (F5).
See `battle.md` for the rules referenced here and `structs.md` for fields.

## 1. Phase driver

`ai_phase(army, mode)` (`FUN_00433db9`; enemy phase `FUN_0043310e` → mode 1;
end of player phase `FUN_00433da4(player)` → mode 1; auto battle
`FUN_00433d8f(player)` → mode 0):

```text
last = (0, 0)
loop:
  u = next_ai_unit(army, last, mode)       // FUN_00433e6e
  if none: return 0
  last = u.pos; cursor = u.pos
  ai_act(u)                                // FUN_0040a443
  r = check_end(); if r: return r          // 1 victory, 2 defeat
```

`next_ai_unit` candidates: deployed, has a sprite, `act == 0`, can act, and
(mode 0: any; mode 1: `npc == 1` or berserk; mode 2: any alive). For the first
pick (last = (0,0)) the unit with the smallest distance (`FUN_00472d46`) to the
nearest *ready* opposing unit (`FUN_00433fa8`) goes first; afterwards the
candidate closest to the previous actor's cell. Ties keep the lower slot.

**Wake-up triggers** before each side's phase (`FUN_004447e6`, for deployed living
non-summons): `31 ≤ ai_mode ≤ 100` → when the unit whose tag (`Unit+0x06`) equals
`ai_mode − 30` is dead, `ai_mode = 1, ai_sub = 0`; `101 ≤ ai_mode ≤ 150` → on
turn `ai_mode − 100`, same. Being damaged (attack or debuff) turns `ai_mode 2`
into 1 (`FUN_00436e46`, `FUN_00460c85`, `FUN_00462144`).

## 2. Unit action (`FUN_0040a443` → `FUN_0040a4d0`)

```text
ai_act(u):
  u.act = 1
  if u.status & 0x80000 (fear) and rand100() > 50: goto done   // loses the turn
  range = move_range(u)                     // MoveRange, battle.md §4.1
  ctl.reach = longest path length in range  // FUN_0047188c
  if hp ≥ 90% and ai_state == 5: ai_state = 0
  m = ai_mode; if m != 1 and ai_state == 5: m = 5
  if berserk: m = 1
  switch m:
    1: if ai_sub == 1: if target_reachable(u): ai_sub = 0; redo   // else do nothing
       else fight(u)
    2: if target_reachable(u): ai_mode = 1; redo with mode 1 (clearing ai_sub)
       else end_action(u)
    3: end_action(u)                                   // hold position
    4: keep_away(u)                                    // FUN_0040c0b3
    5: retreat(u, 0)                                   // FUN_0040bf6a
    6: if !berserk and go_to_point(u, 1): done else fight(u)    // FUN_0040cce3
    7: approach(u)                                     // FUN_0040a8fa
    else: if target_reachable(u): ai_mode = 1; redo else end_action(u)
done: u.act = 2; redraw                    // FUN_0040a443 epilogue
```

`target_reachable` (`FUN_0040bc9e`): any spell/technique plan from the current
cell is accepted by `offer()` (`FUN_0046b84e` → `FUN_0046b890` returns 1), or any
attack skill has a target from the current cell or from any reachable stop-cell
that is not `FUN_004281d7`-blocked (the unit is not put back between skills).

`end_action(u)` (`FUN_0044e210`): face the nearest enemy that can reach us;
if `ap ≥ rules[0xf0]` spend it and rest-heal (battle.md §11); mark done
(`turn_flags |= 0x80`, `act = 2` via the redraw path).

## 3. Fighting (`FUN_0040aadc`)

The loop repeats as long as the unit keeps finding something worth doing — AP
allows several attacks per turn.

```text
fight(u):
 loop:
  atk_cost = attack_cost(u)                       // battle.md §3.1
  situation = assess(u)                           // FUN_0040c424: 0 danger, 1 threatened, 3 safe
  best = {type 0, score 0, kills 0}; spell = none; move_to = 0; target = none
  if not berserk:
     show "thinking" text (system string 0x3f0/4) above the unit
     if knows magic:      consider_spells(u, magic, from current cell)       // FUN_0046b890
     if knows techniques: consider_spells(u, technique, from current cell)
     p = {1: 5, 2: 30, 3: 50, 4: 80}[difficulty] or 100
     if rand100() ≤ p:
        for node in range (skipping node 0) with can_stop and mark ≥ 2:
           u.pos = node; consider magic and techniques; if improved: move_to = node
     if (best.type > 0x1d and situation != 1) or (best.type > 0 and best.score == 0):
        forget the spell plan          // support types > 0x1d only when threatened
  for each class AtkSkill k (not type 0xb/0xc) usable with the weapon, while ap ≥ atk_cost:
     u.pos = original
     t = best_attack_target(u, k)                 // FUN_0040b740, below
     if t: plan = attack k on t from here; move_to = 0; if best.type == 1: stop searching
     if not moved this turn:
        for node in range with can_stop and mark != 0:
           u.pos = node; t = best_attack_target(u, k)
           if t: plan = attack k on t from node; if best.type == 1: stop searching
  restore u.pos and facing
  low_mp = mp_max > 0 and mp < mp_max and mp*100/mp_max < 20
  low_st = same for ST;  hp% = hp*100/hp_max;  critical = hp% ≤ 29
  if best.type == 3 and best.score < 2000: best.type = 0
  act = best.type > 0 and (not critical or best.type == 1 or (best.type == 2 and best.kills > 0)
                           or (best.type == 3 and the spell is a heal: effect 0x12/0x31/0x5a))
  if not act:
     if ai_mode in {6, 7} or berserk: approach(u)
     elif critical:      retreat(u, 0)
     elif low_mp or low_st: (situation == 3 ? close_in(u, 0) + end_action : retreat(u, 1))
     elif turn_flags & 0xe == 0: approach(u)            // hasn't attacked/cast yet
     elif not moved:
        s = assess(u)
        s == 3: approach(u)
        s == 1: hp% ≥ 86 → close_in(u, 1); ≥ 76 → close_in(u, 0); ≥ 51 → step_toward(u); else retreat(u, 0)
        s == 0: hp% ≥ 96 → close_in(u, 1); ≥ 86 → close_in(u, 0); ≥ 61 → step_toward(u); else retreat(u, 0)
        end_action(u)
     else end_action(u)
     return
  if move_to: walk to move_to; turn_flags |= 1
  if plan is a spell: cast it at its chosen cell (FUN_0046bb67)
  else: aim the cursor at the target and attack (FUN_00436b24, pays AP like the player)
  rebuild range from the new position; repeat loop
```

### 3.1 Scoring attacks (`FUN_0040b740`, `FUN_00407dd9`)

For every target in the skill's range from the evaluated cell:

```text
duel = forecast(u, t, k)                               // battle.md §5.2–5.5
vip  = t.char_id in {1, 0xb} or t.rank & 3
s    = expected(duel)          // FUN_004082e4: (hit + 10 if status_proc(u,t,k)) * dmg ... see note
if vip: s *= rules[0x100 + difficulty]                  // u8
if t.hp < duel.fc_dmg and duel.fc_hit > 50: s *= rules[0x105 + difficulty]; kills++, vip_kills += vip
s -= expected_counter(duel)    // FUN_00408354
if duel.fc_cdmg ≤ u.hp or duel.fc_chit < 51 or u is berserk:
   if shape == 3 (piercing): add the same terms for every other enemy on the ray before t
   if s > 0: type = vip_kills > 0 ? 1 : 2; offer(type, s, kills) → keep t if accepted
// otherwise the attack is rejected: the counter could kill us
```

`expected(duel)` = `(fc_hit + 10) * fc_dmg` if `FUN_004083c4` (the skill/weapon
would inflict a status; it rolls rand100 internally) else `fc_hit * fc_dmg`;
`expected_counter` the same with the counter values.

`offer(type, s, kills)` (`FUN_00407dd9`) against the current best `(T, S, K)`,
with thresholds `L = rules[0x10b]*100 + rules[0x10c]*battles_fought` (`DAT_00497b50`), `L2 = L/2`:

```text
T == 0                       → accept
type 1: accept if T != 1 or S < s
type 2, kills == 0: T==2: K==0 and S<s;  T==3: S<s;  T>3: L<s or (s<L and S<s)
type 2, kills ≥ 1: T > 2, or T == 2 and (K < kills or (K == kills and S < s))
type 3: T==2: K==0 and S<s;  T==3: S<s;  T>3: L2<s or (s<L2 and S<s)
type > 3: T==2: S<L and S<s;  T==3: S<L2 and S<s;  T>3: S<s
accept → (T, S, K) = (type, s, kills)
```

### 3.2 Scoring spells (`FUN_0046b890` → `FUN_0040576b`)

Requires `ap ≥ spell_cost`, no stage seal (player side only) and not 禁錮 for
magic. Every learned, castable skill (`FUN_0045f145`, `FUN_0046586d`) is tried
on every candidate centre cell of its targeting area (`DAT_00492a68`):

- **Damage spells** (effects 1, 3, 0x24, 0x29, 0x2a, 0x30, 0x40;
  `FUN_004059a6`, line/cone areas `FUN_00405ebe`): for each enemy in the area,
  `(chance, amount)` = expected values (`FUN_00467f96(.., 0)`), amount capped at
  HP; `s = chance*amount` (×`rules[0x100+d]` for VIPs); if `amount ≥ hp` and
  `chance > 50`: kill, `s ×= rules[0x105+d]`; effect 0x29 scores only kills;
  0x2a adds the caster's heal. Allies inside an "all units" area: reject the
  cell if the spell would kill one or leave it under 20% HP, else subtract
  `chance*amount*(amount*100/hp + 100)/100` (×VIP). Best cell by kills, then score;
  type 1 if a VIP dies, else 2.
- **Heals** (0x12, 0x31, 0x5a): `FUN_004065d3`; **other support/debuffs**:
  `FUN_004063c4` using the per-effect usefulness filter `FUN_0046be92` (e.g. don't
  re-apply an existing buff, only buff allies within 15 cells of an enemy,
  summons only when no summon is active) and value `MagicRec.+0x23 *
  (rules[0x10d]+100)/100 * chance` (`FUN_0046be07`). These produce types ≥ 3.
  Only enemies *add* their value; own units in the area *subtract* theirs, so
  spells on own units (target 0/1/3) are never chosen. A unit the filter
  rejects adds the previous unit's value again (uninitialised local).
- Rays and cones (`FUN_00405ebe`) first test for an enemy in the area built at
  the cursor as it is on entry, then score each of the four directions from the
  caster; their ally check skips the "all units" test.
- The result is merged with `offer()`.

## 4. Movement helpers

- `assess(u)` (`FUN_0040c424`): **0** if a visible enemy whose power
  (`power(e,0,0)`) exceeds a third of ours stands within `reach + 1` rings
  (`FUN_0040c5d8`); else **1** if the nearest visible enemy whose power exceeds
  twice ours (`FUN_00434138` mode 3) could reach us next turn (`FUN_0040c459`):
  `dist − enemy_reach − reach − 2 < 1`, with `a = max(0, e.ap_base + e.ap_carry −
  rules[0xe8])`; when *our* class type is 6/7 and the enemy's class has a
  usable technique command (`FUN_0043cd7f == 2`): `enemy_reach = max(0, (a − rules[0xef]) / rules[0xe9]) + 5`,
  otherwise `max(0, (a − rules[0xee]) / rules[0xe9]) + longest weapon range`
  (`FUN_00458283`); else **3** (safe).
- `approach(u)` (`FUN_0040a8fa`): if not moved: modes 6/7 → `go_to_point(u, 0)`;
  otherwise (unless berserk, or difficulty 1 enemy side): `decide_close(u)`
  (`FUN_0040cb85`: safe → close_in(hp<max ? 0 : 1); healthy (`FUN_0040c847`) →
  close_in(1); badly hurt (`FUN_0040ca2d`) → retreat; else close_in(0)); berserk /
  easy enemies → walk toward the nearest ready opposing unit that can be
  attacked (`FUN_00434045`). Then `end_action`.
- `close_in(u, eager)` (`FUN_0040cc56`): target = nearest enemy (`FUN_00434138`
  mode 0); walk to the reachable stop-cell nearest to it (ties: smaller squared
  Euclidean distance) keeping `budget ≥ rules[0xed]` (+`rules[0xf0]` when not
  eager) and not adjacent to an enemy (`FUN_0047164d`).
- `step_toward(u)` (`FUN_0040aa76`, `FUN_00471923`): one cautious move toward the
  nearest *ally* (`FUN_00434138(own army, u, 2)`) to a cell not next to an enemy,
  preferring cells farther from the nearest enemy; sets the moved flag.
- `retreat(u, keep)` (`FUN_0040bf6a`): if allowed (not moved; difficulty > 1 or
  not enemy side, or nothing done yet): walk to the reachable cell farthest
  (ring distance) from the nearest threatening enemy (`FUN_0040c18d`; when hurt
  (< 90%) prefer cells with `budget ≥ rules[0xf0]` so it can rest); then
  `end_action`; if not berserk, `keep == 0` and `ai_state != 5` and the unit is
  below its retreat threshold (`FUN_0040ca2d`) → `ai_state = 5`.
- `keep_away(u)` (mode 4, `FUN_0040c0b3`): like retreat, but if no better cell
  exists and HP > 70% it fights instead.
- `go_to_point(u, strict)` (`FUN_0040cd2a`): find the map goal of type 3/5
  (enemy side) or 4/6 (player side) (`FUN_0042890a`); walk to it or the closest
  reachable cell; `strict` → only if the goal itself is reachable. (With no goal
  on the map `FUN_0042890a` returns an uninitialised flag; the port treats it as
  "none".)

Health thresholds:

- `FUN_0040c847(u, eager)` "fit to advance": invisible/hidden units with HP > 39%
  always (on easy difficulties for the enemy side); class types 6/7/0x13/0x14
  (casters): HP ≥ 70/90% (eager/not) and MP ≥ 10%; types 4/0x17: ≥ 70/80% and ST
  ≥ 10%; type 3: ≥ 60/70%; others ≥ 45/50%.
- `FUN_0040ca2d(u, eager)` "should retreat": weighted threshold
  `(P1*40 + P2*w2 + P3*w3 + P4*25) / power` vs HP% (w2 = 30/40, w3 = 25 for class
  type 3 else 15/20); retreat when HP% ≤ threshold.

## 5. Notes for porting

- The AI uses the same forecast objects as the UI, so `FUN_00409488` (counter
  skill pick) and `FUN_004083c4` consume RNG during evaluation; keep the order.
- Candidate cells are iterated in `MoveRange.order` (creation order of the
  depth-first fill), which decides ties.
- Units temporarily teleport (`u.pos = node`) during evaluation; nothing else
  may observe that state.
