//! The game thread's view of the machine: a 640x480 8-bit frame, a palette,
//! a clock and the original program's input event queue.
//!
//! The original program is written as nested blocking loops (every screen
//! polls input, draws, presents and waits on its own). We keep that shape:
//! game code runs on its own thread and calls [`Sys::flip`] like the original
//! called its DirectDraw blit, and [`Sys::input_get`] like it called its
//! message pump, while a frontend (window or headless harness) consumes
//! frames and feeds input through [`Shared`].
//!
//! Input follows `FUN_004751a0` (docs/ui.md §4): every queue call first turns
//! at most one OS message into a 36-byte event record, mouse moves are merged,
//! at most 100 events wait in the queue.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use s2data::sprite::Image;
use s2data::Palette;

pub const SCREEN_W: usize = 640;
pub const SCREEN_H: usize = 480;

/// Windows virtual-key codes; frontends report keys with these.
pub mod vk {
    pub const BACK: u8 = 0x08;
    pub const TAB: u8 = 0x09;
    pub const RETURN: u8 = 0x0d;
    pub const SHIFT: u8 = 0x10;
    pub const CONTROL: u8 = 0x11;
    pub const MENU: u8 = 0x12; // Alt
    pub const ESCAPE: u8 = 0x1b;
    pub const SPACE: u8 = 0x20;
    pub const PRIOR: u8 = 0x21;
    pub const NEXT: u8 = 0x22;
    pub const END: u8 = 0x23;
    pub const HOME: u8 = 0x24;
    pub const LEFT: u8 = 0x25;
    pub const UP: u8 = 0x26;
    pub const RIGHT: u8 = 0x27;
    pub const DOWN: u8 = 0x28;
    pub const F1: u8 = 0x70;
}

/// PC set-1 scan codes as the screens compare them (`(key >> 8) & 0xff`).
pub mod scan {
    pub const ESC: u8 = 0x01;
    pub const TAB: u8 = 0x0f;
    pub const ENTER: u8 = 0x1c;
    pub const ALT: u8 = 0x38;
    pub const SPACE: u8 = 0x39;
    pub const HOME: u8 = 0x47;
    pub const UP: u8 = 0x48;
    pub const PGUP: u8 = 0x49;
    pub const LEFT: u8 = 0x4b;
    pub const RIGHT: u8 = 0x4d;
    pub const END: u8 = 0x4f;
    pub const DOWN: u8 = 0x50;
    pub const PGDN: u8 = 0x51;
}

/// Event types (`DAT_004952dc`).
pub mod ev {
    pub const KEY_DOWN: u32 = 1;
    pub const KEY_REPEAT: u32 = 2;
    pub const KEY_UP: u32 = 4;
    pub const MOUSE_DOWN: u32 = 8;
    pub const MOUSE_UP: u32 = 0x10;
    pub const MOUSE_MOVE: u32 = 0x20;
    pub const TIMER: u32 = 0x40;
    /// Modifier bits (`DAT_004952f0`).
    pub const M_LBUTTON: u32 = 1;
    pub const M_RBUTTON: u32 = 2;
    pub const M_SHIFT: u32 = 0x18;
    pub const M_CTRL: u32 = 0x20;
    pub const M_ALT: u32 = 0x40;
}

/// What frontends report. Keys are virtual-key codes, buttons 0 = left, 1 = right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown(u8),
    /// Auto-repeat while held.
    KeyRepeat(u8),
    KeyUp(u8),
    MouseDown(u8),
    MouseUp(u8),
}

/// One OS message waiting to be pumped (the WM_* messages the original saw).
#[derive(Debug, Clone, Copy)]
enum Msg {
    Key { vk: u8, down: bool, repeat: bool },
    Button { button: u8, down: bool, x: i32, y: i32 },
    Move { x: i32, y: i32 },
    /// WM_CHAR posted by TranslateMessage after a character key's WM_KEYDOWN.
    Char,
}

/// The original 36-byte event record (`DAT_004952d8`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Event {
    /// +0x04 type, see [`ev`].
    pub kind: u32,
    /// +0x08 message time (ms).
    pub time: u32,
    /// +0x0c / +0x10 mouse position.
    pub x: i32,
    pub y: i32,
    /// +0x14 low byte character, bits 8..15 scan code (keys); 1 / 4 = left / right (mouse).
    pub key: u32,
    /// +0x18 modifiers, see [`ev`].
    pub mods: u32,
}

