//! Movement range, paths and walking (`DAT_004a2b48`, docs/battle.md §4).
//!
//! The fill is the original's recursive depth-first expansion with in-place
//! improvement, node free list and order-list compaction; node indices and the
//! order of `MoveRange::order` match the original so AI tie-breaks do too.

use super::army::{Side, UnitRef};
use super::map::{distance, Pass};
use super::unit::{flag, status, turn};
use super::Battle;

/// Node capacity (`0xbb2e / 0x13`).
pub const MAX_NODES: usize = 2466;
/// Order list capacity (`0x2768 / 2`).
const MAX_ORDER: usize = 0x13b4;

/// One reachable cell (0x13 bytes). An unused node has every byte 0xff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub x: i16,
    pub y: i16,
    /// AP left on arrival.
    pub budget: i16,
    pub height: u8,
    /// 1 = may end the move here.
    pub can_stop: u8,
    pub parent: i16,
    pub child: [i16; 4],
    /// 0 nothing affordable after arriving, 1 can attack, 2 can cast.
    pub mark: u8,
}

const DEAD_NODE: Node =
    Node { x: -1, y: -1, budget: -1, height: 0xff, can_stop: 0xff, parent: -1, child: [-1; 4], mark: 0xff };

/// Movement range of one unit (`DAT_004a2b48`).
#[derive(Debug, Clone)]
pub struct MoveRange {
    pub nodes: Vec<Node>,
    /// +0x04 live node count (= used length of `order`; `nodes`/`count` = FUN_004715f7).
    pub count: usize,
    /// +0x06/+0x0a path from destination back to the root (node 0; FUN_004715d8).
    pub path: Vec<usize>,
    /// +0x14
    pub unit: Option<UnitRef>,
    /// +0x2e node indices in creation order (FUN_00471615).
    pub order: Vec<i16>,
    /// +0x32/+0x36 free node indices.
    free: Vec<i16>,
    /// +0x42 1 | caller bits; 2 = unit knows magic/techniques; 0x8000 ignore units.
    pub flags: u16,
    /// +0x44
    pub move_type: i32,
    /// +0x48
    pub spell_cost: i32,
    /// +0x4c
    pub attack_cost: i32,
}

impl Default for MoveRange {
    fn default() -> Self {
        MoveRange {
            nodes: vec![DEAD_NODE; MAX_NODES],
            count: 0,
            path: Vec::new(),
            unit: None,
            order: vec![0; MAX_ORDER],
            free: Vec::new(),
            flags: 0,
            move_type: 0,
            spell_cost: 0,
            attack_cost: 0,
        }
    }
}

impl MoveRange {
    /// Port of FUN_00470c00: forget the range (count and path).
    pub fn reset(&mut self) {
        self.path.clear();
        self.count = 0;
        self.free.clear();
    }

    fn node(&self, i: i32) -> Node {
        if i < 0 {
            return DEAD_NODE;
        }
        self.nodes.get(i as usize).copied().unwrap_or(DEAD_NODE)
    }

