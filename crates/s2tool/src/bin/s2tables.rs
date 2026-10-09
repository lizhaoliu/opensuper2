//! s2tables: dump the rule tables, stages, battle maps and saves as text/PNG.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use s2data::map::{map_count, BattleMap, Passage, TileAttrs};
use s2data::save::SaveFile;
use s2data::sprite::Image;
use s2data::tables::{source, RuleTables, GROWTH_STATS};
use s2data::tbl_stage::{Reward, StageFile};
use s2data::{Archives, GameDir, Kind, Palette};
use s2tool::write_png;

#[derive(Parser)]
#[command(about = "Dump SuperSp rule tables, stages, maps and saves")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR")]
    game: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write every table, stage and map to an output directory
    All { out: PathBuf },
    Classes,
    Actions,
    Items,
    Terrain,
    Spells,
    Growth,
    Params,
    World,
    /// List stages (campaign, or FSTAGE.BIN with --vs)
    Stages {
        #[arg(long)]
        vs: bool,
    },
    /// Show one stage: header, rewards, texts, unit placements
    Stage {
        id: usize,
        #[arg(long)]
        vs: bool,
    },
    /// Render a battle map's terrain as ASCII, optionally to PNG
    Map { id: usize, png: Option<PathBuf> },
    /// Decode a save file
    Save { file: PathBuf },
}

struct Ctx {
    dir: GameDir,
    arcs: Archives,
    t: RuleTables,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = GameDir::open(&cli.game).context("opening game directory")?;
    let arcs = Archives::open(&dir)?;
    let t = RuleTables::load(&dir, &arcs)?;
    let cx = Ctx { dir, arcs, t };
    let out = match cli.cmd {
        Cmd::All { out } => return all(&cx, &out),
        Cmd::Classes => classes(&cx),
        Cmd::Actions => actions(&cx),
        Cmd::Items => items(&cx),
        Cmd::Terrain => terrain(&cx),
        Cmd::Spells => spells(&cx),
        Cmd::Growth => growth(&cx),
        Cmd::Params => params(&cx),
        Cmd::World => world(&cx),
        Cmd::Stages { vs } => stages(&cx, vs)?,
        Cmd::Stage { id, vs } => stage(&cx, id, vs)?,
        Cmd::Map { id, png } => map(&cx, id, png.as_deref())?,
        Cmd::Save { file } => save(&cx, &file)?,
    };
    print!("{out}");
    Ok(())
}

fn all(cx: &Ctx, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let w = |name: &str, s: String| fs::write(out.join(name), s).with_context(|| format!("writing {name}"));
    w("classes.tsv", classes(cx))?;
    w("actions.tsv", actions(cx))?;
    w("items.tsv", items(cx))?;
    w("terrain.tsv", terrain(cx))?;
    w("spells.tsv", spells(cx))?;
    w("growth.tsv", growth(cx))?;
    w("params.txt", params(cx))?;
    w("world.tsv", world(cx))?;
    w("stages.tsv", stages(cx, false)?)?;
    w("vs_stages.tsv", stages(cx, true)?)?;
    let sf = campaign(cx)?;
    fs::create_dir_all(out.join("stages"))?;
    for id in 1..=sf.len() {
        w(&format!("stages/{id:03}.txt"), stage(cx, id, false)?)?;
    }
    let prg6 = cx.arcs.read(Kind::Prg, 6)?;
    fs::create_dir_all(out.join("maps"))?;
    for id in 1..=map_count(&prg6)? {
        let png = out.join(format!("maps/{id:03}.png"));
        w(&format!("maps/{id:03}.txt"), map(cx, id, Some(&png))?)?;
    }
    println!("wrote tables to {}", out.display());
    Ok(())
}

fn mask_names(mask: u32, names: &[String]) -> String {
    let v: Vec<&str> = (0..32).filter(|b| mask & (1 << b) != 0).map(|b| names.get(b).map_or("?", |s| s.as_str())).collect();
    if v.is_empty() { "-".into() } else { v.join("+") }
}

