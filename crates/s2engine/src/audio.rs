//! Sound effects and music, following the original's Miles-based player
//! (`FUN_00419b1d` play_sound, `FUN_004198d4` play_music; see
//! docs/formats-audio.md).
//!
//! Sound effects go through [`Mixer`], a 16-voice software mixer the
//! frontend pulls samples from. Music is a MIDI event stream handed to a
//! [`MusicOut`] backend (the OS MIDI synth on Windows, a SoundFont synth or
//! nothing elsewhere).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use s2data::audio::{Bank, Wave, Xmidi, MAX_MUSIC_ID, MAX_SFX_ID, SFX_VOICES};
use s2data::Archives;

/// One decoded sample, stored as interleaved stereo f32 at its own rate.
#[derive(Debug)]
pub struct Sample {
    pub rate: u32,
    pub stereo: Vec<f32>,
}

impl Sample {
    fn from_wave(w: &Wave) -> Sample {
        let s = w.samples_f32();
        let stereo = if w.channels == 1 { s.iter().flat_map(|&v| [v, v]).collect() } else { s };
        Sample { rate: w.sample_rate, stereo }
    }

    fn frames(&self) -> usize {
        self.stereo.len() / 2
    }
}

#[derive(Default)]
struct Voice {
    sample: Option<Arc<Sample>>,
    id: u16,
    /// Position in source frames.
    pos: f64,
    playing: bool,
    gain: f32,
}

/// The software mixer for sound effects. The frontend calls [`Mixer::render`]
/// from its audio callback; the game thread starts and stops voices.
pub struct Mixer {
    voices: Vec<Voice>,
    pub out_rate: u32,
}

impl Mixer {
    pub fn new(out_rate: u32) -> Mixer {
        Mixer { voices: (0..SFX_VOICES).map(|_| Voice::default()).collect(), out_rate }
    }

    /// Mix all playing voices into `out` (interleaved stereo), adding to it.
    pub fn render(&mut self, out: &mut [f32]) {
        let out_rate = self.out_rate.max(1) as f64;
        for v in self.voices.iter_mut().filter(|v| v.playing) {
            let Some(s) = v.sample.clone() else { continue };
            let step = s.rate as f64 / out_rate;
            let n = s.frames();
            for frame in out.chunks_exact_mut(2) {
                let i = v.pos as usize;
                if i + 1 >= n {
                    if i < n {
                        frame[0] += s.stereo[2 * i] * v.gain;
                        frame[1] += s.stereo[2 * i + 1] * v.gain;
                    }
                    v.playing = false;
                    break;
                }
                let t = (v.pos - i as f64) as f32;
                let l = s.stereo[2 * i] * (1.0 - t) + s.stereo[2 * i + 2] * t;
                let r = s.stereo[2 * i + 1] * (1.0 - t) + s.stereo[2 * i + 3] * t;
                frame[0] += l * v.gain;
                frame[1] += r * v.gain;
                v.pos += step;
            }
        }
    }
}

/// A timed MIDI message (`time` in milliseconds from song start).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiEvent {
    pub time: u32,
    pub msg: [u8; 3],
    pub len: u8,
}

/// A song ready for a MIDI backend.
#[derive(Debug, Clone)]
pub struct Song {
    pub id: u16,
    pub events: Arc<Vec<MidiEvent>>,
    /// Length of one loop in milliseconds.
    pub length: u32,
}

/// Where music goes. Implementations must not block the game thread.
pub trait MusicOut: Send {
    fn play(&mut self, song: &Song, looping: bool);
    fn stop(&mut self);
    /// 0..=100, as in `VOLUME=`.
    fn set_volume(&mut self, volume: u8);
}

/// Discards music; used headless and when no MIDI output is available.
#[derive(Default)]
pub struct NoMusic {
    pub log: Vec<String>,
}

impl MusicOut for NoMusic {
    fn play(&mut self, song: &Song, looping: bool) {
        self.log.push(format!("play {} loop={looping}", song.id));
    }
    fn stop(&mut self) {
        self.log.push("stop".into());
    }
    fn set_volume(&mut self, _volume: u8) {}
}

/// Convert an XMIDI song to timed MIDI events via its SMF rendering.
pub fn song_events(x: &Xmidi) -> Result<(Vec<MidiEvent>, u32)> {
    let smf = x.to_smf()?;
    parse_smf_type0(&smf)
}