    /// Live nodes in creation order (skipping the root).
    pub fn reachable(&self) -> impl Iterator<Item = (usize, &Node)> + '_ {
        (1..self.count).map(move |k| {
            let i = self.order[k] as usize;
            (i, &self.nodes[i])
        })
    }

    /// Port of FUN_00470c27: node index + 1 of a cell where the move may end, 0 if none.
    pub fn find_stop(&self, x: i32, y: i32, move5: bool) -> usize {
        let start = if move5 { 0 } else { 1 };
        for k in start..self.count {
            let i = self.order[k] as usize;
            let n = &self.nodes[i];
            if n.can_stop == 1 && n.x as i32 == x && n.y as i32 == y {
                return i + 1;
            }
        }
        0
    }

    /// Port of FUN_00470ce5: the mark of a stop cell, −1 if not reachable.
    pub fn mark_at(&self, x: i32, y: i32, move5: bool) -> i32 {
        match self.find_stop(x, y, move5) {
            0 => -1,
            k => self.nodes[k - 1].mark as i32,
        }
    }

    /// Port of FUN_0047152e: build the path to node `k - 1` (k is 1-based).
    pub fn build_path(&mut self, k: usize, move5: bool) {
        self.path.clear();
        if k == 0 {
            return;
        }
        let mut i = k as i32 - 1;
        if !move5 || i != 0 {
            loop {
                self.path.push(i as usize);
                i = self.node(i).parent as i32;
                if i <= 0 {
                    break;
                }
            }
            self.path.push(0);
        }
    }

    /// Port of FUN_0047188c: longest parent chain, reproducing the original's
    /// off-by-one (it walks from `order[k] + 1`).
    pub fn longest_path(&self) -> i32 {
        let mut best = 0;
        for k in 1..self.count {
            let mut i = self.order[k] as i32 + 1;
            if self.node(i).can_stop != 0 {
                let mut len = 0;
                loop {
                    i = self.node(i).parent as i32;
                    len += 1;
                    if i <= 0 {
                        break;
                    }
                }
                best = best.max(len);
            }
        }
        best
    }

    /// Port of FUN_00471b3b: delete a subtree (children first, then the node).
    fn delete_subtree(&mut self, i: usize) {
        let n = self.nodes[i];
        for c in n.child {
            if c != -1 {
                self.delete_subtree(c as usize);
            }
        }
        self.nodes[i] = DEAD_NODE;
    }

    /// Port of FUN_00472403: if (x, y) is already in the tree with a budget
    /// ≥ `nb` return true (keep it); if worse, delete its subtree, compact the
    /// order list and return false. Faithful quirk: when the tail entry being
    /// swapped in was itself deleted, it is zeroed first and then tested as
    /// node 0 (the root, never deleted), so index 0 is copied into the list —
    /// the root then appears among the "reachable" nodes, as in the original.
    fn existing_is_better(&mut self, x: i32, y: i32, nb: i32) -> bool {
        let mut k = 1;
        let mut e = 0usize;
        while k < self.count {
            e = self.order[k] as usize;
            if self.nodes[e].x as i32 == x && self.nodes[e].y as i32 == y {
                break;
            }
            k += 1;
        }
        if k == self.count {
            return false;
        }
        if (self.nodes[e].budget as i32) >= nb {
            return true;
        }
        let p = self.nodes[e].parent;
        if p > -1 {
            let pn = &mut self.nodes[p as usize];
            if let Some(slot) = pn.child.iter_mut().find(|c| **c == e as i16) {
                *slot = -1;
            }
            // else: the original raises error 5
        }
        self.delete_subtree(e);
        let mut k = 1;
        while k < self.count {
            if self.nodes[self.order[k] as usize].parent == -1 {
                self.count -= 1;
                loop {
                    let oc = self.order[self.count];
                    if self.nodes[oc as usize].parent == -1 {
                        self.free.push(oc);
                        self.order[self.count] = 0;
                    }
                    if self.count <= k {
                        return false;
                    }
                    if self.nodes[self.order[self.count] as usize].parent != -1 {
                        break;
                    }
                    self.count -= 1;
                }
                self.free.push(self.order[k]);
                self.order[k] = self.order[self.count];
                self.order[self.count] = 0;
            }
            k += 1;
        }
        false
    }
}

impl Battle {
    /// Port of FUN_004709dc: compute the movement range of `r` into `self.range`.
    /// `extra_flags` is OR-ed into `MoveRange::flags` (0 for normal moves).
    pub fn build_range(&mut self, r: UnitRef, extra_flags: u16) {
        let u = self.unit(r).clone();
        let mut mr = std::mem::take(&mut self.range);
        mr.reset();
        mr.nodes.fill(DEAD_NODE);
        mr.order.fill(0);
        mr.unit = Some(r);
        let mut root = DEAD_NODE;
        root.x = u.x as i16;
        root.y = u.y as i16;
        root.can_stop = 1;
        root.budget = (u.ap as i32 - self.c(0xe8)) as i16;
        root.parent = 0;
        if root.budget > 0x6e {
            root.budget = 0x6e;
        }
        root.height = self.map().root_cell(u.x, u.y).map_or(0, |c| c.height);
        mr.nodes[0] = root;
        mr.count = 1;
        mr.flags = 1 | extra_flags;
        if self.knows_skill_kind(r, 3) {
            mr.flags |= 2;
        }
        mr.move_type = self.move_type(&u);
        mr.spell_cost = self.c(0xef) + if u.turn_flags & 0xc != 0 { self.c(0xf3) } else { 0 };
        mr.attack_cost = self.c(0xee) - if u.abilities & 0x2000_0000 != 0 { 4 } else { 0 }
            + if u.turn_flags & turn::ATTACKED != 0 && u.abilities & 0x8000 == 0 { self.c(0xf3) } else { 0 };
        self.range = mr;
        self.expand(0);
    }

