//! Battle-side effects the event engine needs. Every function here stands
//! for one original callee in the army / map / spell code (addresses
//! given). Each forwards to the BATTLE port's implementation of the same
//! name in `crate::battle::hooks`; actions are also logged (`Log::Battle`)
//! for the event tests.
//!
//! Conventions (see docs/structs.md): `Side::Player` is `DAT_0049f9f8`,
//! `Side::Enemy` is `DAT_004976c8`; a unit is addressed by its army and
//! 1-based slot. `Unit+0x02` is the character id, `+0x23/+0x27` the map
//! cell, `+0x2d` a draw/move mode byte, `+0x30` the state bits
//! (0x80000000 on the map, 0x40000000 retreated, 0x20000000 dead,
//! 0x10000000 "no exit"), `+0x11b` four item slots (u16), `+0x141 + n`
//! relation values towards character n (i8, n = 1..=40).

use crate::game::Game;

use super::Log;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Player,
    Enemy,
}

impl Side {
    /// `Army+0x282`: the other army.
    pub fn opponent(self) -> Side {
        match self {
            Side::Player => Side::Enemy,
            Side::Enemy => Side::Player,
        }
    }
}

/// A unit in an army (army + 1-based slot, `Unit+0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitRef {
    pub side: Side,
    pub slot: u16,
}

/// Operands of the event being evaluated, for hooks that take the whole record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvArgs {
    pub sub: i16,
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
}

/// What `battle_talk_begin` saved (`FUN_004254eb` scroll, `Unit+0x2d`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TalkSave {
    pub scroll: (i32, i32),
    pub mode_2d: u8,
}

/// One member of a type-15 group walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkMember {
    pub unit: UnitRef,
    /// Member record `c` (first array of `FUN_0044ecf8`).
    pub c: i32,
    /// Member record `d << 2` (second array).
    pub d4: i32,
}

/// One member of a type-16 group move; arrays of `FUN_0045c37d`, zero unless set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveMember {
    pub unit: UnitRef,
    /// Array 0: `c` of a sub 2/6 record.
    pub p0: i32,
    /// Array 1 / 2: `c`, `d` of a sub 1/5 record.
    pub p1: i32,
    pub p2: i32,
}

fn log(game: &mut Game, name: &'static str, a: [i32; 4]) {
    game.events.log.push(Log::Battle(name, a));
}

fn side_n(s: Side) -> i32 {
    match s {
        Side::Player => 0,
        Side::Enemy => 1,
    }
}

// ---- queries ---------------------------------------------------------------

/// `FUN_00457578(army, char_id, 0)`: the unit with this character id that
/// is currently on the map / active in `side`, if any.
pub fn find_unit(game: &mut Game, side: Side, char_id: i16) -> Option<UnitRef> {
    crate::battle::hooks::find_unit(game, side, char_id)
}

/// `FUN_004576a9(army, char_id)`: the unit with this character id in
/// `side` in any state (dead, retreated, off map).
pub fn find_unit_any(game: &mut Game, side: Side, char_id: i16) -> Option<UnitRef> {
    crate::battle::hooks::find_unit_any(game, side, char_id)
}

/// `FUN_0043a860(army, char_id, 0) == 1`: character is in `side`'s roster.
pub fn in_roster(game: &mut Game, side: Side, char_id: i16) -> bool {
    crate::battle::hooks::in_roster(game, side, char_id)
}

/// `Unit+0x30` state bits.
pub fn unit_flags(game: &mut Game, u: UnitRef) -> u32 {
    crate::battle::hooks::unit_flags(game, u)
}

pub fn set_unit_flags(game: &mut Game, u: UnitRef, v: u32) {
    log(game, "set_unit_flags", [side_n(u.side), u.slot as i32, v as i32, 0]);
    crate::battle::hooks::set_unit_flags(game, u, v)
}

/// `Unit+0x23` / `+0x27`: map cell.
pub fn unit_pos(game: &mut Game, u: UnitRef) -> (i32, i32) {
    crate::battle::hooks::unit_pos(game, u)
}

/// `FUN_0044e99d(army, k)`: k > 0 → character k is dead (on-map unit with
/// the dead bit); 0 → no living deployed unit; -1 → no living unit with
/// `Unit+8 == 1` (main party); -2 → no living leader (`Unit+0x2f == 1`);
/// -3 → no living non-leader.
pub fn army_all_down(game: &mut Game, side: Side, k: i32) -> bool {
    crate::battle::hooks::army_all_down(game, side, k)
}