impl Event {
    pub fn scan(&self) -> u8 {
        (self.key >> 8) as u8
    }

    pub fn ch(&self) -> u8 {
        self.key as u8
    }
}

#[derive(Debug)]
struct InputState {
    keys: [bool; 256],
    mouse: (i32, i32),
    msgs: VecDeque<Msg>,
    warp: Option<(i32, i32)>,
}

/// A presented frame: indexed pixels plus the palette in effect.
#[derive(Clone)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub palette: Palette,
    pub serial: u64,
}

/// State shared between the game thread and the frontend.
pub struct Shared {
    pub frame: Mutex<Frame>,
    input: Mutex<InputState>,
    pub quit: AtomicBool,
    input_cv: Condvar,
    notify: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            frame: Mutex::new(Frame { pixels: vec![0; SCREEN_W * SCREEN_H], palette: Palette([[0; 3]; 256]), serial: 0 }),
            input: Mutex::new(InputState { keys: [false; 256], mouse: (320, 240), msgs: VecDeque::new(), warp: None }),
            quit: AtomicBool::new(false),
            input_cv: Condvar::new(),
            notify: Mutex::new(None),
        })
    }

    /// Called by the frontend whenever a new frame is ready (e.g. request a redraw).
    pub fn set_frame_notifier(&self, f: Box<dyn Fn() + Send>) {
        *self.notify.lock().unwrap() = Some(f);
    }

    pub fn push_input(&self, e: InputEvent) {
        let mut st = self.input.lock().unwrap();
        let (x, y) = st.mouse;
        let msg = match e {
            InputEvent::KeyDown(k) => {
                // A second key-down without a key-up is an auto-repeat.
                let repeat = std::mem::replace(&mut st.keys[k as usize], true);
                Msg::Key { vk: k, down: true, repeat }
            }
            InputEvent::KeyRepeat(k) => Msg::Key { vk: k, down: true, repeat: true },
            InputEvent::KeyUp(k) => {
                st.keys[k as usize] = false;
                Msg::Key { vk: k, down: false, repeat: false }
            }
            InputEvent::MouseDown(b) => Msg::Button { button: b & 1, down: true, x, y },
            InputEvent::MouseUp(b) => Msg::Button { button: b & 1, down: false, x, y },
        };
        st.msgs.push_back(msg);
        drop(st);
        self.input_cv.notify_all();
    }

    /// Report the pointer position in game coordinates; generates a move message.
    ///
    /// Like Windows' WM_MOUSEMOVE, a move never queues up behind another
    /// move: the pending one is updated in place. Without this, high-rate
    /// mice flood the queue faster than the game's one-message-per-poll pump
    /// drains it, and clicks arrive seconds late.
    pub fn set_mouse(&self, x: i32, y: i32) {
        let p = (x.clamp(0, SCREEN_W as i32 - 1), y.clamp(0, SCREEN_H as i32 - 1));
        let mut st = self.input.lock().unwrap();
        if st.mouse != p {
            st.mouse = p;
            match st.msgs.back_mut() {
                Some(Msg::Move { x, y }) => (*x, *y) = p,
                _ => st.msgs.push_back(Msg::Move { x: p.0, y: p.1 }),
            }
        }
    }

    pub fn mouse(&self) -> (i32, i32) {
        self.input.lock().unwrap().mouse
    }

    /// The game asked to move the OS pointer (`SetCursorPos`); frontends may honour it.
    pub fn take_warp(&self) -> Option<(i32, i32)> {
        self.input.lock().unwrap().warp.take()
    }

    pub fn request_quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
        self.input_cv.notify_all();
    }
}

/// Panic payload used to unwind the game thread when the user closes the window.
pub struct Quit;

/// Called on the game thread after every frame in headless runs. It sees the
/// frame on screen and may inject input before the game continues.
pub type FlipHook = Box<dyn FnMut(&FlipInfo, &Shared) + Send>;

pub struct FlipInfo<'a> {
    pub serial: u64,
    pub ticks: u32,
    pub pixels: &'a [u8],
    pub palette: &'a Palette,
    /// A present (`flip`) rather than a 60 Hz boundary passing while the game waits.
    pub presented: bool,
}

/// One display frame at 60 Hz, in milliseconds.
const FRAME_MS: u32 = 16;
/// Queue length limit of the original pump.
const MAX_EVENTS: usize = 100;

