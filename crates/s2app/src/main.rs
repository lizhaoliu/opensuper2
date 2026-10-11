//! OpenSuper2 desktop frontend: owns the window, scales the 640x480 indexed
//! frame to it and forwards keyboard/mouse input to the game thread.
//!
//! Frontend keys (not seen by the game): Alt+Enter or F11 toggles fullscreen,
//! F12 cycles the display filter, F9 toggles smooth motion.

// Release builds on Windows are GUI programs: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod applog;
mod audio_out;
mod frontcfg;
mod gpu;
mod toast;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use s2engine::audio::{Audio, Mixer, MusicOut, NoMusic};
use s2engine::sys::{spawn_game, vk, InputEvent, Shared, SCREEN_H, SCREEN_W};
use s2engine::Data;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use frontcfg::{FrontConfig, Renderer};
use toast::Toast;

#[derive(Parser)]
#[command(name = "opensuper2", about = "Play 超時空英雄傳說2 北方密使 from your own game files")]
struct Cli {
    /// Game directory (the one holding SUPERSP.PAL)
    #[arg(long, env = "S2_GAME_DIR", default_value = ".")]
    game: PathBuf,
    /// Window scale factor
    #[arg(long, default_value_t = 2)]
    scale: u32,
    /// Play MIDI music with this General MIDI SoundFont (.sf2) instead of the
    /// system synthesizer (required for music on non-Windows systems)
    #[arg(long, env = "S2_SOUNDFONT")]
    soundfont: Option<PathBuf>,
    /// Disable all sound
    #[arg(long)]
    no_audio: bool,
    /// Render a 640x480 PNG screenshot through every display filter into
    /// DIR (no window, no game data needed) and exit
    #[arg(long, value_names = ["PNG", "DIR"], num_args = 2)]
    filter_shots: Option<Vec<PathBuf>>,
    /// Output size for --filter-shots
    #[arg(long, default_value = "1920x1440")]
    shot_size: String,
}

/// Build the audio side: output stream, mixer and music backend. Failures
/// degrade to silence rather than stopping the game.
fn setup_audio(cli: &Cli) -> (Audio, Option<audio_out::Output>) {
    if cli.no_audio {
        return (Audio::silent(), None);
    }
    let rate = audio_out::output_rate().unwrap_or(44_100);
    let mixer = Arc::new(std::sync::Mutex::new(Mixer::new(rate)));
    let mut synth = None;
    let music: Box<dyn MusicOut> = match &cli.soundfont {
        Some(path) => match audio_out::SynthMusic::load(path, rate) {
            Ok(m) => {
                synth = Some(m.0.clone());
                Box::new(m)
            }
            Err(e) => {
                log!("soundfont {}: {e}; music disabled", path.display());
                Box::<NoMusic>::default()
            }
        },
        None => system_midi(),
    };
    let output = match audio_out::start(mixer.clone(), synth) {
        Ok(o) => Some(o),
        Err(e) => {
            log!("audio output unavailable: {e}");
            None
        }
    };
    (Audio::new(mixer, music), output)
}

#[cfg(windows)]
fn system_midi() -> Box<dyn MusicOut> {
    match audio_out::winmidi::WinMidi::open() {
        Some(m) => Box::new(m),
        None => {
            log!("no MIDI output device; music disabled");
            Box::<NoMusic>::default()
        }
    }
}

#[cfg(not(windows))]
fn system_midi() -> Box<dyn MusicOut> {
    log!("no system MIDI synth on this platform; pass --soundfont FILE.sf2 for music");
    Box::<NoMusic>::default()
}

enum UserEvent {
    Frame,
    GameExited,
}

/// How frames reach the window.
enum Presenter {
    Gpu(Box<gpu::Gpu>),
    Cpu(softbuffer::Surface<Arc<Window>, Arc<Window>>),
}