fn class_name(cx: &Ctx, id: i32) -> String {
    if id <= 0 {
        return "-".into();
    }
    cx.t.kinds.class(id as u16).map_or(format!("#{id}"), |c| c.name.clone())
}

fn item_name(cx: &Ctx, id: i32) -> String {
    if id <= 0 {
        return "-".into();
    }
    cx.t.items.get(id as usize - 1).map_or(format!("#{id}"), |i| i.name.clone())
}

fn classes(cx: &Ctx) -> String {
    let k = &cx.t.kinds;
    let mut s = String::from(
        "id\tname\ttier\ttype\tmove\tskill\texp/lv\tprof_rate\tap\talign\tresist\tweak\tschools\tchange_item\tchange_flags\trating\tsprite\tpromotions\tactions\n",
    );
    for (i, c) in k.classes.iter().enumerate() {
        let promos: Vec<String> = c.promotions.iter().filter(|&&p| p != 0).map(|&p| class_name(cx, p as i32)).collect();
        let acts: Vec<&str> = k.actions[c.actions.clone()].iter().map(|a| a.name.as_str()).collect();
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:#x}\t{}\t{}\t{}\t{}",
            i + 1,
            c.name,
            c.tier,
            k.unit_types.get(c.unit_type as usize - 1).map_or("?", |s| s.as_str()),
            c.move_type,
            k.skills.get(c.innate_skill as usize).map_or("?", |s| s.as_str()),
            c.exp_per_level,
            c.proficiency_rate,
            c.base_ap,
            c.alignment,
            mask_names(c.resist_elements as u32, &k.elements),
            mask_names(c.weak_elements as u32, &k.elements),
            mask_names(c.schools as u32, &cx.t.spells.schools),
            item_name(cx, c.change_item as i32),
            c.change_flags,
            c.rating_mult,
            c.sprite_set,
            promos.join(","),
            acts.join(","),
        );
    }
    s.push_str("\n# type matrix (row = attacker type, col = defender type, % damage bonus)\n\t");
    s.push_str(&k.unit_types.join("\t"));
    s.push('\n');
    for (r, row) in k.type_matrix.iter().enumerate() {
        let _ = writeln!(s, "{}\t{}", k.unit_types[r], row.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\t"));
    }
    let _ = writeln!(s, "\n# skills (隱技)\n{}", k.skills.iter().enumerate().map(|(i, n)| format!("{i}={n}")).collect::<Vec<_>>().join(" "));
    let _ = writeln!(s, "# elements\n{}", k.elements.iter().enumerate().map(|(i, n)| format!("{i}={n}")).collect::<Vec<_>>().join(" "));
    s
}

fn actions(cx: &Ctx) -> String {
    let mut s = String::from("idx\tkind\tname\tpower\thit\trange\tmotion\tstatus(type,chance,dur)\tanim\tsound\tshape\tused_by\n");
    for (i, a) in cx.t.kinds.actions.iter().enumerate() {
        let users: Vec<&str> =
            cx.t.kinds.classes.iter().filter(|c| c.actions.contains(&i)).map(|c| c.name.as_str()).collect();
        let _ = writeln!(
            s,
            "{i}\t{}\t{}\t{}\t{}\t{}-{}\t{},{}\t{},{},{}\t{}\t{}\t{}\t{}",
            a.kind,
            a.name,
            a.power,
            a.hit,
            a.range.0,
            a.range.1,
            a.motion.0,
            a.motion.1,
            a.status.0,
            a.status.1,
            a.status.2,
            a.anim,
            a.sound,
            a.shape,
            users.join(",")
        );
    }
    s
}

