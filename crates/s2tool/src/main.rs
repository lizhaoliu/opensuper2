//! s2tool: inspect and dump SuperSp (北方密使) game data.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use s2data::archive::{offset_table, parse_header};
use s2data::sprite::{FrameSet, Image};
use s2data::{Archives, GameDir, Kind};
use s2tool::{master_palette, write_png};

#[derive(Parser)]
#[command(about = "Inspect and dump SuperSp game data")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR")]
    game: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print archive headers and index sizes
    Info,
    /// Render every frame of one frame set from a CEL group into a contact sheet PNG
    Sheet {
        /// CEL top-level entry (1-based)
        group: usize,
        /// Frame set within the group (0-based)
        set: usize,
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = GameDir::open(&cli.game).context("opening game directory")?;
    let arcs = Archives::open(&dir)?;
    match cli.cmd {
        Cmd::Info => info(&dir, &arcs),
        Cmd::Sheet { group, set, out } => sheet(&dir, &arcs, group, set, &out),
    }
}

fn info(dir: &GameDir, arcs: &Archives) -> Result<()> {
    for (rel, kind) in [
        ("supersp.bin", Kind::Bin),
        ("supersp.cel", Kind::Cel),
        ("supersp.prg", Kind::Prg),
        ("supersp/supersp.dat", Kind::Dat),
    ] {
        let head = dir.read(rel).map(|b| b[..28].to_vec()).unwrap_or_default();
        let hdr = parse_header(&head).map(|(t, v, n)| format!("type {} v{v} n={n}", t as char)).unwrap_or_else(|e| e.to_string());
        println!("{rel:22} {hdr}; index entries: {}", arcs.count(kind));
        for id in 1..=arcs.count(kind) {
            let s = arcs.span(kind, id)?;
            println!("  {:>3}: off {:#010x} size {:>9}", id, s.offset, s.size);
        }
    }
    Ok(())
}

fn sheet(dir: &GameDir, arcs: &Archives, group: usize, set: usize, out: &Path) -> Result<()> {
    let pal = master_palette(dir)?;
    let g = arcs.read(Kind::Cel, group)?;
    let table = offset_table(&g)?;
    let span = table.get(set).with_context(|| format!("group {group} has {} sets", table.len()))?;
    let fs = FrameSet::parse(g[span.offset as usize..(span.offset + span.size) as usize].to_vec())?;
    let frames: Vec<Image> = (0..fs.frame_count).map(|i| fs.frame(i)).collect::<Result<_, _>>()?;
    let cw = frames.iter().map(|f| f.width).max().unwrap_or(1);
    let ch = frames.iter().map(|f| f.height).max().unwrap_or(1);
    let cols = frames.len().clamp(1, 8);
    let rows = frames.len().div_ceil(cols).max(1);
    let mut canvas = Image::new(cw * cols, ch * rows);
    for (i, f) in frames.iter().enumerate() {
        canvas.blit(f, ((i % cols) * cw) as isize, ((i / cols) * ch) as isize);
    }
    write_png(out, &canvas, &pal, false)?;
    println!("{} frames, cell {}x{}, anchor {:?}, delay {}", fs.frame_count, cw, ch, fs.anchor, fs.delay);
    Ok(())
}
