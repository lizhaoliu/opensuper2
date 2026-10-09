//! s2gfx: decode and dump every graphics format of SuperSp (北方密使).

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use s2data::anim::{load_cutscenes, open_cutscene};
use s2data::archive::offset_table;
use s2data::gfx_map::{BattleMap, MapRenderer, MapTiles, RenderOptions, Shading};
use s2data::gfx_palette::{tint_tables, vga_to_rgb, ColorTree};
use s2data::gfx_pcx::decode_pcx;
use s2data::gfx_portrait::{PortraitKind, Portraits};
use s2data::gfx_ui::{dat10_images, dat7_images, UiImages};
use s2data::gfx_unit::UnitSprites;
use s2data::sprite::{FrameSet, Image};
use s2data::{Archives, GameDir, Kind};

type Rgb = [[u8; 3]; 256];

#[derive(Parser)]
#[command(about = "Decode and dump SuperSp graphics")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR")]
    game: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dump everything into OUTDIR (sub-folders per kind)
    DumpAll {
        out: PathBuf,
        /// Keep every Nth cutscene frame
        #[arg(long, default_value_t = 60)]
        flc_every: usize,
    },
    /// Render one battle map background (1-based id)
    Map {
        id: usize,
        out: PathBuf,
        /// Stand a sample unit on every 7th cell to check occlusion
        #[arg(long)]
        units: bool,
    },
    /// Dump frames of one cutscene (1-based id)
    Flc {
        id: usize,
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        every: usize,
    },
    /// Contact sheet of one unit type (1-based)
    Unit {
        id: usize,
        out: PathBuf,
        #[arg(long)]
        summon: bool,
    },
}

struct Ctx {
    arcs: Archives,
    pal6: Vec<u8>,
    rgb: Rgb,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = GameDir::open(&cli.game).context("opening game directory")?;
    let arcs = Archives::open(&dir)?;
    let pal6 = dir.read("supersp.pal")?[..0x300].to_vec();
    let rgb = vga_to_rgb(&pal6);
    let ctx = Ctx { arcs, pal6, rgb };
    match cli.cmd {
        Cmd::DumpAll { out, flc_every } => dump_all(&ctx, &out, flc_every),
        Cmd::Map { id, out, units } => {
            let tiles = MapTiles::load(&ctx.arcs)?;
            let shading = shading(&ctx);
            let img = render_map(&ctx, &tiles, &shading, id, units)?;
            write_png(&out, &img, &ctx.rgb, false)
        }
        Cmd::Flc { id, out, every } => dump_flc(&ctx, id, &out, every.max(1)),
        Cmd::Unit { id, out, summon } => {
            let bank = if summon { UnitSprites::load_summon(&ctx.arcs)? } else { UnitSprites::load_normal(&ctx.arcs)? };
            unit_sheet(&bank, id, &out, &ctx.rgb)
        }
    }
}

fn shading(ctx: &Ctx) -> Shading {
    Shading::from_tints(&tint_tables(&ColorTree::for_master(&ctx.pal6)))
}

