//! s2text: dump SuperSp text, fonts and stage event scripts.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use s2data::font::{Font, GlyphKind, Mode, Style};
use s2data::msg::{self, parse_dialogue, Messages, Token};
use s2data::script::{self, Stage, EVENT_LEN, LINE_LEN};
use s2data::sprite::Image;
use s2data::text::display;
use s2data::{Archives, GameDir, Kind};
use s2tool::{master_palette, write_png};

#[derive(Parser)]
#[command(about = "Dump SuperSp text, fonts and stage scripts")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR")]
    game: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write every stage listing to <out>/stages.txt and verify script coverage
    Stages { out: PathBuf },
    /// Write UI strings, help texts, endings and town text to <out>/*.txt
    Strings { out: PathBuf },
    /// Render a Big5 glyph sheet (size 16 or 24) to a PNG
    FontSheet {
        out: PathBuf,
        #[arg(long, default_value_t = 24)]
        size: u32,
        /// Glyphs per row
        #[arg(long, default_value_t = 64)]
        cols: usize,
        /// Use the alternate 16x16 set (BIN 14, the 簡體版 shapes)
        #[arg(long)]
        alt: bool,
    },
    /// Render one stage event's dialogue lines (text only) to a PNG
    Dialogue { stage: usize, event: usize, out: PathBuf },
    /// List every distinct markup token used in dialogue
    Markup,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = GameDir::open(&cli.game).context("opening game directory")?;
    let arcs = Archives::open(&dir)?;
    match cli.cmd {
        Cmd::Stages { out } => stages(&arcs, &out),
        Cmd::Strings { out } => strings(&dir, &arcs, &out),
        Cmd::FontSheet { out, size, cols, alt } => font_sheet(&dir, &arcs, &out, size, cols, alt),
        Cmd::Dialogue { stage, event, out } => dialogue(&dir, &arcs, stage, event, &out),
        Cmd::Markup => markup(&arcs),
    }
}

fn stages(arcs: &Archives, out: &std::path::Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let bin1 = arcs.read(Kind::Bin, script::BIN_STAGES)?;
    let stages = Stage::parse_all(&bin1)?;
    let mut text = String::new();
    let mut problems = 0;
    let mut kinds = [0usize; 17];
    let mut lines = 0;
    for st in &stages {
        text.push_str(&script::listing(st));
        text.push('\n');
        // Coverage: header + events + every line table + text pool must tile the script.
        let mut used = 4 + EVENT_LEN * st.events.len();
        let mut cursor = used;
        for e in &st.events {
            kinds[(e.kind as usize).min(16)] += 1;
            lines += e.lines.len();
            if e.lines.is_empty() && e.data_size == 0 {
                continue;
            }
            if e.lines_offset as usize != cursor {
                println!("stage {}: event lines at {:#x}, expected {:#x}", st.id, e.lines_offset, cursor);
                problems += 1;
            }
            let span = e.data_size as usize;
            if span < LINE_LEN * e.lines.len() {
                println!("stage {}: event data {} smaller than its line table", st.id, span);
                problems += 1;
            }
            used += span;
            cursor = e.lines_offset as usize + span;
        }
        if used != st.script_len {
            println!("stage {}: script {} bytes, events+lines+text cover {}", st.id, st.script_len, used);
            problems += 1;
        }
        if st.events.iter().any(|e| e.kind > 16) {
            println!("stage {}: unknown event type", st.id);
            problems += 1;
        }
        for r in &st.rewards {
            if let script::Reward::Unknown(k, _) = r {
                println!("stage {}: unknown reward kind {k}", st.id);
                problems += 1;
            }
        }
    }
    fs::write(out.join("stages.txt"), text)?;
    println!("{} stages, {} dialogue lines; events by type: {:?}", stages.len(), lines, &kinds[..]);
    println!("{problems} problems; listing in {}", out.join("stages.txt").display());
    Ok(())
}

fn numbered(list: &[Vec<u8>]) -> String {
    list.iter().enumerate().map(|(i, s)| format!("{i:>4}: {}\n", display(s).replace('\n', "⏎"))).collect()
}

fn strings(dir: &GameDir, arcs: &Archives, out: &std::path::Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let loose = dir.read("itemhlsp.bin").ok();
    let m = Messages::load(arcs, loose.as_deref())?;
    fs::write(out.join("ui.txt"), numbered(&m.ui))?;
    fs::write(out.join("spell_help.txt"), numbered(&m.spell_help))?;
    fs::write(out.join("item_help.txt"), numbered(&m.item_help))?;
    let alt = msg::parse_indexed_strings(&arcs.read(Kind::Bin, msg::BIN_UI_STRINGS_ALT)?)?;
    let diffs: String = m
        .ui
        .iter()
        .zip(&alt)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| format!("{i:>4}: {} | {}\n", display(a), display(b)))
        .collect();
    fs::write(out.join("ui_alt_diff.txt"), diffs)?;
    let setup = msg::parse_indexed_strings(&arcs.read(Kind::Dat, msg::DAT_SETUP_STRINGS)?)?;
    fs::write(out.join("setup.txt"), numbered(&setup))?;
    let mut e = String::new();
    for (i, p) in m.endings.pages.iter().enumerate() {
        e.push_str(&format!("--- page {i}\n{}\n", display(p)));
    }
    for (v, ep) in m.endings.epilogues.iter().enumerate() {
        e.push_str(&format!("--- epilogue {v}\n"));
        for en in ep {
            e.push_str(&format!("[{}] {}\n", en.char_id, display(&en.text)));
        }
    }
    fs::write(out.join("endings.txt"), e)?;
    let mut t = String::new();
    for (i, town) in m.towns.iter().enumerate() {
        t.push_str(&format!("{:>3} {} @({},{}) kind {} shops {:?}\n", i + 1, display(&town.name), town.x, town.y, town.kind, town.shops));
        for g in &town.gossip {
            t.push_str(&format!("      {}\n", display(g)));
        }
    }
    fs::write(out.join("towns.txt"), t)?;
    println!(
        "ui {} (alt differs in {}), spell help {}, item help {} ({}), endings {} pages + {} epilogues, towns {}, setup {}",
        m.ui.len(),
        m.ui.iter().zip(&alt).filter(|(a, b)| a != b).count(),
        m.spell_help.len(),
        m.item_help.len(),
        if loose.is_some() { "ITEMHLSP.BIN" } else { "DAT 3" },
        m.endings.pages.len(),
        m.endings.epilogues.len(),
        m.towns.len(),
        setup.len()
    );
    Ok(())
}