/// Parse a type-0 SMF with a single tempo into millisecond-timed events.
fn parse_smf_type0(b: &[u8]) -> Result<(Vec<MidiEvent>, u32)> {
    use anyhow::{bail, ensure};
    ensure!(b.len() >= 14 && &b[0..4] == b"MThd", "not an SMF");
    let division = u16::from_be_bytes([b[12], b[13]]) as u64;
    ensure!(division & 0x8000 == 0 && division > 0, "SMPTE time division unsupported");
    let mut p = 8 + u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
    ensure!(b.get(p..p + 4) == Some(b"MTrk"), "missing MTrk");
    let len = u32::from_be_bytes([b[p + 4], b[p + 5], b[p + 6], b[p + 7]]) as usize;
    p += 8;
    let end = (p + len).min(b.len());
    let mut tempo = 500_000u64; // µs per quarter note
    let (mut tick, mut us) = (0u64, 0u64);
    let mut running = 0u8;
    let mut out = Vec::new();
    let vlq = |p: &mut usize| -> u64 {
        let mut v = 0u64;
        while *p < end {
            let c = b[*p];
            *p += 1;
            v = (v << 7) | (c & 0x7f) as u64;
            if c & 0x80 == 0 {
                break;
            }
        }
        v
    };
    while p < end {
        let delta = vlq(&mut p);
        tick += delta;
        us += delta * tempo / division;
        let mut status = b[p];
        if status < 0x80 {
            status = running;
        } else {
            p += 1;
        }
        match status {
            0xff => {
                let kind = b[p];
                p += 1;
                let l = vlq(&mut p) as usize;
                if kind == 0x51 && l == 3 {
                    tempo = ((b[p] as u64) << 16) | ((b[p + 1] as u64) << 8) | b[p + 2] as u64;
                }
                p += l;
                if kind == 0x2f {
                    break;
                }
            }
            0xf0 | 0xf7 => {
                let l = vlq(&mut p) as usize;
                p += l;
            }
            0x80..=0xef => {
                running = status;
                let n = if matches!(status & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
                let mut msg = [status, 0, 0];
                msg[1..1 + n].copy_from_slice(&b[p..p + n]);
                p += n;
                out.push(MidiEvent { time: (us / 1000) as u32, msg, len: 1 + n as u8 });
            }
            s => bail!("bad SMF status {s:#x} at {p}"),
        }
    }
    let _ = tick;
    Ok((out, (us / 1000) as u32))
}

/// Game-facing audio state: the play_sound / play_music logic of the original.
pub struct Audio {
    pub mixer: Arc<Mutex<Mixer>>,
    music_out: Box<dyn MusicOut>,
    sounds: Option<Bank>,
    songs_bank: Option<Bank>,
    cache: HashMap<u16, Arc<Sample>>,
    songs: HashMap<u16, Song>,
    /// Last (sound id, voice) started, for the "same sound again" fast path.
    last: Option<(u16, usize)>,
    /// `VOLUME=midi,cd,sfx`.
    pub midi_volume: u8,
    pub sfx_volume: u8,
    /// Current music id (`_DAT_00494653`) and whether it is playing.
    pub current_music: u16,
    music_active: bool,
}

impl Audio {
    pub fn new(mixer: Arc<Mutex<Mixer>>, music_out: Box<dyn MusicOut>) -> Audio {
        Audio {
            mixer,
            music_out,
            sounds: None,
            songs_bank: None,
            cache: HashMap::new(),
            songs: HashMap::new(),
            last: None,
            midi_volume: 100,
            sfx_volume: 100,
            current_music: 0,
            music_active: false,
        }
    }

    /// Silent audio for tests and headless runs.
    pub fn silent() -> Audio {
        Audio::new(Arc::new(Mutex::new(Mixer::new(44_100))), Box::<NoMusic>::default())
    }

    pub fn attach(&mut self, arcs: &Archives) -> Result<()> {
        self.sounds = Some(Bank::sounds(arcs)?);
        self.songs_bank = Some(Bank::music(arcs)?);
        Ok(())
    }

    fn sample(&mut self, arcs: &Archives, id: u16) -> Option<Arc<Sample>> {
        if let Some(s) = self.cache.get(&id) {
            return Some(s.clone());
        }
        let raw = self.sounds.as_ref()?.raw(arcs, id as usize).ok()?;
        let s = Arc::new(Sample::from_wave(&Wave::parse(&raw).ok()?));
        self.cache.insert(id, s.clone());
        Some(s)
    }

    /// `FUN_00419b1d`: start sound `id`, or keep re-triggering the voice
    /// returned by a previous call when `slot` is passed back. Returns the
    /// 1-based voice used, or 0 if the sound was dropped.
    pub fn play_sound(&mut self, arcs: &Archives, id: u16, slot: usize) -> usize {
        if self.sfx_volume == 0 || id == 0 || id > MAX_SFX_ID {
            return 0;
        }
        let gain = self.sfx_volume as f32 / 127.0;
        let mixer = self.mixer.clone();
        let mut m = mixer.lock().unwrap();
        if (1..=SFX_VOICES).contains(&slot) && m.voices[slot - 1].sample.is_some() {
            self.last = Some((id, slot - 1));
            let v = &mut m.voices[slot - 1];
            if !v.playing {
                v.pos = 0.0;
                v.playing = true;
            }
            return slot;
        }
        if let Some((last_id, vi)) = self.last {
            let v = &mut m.voices[vi];
            if last_id == id && v.sample.is_some() && v.id == id && !v.playing {
                v.pos = 0.0;
                v.playing = true;
                v.gain = gain;
                return vi + 1;
            }
        }
        let Some(vi) = m.voices.iter().position(|v| !v.playing) else { return 0 };
        drop(m);
        let Some(sample) = self.sample(arcs, id) else { return 0 };
        let mut m = mixer.lock().unwrap();
        m.voices[vi] = Voice { sample: Some(sample), id, pos: 0.0, playing: true, gain };
        self.last = Some((id, vi));
        vi + 1
    }

    /// `FUN_00419b08`: stop every sound effect voice.
    pub fn stop_sounds(&mut self) {
        for v in self.mixer.lock().unwrap().voices.iter_mut() {
            v.playing = false;
        }
    }

    pub fn sound_playing(&self, slot: usize) -> bool {
        (1..=SFX_VOICES).contains(&slot) && self.mixer.lock().unwrap().voices[slot - 1].playing
    }

    fn song(&mut self, arcs: &Archives, id: u16) -> Option<Song> {
        if let Some(s) = self.songs.get(&id) {
            return Some(s.clone());
        }
        let raw = self.songs_bank.as_ref()?.raw(arcs, id as usize).ok()?;
        let (events, length) = song_events(&Xmidi::parse(&raw).ok()?).ok()?;
        let song = Song { id, events: Arc::new(events), length };
        self.songs.insert(id, song.clone());
        Some(song)
    }

    /// `FUN_004198d4`: play music `id` (looping when `looping`). Re-requesting
    /// the current song is a no-op. CD audio is not available, so every id
    /// resolves to its MIDI song, as in a `CDDA=OFF` install.
    pub fn play_music(&mut self, arcs: &Archives, id: u16, looping: bool) {
        if id == 0 || id > MAX_MUSIC_ID {
            return;
        }
        if self.music_active && self.current_music == id {
            return;
        }
        if let Some(song) = self.song(arcs, id) {
            self.music_out.set_volume(self.midi_volume);
            self.music_out.play(&song, looping);
            self.music_active = true;
        }
        self.current_music = id;
    }

    /// `FUN_00419a7b`: stop all music.
    pub fn stop_music(&mut self) {
        self.music_out.stop();
        self.music_active = false;
    }

    pub fn set_midi_volume(&mut self, v: u8) {
        self.midi_volume = v.min(100);
        self.music_out.set_volume(self.midi_volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smf(track: &[u8]) -> Vec<u8> {
        let mut b = b"MThd\0\0\0\x06\0\0\0\x01\0\x3c".to_vec(); // 60 PPQN
        b.extend_from_slice(b"MTrk");
        b.extend_from_slice(&(track.len() as u32).to_be_bytes());
        b.extend_from_slice(track);
        b
    }

    #[test]
    fn smf_timing_and_running_status() {
        // 60 PPQN at 500000 µs/qn => 1 tick = 8.333 ms.
        let t = [0x00, 0x90, 60, 100, 0x3c, 60, 0, 0x00, 0xc0, 5, 0x00, 0xff, 0x2f, 0x00];
        let (ev, len) = parse_smf_type0(&smf(&t)).unwrap();
        assert_eq!(ev.len(), 3);
        assert_eq!(ev[0], MidiEvent { time: 0, msg: [0x90, 60, 100], len: 3 });
        assert_eq!(ev[1], MidiEvent { time: 500, msg: [0x90, 60, 0], len: 3 });
        assert_eq!(ev[2].len, 2);
        assert_eq!(len, 500);
    }

    #[test]
    fn mixer_resamples_and_stops() {
        let mut m = Mixer::new(22_050);
        let s = Arc::new(Sample { rate: 11_025, stereo: vec![1.0; 2 * 4] });
        m.voices[0] = Voice { sample: Some(s), id: 1, pos: 0.0, playing: true, gain: 0.5 };
        let mut out = vec![0.0; 2 * 16];
        m.render(&mut out);
        // 4 source frames at half rate last ~8 output frames.
        assert!(out[0] > 0.49 && out[0] < 0.51);
        assert_eq!(out[2 * 12], 0.0);
        assert!(!m.voices[0].playing);
    }
}