struct App {
    shared: Arc<Shared>,
    scale: u32,
    cfg: FrontConfig,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    /// Destination rectangle of the game image inside the window: x, y, w, h.
    view: (u32, u32, u32, u32),
    alt_down: bool,
    shift_down: bool,
    /// Frontend shortcut key-downs; their key-ups are dropped too.
    swallowed: Vec<KeyCode>,
    /// Scratch row for scaling.
    row: Vec<u32>,
    /// The frame as 0RGB words (with any notice drawn over it).
    rgb: Vec<u32>,
    /// The same as RGBA bytes for the GPU.
    rgba: Vec<u8>,
    toasts: toast::Painter,
    toast: Option<Toast>,
}

impl App {
    fn window_to_game(&self, p: PhysicalPosition<f64>) -> (i32, i32) {
        let (vx, vy, vw, vh) = self.view;
        if vw == 0 || vh == 0 {
            return (0, 0);
        }
        let x = (p.x - vx as f64) * SCREEN_W as f64 / vw as f64;
        let y = (p.y - vy as f64) * SCREEN_H as f64 / vh as f64;
        (x as i32, y as i32)
    }

    /// The current game frame, converted, with the notice over it.
    fn compose(&mut self) {
        {
            let frame = self.shared.frame.lock().unwrap();
            let lut: Vec<u32> =
                frame.palette.0.iter().map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect();
            self.rgb.resize(SCREEN_W * SCREEN_H, 0);
            for (d, &s) in self.rgb.iter_mut().zip(frame.pixels.iter()) {
                *d = lut[s as usize];
            }
        }
        if let Some(t) = &self.toast {
            t.draw(&mut self.rgb);
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let (ww, wh) = (size.width, size.height);
        self.view = frontcfg::view_rect(ww, wh, self.cfg.scaling);
        self.compose();
        match self.presenter.as_mut() {
            Some(Presenter::Gpu(g)) => {
                self.rgba.resize(SCREEN_W * SCREEN_H * 4, 0);
                for (d, &c) in self.rgba.chunks_exact_mut(4).zip(self.rgb.iter()) {
                    d.copy_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 0xff]);
                }
                match g.render(&self.rgba, (ww, wh), self.view, self.cfg.filter) {
                    Ok(()) | Err(gpu::FrameError::Skip) => {}
                    Err(gpu::FrameError::Fatal(e)) => {
                        log!("GPU presentation failed ({e}); using the CPU path");
                        self.presenter = cpu_presenter(&window);
                        window.request_redraw();
                    }
                }
            }
            Some(Presenter::Cpu(_)) => self.redraw_cpu(ww, wh),
            None => {}
        }
    }

    /// Nearest-neighbour scaling into a softbuffer surface.
    fn redraw_cpu(&mut self, ww: u32, wh: u32) {
        let Some(Presenter::Cpu(surface)) = self.presenter.as_mut() else { return };
        let (Some(w), Some(h)) = (NonZeroU32::new(ww), NonZeroU32::new(wh)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let (vx, vy, vw, vh) = self.view;
        let xmap: Vec<usize> = (0..vw).map(|x| (x as usize * SCREEN_W / vw as usize).min(SCREEN_W - 1)).collect();
        let Ok(mut buf) = surface.buffer_mut() else { return };
        buf.fill(0);
        self.row.resize(vw as usize, 0);
        let mut last_sy = usize::MAX;
        for y in 0..vh {
            // Each source row is converted once and then copied to every
            // output row it covers.
            let sy = (y as usize * SCREEN_H / vh as usize).min(SCREEN_H - 1);
            if sy != last_sy {
                let src = &self.rgb[sy * SCREEN_W..(sy + 1) * SCREEN_W];
                for (d, &sx) in self.row.iter_mut().zip(&xmap) {
                    *d = src[sx];
                }
                last_sy = sy;
            }
            let start = ((vy + y) * ww + vx) as usize;
            buf[start..start + vw as usize].copy_from_slice(&self.row);
        }
        let _ = buf.present();
    }

    fn notify(&mut self, text: &str) {
        self.toast = self.toasts.make(text);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn cycle_filter(&mut self) {
        if !matches!(self.presenter, Some(Presenter::Gpu(_))) {
            self.notify("畫面濾鏡需要 GPU");
            return;
        }
        self.cfg.filter = if self.shift_down {
            frontcfg::Filter::ALL[(self.cfg.filter.index() + frontcfg::Filter::ALL.len() - 1) % frontcfg::Filter::ALL.len()]
        } else {
            self.cfg.filter.next()
        };
        self.cfg.save();
        let label = self.cfg.filter.label();
        self.notify(label);
    }

    fn toggle_smooth(&mut self) {
        self.cfg.smooth = !self.cfg.smooth;
        self.shared.smooth.store(self.cfg.smooth, Ordering::Relaxed);
        self.cfg.save();
        self.notify(if self.cfg.smooth { "平滑移動：開" } else { "平滑移動：關（原版）" });
    }

    /// Tell the game the refresh rate of the window's display.
    fn update_refresh(&self) {
        let mhz = self.window.as_ref().and_then(|w| w.current_monitor()).and_then(|m| m.refresh_rate_millihertz()).unwrap_or(0);
        self.shared.refresh_mhz.store(mhz, Ordering::Relaxed);
    }
}

fn cpu_presenter(window: &Arc<Window>) -> Option<Presenter> {
    let ctx = softbuffer::Context::new(window.clone()).ok()?;
    softbuffer::Surface::new(&ctx, window.clone()).ok().map(Presenter::Cpu)
}

impl App {
    fn toggle_fullscreen(&mut self) {
        let Some(w) = &self.window else { return };
        let full = w.fullscreen().is_none();
        w.set_fullscreen(full.then_some(Fullscreen::Borderless(None)));
        self.cfg.fullscreen = full;
        self.cfg.save();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = Window::default_attributes().with_title("超時空英雄傳說2 北方密使 — OpenSuper2");
        attrs = match self.cfg.window {
            Some((w, h)) => attrs.with_inner_size(winit::dpi::PhysicalSize::new(w, h)),
            None => attrs.with_inner_size(LogicalSize::new(SCREEN_W as u32 * self.scale, SCREEN_H as u32 * self.scale)),
        };
        if self.cfg.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        // The game draws its own cursor into the frame.
        window.set_cursor_visible(false);
        self.presenter = match self.cfg.renderer {
            Renderer::Gpu => match gpu::Gpu::new(window.clone()) {
                Ok(g) => {
                    log!("GPU presentation on {}", g.adapter_info);
                    Some(Presenter::Gpu(Box::new(g)))
                }
                Err(e) => {
                    log!("GPU presentation unavailable ({e:#}); using the CPU path");
                    None
                }
            },
            Renderer::Cpu => None,
        };
        if self.presenter.is_none() {
            self.presenter = cpu_presenter(&window);
        }
        self.window = Some(window);
        self.update_refresh();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Take the notice down when its time is up.
        match self.toast.as_ref().map(|t| t.until) {
            Some(until) if Instant::now() >= until => {
                self.toast = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(until) => el.set_control_flow(ControlFlow::WaitUntil(until)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Frame => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                    // SetCursorPos from the game (the original warps the pointer onto menu items).
                    if let Some((x, y)) = self.shared.take_warp() {
                        let (vx, vy, vw, vh) = self.view;
                        if vw > 0 && vh > 0 {
                            let px = vx as f64 + (x as f64 + 0.5) * vw as f64 / SCREEN_W as f64;
                            let py = vy as f64 + (y as f64 + 0.5) * vh as f64 / SCREEN_H as f64;
                            let _ = w.set_cursor_position(PhysicalPosition::new(px, py));
                        }
                    }
                }
            }
            UserEvent::GameExited => el.exit(),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.shared.request_quit();
                el.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Moved(_) | WindowEvent::ScaleFactorChanged { .. } => self.update_refresh(),
            WindowEvent::Resized(size) => {
                self.update_refresh();
                if let Some(w) = &self.window {
                    if w.fullscreen().is_none() && size.width > 0 && size.height > 0 {
                        self.cfg.window = Some((size.width, size.height));
                    }
                    w.request_redraw();
                }
            }
            WindowEvent::Focused(false) => {
                // Keys held while focus leaves would otherwise stay down.
                self.alt_down = false;
                self.shift_down = false;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if matches!(code, KeyCode::AltLeft | KeyCode::AltRight) {
                        self.alt_down = event.state == ElementState::Pressed;
                    }
                    if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                        self.shift_down = event.state == ElementState::Pressed;
                    }
                    let fullscreen = code == KeyCode::F11
                        || (self.alt_down && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter));
                    let shortcut = fullscreen || matches!(code, KeyCode::F12 | KeyCode::F9);
                    if event.state == ElementState::Pressed && shortcut {
                        if !event.repeat {
                            match code {
                                KeyCode::F12 => self.cycle_filter(),
                                KeyCode::F9 => self.toggle_smooth(),
                                _ => self.toggle_fullscreen(),
                            }
                        }
                        if !self.swallowed.contains(&code) {
                            self.swallowed.push(code);
                        }
                        return;
                    }
                    if event.state == ElementState::Released {
                        if let Some(i) = self.swallowed.iter().position(|&c| c == code) {
                            self.swallowed.remove(i);
                            return;
                        }
                    }
                    if let Some(k) = vk_of(code) {
                        let ev = match event.state {
                            ElementState::Pressed if event.repeat => InputEvent::KeyRepeat(k),
                            ElementState::Pressed => InputEvent::KeyDown(k),
                            ElementState::Released => InputEvent::KeyUp(k),
                        };
                        self.shared.push_input(ev);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = self.window_to_game(position);
                self.shared.set_mouse(x, y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    _ => return,
                };
                self.shared.push_input(match state {
                    ElementState::Pressed => InputEvent::MouseDown(b),
                    ElementState::Released => InputEvent::MouseUp(b),
                });
            }
            _ => {}
        }
    }
}

