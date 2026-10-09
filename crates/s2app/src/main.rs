//! OpenSuper2 desktop frontend: owns the window, scales the 640x480 indexed
//! frame to it and forwards keyboard/mouse input to the game thread.
//!
//! Frontend keys (not seen by the game): Alt+Enter or F11 toggles fullscreen.

// Release builds on Windows are GUI programs: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod applog;
mod audio_out;
mod frontcfg;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

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

use frontcfg::FrontConfig;

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

struct App {
    shared: Arc<Shared>,
    scale: u32,
    cfg: FrontConfig,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    /// Destination rectangle of the game image inside the window: x, y, w, h.
    view: (u32, u32, u32, u32),
    alt_down: bool,
    /// Enter/F11 key-downs consumed as frontend shortcuts; their key-ups are dropped too.
    swallowed: Vec<KeyCode>,
    /// Scratch row for scaling.
    row: Vec<u32>,
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

    fn redraw(&mut self) {
        let (Some(window), Some(surface)) = (self.window.as_ref(), self.surface.as_mut()) else { return };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let (ww, wh) = (size.width, size.height);
        let (vx, vy, vw, vh) = frontcfg::view_rect(ww, wh, self.cfg.scaling);
        self.view = (vx, vy, vw, vh);
        let frame = self.shared.frame.lock().unwrap().clone();
        let lut: Vec<u32> =
            frame.palette.0.iter().map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect();
        let xmap: Vec<usize> = (0..vw).map(|x| (x as usize * SCREEN_W / vw as usize).min(SCREEN_W - 1)).collect();
        let Ok(mut buf) = surface.buffer_mut() else { return };
        buf.fill(0);
        self.row.resize(vw as usize, 0);
        let mut last_sy = usize::MAX;
        for y in 0..vh {
            // Nearest-neighbour: each source row is converted once and then
            // copied to every output row it covers.
            let sy = (y as usize * SCREEN_H / vh as usize).min(SCREEN_H - 1);
            if sy != last_sy {
                let src = &frame.pixels[sy * SCREEN_W..(sy + 1) * SCREEN_W];
                for (d, &sx) in self.row.iter_mut().zip(&xmap) {
                    *d = lut[src[sx] as usize];
                }
                last_sy = sy;
            }
            let start = ((vy + y) * ww + vx) as usize;
            buf[start..start + vw as usize].copy_from_slice(&self.row);
        }
        let _ = buf.present();
    }
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
        let window = Rc::new(el.create_window(attrs).expect("create window"));
        // The game draws its own cursor into the frame.
        window.set_cursor_visible(false);
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("softbuffer surface"));
        self.window = Some(window);
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
            WindowEvent::Resized(size) => {
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
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if matches!(code, KeyCode::AltLeft | KeyCode::AltRight) {
                        self.alt_down = event.state == ElementState::Pressed;
                    }
                    let shortcut = code == KeyCode::F11
                        || (self.alt_down && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter));
                    if event.state == ElementState::Pressed && shortcut {
                        if !event.repeat {
                            self.toggle_fullscreen();
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

fn run() -> Result<()> {
    prefer_x11_on_wsl();
    high_res_timer();
    let cli = Cli::parse();
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
        surface: None,
        view: (0, 0, 0, 0),
        alt_down: false,
        swallowed: Vec::new(),
        row: Vec::new(),
    };
    event_loop.run_app(&mut app)?;
    app.cfg.save();
    shared.request_quit();
    match game.join() {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}