fn items(cx: &Ctx) -> String {
    let mut s = String::from(
        "id\tname\tkind\tusage\treach\tatk\thit\tdef\tmag\teva\tap\teffect\tprice\trank\tanim\tsound\tshape\tunit_types\trestrict\tcrit\thelp\n",
    );
    for it in &cx.t.items {
        let (lo, hi) = it.reach_range();
        let help = cx.t.item_help.get(it.id as usize).cloned().unwrap_or_default().replace(['\n', '\r', '\t'], " ");
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{}\t{lo}-{hi}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:#x}\t{}\t{}",
            it.id,
            it.name,
            it.kind,
            it.usage,
            it.attack,
            it.hit,
            it.defense,
            it.magic,
            it.evasion,
            it.ap,
            it.effect,
            it.price,
            it.rank,
            it.anim,
            it.sound,
            it.shape,
            mask_names(it.unit_types, &cx.t.kinds.unit_types),
            it.restrict,
            it.crit,
            help
        );
    }
    s
}

fn terrain(cx: &Ctx) -> String {
    let t = &cx.t.terrain;
    let mut s = format!("# {} ground + {} object terrain types; cell = cost/atk%/def% (x = impassable)\nclass", t.ground, t.objects);
    for n in &t.names {
        let _ = write!(s, "\t{n}");
    }
    s.push('\n');
    for (c, row) in t.effects.iter().enumerate() {
        let _ = write!(s, "{}", class_name(cx, c as i32 + 1));
        for e in row {
            if e.cost == -1 {
                let _ = write!(s, "\tx/{}/{}", e.attack, e.defense);
            } else {
                let _ = write!(s, "\t{}/{}/{}", e.cost, e.attack, e.defense);
            }
        }
        s.push('\n');
    }
    s
}

fn spells(cx: &Ctx) -> String {
    let sp = &cx.t.spells;
    let mut s = String::from(
        "id\tschool\tidx\tname\tcost\tlevel\trange\tshape\teffect\tformula\tpower\thit\tduration\telements\tflags\ttarget\tcombo\tstat\textra\n",
    );
    for (i, d) in sp.spells.iter().enumerate() {
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:#x}\t{:#x}\t{}\t{}\t{}\t{}\t{:#x}\t{}\t{:#x}\t{:#x}\t{:?}",
            i + 1,
            sp.schools.get(d.school as usize - 1).map_or("?", |s| s.as_str()),
            d.index,
            d.name,
            d.cost,
            d.level,
            d.range,
            d.shape,
            d.effect,
            d.formula,
            d.power,
            d.hit,
            d.duration,
            mask_names(d.elements as u32, &cx.t.kinds.elements),
            d.flags,
            d.target,
            d.combo,
            d.stat_flags,
            d.extra
        );
    }
    let _ = writeln!(s, "\n# schools (first spell, count)");
    for (n, (f, c)) in sp.schools.iter().zip(&sp.school_spells) {
        let _ = writeln!(s, "{n}\t{f}\t{c}");
    }
    let _ = writeln!(s, "# status names\n{}", sp.status_names.iter().enumerate().map(|(i, n)| format!("{}={n}", i + 1)).collect::<Vec<_>>().join(" "));
    s
}

fn growth(cx: &Ctx) -> String {
    let g = &cx.t.growth;
    let mut s = format!("# growth descriptors d0..d5 per stat: gain(L) = trunc(sqrt(d0/d1*L) + d2/d3 + rand(0..=d4)/(rand(0..=d5)+1) + 0.5)\n# id_limit {}\nclass\tstat\td0\td1\td2\td3\td4\td5\n", g.id_limit);
    for (c, row) in g.growth.iter().enumerate() {
        for (k, d) in row.iter().enumerate() {
            let _ = writeln!(s, "{}\t{}\t{}", class_name(cx, c as i32 + 1), GROWTH_STATS[k], d.0.map(|v| v.to_string()).join("\t"));
        }
    }
    s.push_str("\n# class base stats\nclass\t");
    s.push_str(&GROWTH_STATS.join("\t"));
    s.push_str("\tequipment\tspell masks\n");
    for (c, b) in g.base.iter().enumerate() {
        let eq: Vec<String> = b.equipment.iter().filter(|&&e| e > 0).map(|&e| item_name(cx, e as i32)).collect();
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{:x?}",
            class_name(cx, c as i32 + 1),
            b.stats.map(|v| v.to_string()).join("\t"),
            eq.join(","),
            b.spells
        );
    }
    s
}