fn font_sheet(dir: &GameDir, arcs: &Archives, out: &std::path::Path, size: u32, cols: usize, alt: bool) -> Result<()> {
    let font = Font::load(arcs, alt)?;
    let pal = master_palette(dir)?;
    let (kind, cell) = match size {
        16 => (GlyphKind::Wide16, 18),
        24 => (GlyphKind::Wide24, 26),
        _ => bail!("size must be 16 or 24"),
    };
    let n = font.glyph_count();
    let rows = n.div_ceil(cols);
    let mut img = Image::new(cols * cell, rows * cell);
    let style = Style::new(0x0f, 0, Mode::Plain);
    for g in 1..=n {
        if let Some(bits) = font.bits_by_number(kind, g) {
            let i = g - 1;
            s2data::font::draw_bits(&mut img, kind, bits, ((i % cols) * cell) as isize, ((i / cols) * cell) as isize, style);
        }
    }
    write_png(out, &img, &pal, false)?;
    println!("{n} glyphs, {}x{} px", img.width, img.height);
    Ok(())
}

fn dialogue(dir: &GameDir, arcs: &Archives, stage: usize, event: usize, out: &std::path::Path) -> Result<()> {
    let bin1 = arcs.read(Kind::Bin, script::BIN_STAGES)?;
    let st = Stage::parse(&bin1, stage)?;
    let ev = st.events.get(event).with_context(|| format!("stage {stage} has {} events", st.events.len()))?;
    let font = Font::load(arcs, false)?;
    let pal = master_palette(dir)?;
    // One 640x200 panel per line, laid out like the in-game box (text area only).
    let panel = 200;
    let mut img = Image::new(640, panel * ev.lines.len().max(1));
    for (i, line) in ev.lines.iter().enumerate() {
        let oy = (i * panel) as isize;
        for y in 0..panel {
            for x in 0..640 {
                img.pixels[(oy as usize + y) * 640 + x] = if y == 0 { 0x0f } else { 0 };
            }
        }
        let name_w = Font::line_width(&line.name);
        font.draw_line(&mut img, msg::layout::NAME_CX - name_w / 2, oy + msg::layout::NAME_Y - 60, &line.name, Style::new(0x36, 0, Mode::Outline));
        let (mut x, mut y) = (msg::layout::TEXT_X, oy + msg::layout::TEXT_Y);
        let mut style = Style::new(msg::layout::TEXT_COLOR, 0, Mode::Plain);
        let mut at_line_start = true;
        for tok in parse_dialogue(&line.text) {
            match tok {
                Token::Text(t) => {
                    for ch in s2data::text::chars(&t) {
                        let (bytes, adv): (Vec<u8>, isize) = match ch {
                            s2data::text::Ch::Byte(c) => (vec![c], 16),
                            s2data::text::Ch::Wide(h, l) => (vec![h, l], 24),
                        };
                        // The typewriter draws ASCII without the +2 nudge of draw_line.
                        match ch {
                            s2data::text::Ch::Byte(c) => font.draw_glyph(&mut img, GlyphKind::Ascii16, c.max(0x1e), 0, x, y, style),
                            _ => font.draw_glyph(&mut img, GlyphKind::Wide24, bytes[0], bytes[1], x, y, style),
                        }
                        x += adv;
                        at_line_start = false;
                        if x >= msg::layout::WRAP_X {
                            x = msg::layout::TEXT_X;
                            y += msg::layout::LINE_H;
                            at_line_start = true;
                        }
                    }
                }
                Token::Break => {
                    if !at_line_start {
                        x = msg::layout::TEXT_X;
                        y += msg::layout::LINE_H;
                        at_line_start = true;
                    }
                }
                Token::Color { color, second } => style = Style::new(color, second, Mode::Plain),
                Token::ResetColor => style = Style::new(msg::layout::TEXT_COLOR, 0, Mode::Plain),
                Token::Delay(_) | Token::ResetDelay => {}
            }
        }
    }
    write_png(out, &img, &pal, false)?;
    println!("stage {stage} event {event}: {}", script::describe_event(ev));
    Ok(())
}

fn markup(arcs: &Archives) -> Result<()> {
    let bin1 = arcs.read(Kind::Bin, script::BIN_STAGES)?;
    let stages = Stage::parse_all(&bin1)?;
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut breaks = 0;
    let mut raw_at = 0;
    for st in &stages {
        for e in &st.events {
            for l in &e.lines {
                if l.text.windows(2).any(|w| w == b"@@") {
                    raw_at += 1;
                }
                for t in parse_dialogue(&l.text) {
                    match t {
                        Token::Text(_) => {}
                        Token::Break => breaks += 1,
                        other => *seen.entry(format!("{other:?}")).or_default() += 1,
                    }
                }
            }
        }
    }
    println!("lines containing @@: {raw_at}; '*' breaks: {breaks}");
    for (k, v) in seen {
        println!("{v:>6}  {k}");
    }
    Ok(())
}