    /// Port of FUN_00471bda: expand node `i` in the four directions
    /// (up-right, down-left, up-left, down-right), skipping the parent cell.
    fn expand(&mut self, i: usize) {
        let pi = self.range.nodes[i].parent as i32;
        for dir in 0..4 {
            let n = self.range.nodes[i];
            let p = self.range.node(pi);
            let odd = n.y & 1 != 0;
            let (tx, ty) = match dir {
                0 => (if odd { n.x + 1 } else { n.x }, n.y - 1),
                1 => (if odd { n.x } else { n.x - 1 }, n.y + 1),
                2 => (if odd { n.x } else { n.x - 1 }, n.y - 1),
                _ => (if odd { n.x + 1 } else { n.x }, n.y + 1),
            };
            if tx == p.x && ty == p.y {
                self.range.nodes[i].child[dir] = -1;
                continue;
            }
            let c = self.try_step(i, tx as i32, ty as i32);
            self.range.nodes[i].child[dir] = c;
            if c != -1 {
                self.expand(c as usize);
            }
        }
    }

    /// Port of FUN_00471e51: the node for stepping from node `i` to (x, y), or −1.
    fn try_step(&mut self, i: usize, x: i32, y: i32) -> i16 {
        let n = self.range.nodes[i];
        let root = self.range.nodes[0];
        let r = self.range.unit.expect("range unit");
        let side = r.side;
        let mt = self.range.move_type;
        let mut can_stop = 1u8;
        if n.budget < 1 {
            return -1;
        }
        if root.x as i32 == x && root.y as i32 == y {
            return -1;
        }
        let Some(cell) = self.map().move_cell(x, y).copied() else { return -1 };
        if (y - cell.height as i32) * 0x10 < 0 {
            return -1;
        }
        let dh = n.height as i32 - cell.height as i32;
        if dh.abs() >= 0xb {
            return -1;
        }
        let (abil, stat, kind) = {
            let u = self.unit(r);
            (u.abilities, u.status, u.kind)
        };
        let mut climb;
        if abil & 0x10000 == 0 && stat & status::LIGHT_BODY == 0 {
            if !(-3..=4).contains(&dh) && mt == 0 {
                return -1;
            }
            climb = if dh < 1 { -dh } else { 0 };
        } else {
            climb = 0;
        }
        if self.range.flags & 0x8000 == 0 {
            if mt != 2 && !(root.x == n.x && root.y == n.y) {
                let adj = self.adjacent_enemies(side, n.x as i32, n.y as i32);
                if !adj.is_empty() {
                    if mt == 0 {
                        return -1;
                    }
                    for e in adj.iter().rev() {
                        if self.move_type(self.unit(*e)) != 0 {
                            return -1;
                        }
                    }
                }
            }
            if let Some(e) = self.unit_at_cell(side.other(), x, y, false) {
                if mt == 0 {
                    return -1;
                }
                if self.move_type(self.unit(e)) != 0 {
                    return -1;
                }
                can_stop = 0;
            }
            if self.unit_at_cell(side, x, y, false).is_some() {
                can_stop = 0;
            }
        }
        let t = self.t();
        let (pass, eff) = self.map().terrain(&t, x, y, kind, mt);
        if pass == Pass::Blocked {
            return -1;
        }
        if pass == Pass::PassOnly {
            can_stop = 0;
        }
        if self.map().blocked_by_spot(x, y) {
            return -1;
        }
        if mt == 5 {
            climb = 0;
        }
        let nb = if mt == 0 {
            n.budget as i32 - eff.cost as i32 - climb * self.c(0xeb)
        } else {
            n.budget as i32 - climb * self.c(0xea) - self.c(0xe9)
        };
        if nb < 0 {
            return -1;
        }
        if self.range.existing_is_better(x, y, nb) {
            return -1;
        }
        let mr = &mut self.range;
        if mr.count > 0x9d9 {
            return -1; // the original raises error 5
        }
        let idx = match mr.free.pop() {
            Some(f) => f as usize,
            None => mr.count,
        };
        mr.order[mr.count] = idx as i16;
        let node = &mut mr.nodes[idx];
        node.x = x as i16;
        node.y = y as i16;
        node.budget = nb as i16;
        node.height = cell.height;
        node.can_stop = can_stop;
        node.parent = i as i16;
        mr.count += 1;
        let attack_mark = |mr: &MoveRange| if mr.flags & 1 != 0 && (nb >= mr.attack_cost) { 1 } else { 0 };
        let mark = if mr.flags & 2 == 0 || nb < mr.spell_cost { attack_mark(mr) } else { 2 };
        mr.nodes[idx].mark = mark;
        idx as i16
    }