fn params(cx: &Ctx) -> String {
    let p = &cx.t.params;
    let mut s = String::from("# BIN 10 formula constants (index: value)\n");
    for (i, v) in p.p.iter().enumerate() {
        let _ = write!(s, "{i:#05x}:{v:4} ");
        if i % 8 == 7 {
            s.push('\n');
        }
    }
    let _ = writeln!(s, "\n# per-spell bytes\n{:?}\n{:?}\n{:?}\n{:?}", p.per_spell[0], p.per_spell[1], p.per_spell[2], p.per_spell[3]);
    let _ = writeln!(s, "# effect sounds\n{:?}", p.effect_sound);
    s
}

fn world(cx: &Ctx) -> String {
    let mut s = String::from("stage\tx\ty\tchoice\tsegments\ttext\n");
    for (i, w) in cx.t.world.iter().enumerate() {
        let _ = writeln!(s, "{}\t{}\t{}\t{}\t{:?}\t{}", i + 1, w.x, w.y, w.choice, w.segments, w.text.replace(['\n', '\r'], " "));
    }
    s
}

fn campaign(cx: &Ctx) -> Result<StageFile> {
    Ok(StageFile::parse(cx.arcs.read(source::STAGES.0, source::STAGES.1)?)?)
}

fn stage_file(cx: &Ctx, vs: bool) -> Result<StageFile> {
    if vs { Ok(StageFile::parse(cx.dir.read(source::VS_STAGES)?)?) } else { campaign(cx) }
}

fn stages(cx: &Ctx, vs: bool) -> Result<String> {
    let sf = stage_file(cx, vs)?;
    let mut s = String::from("id\tnumber\ttitle\tkind\tmap\tmusic\tnext\tunits\tflags\trewards\n");
    for id in 1..=sf.len() {
        let st = sf.stage(id)?;
        let h = &st.header;
        let _ = writeln!(
            s,
            "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{}",
            h.number,
            st.title,
            h.kind,
            h.map,
            h.music,
            h.next,
            st.units.len(),
            h.flags,
            st.rewards.len()
        );
    }
    Ok(s)
}