/// `FUN_0045cbf8(&player, mode)`: mode 0 → the protagonist (char 0xb, or 1
/// when `DAT_00497b54 != 0`) stands on an exit tile (map attr high byte 6);
/// mode 1 → some active, deployed, non-NPC unit without bit 0x10000000 does.
pub fn on_exit_tile(game: &mut Game, mode: i32) -> bool {
    crate::battle::hooks::on_exit_tile(game, mode)
}

/// `FUN_00441326(a, b)`: b is one of a's four diagonal neighbours on the
/// staggered map (row parity decides the x offset) and the height
/// difference (`FUN_004255da`) is below 4.
pub fn units_adjacent(game: &mut Game, a: UnitRef, b: UnitRef) -> bool {
    crate::battle::hooks::units_adjacent(game, a, b)
}

/// `FUN_00425fbe(map, x, y, &value)`: the event value of a map cell
/// (`None` when the cell is outside the map).
pub fn map_cell_event(game: &mut Game, x: i32, y: i32) -> Option<u16> {
    crate::battle::hooks::map_cell_event(game, x, y)
}

// ---- deploy / withdraw (type 6) ------------------------------------------

/// `FUN_00426b87(map, x, y, 0, 0)`: scroll the view to cell (x, y). With
/// `fastest` the original forces the screen-speed option `DAT_00494638`
/// to 0 for the duration.
pub fn camera_scroll(game: &mut Game, x: i32, y: i32, fastest: bool) {
    log(game, "camera_scroll", [x, y, fastest as i32, 0]);
    crate::battle::hooks::camera_scroll(game, x, y, fastest)
}

/// `FUN_0042681e(map, x, y, 1)`: center the view on cell (x, y) at once.
pub fn camera_center(game: &mut Game, x: i32, y: i32) {
    log(game, "camera_center", [x, y, 0, 0]);
    crate::battle::hooks::camera_center(game, x, y)
}

/// `FUN_0043d9e9(army, &stage_units[record], 1)`: create the unit for a
/// stage deployment record in `side`.
pub fn create_unit(game: &mut Game, side: Side, record: usize) -> Option<UnitRef> {
    log(game, "create_unit", [side_n(side), record as i32, 0, 0]);
    crate::battle::hooks::create_unit(game, side, record)
}

/// `FUN_0044f11c(army, unit, anim, mode)`: effect animation on a unit
/// (deploy uses anim 0xf9, mode 1).
pub fn unit_effect(game: &mut Game, u: UnitRef, anim: i32, mode: i32) {
    log(game, "unit_effect", [side_n(u.side), u.slot as i32, anim, mode]);
    crate::battle::hooks::unit_effect(game, u, anim, mode)
}