fn vk_of(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Escape => vk::ESCAPE,
        Enter | NumpadEnter => vk::RETURN,
        Space => vk::SPACE,
        Backspace => vk::BACK,
        Tab => vk::TAB,
        ShiftLeft | ShiftRight => vk::SHIFT,
        ControlLeft | ControlRight => vk::CONTROL,
        AltLeft | AltRight => vk::MENU,
        ArrowLeft => vk::LEFT,
        ArrowUp => vk::UP,
        ArrowRight => vk::RIGHT,
        ArrowDown => vk::DOWN,
        PageUp => vk::PRIOR,
        PageDown => vk::NEXT,
        Home => vk::HOME,
        End => vk::END,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 => {
            vk::F1 + (code as u32 - F1 as u32) as u8
        }
        KeyA | KeyB | KeyC | KeyD | KeyE | KeyF | KeyG | KeyH | KeyI | KeyJ | KeyK | KeyL | KeyM | KeyN | KeyO
        | KeyP | KeyQ | KeyR | KeyS | KeyT | KeyU | KeyV | KeyW | KeyX | KeyY | KeyZ => {
            b'A' + (code as u32 - KeyA as u32) as u8
        }
        Digit0 | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6 | Digit7 | Digit8 | Digit9 => {
            b'0' + (code as u32 - Digit0 as u32) as u8
        }
        _ => return None,
    })
}