fn stage(cx: &Ctx, id: usize, vs: bool) -> Result<String> {
    let sf = stage_file(cx, vs)?;
    let st = sf.stage(id)?;
    let h = &st.header;
    let mut s = format!(
        "stage {id}: {}\nnumber {} kind {} map {} music {} next {} flags {:?} unk0e {:?} unk17 {:?} unk1c {:?}\n",
        st.title, h.number, h.kind, h.map, h.music, h.next, h.flags, h.unknown_0e, h.unknown_17, h.unknown_1c
    );
    let _ = writeln!(s, "briefing: {}\nvictory: {}\ndefeat: {}", st.briefing, st.victory, st.defeat);
    let _ = writeln!(s, "must survive: {:?}  list_a {:?}  list_b {:?}", st.must_survive, st.list_a, st.list_b);
    for r in &st.rewards {
        let line = match r {
            Reward::Join(v) => format!("join {v:?}"),
            Reward::Leave(v) => format!("leave {v:?}"),
            Reward::Gold(g) => format!("gold {g}"),
            Reward::Items(v) => format!("items {}", v.iter().map(|&i| item_name(cx, i as i32)).collect::<Vec<_>>().join(",")),
            Reward::Treasure(v) => format!(
                "treasure {}",
                v.iter().map(|c| c.map(|i| item_name(cx, i as i32)).join("|")).collect::<Vec<_>>().join(" ; ")
            ),
            Reward::Other(k, v) => format!("reward type {k} {v:?}"),
        };
        let _ = writeln!(s, "reward: {line}");
    }
    let _ = writeln!(s, "script at block offset {} ({} bytes)", st.script_offset, st.block.len() - st.script_offset);
    s.push_str("units: char name gender class lv (x,y) control facing [HP MP ST 魔力 反應 攻擊 防禦 劍術 戰技] align ap items flags ai_mode/tag/ai_sub b33 appear kill_exp drops spells\n");
    for u in &st.units {
        let items: Vec<String> = u.items.iter().filter(|&&i| i > 0).map(|&i| item_name(cx, i as i32)).collect();
        let _ = writeln!(
            s,
            "  {:3} {} g{} {} L{} ({},{}) ctl{} face{} {:?} {} {} [{}] {:#x} {}/{}/{} {} {} {} {:?} {:?}",
            u.character,
            u.name,
            u.gender,
            class_name(cx, u.class as i32),
            u.level,
            u.x,
            u.y,
            u.control,
            u.facing,
            u.stats,
            u.alignment,
            u.ap,
            items.join(","),
            u.skill_flags,
            u.ai_mode,
            u.tag,
            u.ai_sub,
            u.b33,
            u.appear_to,
            u.kill_exp,
            u.drops.iter().map(|&i| item_name(cx, i as i32)).collect::<Vec<_>>(),
            u.spells
        );
    }
    Ok(s)
}

/// Fixed colours per terrain index for the PNG view (deterministic hash).
fn terrain_palette() -> Palette {
    let mut p = [[0u8; 3]; 256];
    for (i, c) in p.iter_mut().enumerate().skip(1) {
        let h = (i as u32).wrapping_mul(2654435761);
        *c = [(h >> 24) as u8 | 0x40, (h >> 16) as u8 | 0x40, (h >> 8) as u8 | 0x40];
    }
    p[255] = [0, 0, 0];
    Palette(p)
}

