//! Platform audio: a cpal output stream that pulls the engine's sound-effect
//! mixer (plus an optional SoundFont synth for music), and on Windows a
//! sequencer thread that plays music through the system MIDI synthesizer,
//! as the original did via Miles' `AIL_midiOutOpen`.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use s2engine::audio::{Mixer, MusicOut, Song};

/// Shared state of a software synth driven from the audio callback.
pub struct SynthState {
    synth: rustysynth::Synthesizer,
    song: Option<(Song, bool)>,
    /// Playback position in milliseconds and index of the next event.
    pos_ms: f64,
    next: usize,
    volume: u8,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl SynthState {
    fn send(&mut self, msg: [u8; 3]) {
        let (status, d1, mut d2) = (msg[0], msg[1], msg[2]);
        if status & 0xf0 == 0xb0 && d1 == 7 {
            d2 = (d2 as u32 * self.volume as u32 / 100) as u8;
        }
        self.synth.process_midi_message((status & 0x0f) as i32, (status & 0xf0) as i32, d1 as i32, d2 as i32);
    }

    fn render(&mut self, out: &mut [f32], rate: u32) {
        let frames = out.len() / 2;
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
        }
        // Dispatch events in 64-frame slices for reasonable timing accuracy.
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(64);
            if let Some((song, looping)) = self.song.clone() {
                let end_ms = self.pos_ms + n as f64 * 1000.0 / rate as f64;
                while let Some(ev) = song.events.get(self.next) {
                    if ev.time as f64 > end_ms {
                        break;
                    }
                    self.send(ev.msg);
                    self.next += 1;
                }
                self.pos_ms = end_ms;
                if self.next >= song.events.len() && self.pos_ms >= song.length as f64 {
                    if looping {
                        self.pos_ms = 0.0;
                        self.next = 0;
                    } else {
                        self.song = None;
                    }
                }
            }
            let (l, r) = (&mut self.left[done..done + n], &mut self.right[done..done + n]);
            self.synth.render(l, r);
            done += n;
        }
        for (i, f) in out.chunks_exact_mut(2).enumerate() {
            f[0] += self.left[i];
            f[1] += self.right[i];
        }
    }
}

/// [`MusicOut`] backed by a SoundFont synth rendered in the audio callback.
pub struct SynthMusic(pub Arc<Mutex<SynthState>>);

impl SynthMusic {
    pub fn load(path: &std::path::Path, rate: u32) -> anyhow::Result<SynthMusic> {
        let mut f = std::io::BufReader::new(std::fs::File::open(path)?);
        let sf = Arc::new(rustysynth::SoundFont::new(&mut f).map_err(|e| anyhow::anyhow!("soundfont: {e:?}"))?);
        let settings = rustysynth::SynthesizerSettings::new(rate as i32);
        let synth = rustysynth::Synthesizer::new(&sf, &settings).map_err(|e| anyhow::anyhow!("synth: {e:?}"))?;
        Ok(SynthMusic(Arc::new(Mutex::new(SynthState {
            synth,
            song: None,
            pos_ms: 0.0,
            next: 0,
            volume: 100,
            left: Vec::new(),
            right: Vec::new(),
        }))))
    }
}

impl MusicOut for SynthMusic {
    fn play(&mut self, song: &Song, looping: bool) {
        let mut s = self.0.lock().unwrap();
        s.synth.reset();
        s.song = Some((song.clone(), looping));
        s.pos_ms = 0.0;
        s.next = 0;
    }
    fn stop(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.song = None;
        s.synth.note_off_all(false);
    }
    fn set_volume(&mut self, volume: u8) {
        self.0.lock().unwrap().volume = volume.min(100);
    }
}

/// Keeps the output stream alive.
pub struct Output {
    _stream: cpal::Stream,
    #[allow(dead_code)]
    pub rate: u32,
}

/// The device's output sample rate, so mixers and synths can be built for it.
pub fn output_rate() -> Option<u32> {
    let dev = cpal::default_host().default_output_device()?;
    dev.default_output_config().ok().map(|c| c.sample_rate().0)
}

/// Open the default output device and start pulling from `mixer` (and `synth`).
pub fn start(mixer: Arc<Mutex<Mixer>>, synth: Option<Arc<Mutex<SynthState>>>) -> anyhow::Result<Output> {
    let host = cpal::default_host();
    let dev = host.default_output_device().ok_or_else(|| anyhow::anyhow!("no audio output device"))?;
    let cfg = dev.default_output_config()?;
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    mixer.lock().unwrap().out_rate = rate;
    let mut stereo: Vec<f32> = Vec::new();
    let err = |e| crate::applog::line(&format!("audio stream error: {e}"));
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &cfg.config(),
            move |out: &mut [f32], _| {
                let frames = out.len() / channels;
                stereo.clear();
                stereo.resize(frames * 2, 0.0);
                mixer.lock().unwrap().render(&mut stereo);
                if let Some(s) = &synth {
                    s.lock().unwrap().render(&mut stereo, rate);
                }
                for (i, f) in out.chunks_exact_mut(channels).enumerate() {
                    for (c, v) in f.iter_mut().enumerate() {
                        *v = stereo[2 * i + c.min(1)].clamp(-1.0, 1.0);
                    }
                }
            },
            err,
            None,
        )?,
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &cfg.config(),
            move |out: &mut [i16], _| {
                let frames = out.len() / channels;
                stereo.clear();
                stereo.resize(frames * 2, 0.0);
                mixer.lock().unwrap().render(&mut stereo);
                if let Some(s) = &synth {
                    s.lock().unwrap().render(&mut stereo, rate);
                }
                for (i, f) in out.chunks_exact_mut(channels).enumerate() {
                    for (c, v) in f.iter_mut().enumerate() {
                        *v = (stereo[2 * i + c.min(1)].clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                }
            },
            err,
            None,
        )?,
        other => anyhow::bail!("unsupported output sample format {other:?}"),
    };
    stream.play()?;
    Ok(Output { _stream: stream, rate })
}