/// WSLg's Wayland socket often isn't where winit looks for it; its X server works.
fn prefer_x11_on_wsl() {
    let wsl = std::fs::read_to_string("/proc/version").map(|v| v.to_lowercase().contains("microsoft")).unwrap_or(false);
    if wsl && std::env::var_os("DISPLAY").is_some() && std::env::var_os("S2_KEEP_WAYLAND").is_none() {
        // Called first thing in main, before any other thread exists.
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

/// Windows sleeps in ~15.6 ms steps by default, which halves the 60 Hz frame
/// pacing (and so doubles every fade and menu transition). Ask for 1 ms.
#[cfg(windows)]
fn high_res_timer() {
    // SAFETY: plain Win32 call; the period is released when the process exits.
    unsafe { windows_sys::Win32::Media::timeBeginPeriod(1) };
}

#[cfg(not(windows))]
fn high_res_timer() {}

fn main() {
    if let Err(e) = run() {
        log!("error: {e:#}");
        applog::fatal_box(&format!("{e:#}"));
        std::process::exit(1);
    }
}

/// `--filter-shots PNG DIR`: the screenshot through each filter, offscreen.
fn filter_shots(input: &std::path::Path, dir: &std::path::Path, size: &str) -> Result<()> {
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .context("--shot-size is WxH")?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(input)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!((info.width, info.height) == (SCREEN_W as u32, SCREEN_H as u32), "the screenshot must be 640x480");
    let ch = info.color_type.samples();
    let rgba: Vec<u8> = buf[..info.buffer_size()]
        .chunks_exact(ch)
        .flat_map(|p| match ch {
            1 | 2 => [p[0], p[0], p[0], 255],
            _ => [p[0], p[1], p[2], 255],
        })
        .collect();
    std::fs::create_dir_all(dir)?;
    let view = frontcfg::view_rect(w, h, frontcfg::Scaling::Fit);
    for f in frontcfg::Filter::ALL {
        let (px, adapter) = gpu::render_offscreen(&rgba, w, h, view, f)?;
        let out = dir.join(format!("{}.png", f.name()));
        let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&out)?), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()?.write_image_data(&px)?;
        println!("{} ({adapter})", out.display());
    }
    Ok(())
}