    /// Port of FUN_004414df: opposing units on the four neighbours of (x, y),
    /// reproducing the original's use of the first neighbour's height
    /// difference for all four tests.
    pub fn adjacent_enemies(&self, side: Side, x: i32, y: i32) -> Vec<UnitRef> {
        let m = self.map();
        let mut out = Vec::new();
        let h0 = m.height(x, y);
        if h0 == 0 {
            return out;
        }
        let odd = y & 1 != 0;
        let cells = [
            (if odd { x + 1 } else { x }, y - 1),
            (if odd { x } else { x - 1 }, y + 1),
            (if odd { x } else { x - 1 }, y - 1),
            (if odd { x + 1 } else { x }, y + 1),
        ];
        let first_h = m.height(cells[0].0, cells[0].1);
        let d = h0 - first_h;
        for (k, &(cx, cy)) in cells.iter().enumerate() {
            let h = if k == 0 { first_h } else { m.height(cx, cy) };
            if h != 0 && d < 4 && -5 < d {
                if let Some(e) = self.unit_at_cell(side.other(), cx, cy, false) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// Port of FUN_0043bcb5: an opposing unit on one of the four neighbours (no height test).
    pub fn enemy_adjacent_cell(&self, side: Side, x: i32, y: i32) -> bool {
        (0..4).any(|d| {
            let (nx, ny) = super::map::neighbour(x, y, [0, 1, 2, 3][d]);
            self.army(side.other()).unit_at(nx, ny) > 0
        })
    }

    /// Port of FUN_004281d7: non-flying classes cannot end on bridge tiles.
    pub fn no_stop_for(&self, r: UnitRef, x: i32, y: i32) -> bool {
        let mc = self.tables.as_ref().and_then(|t| t.kinds.class(self.unit(r).kind as u16)).map_or(0, |c| c.move_type);
        if !(6..=10).contains(&mc) {
            self.map().no_stop_tile(x, y)
        } else {
            false
        }
    }

    /// Port of FUN_0047164d (FUN_00471626 passes a unit's cell): path to the reachable stop cell closest to
    /// (x, y), requiring `budget ≥ min_budget` and no adjacent enemy when
    /// `min_budget > 0`. Ties prefer the smaller squared Euclidean distance.
    pub fn range_closest_to(&mut self, x: i32, y: i32, min_budget: i32) -> bool {
        let r = self.range.unit.expect("range unit");
        let mut best: i32 = -1;
        let mut best_d = 0;
        for k in 1..self.range.count {
            let i = self.range.order[k] as usize;
            let n = self.range.nodes[i];
            if n.can_stop == 0 || self.no_stop_for(r, n.x as i32, n.y as i32) {
                continue;
            }
            #[allow(clippy::nonminimal_bool)] // the original's condition as written
            let skip = min_budget >= 1 && !((min_budget <= n.budget as i32) && !self.enemy_adjacent_cell(r.side, n.x as i32, n.y as i32));
            if skip {
                continue;
            }
            let d = distance(x, y, n.x as i32, n.y as i32);
            if best == -1 || d < best_d {
                best_d = d;
                best = i as i32;
                if d == 1 {
                    break;
                }
            } else if d == best_d {
                let b = self.range.nodes[best as usize];
                let sq = |ax: i32, ay: i32| (x - ax) * (x - ax) + (y - ay) * (y - ay);
                if sq(n.x as i32, n.y as i32) < sq(b.x as i32, b.y as i32) {
                    best = i as i32;
                }
            }
        }
        if best != -1 {
            let move5 = self.unit(r).flags & flag::MOVE5 != 0;
            self.range.build_path(best as usize + 1, move5);
        }
        best != -1
    }

    /// Logic of FUN_0044cccd / FUN_0044c331: walk `r` along `self.range.path`
    /// (destination first), turning at each step, set its AP to the
    /// destination budget. Returns the steps (x, y, facing) for the view;
    /// animation, footsteps and event-tile touches are the caller's.
    pub fn execute_move(&mut self, r: UnitRef) -> Vec<(i32, i32, u8)> {
        let steps = self.walk_steps();
        self.unit_mut(r).act = 3;
        for &(x, y, f) in &steps {
            set_facing(self.unit_mut(r), f);
            let u = self.unit_mut(r);
            u.x = x;
            u.y = y;
        }
        if let Some(&dest) = self.range.path.first() {
            let b = self.range.nodes[dest].budget;
            let u = self.unit_mut(r);
            u.ap = b;
            if u.ap < u.ap_carry {
                u.ap_carry = u.ap;
            }
        }
        self.unit_mut(r).act = 4;
        steps
    }

    /// The steps of a walk along `self.range.path` in walking order
    /// (FUN_0044c331 iterates `path[len-2] .. path[0]`), each with the
    /// facing of FUN_004419dc (which child slot of the previous node it is).
    pub fn walk_steps(&self) -> Vec<(i32, i32, u8)> {
        let p = &self.range.path;
        if p.len() < 2 {
            return Vec::new();
        }
        (0..p.len() - 1)
            .rev()
            .map(|k| {
                let parent = self.range.nodes[p[k + 1]];
                let n = self.range.nodes[p[k]];
                let c = p[k] as i16;
                let f = if parent.child[0] == c {
                    2
                } else if parent.child[1] == c {
                    0
                } else if parent.child[2] == c {
                    3
                } else {
                    1
                };
                (n.x as i32, n.y as i32, f)
            })
            .collect()
    }

    /// Port of FUN_0043ce32: the unit knows a skill of kind 1 (magic), 2
    /// (technique) or 3 (any), honouring the player-side stage seals.
    pub fn knows_skill_kind(&self, r: UnitRef, kind: i32) -> bool {
        let mut k = kind;
        if self.army(r.side).side == Side::Player {
            if self.seal_magic {
                if k == 1 {
                    return false;
                }
                if k == 3 {
                    k = 2;
                }
            }
            if self.seal_tech {
                if k == 2 {
                    return false;
                }
                if k == 3 {
                    k = 1;
                }
            }
        }
        let n_magic = super::stats::MAGIC_SCHOOLS;
        let (a, b) = match k {
            1 => (0, n_magic),
            2 => (n_magic, 11),
            _ => (0, 11),
        };
        self.unit(r).learned[a..b].iter().any(|&m| m != 0)
    }
}

/// FUN_00441a24 (logic part): set the facing bits, keeping the animation set.
pub fn set_facing(u: &mut super::unit::Unit, f: u8) {
    u.facing = (f & 3) | (u.facing & 0xfc);
    // The view half (FUN_0043a1fe for the new facing's action, FUN_0043a34f)
    // runs when the view next syncs the unit (`view::sync_sprites`).
}

/// Port of FUN_004576f7: facing (0 down-left, 1 down-right, 2 up-right,
/// 3 up-left) of a unit at (x, y) toward (tx, ty); `cur` if they coincide.
pub fn facing_toward(x: i32, y: i32, tx: i32, ty: i32, cur: u8) -> u8 {
    if x == tx && y == ty {
        return cur;
    }
    let down_or_same = y <= ty;
    let left = |down: bool| if down { 0 } else { 3 };
    let right = |up: bool| if up { 2 } else { 1 };
    if y & 1 == 0 {
        if tx < x {
            left(down_or_same)
        } else {
            right(ty < y)
        }
    } else if x < tx {
        right(ty < y)
    } else {
        left(down_or_same)
    }
}

/// Port of FUN_004080b0: 1 front, 2 side, 3 back for an attacker at (ax, ay)
/// against `def` at (dx, dy) facing `face` (bits 0-1 of `Unit::facing`).
/// `simple` (difficulty 1) always returns 1.
pub fn facing_relation(ax: i32, ay: i32, dx: i32, dy: i32, face: u8, simple: bool) -> i32 {
    if simple {
        return 1;
    }
    let f = face; // the original compares the whole byte with 0..3
    let pick = |back: u8, front: u8| if f == back { 3 } else if f == front { 1 } else { 2 };
    if ay & 1 == 0 {
        if dx < ax {
            if dy < ay {
                pick(3, 1)
            } else {
                pick(0, 2)
            }
        } else if dy < ay {
            pick(2, 0)
        } else {
            pick(1, 3)
        }
    } else if ax < dx {
        if dy < ay {
            pick(2, 0)
        } else {
            pick(1, 3)
        }
    } else if dy < ay {
        pick(3, 1)
    } else {
        pick(0, 2)
    }
}