enum Clock {
    Real { start: Instant, last_flip: Instant },
    /// Deterministic time; advances when the game presents, waits or polls an empty queue.
    Virtual { now: u32, last_frame: u32 },
}

pub struct Sys {
    /// The frame on screen (640x480, 8-bit indexed).
    pub fb: Image,
    pub palette: Palette,
    shared: Arc<Shared>,
    clock: Clock,
    serial: u64,
    hook: Option<FlipHook>,
    pub audio: crate::audio::Audio,
    /// Real-clock spacing of presents in ms (60 Hz by default).
    pub pace_ms: u32,
    /// Spacing used for the presents of palette fades (`gfx::fade`). The
    /// original's fades are untimed, so their speed was set by how fast the
    /// machine presented; frontends may make them quicker than 60 Hz pacing.
    /// The virtual clock of headless runs ignores both.
    pub fade_pace_ms: u32,
    queue: VecDeque<Event>,
    /// Index of the pending (mergeable) mouse-move event in `queue`.
    pending_move: Option<usize>,
    /// Code stored by each key's last key-down, reported again on key-up (`+0xe28`).
    key_codes: [u32; 256],
    keys: [bool; 256],
    buttons: [bool; 2],
}

impl Sys {
    pub fn new(shared: Arc<Shared>, audio: crate::audio::Audio) -> Sys {
        let now = Instant::now();
        Sys {
            fb: Image::new(SCREEN_W, SCREEN_H),
            palette: Palette([[0; 3]; 256]),
            shared,
            clock: Clock::Real { start: now, last_flip: now },
            serial: 0,
            hook: None,
            audio,
            pace_ms: FRAME_MS,
            fade_pace_ms: FRAME_MS,
            queue: VecDeque::new(),
            pending_move: None,
            key_codes: [0; 256],
            keys: [false; 256],
            buttons: [false; 2],
        }
    }

    /// Headless mode: virtual clock plus a hook run once per 60 Hz frame.
    pub fn new_headless(shared: Arc<Shared>, hook: FlipHook) -> Sys {
        let mut sys = Sys::new(shared, crate::audio::Audio::silent());
        sys.clock = Clock::Virtual { now: 0, last_frame: 0 };
        sys.hook = Some(hook);
        sys
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Milliseconds since start.
    pub fn ticks(&self) -> u32 {
        match &self.clock {
            Clock::Real { start, .. } => start.elapsed().as_millis() as u32,
            Clock::Virtual { now, .. } => *now,
        }
    }

    /// The original clock in centiseconds (`FUN_0046d5d6`; the original
    /// derives it from the local time and wraps hourly, which is not kept).
    pub fn ticks_cs(&self) -> i32 {
        (self.ticks() / 10) as i32
    }

    fn notify_and_hook(&mut self, presented: bool) {
        if let Some(n) = self.shared.notify.lock().unwrap().as_ref() {
            n();
        }
        let ticks = self.ticks();
        if let Some(hook) = self.hook.as_mut() {
            let info = FlipInfo { serial: self.serial, ticks, pixels: &self.fb.pixels, palette: &self.palette, presented };
            hook(&info, &self.shared);
        }
    }

    /// Present `fb` with `palette` (the original's blit to the DirectDraw
    /// surface). Presents are paced to at most one per `pace_ms` (60 Hz by
    /// default), which is how the untimed palette fades get their duration.
    pub fn flip(&mut self) {
        self.check_quit();
        if let Clock::Real { last_flip, .. } = &self.clock {
            let next = *last_flip + Duration::from_millis(self.pace_ms as u64);
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
        }
        self.serial += 1;
        {
            let mut f = self.shared.frame.lock().unwrap();
            f.pixels.copy_from_slice(&self.fb.pixels);
            f.palette = self.palette.clone();
            f.serial = self.serial;
        }
        match &mut self.clock {
            Clock::Real { last_flip, .. } => *last_flip = Instant::now(),
            Clock::Virtual { now, last_frame } => {
                *now += FRAME_MS;
                *last_frame = *now;
            }
        }
        self.notify_and_hook(true);
        self.check_quit();
    }

    /// Let virtual time pass up to `target`, running the frame hook at every
    /// 60 Hz boundary so scripted input can arrive while the game waits.
    fn advance_virtual(&mut self, target: u32) {
        loop {
            let Clock::Virtual { now, last_frame } = &mut self.clock else { return };
            if (target.wrapping_sub(*now) as i32) <= 0 {
                return;
            }
            let frame_at = last_frame.wrapping_add(FRAME_MS);
            if (frame_at.wrapping_sub(target) as i32) > 0 {
                *now = target;
                return;
            }
            *now = frame_at;
            *last_frame = frame_at;
            self.serial += 1;
            self.notify_and_hook(false);
            self.check_quit();
        }
    }

    /// Nothing to do this iteration of a polling loop: yield a little.
    pub fn idle(&mut self) {
        self.check_quit();
        match self.clock {
            Clock::Real { .. } => std::thread::sleep(Duration::from_millis(1)),
            Clock::Virtual { now, .. } => self.advance_virtual(now + 1),
        }
    }

    /// Sleep until `ticks() >= since + delay_ms`.
    pub fn wait_since(&mut self, since: u32, delay_ms: u32) {
        let target = since.wrapping_add(delay_ms);
        if let Clock::Virtual { .. } = self.clock {
            self.advance_virtual(target);
            self.check_quit();
            return;
        }
        loop {
            self.check_quit();
            let now = self.ticks();
            if (now.wrapping_sub(target) as i32) >= 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(((target.wrapping_sub(now)) as u64).min(10)));
        }
    }