fn run() -> Result<()> {
    prefer_x11_on_wsl();
    high_res_timer();
    let cli = Cli::parse();
    if let Some(args) = &cli.filter_shots {
        return filter_shots(&args[0], &args[1], &cli.shot_size);
    }
    applog::init(&cli.game);
    let data = Data::open(&cli.game).with_context(|| {
        format!("opening game data in {} (put opensuper2 next to SUPERSP.PAL, or pass --game DIR)", cli.game.display())
    })?;
    let cfg = FrontConfig::load(&cli.game);
    let (mut audio, _audio_output) = setup_audio(&cli);
    if let Err(e) = audio.attach(&data.arcs) {
        log!("audio data: {e}");
    }
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();
    let shared = Shared::new();
    {
        let p = proxy.clone();
        shared.set_frame_notifier(Box::new(move || {
            let _ = p.send_event(UserEvent::Frame);
        }));
    }
    let fade_pace_ms = cfg.fade_pace_ms();
    shared.smooth.store(cfg.smooth, Ordering::Relaxed);
    if cfg.smooth || cfg.filter != frontcfg::Filter::Nearest {
        log!("not the original: filter={} smooth={} (opensuper2.ini)", cfg.filter.name(), cfg.smooth as u8);
    }
    let toasts = toast::Painter::new(&data.arcs);
    let options = s2engine::game::Options { ai: cfg.ai, enemy_boost: cfg.enemy_boost };
    if options != s2engine::game::Options::default() {
        log!("not the original: enemy ai={} enemy_boost={}% (opensuper2.ini)", options.ai.name(), options.enemy_boost);
    }
    let game = spawn_game(shared.clone(), audio, move |sys| {
        sys.fade_pace_ms = fade_pace_ms;
        let r = s2engine::run_with_options(sys, &data, options);
        let _ = proxy.send_event(UserEvent::GameExited);
        r
    });
    let mut app = App {
        shared: shared.clone(),
        scale: cli.scale.max(1),
        cfg,
        window: None,
        presenter: None,
        view: (0, 0, 0, 0),
        alt_down: false,
        shift_down: false,
        swallowed: Vec::new(),
        row: Vec::new(),
        rgb: Vec::new(),
        rgba: Vec::new(),
        toasts,
        toast: None,
    };
    event_loop.run_app(&mut app)?;
    app.cfg.save();
    shared.request_quit();
    match game.join() {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}
