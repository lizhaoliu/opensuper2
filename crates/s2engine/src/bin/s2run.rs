//! Headless runner: plays the game on a virtual clock, feeds scripted input
//! and saves screenshots, so behaviour can be checked without a window.
//!
//! Script steps (separated by `;` or newlines):
//!   frames N        let N frames pass
//!   key K           press and release K (RETURN, ESCAPE, SPACE, LEFT, UP, A, 1, ...)
//!   hold K / up K   press / release K
//!   move X Y        move the mouse to game coordinates
//!   click X Y       left click at X Y (rclick for right)
//!   shot NAME       save the current frame as OUT/NAME.png
//!   quit            stop the game
//!
//! `--frame-log FILE` writes "serial ticks" for every frame, plus " P" for a
//! present (compare with the oracle's, tools/oracle/oracle.py --frame-log).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use s2engine::sys::{spawn_headless, vk, FlipInfo, InputEvent, Shared, SCREEN_H, SCREEN_W};
use s2engine::Data;

#[derive(Debug, Clone)]
enum Step {
    Frames(u64),
    Key(u8),
    Hold(u8),
    Up(u8),
    Move(i32, i32),
    Click(u8, i32, i32),
    Shot(String),
    Quit,
}

fn key_code(name: &str) -> Result<u8> {
    Ok(match name.to_ascii_uppercase().as_str() {
        "RETURN" | "ENTER" => vk::RETURN,
        "ESCAPE" | "ESC" => vk::ESCAPE,
        "SPACE" => vk::SPACE,
        "LEFT" => vk::LEFT,
        "RIGHT" => vk::RIGHT,
        "UP" => vk::UP,
        "DOWN" => vk::DOWN,
        "ALT" => vk::MENU,
        "SHIFT" => vk::SHIFT,
        "CTRL" => vk::CONTROL,
        "TAB" => vk::TAB,
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => s.as_bytes()[0],
        s => bail!("unknown key {s}"),
    })
}

fn parse(script: &str) -> Result<Vec<Step>> {
    let mut out = Vec::new();
    for raw in script.split([';', '\n']) {
        let w: Vec<&str> = raw.split_whitespace().collect();
        if w.is_empty() || w[0].starts_with('#') {
            continue;
        }
        let num = |i: usize| -> Result<i64> { w.get(i).context("missing argument")?.parse().context("bad number") };
        out.push(match w[0] {
            "frames" => Step::Frames(num(1)? as u64),
            "key" => Step::Key(key_code(w.get(1).context("missing key")?)?),
            "hold" => Step::Hold(key_code(w.get(1).context("missing key")?)?),
            "up" => Step::Up(key_code(w.get(1).context("missing key")?)?),
            "move" => Step::Move(num(1)? as i32, num(2)? as i32),
            "click" => Step::Click(0, num(1)? as i32, num(2)? as i32),
            "rclick" => Step::Click(1, num(1)? as i32, num(2)? as i32),
            "shot" => Step::Shot(w.get(1).context("missing name")?.to_string()),
            "quit" => Step::Quit,
            other => bail!("unknown step {other}"),
        });
    }
    Ok(out)
}

fn save_png(path: &PathBuf, info: &FlipInfo) -> Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, SCREEN_W as u32, SCREEN_H as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(info.palette.to_rgb_bytes());
    enc.write_header()?.write_image_data(info.pixels)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut game = std::env::var_os("S2_GAME_DIR").map(PathBuf::from);
    let mut out = PathBuf::from(".");
    let mut script = String::new();
    let mut frame_log: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from).context("--out needs a dir")?,
            "--script" => script = args.next().context("--script needs text")?,
            "--script-file" => script = std::fs::read_to_string(args.next().context("--script-file needs a path")?)?,
            "--frame-log" => frame_log = Some(args.next().map(PathBuf::from).context("--frame-log needs a path")?),
            other => bail!("unknown argument {other}"),
        }
    }
    let game = game.context("--game DIR (or S2_GAME_DIR) is required")?;
    let steps = parse(&script)?;
    std::fs::create_dir_all(&out)?;
    let data = Data::open(&game)?;
    let shared = Shared::new();
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let errs = errors.clone();
    let mut pc = 0usize;
    let mut wait_until = 0u64;
    let mut frame_log = match frame_log {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    let hook = Box::new(move |info: &FlipInfo, sh: &Shared| {
        if let Some(f) = frame_log.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{} {}{}", info.serial, info.ticks, if info.presented { " P" } else { "" });
        }
        while pc < steps.len() && info.serial >= wait_until {
            match &steps[pc] {
                Step::Frames(n) => wait_until = info.serial + n,
                Step::Key(k) => {
                    sh.push_input(InputEvent::KeyDown(*k));
                    sh.push_input(InputEvent::KeyUp(*k));
                }
                Step::Hold(k) => sh.push_input(InputEvent::KeyDown(*k)),
                Step::Up(k) => sh.push_input(InputEvent::KeyUp(*k)),
                Step::Move(x, y) => sh.set_mouse(*x, *y),
                Step::Click(b, x, y) => {
                    sh.set_mouse(*x, *y);
                    sh.push_input(InputEvent::MouseDown(*b));
                    sh.push_input(InputEvent::MouseUp(*b));
                }
                Step::Shot(name) => {
                    let p = out.join(format!("{name}.png"));
                    match save_png(&p, info) {
                        Ok(()) => println!("frame {} t={}ms -> {}", info.serial, info.ticks, p.display()),
                        Err(e) => errs.lock().unwrap().push(format!("{}: {e}", p.display())),
                    }
                }
                Step::Quit => sh.request_quit(),
            }
            pc += 1;
        }
        if pc >= steps.len() && info.serial >= wait_until {
            sh.request_quit();
        }
    });
    let handle = spawn_headless(shared, hook, move |sys| s2engine::run(sys, &data));
    let r = match handle.join() {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    };
    for e in errors.lock().unwrap().iter() {
        eprintln!("error: {e}");
    }
    r
}