    pub fn sleep(&mut self, ms: u32) {
        let t = self.ticks();
        self.wait_since(t, ms);
    }

    /// Pointer position in game coordinates (`GetCursorPos` + `ScreenToClient`).
    pub fn mouse(&self) -> (i32, i32) {
        self.shared.mouse()
    }

    /// `SetCursorPos`: move the pointer (and ask the frontend to move the OS one).
    pub fn warp_mouse(&mut self, x: i32, y: i32) {
        let p = (x.clamp(0, SCREEN_W as i32 - 1), y.clamp(0, SCREEN_H as i32 - 1));
        // Windows posts a WM_MOUSEMOVE when SetCursorPos moves the pointer
        // (the oracle does the same): the game's next poll sees the move.
        self.shared.set_mouse(p.0, p.1);
        self.shared.input.lock().unwrap().warp = Some(p);
    }

    fn check_quit(&self) {
        if self.shared.quit.load(Ordering::SeqCst) {
            std::panic::panic_any(Quit);
        }
    }

    fn push_event(&mut self, e: Event) {
        if self.queue.len() < MAX_EVENTS {
            self.queue.push_back(e);
        }
    }

    fn mouse_mods(&self) -> u32 {
        let mut m = 0;
        if self.buttons[0] {
            m |= ev::M_LBUTTON;
        }
        if self.buttons[1] {
            m |= ev::M_RBUTTON;
        }
        if self.keys[vk::SHIFT as usize] {
            m |= ev::M_SHIFT;
        }
        if self.keys[vk::CONTROL as usize] {
            m |= ev::M_CTRL;
        }
        m
    }

    /// `FUN_004751a0`: turn at most one OS message into an event. Returns
    /// whether a message was taken.
    fn pump_one(&mut self) -> bool {
        self.check_quit();
        let Some(msg) = self.shared.input.lock().unwrap().msgs.pop_front() else { return false };
        let time = self.ticks();
        match msg {
            Msg::Move { x, y } => {
                let mods = self.mouse_mods();
                // Consecutive moves update the pending move in place.
                match self.pending_move {
                    Some(i) if i < self.queue.len() && self.queue[i].kind == ev::MOUSE_MOVE => {
                        self.queue[i].x = x;
                        self.queue[i].y = y;
                    }
                    _ => {
                        if self.queue.len() < MAX_EVENTS {
                            self.pending_move = Some(self.queue.len());
                        }
                        self.push_event(Event { kind: ev::MOUSE_MOVE, time, x, y, key: 0, mods });
                    }
                }
            }
            // No event (FUN_004751a0 only peeks WM_CHAR for the key code).
            Msg::Char => {}
            Msg::Button { button, down, x, y } => {
                self.buttons[button as usize] = down;
                self.pending_move = None;
                let kind = if down { ev::MOUSE_DOWN } else { ev::MOUSE_UP };
                let key = if button == 0 { 1 } else { 4 };
                let mods = self.mouse_mods();
                self.push_event(Event { kind, time, x, y, key, mods });
            }
            Msg::Key { vk: k, down, repeat } => {
                self.pending_move = None;
                let sc = scan_code(k);
                let kind;
                let key;
                if down {
                    self.keys[k as usize] = true;
                    // TranslateMessage posts WM_CHAR, retrieved before later input.
                    if char_code(k, self.keys[vk::SHIFT as usize]) != 0 {
                        self.shared.input.lock().unwrap().msgs.push_front(Msg::Char);
                    }
                    let code = char_code(k, self.keys[vk::SHIFT as usize]) as u32 | (sc as u32) << 8;
                    self.key_codes[sc as usize] = code;
                    kind = if repeat { ev::KEY_REPEAT } else { ev::KEY_DOWN };
                    key = if repeat { code | 1 << 16 } else { code };
                } else {
                    self.keys[k as usize] = false;
                    kind = ev::KEY_UP;
                    key = std::mem::take(&mut self.key_codes[sc as usize]);
                }
                let mut mods = 0;
                if self.keys[vk::MENU as usize] {
                    mods |= ev::M_ALT;
                }
                if self.keys[vk::SHIFT as usize] {
                    mods |= ev::M_SHIFT;
                }
                if self.keys[vk::CONTROL as usize] {
                    mods |= ev::M_CTRL;
                }
                let (x, y) = self.mouse();
                self.push_event(Event { kind, time, x, y, key, mods });
            }
        }
        true
    }