/// `FUN_00440264(army, unit, 1)`: put the unit on the map and draw it.
pub fn place_unit(game: &mut Game, u: UnitRef) {
    log(game, "place_unit", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::place_unit(game, u)
}

/// `FUN_00441c0b(army, unit)`: take the unit off the map (erase it).
pub fn remove_from_map(game: &mut Game, u: UnitRef) {
    log(game, "remove_from_map", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::remove_from_map(game, u)
}

/// `FUN_0044cf97(army, unit)`: animated withdrawal of the unit.
pub fn withdraw_animated(game: &mut Game, u: UnitRef) {
    log(game, "withdraw_animated", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::withdraw_animated(game, u)
}

/// `FUN_00424c5f(map)`: redraw the map into the back page.
pub fn redraw_map(game: &mut Game) {
    log(game, "redraw_map", [0; 4]);
    crate::battle::hooks::redraw_map(game)
}

/// `FUN_004252c9(map)`: full map rebuild/redraw (after a full-screen picture).
pub fn redraw_map_full(game: &mut Game) {
    log(game, "redraw_map_full", [0; 4]);
    crate::battle::hooks::redraw_map_full(game)
}

// ---- moves (type 7) --------------------------------------------------------

/// `FUN_004588f2(army, unit, x, y, mode)`: walk the unit to (x, y); mode 1
/// is the variant used by subs 1 and 3.
pub fn walk_unit(game: &mut Game, u: UnitRef, x: i32, y: i32, mode: i32) {
    log(game, "walk_unit", [u.slot as i32, x, y, mode]);
    crate::battle::hooks::walk_unit(game, u, x, y, mode)
}

/// `FUN_00459131(army, unit, x, y)`: warp (vanish effect 0xc2, sound 0x68,
/// reappear at the nearest free cell to (x, y)).
pub fn warp_unit(game: &mut Game, u: UnitRef, x: i32, y: i32) {
    log(game, "warp_unit", [u.slot as i32, x, y, 0]);
    crate::battle::hooks::warp_unit(game, u, x, y)
}

// ---- camera / effects (type 8) ---------------------------------------------

/// `FUN_00466e71(&DAT_004a1f58, n)`: full-screen effect after a camera jump (n = 100).
pub fn screen_effect(game: &mut Game, n: i32) {
    log(game, "screen_effect", [n, 0, 0, 0]);
    crate::battle::hooks::screen_effect(game, n)
}

/// `FUN_0046cd1c(&DAT_004a1f58, unit, anim, army, opponent)`: play effect
/// animation `anim` (CEL 1 set) on a unit.
pub fn effect_on_unit(game: &mut Game, anim: i32, u: UnitRef) {
    log(game, "effect_on_unit", [anim, side_n(u.side), u.slot as i32, 0]);
    crate::battle::hooks::effect_on_unit(game, anim, u)
}

/// `FUN_0046cd78(&DAT_004a1f58, x, y, anim)`: effect animation at a map cell.
pub fn effect_at(game: &mut Game, anim: i32, x: i32, y: i32) {
    log(game, "effect_at", [anim, x, y, 0]);
    crate::battle::hooks::effect_at(game, anim, x, y)
}

// ---- items (type 10) ------------------------------------------------------

/// `FUN_00446b84(army, item, count)`: add items to the army inventory.
pub fn give_item(game: &mut Game, side: Side, item: i32, count: i32) {
    log(game, "give_item", [side_n(side), item, count, 0]);
    crate::battle::hooks::give_item(game, side, item, count)
}

/// `FUN_0043a96a(army, item)`: someone in the army has the item (inventory or equipped).
pub fn army_has_item(game: &mut Game, side: Side, item: i32) -> bool {
    crate::battle::hooks::army_has_item(game, side, item)
}

/// `FUN_0043a9af(army, char_id, item)`: that character holds the item.
pub fn unit_holds_item(game: &mut Game, side: Side, char_id: i32, item: i32) -> bool {
    crate::battle::hooks::unit_holds_item(game, side, char_id, item)
}

/// `Unit+0x11b + 2k` (k = 0..4): item slot.
pub fn unit_item_slot(game: &mut Game, u: UnitRef, k: usize) -> i32 {
    crate::battle::hooks::unit_item_slot(game, u, k)
}

pub fn set_unit_item_slot(game: &mut Game, u: UnitRef, k: usize, v: i32) {
    log(game, "set_unit_item_slot", [u.slot as i32, k as i32, v, 0]);
    crate::battle::hooks::set_unit_item_slot(game, u, k, v)
}

/// `FUN_00446c0c(army, item)`: put one item into the army inventory.
pub fn inventory_add(game: &mut Game, side: Side, item: i32) {
    log(game, "inventory_add", [side_n(side), item, 0, 0]);
    crate::battle::hooks::inventory_add(game, side, item)
}

/// `FUN_00446c6b(army, item) == 1`.
pub fn inventory_check(game: &mut Game, side: Side, item: i32) -> bool {
    crate::battle::hooks::inventory_check(game, side, item)
}

// ---- unit / party state (type 11) -------------------------------------------

/// `FUN_0045d922(&player, unit, mode)`.
pub fn set_unit_mode(game: &mut Game, u: UnitRef, mode: i32) {
    log(game, "set_unit_mode", [u.slot as i32, mode, 0, 0]);
    crate::battle::hooks::set_unit_mode(game, u, mode)
}

/// `FUN_0045d8b2(&player, char_id, flag)` (type 11 subs 8 / 9 pass 0 / 1).
pub fn party_char_op(game: &mut Game, char_id: i32, flag: i32) {
    log(game, "party_char_op", [char_id, flag, 0, 0]);
    crate::battle::hooks::party_char_op(game, char_id, flag)
}

/// `FUN_00442eb7(&player)` (type 11/10).
pub fn party_op_10(game: &mut Game) {
    log(game, "party_op_10", [0; 4]);
    crate::battle::hooks::party_op_10(game)
}

/// `FUN_00441038(&player)` (type 11/13).
pub fn party_op_13(game: &mut Game) {
    log(game, "party_op_13", [0; 4]);
    crate::battle::hooks::party_op_13(game)
}

/// Type 11/14: `FUN_0041bca5()` (save the spell cursor), `FUN_004411cf(&player)`
/// (phase start), `FUN_00433d8f(&player)` (party/formation screen),
/// `FUN_0041bcb9()` (restore cursors of `DAT_00492b08`, `DAT_004a1f58`, `DAT_00492af0`).
pub fn party_setup_screen(game: &mut Game) {
    log(game, "party_setup_screen", [0; 4]);
    crate::battle::hooks::party_setup_screen(game)
}

// ---- unit actions (type 12) -----------------------------------------------

/// `FUN_0045bc6a(army, event)`. Unit = `FUN_004576a9(army, b)`; none → false.
/// Ops (`sub`) 0 and 1 always run; 10..=28 only when the unit is on the map
/// and alive (otherwise return true without effect). The original saves
/// cursors (`FUN_0041bca5`), starts a phase redraw (`FUN_004411cf`), and if
/// the unit is on the map but its cell is not lit (`FUN_00426267(..,2)`)
/// scrolls there (`FUN_00426905`) first, and restores (`FUN_0041bcb9`) after:
///
/// | op | effect |
/// |----|--------|
/// | 0 | erase if on map; `+0x30 = 0`, `+8 = 1`, `+2 = 0` |
/// | 1 | erase if on map; set 0x40000000, clear 0x80000000 |
/// | 10 | erase, `FUN_0044ebb2(unit, d, c << 2)`, place, `FUN_004252c9` |
/// | 11 | `+0x2d = 2`, place |
/// | 12 | `FUN_0040a443(&DAT_00492af0, unit)` (AI controller) |
/// | 13 / 14 | erase, `FUN_0044ebb2(unit, c, 4 / 8)`, place, full redraw |
/// | 15 | as 13 with steps = (first class attack skill of kind 0xb)`[+10] << 2`, default 8 |
/// | 16 | erase, `FUN_00441a24(unit, c)` (set facing), place, full redraw |
/// | 17 / 18 | (17: `+0x30 = 0, +8 = 1, +2 = 0`) erase, `FUN_0044d14b(unit, 0)` (defeat) |
/// | 19 / 20 | (19: same reset) `FUN_0044cf97` (withdraw) |
/// | 21 | erase, `+8 = 1`, `FUN_00459214(opponent, unit)` (switch army), redraw, reset |
/// | 22 | `+8 = 0, +0xa = 1, +0xc = 0`, clear 0x40000000 |
/// | 23 | HP (`+0x124`) -= c; ≤ 0 → 0, erase, defeat |
/// | 24 | HP -= max HP (`+0xb7`) * c / 100; ≤ 0 → defeat |
/// | 25 | `+0xa = c` (AI mode) |
/// | 26 | full restore (`FUN_0044e460` with `+0xc` forced 1; `+0x2d = 0`, `+0x34 = 0`, AP refill capped by rules[0xed]) |
/// | 27 | `FUN_0044bfb2(unit, c, d, d, 1)` |
/// | 28 | `FUN_0044bfb2(unit, c, 1, d, 1)` |
pub fn army_unit_action(game: &mut Game, side: Side, ev: EvArgs) -> bool {
    log(game, "army_unit_action", [side_n(side), ev.sub as i32, ev.b as i32, ev.c as i32]);
    crate::battle::hooks::army_unit_action(game, side, ev)
}

// ---- stats (type 14) ---------------------------------------------------------

/// `Unit+0x141 + n` (n = 1..=40): relation towards character n.
pub fn unit_relation(game: &mut Game, u: UnitRef, n: i32) -> i8 {
    crate::battle::hooks::unit_relation(game, u, n)
}

pub fn set_unit_relation(game: &mut Game, u: UnitRef, n: i32, v: i8) {
    log(game, "set_unit_relation", [u.slot as i32, n, v as i32, 0]);
    crate::battle::hooks::set_unit_relation(game, u, n, v)
}

/// `FUN_0045d9d1(&player, b, c)` (type 14/1).
pub fn party_relation_op(game: &mut Game, b: i32, c: i32) {
    log(game, "party_relation_op", [b, c, 0, 0]);
    crate::battle::hooks::party_relation_op(game, b, c)
}

// ---- group moves (types 15 / 16) -------------------------------------------

/// Type 15 execution. For each member: save `+0x2d`, `FUN_00441c0b`
/// (erase), set `+0x2d = 1`; then `FUN_0044ecf8(army, units, c[], d4[], n)`
/// walks them together; then restore each `+0x2d` and `FUN_00440264(.., 1)`
/// (place) each. Not called when no member was found.
pub fn group_walk(game: &mut Game, side: Side, members: &[WalkMember]) {
    log(game, "group_walk", [side_n(side), members.len() as i32, 0, 0]);
    crate::battle::hooks::group_walk(game, side, members)
}

/// Type 16 execution: `FUN_0045c37d(army, units, arrays, n, mode)`.
pub fn group_move(game: &mut Game, side: Side, members: &[MoveMember], mode: i32) {
    log(game, "group_move", [side_n(side), members.len() as i32, mode, 0]);
    crate::battle::hooks::group_move(game, side, members, mode)
}

// ---- story flag side effects -------------------------------------------------

/// `FUN_0041be7a` for flags 1 (attr 6, the exit) and 3 (attr 0xb):
/// `FUN_00441222` both armies (hide sprites), `FUN_00428774(map, attr)`
/// finds the cell (error 5 if none), `FUN_00441ef1` + `FUN_004403b5` both
/// armies, `FUN_00426905(map, x, y)` scroll there, draw the cell's location
/// name (`FUN_0042574c`) centered at the cell's screen position + 0x20 −
/// len·6 (x clamped 5..=0x1fe, y 5..=0x1c2) with `FUN_0041f748` colors
/// 99/0xa7, target 1, outline; present; click-to-continue 400 cs.
pub fn reveal_special_tile(game: &mut Game, attr: i32) {
    log(game, "reveal_special_tile", [attr, 0, 0, 0]);
    crate::battle::hooks::reveal_special_tile(game, attr)
}

/// `FUN_0041be7a` for flag 7: `FUN_0045ceb9(&enemy)`.
pub fn army_flag7(game: &mut Game) {
    log(game, "army_flag7", [0; 4]);
    crate::battle::hooks::army_flag7(game)
}

// ---- dialogue context --------------------------------------------------------

/// Before a unit-context dialogue (`FUN_00417f26`, `FUN_00418203` mode 1):
/// `FUN_0046ec6b(page, 1, 2)`, `FUN_0046eb02(page, 1)`,
/// `FUN_0043ff88(army, unit, 0)` (draw the unit), `FUN_0046eb02(page, 0)`.
pub fn unit_dialogue_begin(game: &mut Game, u: Option<UnitRef>) {
    let (s, n) = u.map(|u| (side_n(u.side), u.slot as i32)).unwrap_or((-1, 0));
    log(game, "unit_dialogue_begin", [s, n, 0, 0]);
    crate::battle::hooks::unit_dialogue_begin(game, u)
}

/// After it: `FUN_0046ec6b(page, 2, 1)`.
pub fn unit_dialogue_end(game: &mut Game) {
    log(game, "unit_dialogue_end", [0; 4]);
    crate::battle::hooks::unit_dialogue_end(game)
}

/// End of `FUN_00417f26`: `FUN_0046ec6b(page, 1, 0)`, `FUN_0043ff88(army, unit, 0)`.
pub fn unit_dialogue_finish(game: &mut Game, u: UnitRef) {
    log(game, "unit_dialogue_finish", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::unit_dialogue_finish(game, u)
}

/// `FUN_00417d8e` before the talk: `FUN_004254eb` (save scroll), save
/// `+0x2d` and set it 0, `FUN_00442022(army, unit, 0)`, `FUN_00440264(army, unit, 1)`.
pub fn battle_talk_begin(game: &mut Game, u: UnitRef) -> TalkSave {
    log(game, "battle_talk_begin", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::battle_talk_begin(game, u)
}

/// After it: `FUN_00441c0b(army, unit)`, restore `+0x2d`, `FUN_0042550e` (restore scroll).
pub fn battle_talk_end(game: &mut Game, u: UnitRef, saved: TalkSave) {
    log(game, "battle_talk_end", [side_n(u.side), u.slot as i32, 0, 0]);
    crate::battle::hooks::battle_talk_end(game, u, saved)
}