#[cfg(windows)]
pub mod winmidi {
    //! Music through the Windows MIDI mapper (Microsoft GS Wavetable Synth by
    //! default), driven by a dedicated sequencer thread.

    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    use s2engine::audio::{MusicOut, Song};
    use windows_sys::Win32::Media::Audio::{midiOutClose, midiOutOpen, midiOutReset, midiOutShortMsg, HMIDIOUT};
    use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};

    enum Cmd {
        Play(Song, bool),
        Stop,
        Volume(u8),
    }

    pub struct WinMidi {
        tx: Sender<Cmd>,
    }

    struct Out(HMIDIOUT);
    // SAFETY: the handle is only used from the sequencer thread that owns it.
    unsafe impl Send for Out {}

    impl WinMidi {
        /// Open the MIDI mapper; `None` if the system has no MIDI output.
        pub fn open() -> Option<WinMidi> {
            let mut h: HMIDIOUT = std::ptr::null_mut();
            // SAFETY: plain Win32 call with a valid out-pointer; MIDI_MAPPER = -1.
            let r = unsafe { midiOutOpen(&mut h, u32::MAX, 0, 0, 0) };
            if r != 0 {
                return None;
            }
            let out = Out(h);
            let (tx, rx) = channel();
            std::thread::Builder::new().name("midi".into()).spawn(move || sequencer(out, rx)).ok()?;
            Some(WinMidi { tx })
        }
    }

    fn short(out: &Out, msg: [u8; 3], volume: u8) {
        let (s, d1, mut d2) = (msg[0], msg[1], msg[2]);
        if s & 0xf0 == 0xb0 && d1 == 7 {
            d2 = (d2 as u32 * volume as u32 / 100) as u8;
        }
        let packed = s as u32 | (d1 as u32) << 8 | (d2 as u32) << 16;
        // SAFETY: valid open handle owned by this thread.
        unsafe { midiOutShortMsg(out.0, packed) };
    }

    fn sequencer(out: Out, rx: Receiver<Cmd>) {
        // SAFETY: matched with timeEndPeriod below; improves sleep granularity.
        unsafe { timeBeginPeriod(1) };
        let mut song: Option<(Song, bool)> = None;
        let mut start = Instant::now();
        let mut next = 0usize;
        let mut volume = 100u8;
        loop {
            let wait = match &song {
                Some((s, _)) => match s.events.get(next) {
                    Some(ev) => Duration::from_millis(ev.time as u64).saturating_sub(start.elapsed()),
                    None => Duration::from_millis(s.length as u64).saturating_sub(start.elapsed()),
                },
                None => Duration::from_secs(3600),
            };
            match rx.recv_timeout(wait) {
                Ok(Cmd::Play(s, looping)) => {
                    // SAFETY: valid handle.
                    unsafe { midiOutReset(out.0) };
                    song = Some((s, looping));
                    start = Instant::now();
                    next = 0;
                }
                Ok(Cmd::Stop) => {
                    unsafe { midiOutReset(out.0) };
                    song = None;
                }
                Ok(Cmd::Volume(v)) => volume = v,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let Some((s, looping)) = song.clone() else { continue };
                    let now = start.elapsed().as_millis() as u32;
                    while let Some(ev) = s.events.get(next) {
                        if ev.time > now {
                            break;
                        }
                        short(&out, ev.msg, volume);
                        next += 1;
                    }
                    if next >= s.events.len() && now >= s.length {
                        if looping {
                            start = Instant::now();
                            next = 0;
                        } else {
                            song = None;
                        }
                    }
                }
            }
        }
        unsafe {
            midiOutReset(out.0);
            midiOutClose(out.0);
            timeEndPeriod(1);
        }
    }

    impl MusicOut for WinMidi {
        fn play(&mut self, song: &Song, looping: bool) {
            let _ = self.tx.send(Cmd::Play(song.clone(), looping));
        }
        fn stop(&mut self) {
            let _ = self.tx.send(Cmd::Stop);
        }
        fn set_volume(&mut self, volume: u8) {
            let _ = self.tx.send(Cmd::Volume(volume));
        }
    }
}
