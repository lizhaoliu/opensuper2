//! s2audio: list and dump SuperSp sound effects and music, print music cues.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use s2data::audio::{cd_track, parse_effect_sounds, parse_movies, Bank, Disc, Wave, Xmidi};
use s2data::audio::{EFFECT_SOUND_ENTRY, MAX_MUSIC_ID, MOVIE_CUE_ENTRY};
use s2data::{Archives, GameDir, Kind};

#[derive(Parser)]
#[command(about = "List and dump SuperSp sound effects and music")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR")]
    game: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every sound effect and music track with its format and length
    List,
    /// Write sounds as .wav and music as .xmi plus converted .mid
    Dump { out: PathBuf },
    /// Print which music plays where (CD track tables, scenes, stages, movies)
    Cues,
}

/// Fixed music cues found in the exe; (music id, where, call site).
const SCENE_MUSIC: &[(u16, &str, &str)] = &[
    (25, "title / main menu, load screen, quit-to-title", "FUN_0040d7e1, FUN_0040de96, FUN_00444bea"),
    (4, "intermission menu between stages", "FUN_00410763, FUN_004109b3"),
    (37, "intermission sub-menu (menu option 3)", "FUN_00410c21"),
    (36, "intermission map/selection screen", "FUN_00414b17"),
    (13, "ending sequence part 1 (MIDI forced)", "FUN_0041abf4"),
    (20, "ending sequence part 2 (MIDI forced)", "FUN_0041b489"),
];

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = GameDir::open(&cli.game).context("opening game directory")?;
    let arcs = Archives::open(&dir)?;
    match cli.cmd {
        Cmd::List => list(&arcs),
        Cmd::Dump { out } => dump(&arcs, &out),
        Cmd::Cues => cues(&arcs),
    }
}

fn list(arcs: &Archives) -> Result<()> {
    let sounds = Bank::sounds(arcs)?;
    let mut total = 0.0;
    for id in 1..=sounds.len() {
        let w = Wave::parse(&sounds.raw(arcs, id)?).with_context(|| format!("sound {id}"))?;
        total += w.duration_secs();
        println!(
            "sfx {id:3}: {:5} Hz {}ch {:2}-bit {:6.2}s",
            w.sample_rate,
            w.channels,
            w.bits_per_sample,
            w.duration_secs()
        );
    }
    println!("{} sounds, {total:.1}s total", sounds.len());
    let music = Bank::music(arcs)?;
    for id in 1..=music.len() {
        let x = Xmidi::parse(&music.raw(arcs, id)?).with_context(|| format!("music {id}"))?;
        println!("music {id:2}: {:6.1}s, {} timbres", x.duration_secs(), x.timbres.len());
    }
    Ok(())
}

fn dump(arcs: &Archives, out: &PathBuf) -> Result<()> {
    fs::create_dir_all(out)?;
    let sounds = Bank::sounds(arcs)?;
    for id in 1..=sounds.len() {
        // Members are already complete RIFF WAVE files.
        fs::write(out.join(format!("sfx_{id:03}.wav")), sounds.raw(arcs, id)?)?;
    }
    let music = Bank::music(arcs)?;
    for id in 1..=music.len() {
        let raw = music.raw(arcs, id)?;
        let smf = Xmidi::parse(&raw)?.to_smf().with_context(|| format!("converting music {id}"))?;
        fs::write(out.join(format!("music_{id:02}.xmi")), &raw)?;
        fs::write(out.join(format!("music_{id:02}.mid")), smf)?;
    }
    println!("wrote {} sounds and {} songs to {}", sounds.len(), music.len(), out.display());
    Ok(())
}

fn cues(arcs: &Archives) -> Result<()> {
    println!("music id -> CD track with CDDA=ON (MIDI otherwise):");
    for id in 1..=MAX_MUSIC_ID {
        let show = |d| cd_track(d, id).map(|t| format!("track {t:2}")).unwrap_or_else(|| "MIDI    ".into());
        println!("  {id:2}: disc1 {}  disc2 {}", show(Disc::One), show(Disc::Two));
    }
    println!("fixed scene music:");
    for (id, what, site) in SCENE_MUSIC {
        println!("  {id:2}: {what} ({site})");
    }
    // Stage records: BIN 1, i16 n then 30-byte records; byte 10 is the music id.
    let stages = arcs.read(Kind::Bin, 1)?;
    let n = i16::from_le_bytes([stages[0], stages[1]]).max(0) as usize;
    let ids: Vec<String> = (0..n).map(|i| stages[2 + 30 * i + 10].to_string()).collect();
    println!("stage music ({n} stages, in record order):");
    for (row, chunk) in ids.chunks(20).enumerate() {
        println!("  {:3}..: {}", row * 20 + 1, chunk.join(" "));
    }
    println!("movie cues (frame, music, sound):");
    for (i, m) in parse_movies(&arcs.read(Kind::Bin, MOVIE_CUE_ENTRY)?)?.iter().enumerate() {
        let cues: Vec<String> = m.cues.iter().map(|c| format!("({},{},{})", c.frame, c.music, c.sound)).collect();
        println!("  {} {:12} {}", i + 1, m.name, cues.join(" "));
    }
    let fx = parse_effect_sounds(&arcs.read(Kind::Bin, EFFECT_SOUND_ENTRY)?)?;
    println!("effect animation -> sound id ({} effects):", fx.len());
    for (row, chunk) in fx.chunks(20).enumerate() {
        let s: Vec<String> = chunk.iter().map(|v| v.to_string()).collect();
        println!("  {:3}..: {}", row * 20 + 1, s.join(" "));
    }
    Ok(())
}