fn map(cx: &Ctx, id: usize, png: Option<&Path>) -> Result<String> {
    let prg6 = cx.arcs.read(Kind::Prg, 6)?;
    let attrs = TileAttrs::parse(&cx.arcs.read(Kind::Prg, 7)?)?;
    let m = BattleMap::parse(&prg6, id, attrs.counts())?;
    let t = &cx.t.terrain;
    let mut s = format!(
        "map {id}: {} cols x {} rows, tile {}x{} step {}, objects {}, spots {}, extra {:?}\n",
        m.width,
        m.rows(),
        m.tile_w,
        m.tile_h,
        m.row_step,
        m.objects.len(),
        m.spots.len(),
        m.extra
    );
    // legend of terrain letters used
    let letter = |i: usize| (if i < 26 { b'a' + i as u8 } else { b'A' + (i - 26) as u8 }) as char;
    s.push_str("legend:");
    for (i, n) in t.names.iter().enumerate() {
        let _ = write!(s, " {}={n}", letter(i));
    }
    s.push_str("\nground terrain (object terrain in capitals over ground; '.' = off-map; digit row = height)\n");
    for row in 0..m.rows() {
        if row % 2 == 1 {
            s.push(' ');
        }
        for col in 0..m.width {
            let ch = match m.cell(col, row) {
                None => '.',
                Some(c) => match m.terrain_of(&attrs, t, c) {
                    (_, Some(o)) => letter(o).to_ascii_uppercase(),
                    (Some(g), None) => letter(g),
                    _ => '#',
                },
            };
            s.push(ch);
            s.push(' ');
        }
        s.push('\n');
    }
    s.push_str("heights:\n");
    for row in 0..m.rows() {
        if row % 2 == 1 {
            s.push(' ');
        }
        for col in 0..m.width {
            match m.cell(col, row) {
                None => s.push_str(". "),
                Some(c) => {
                    let _ = write!(s, "{} ", c.height % 10);
                }
            }
        }
        s.push('\n');
    }
    // passability for an infantry class (class 1) as a sanity view
    if let Some(c1) = cx.t.kinds.class(1) {
        s.push_str("passage for class 1 (W walk, F fly-only, x blocked):\n");
        for row in 0..m.rows() {
            if row % 2 == 1 {
                s.push(' ');
            }
            for col in 0..m.width {
                let ch = match m.cell(col, row) {
                    None => '.',
                    Some(c) => match m.passage(&attrs, t, 1, c1, c, false).0 {
                        Passage::Walk => 'W',
                        Passage::Fly => 'F',
                        Passage::Blocked => 'x',
                    },
                };
                s.push(ch);
                s.push(' ');
            }
            s.push('\n');
        }
    }
    for o in &m.objects {
        let _ = writeln!(s, "object ({},{}) {}x{} tile {}", o.col, o.row, o.w, o.h, o.tile);
    }
    for sp in &m.spots {
        let _ = writeln!(s, "spot ({},{}) flags {:#06x}", sp.col, sp.row, sp.flags);
    }
    if let Some(path) = png {
        // 8x8 px per cell, staggered rows shifted 4 px; colour = terrain (object over ground).
        let (cw, ch) = (8usize, 8usize);
        let mut img = Image::new(m.width * cw + cw / 2, m.rows() * ch / 2 + ch);
        for row in 0..m.rows() {
            for col in 0..m.width {
                let Some(c) = m.cell(col, row) else { continue };
                let (g, o) = m.terrain_of(&attrs, t, c);
                let idx = o.or(g).map_or(255u8, |i| i as u8 + 1);
                let x0 = col * cw + if row % 2 == 1 { cw / 2 } else { 0 };
                let y0 = row * ch / 2;
                for dy in 0..ch {
                    for dx in 0..cw {
                        // diamond mask
                        let (fx, fy) = (dx as i32 * 2 - cw as i32 + 1, dy as i32 * 2 - ch as i32 + 1);
                        if fx.abs() * ch as i32 + fy.abs() * cw as i32 <= (cw * ch) as i32 {
                            img.pixels[(y0 + dy) * img.width + x0 + dx] = idx;
                        }
                    }
                }
            }
        }
        write_png(path, &img, &terrain_palette(), true)?;
    }
    Ok(s)
}

fn save(cx: &Ctx, file: &Path) -> Result<String> {
    let raw = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let sv = SaveFile::parse(&raw, cx.t.items.len())?;
    let mut s = format!("{}: stage {} difficulty {}\n", sv.description_text(), sv.stage(), sv.difficulty());
    for (label, a) in [("player", &sv.player), ("enemy", &sv.enemy)] {
        let _ = writeln!(s, "{label}: {} units, gold {}, scale {}%, flags {:#x}", a.units.len(), a.gold, a.scale, a.flags);
        for u in &a.units {
            let _ = writeln!(
                s,
                "  {} {} L{} HP {}/{} MP {}/{} ST {}/{} 攻{} 防{} 戰技{} 反應{} 魔力{} 劍術{} AP{} exp {}/{} at ({},{})",
                u.name(),
                class_name(cx, u.class() as i32),
                u.level(),
                u.hp(),
                u.hp_max(),
                u.mp(),
                u.mp_max(),
                u.st(),
                u.st_max(),
                u.attack(),
                u.defense(),
                u.skill(),
                u.reaction_base(),
                u.magic(),
                u.sword(),
                u.ap(),
                u.exp(),
                u.exp_per_level(),
                u.x(),
                u.y()
            );
        }
    }
    if let Some(f) = &sv.script_flags {
        let _ = writeln!(s, "script flags ({}): {:?}", f.len(), f);
    }
    Ok(s)
}