    fn take_matching(&mut self, mask: u32, remove: bool) -> Option<Event> {
        let i = self.queue.iter().position(|e| e.kind & mask != 0)?;
        let e = self.queue[i];
        if remove {
            self.queue.remove(i);
            match self.pending_move {
                Some(p) if p == i => self.pending_move = None,
                Some(p) if p > i => self.pending_move = Some(p - 1),
                _ => {}
            }
        }
        Some(e)
    }

    /// `FUN_00474aac` / `FUN_00472f69`: pump one message, then remove and
    /// return the first queued event whose type matches `mask`. When nothing
    /// is waiting the thread yields briefly (the original spun).
    pub fn input_get(&mut self, mask: u32) -> Option<Event> {
        let pumped = self.pump_one();
        let e = self.take_matching(mask, true);
        if e.is_none() && !pumped {
            self.idle();
        }
        e
    }

    /// `FUN_00474bb9` / `FUN_00472f86`: like [`Sys::input_get`] without removing.
    pub fn input_peek(&mut self, mask: u32) -> Option<Event> {
        self.pump_one();
        self.take_matching(mask, false)
    }

    /// `FUN_00474c54` / `FUN_00472f9e`: drain events of `mask`.
    pub fn input_flush(&mut self, mask: u32) {
        // FUN_00474c54: `do FUN_00474aac(...) while (event)` — stops at the
        // first pump that yields no matching event (e.g. a WM_CHAR).
        loop {
            self.pump_one();
            if self.take_matching(mask, true).is_none() {
                break;
            }
        }
    }

    /// `FUN_00474c79` / `FUN_00472fb2`: take events (any type, discarding them)
    /// until one of `mask` arrives.
    pub fn input_wait(&mut self, mask: u32) -> Event {
        loop {
            if let Some(e) = self.input_get(0xffff) {
                if e.kind & mask != 0 {
                    return e;
                }
            }
        }
    }

    pub fn clear(&mut self, color: u8) {
        self.fb.pixels.fill(color);
    }
}