fn write_png(path: &Path, img: &Image, pal: &Rgb, transparent: bool) -> Result<()> {
    let w = BufWriter::new(File::create(path).with_context(|| format!("creating {}", path.display()))?);
    let mut enc = png::Encoder::new(w, img.width as u32, img.height as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(pal.iter().flatten().copied().collect::<Vec<u8>>());
    if transparent {
        enc.set_trns(vec![0u8]);
    }
    enc.write_header()?.write_image_data(&img.pixels)?;
    Ok(())
}

/// Grid of images in equal cells.
fn sheet(images: &[Image], cols: usize) -> Image {
    let cw = images.iter().map(|i| i.width).max().unwrap_or(1);
    let ch = images.iter().map(|i| i.height).max().unwrap_or(1);
    let cols = cols.clamp(1, images.len().max(1));
    let rows = images.len().div_ceil(cols).max(1);
    let mut canvas = Image::new(cw * cols, ch * rows);
    for (i, f) in images.iter().enumerate() {
        canvas.blit(f, ((i % cols) * cw) as isize, ((i / cols) * ch) as isize);
    }
    canvas
}

fn subdir(out: &Path, name: &str) -> Result<PathBuf> {
    let d = out.join(name);
    fs::create_dir_all(&d)?;
    Ok(d)
}

fn dump_all(ctx: &Ctx, out: &Path, flc_every: usize) -> Result<()> {
    let pal = &ctx.rgb;

    let ui = UiImages::load(&ctx.arcs)?;
    let d = subdir(out, "ui")?;
    for id in 1..=ui.max_id() {
        write_png(&d.join(format!("img{id:03}.png")), &ui.get(id)?, pal, true)?;
    }
    println!("ui: {} images", ui.max_id());

    let d = subdir(out, "dat7")?;
    let bank = dat7_images(&ctx.arcs)?;
    for i in 0..bank.len() {
        write_png(&d.join(format!("dat7_{:02}.png", i + 1)), &bank.image(i)?, pal, false)?;
    }
    let bank = dat10_images(&ctx.arcs)?;
    for i in 0..bank.len() {
        write_png(&d.join(format!("dat10_{:02}_unknown_palette.png", i + 1)), &bank.image(i)?, pal, false)?;
    }
    println!("dat7/dat10: done");

    let portraits = Portraits::load(&ctx.arcs)?;
    let d = subdir(out, "portraits")?;
    for id in 1..=portraits.count() {
        write_png(&d.join(format!("{id:03}_dialogue.png")), &portraits.get(id, PortraitKind::Dialogue)?, pal, false)?;
        write_png(&d.join(format!("{id:03}_battle.png")), &portraits.get(id, PortraitKind::Battle)?, pal, false)?;
    }
    println!("portraits: {}", portraits.count());

    let d = subdir(out, "units")?;
    for (name, bank) in [("normal", UnitSprites::load_normal(&ctx.arcs)?), ("summon", UnitSprites::load_summon(&ctx.arcs)?)] {
        for id in 1..=bank.types.len() {
            unit_sheet(&bank, id, &d.join(format!("{name}_{id:03}.png")), pal)?;
        }
        println!("units {name}: {} types, {} frames", bank.types.len(), bank.frames.len());
    }

    let d = subdir(out, "effects")?;
    let g = ctx.arcs.read(Kind::Cel, 1)?;
    let table = offset_table(&g)?;
    for (i, span) in table.iter().enumerate() {
        let fs = FrameSet::parse(g[span.offset as usize..(span.offset + span.size) as usize].to_vec())?;
        let frames: Vec<Image> = (0..fs.frame_count).map(|k| fs.frame(k)).collect::<Result<_, _>>()?;
        if !frames.is_empty() {
            write_png(&d.join(format!("fx_{:03}.png", i + 1)), &sheet(&frames, 8), pal, false)?;
        }
    }
    println!("effects: {}", table.len());

    let d = subdir(out, "pcx")?;
    for entry in (11..=20).chain(26..=40) {
        let pcx = decode_pcx(&ctx.arcs.read(Kind::Dat, entry)?)?;
        write_png(&d.join(format!("dat{entry:02}.png")), &pcx.image, &pcx.palette, false)?;
    }
    println!("pcx: done");

    let tiles = MapTiles::load(&ctx.arcs)?;
    let d = subdir(out, "backdrops")?;
    for i in 0..tiles.backdrop.len() {
        write_png(&d.join(format!("prg8_{:02}.png", i + 1)), &tiles.backdrop.image(i)?, pal, false)?;
    }
    let shading = shading(ctx);
    let d = subdir(out, "maps")?;
    let n = BattleMap::count(&ctx.arcs)?;
    for id in 1..=n {
        write_png(&d.join(format!("map_{id:03}.png")), &render_map(ctx, &tiles, &shading, id, false)?, pal, false)?;
    }
    println!("maps: {n}");

    let d = subdir(out, "flc")?;
    for cs in load_cutscenes(&ctx.arcs)? {
        let sub = d.join(format!("{}_{}", cs.id, cs.name.replace('.', "_")));
        dump_flc(ctx, cs.id, &sub, flc_every)?;
    }
    Ok(())
}

fn render_map(ctx: &Ctx, tiles: &MapTiles, shading: &Shading, id: usize, units: bool) -> Result<Image> {
    let counts = [tiles.ground.len(), tiles.overlay.len(), tiles.object.len()];
    let map = BattleMap::load(&ctx.arcs, id, counts)?;
    let mut r = MapRenderer::new(&map, tiles, shading, RenderOptions::default());
    let mut img = r.render_background();
    if units {
        let bank = UnitSprites::load_normal(&ctx.arcs)?;
        let (frame, dx) = bank.frame(1, 1)?;
        for row in 1..map.rows() {
            for col in 0..map.width {
                if (row * map.width + col) % 7 != 0 || map.cell(col, row).is_none_or(|c| c.height == 0 || c.ground < 6) {
                    continue;
                }
                let mut f = frame.clone();
                r.occlude(&mut f, col, row, s2data::gfx_map::Occlusion { dx: dx as i32, ..Default::default() });
                let (x, y) = map.unit_position(col, row, f.width, f.height, dx as i32, 0);
                img.blit(&f, x as isize, y as isize);
            }
        }
    }
    Ok(img)
}

fn dump_flc(ctx: &Ctx, id: usize, out: &Path, every: usize) -> Result<()> {
    fs::create_dir_all(out)?;
    let mut flc = open_cutscene(&ctx.arcs, id)?;
    let mut n = 0;
    while flc.next_frame()? {
        if (flc.position() - 1) % every == 0 || flc.position() == flc.frame_count() {
            let (img, x, y) = flc.screen_frame();
            let mut screen = Image::new(640, 480);
            screen.blit_opaque(&img, x as isize, y as isize);
            write_png(&out.join(format!("frame_{:04}.png", flc.position())), &screen, &vga_to_rgb(&flc.palette6), false)?;
            n += 1;
        }
    }
    println!("cutscene {id}: {} frames ({}x{}, {} cs/frame), wrote {n}", flc.frame_count(), flc.header.width, flc.header.height, flc.delay_cs());
    Ok(())
}

fn unit_sheet(bank: &UnitSprites, id: usize, out: &Path, pal: &Rgb) -> Result<()> {
    let t = bank.unit_type(id)?;
    let frames: Vec<Image> = (1..=t.frame_count).map(|n| bank.frame(id, n).map(|(f, _)| f)).collect::<Result<_, _>>()?;
    write_png(out, &sheet(&frames, 12), pal, false)
}