/// PC set-1 scan code of a virtual key.
pub fn scan_code(k: u8) -> u8 {
    const LETTERS: &[u8; 26] = &[
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1f,
        0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    match k {
        vk::ESCAPE => 0x01,
        b'1'..=b'9' => 0x02 + (k - b'1'),
        b'0' => 0x0b,
        vk::BACK => 0x0e,
        vk::TAB => 0x0f,
        b'A'..=b'Z' => LETTERS[(k - b'A') as usize],
        vk::RETURN => 0x1c,
        vk::CONTROL => 0x1d,
        vk::SHIFT => 0x2a,
        vk::MENU => 0x38,
        vk::SPACE => 0x39,
        0x70..=0x79 => 0x3b + (k - 0x70),
        vk::HOME => 0x47,
        vk::UP => 0x48,
        vk::PRIOR => 0x49,
        vk::LEFT => 0x4b,
        vk::RIGHT => 0x4d,
        vk::END => 0x4f,
        vk::DOWN => 0x50,
        vk::NEXT => 0x51,
        _ => 0,
    }
}

/// The WM_CHAR code `TranslateMessage` would produce (0 for non-character keys).
fn char_code(k: u8, shift: bool) -> u8 {
    match k {
        b'A'..=b'Z' if !shift => k + 0x20,
        b'A'..=b'Z' | b'0'..=b'9' => k,
        vk::RETURN | vk::ESCAPE | vk::SPACE | vk::BACK | vk::TAB => k,
        _ => 0,
    }
}

/// Run `game` on a new thread with a fresh [`Sys`]; returns the join handle.
/// Unwinding with [`Quit`] is treated as a normal exit.
pub fn spawn_game<F>(shared: Arc<Shared>, audio: crate::audio::Audio, game: F) -> std::thread::JoinHandle<anyhow::Result<()>>
where
    F: FnOnce(&mut Sys) -> anyhow::Result<()> + Send + 'static,
{
    spawn_with(shared, Some(audio), None, game)
}

/// Like [`spawn_game`] but headless, with a virtual clock and a frame hook.
pub fn spawn_headless<F>(shared: Arc<Shared>, hook: FlipHook, game: F) -> std::thread::JoinHandle<anyhow::Result<()>>
where
    F: FnOnce(&mut Sys) -> anyhow::Result<()> + Send + 'static,
{
    spawn_with(shared, None, Some(hook), game)
}

fn spawn_with<F>(
    shared: Arc<Shared>,
    audio: Option<crate::audio::Audio>,
    hook: Option<FlipHook>,
    game: F,
) -> std::thread::JoinHandle<anyhow::Result<()>>
where
    F: FnOnce(&mut Sys) -> anyhow::Result<()> + Send + 'static,
{
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<Quit>().is_none() {
            default_hook(info);
        }
    }));
    std::thread::Builder::new()
        .name("game".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let mut sys = match hook {
                Some(h) => Sys::new_headless(shared.clone(), h),
                None => Sys::new(shared.clone(), audio.unwrap_or_else(crate::audio::Audio::silent)),
            };
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| game(&mut sys)));
            shared.request_quit();
            match r {
                Ok(res) => res,
                Err(p) if p.downcast_ref::<Quit>().is_some() => Ok(()),
                Err(p) => std::panic::resume_unwind(p),
            }
        })
        .expect("spawn game thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> Sys {
        Sys::new_headless(Shared::new(), Box::new(|_, _| {}))
    }

    #[test]
    fn keys_become_original_events() {
        let mut s = sys();
        s.shared().push_input(InputEvent::KeyDown(vk::RETURN));
        s.shared().push_input(InputEvent::KeyUp(vk::RETURN));
        let d = s.input_get(0xffff).unwrap();
        assert_eq!((d.kind, d.scan(), d.ch()), (ev::KEY_DOWN, scan::ENTER, 0x0d));
        // The WM_CHAR that TranslateMessage posts after Enter is pumped next
        // and yields no event, as in FUN_004751a0.
        assert!(s.input_get(0xffff).is_none());
        let u = s.input_get(0xffff).unwrap();
        assert_eq!((u.kind, u.key), (ev::KEY_UP, d.key));
    }

    #[test]
    fn moves_merge_and_masks_skip() {
        let mut s = sys();
        s.shared().set_mouse(10, 10);
        s.shared().set_mouse(20, 30);
        s.shared().push_input(InputEvent::MouseDown(0));
        // Pump everything; only one move remains, and the click is reachable by mask.
        let click = loop {
            if let Some(e) = s.input_get(ev::MOUSE_DOWN) {
                break e;
            }
        };
        assert_eq!((click.x, click.y, click.key, click.mods & ev::M_LBUTTON), (20, 30, 1, 1));
        let mv = s.input_get(ev::MOUSE_MOVE).unwrap();
        assert_eq!((mv.x, mv.y), (20, 30));
        assert!(s.input_get(0xffff).is_none());
    }

    #[test]
    fn mouse_flood_does_not_delay_clicks() {
        let mut s = sys();
        // A 1000 Hz mouse moving for a second before the click.
        for i in 0..1000 {
            s.shared().set_mouse(i % 640, i % 480);
        }
        s.shared().push_input(InputEvent::MouseDown(0));
        let polls = (1..=10).find(|_| s.input_get(ev::MOUSE_DOWN).is_some());
        assert!(polls.is_some_and(|n| n <= 2), "click took {polls:?} polls");
    }
}
