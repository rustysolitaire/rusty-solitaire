// Rusty Solitaire - sound.rs
//
// The sound effects, which ar-sound.c played through libcanberra: "click"
// when cards are put down, "slide" when they go back, "victory" and "splat"
// when a game is won or lost. The original Ogg Vorbis files are decoded in
// Rust and mixed on a background thread.
//
// Linux talks to ALSA directly, loading libasound when the first sound
// plays, so the program needs no audio library to start (or to build for
// ARM64). On PipeWire and PulseAudio desktops ALSA's default device goes
// through them. Windows and macOS use cpal (WASAPI, Core Audio).
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

static SOUNDS: [(&str, &[u8]); 4] = [
    ("click", include_bytes!("../assets/sounds/click.ogg")),
    ("slide", include_bytes!("../assets/sounds/slide.ogg")),
    ("splat", include_bytes!("../assets/sounds/splat.ogg")),
    ("victory", include_bytes!("../assets/sounds/victory.ogg")),
];

/// Plays sounds without blocking the window. The audio thread starts with
/// the first sound.
pub struct Sound {
    tx: OnceCell<Option<Sender<&'static str>>>,
}

impl Sound {
    pub fn new() -> Self {
        Sound { tx: OnceCell::new() }
    }

    pub fn play(&self, name: &'static str) {
        let tx = self.tx.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new().name("sound".into()).spawn(move || backend::run(rx)).ok().map(|_| tx)
        });
        if let Some(tx) = tx {
            let _ = tx.send(name);
        }
    }
}

/// Decode a sound to mono samples in -1..1, resampled to `rate`.
fn decode(name: &str, rate: u32) -> Option<Arc<[f32]>> {
    let data = SOUNDS.iter().find(|(n, _)| *n == name)?.1;
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(data)).ok()?;
    let source_rate = reader.ident_hdr.audio_sample_rate;
    let channels = reader.ident_hdr.audio_channels.max(1) as usize;
    let mut mono = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().ok()? {
        for frame in packet.chunks(channels) {
            mono.push(frame.iter().map(|&s| s as f32).sum::<f32>() / (channels as f32 * 32768.0));
        }
    }
    if mono.is_empty() {
        return None;
    }
    // Linear interpolation is plenty for short effects.
    let step = source_rate as f64 / rate as f64;
    let len = (mono.len() as f64 / step) as usize;
    let samples: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f64 * step;
            let (i0, frac) = (t as usize, (t.fract()) as f32);
            let a = mono[i0.min(mono.len() - 1)];
            let b = mono[(i0 + 1).min(mono.len() - 1)];
            a + (b - a) * frac
        })
        .collect();
    Some(samples.into())
}

/// Sums the sounds that are playing.
struct Mixer {
    rate: u32,
    clips: HashMap<&'static str, Option<Arc<[f32]>>>,
    voices: Vec<(Arc<[f32]>, usize)>,
}

impl Mixer {
    fn new(rate: u32) -> Self {
        Mixer { rate, clips: HashMap::new(), voices: Vec::new() }
    }

    fn start(&mut self, name: &'static str) {
        let rate = self.rate;
        if let Some(clip) = self.clips.entry(name).or_insert_with(|| decode(name, rate)) {
            self.voices.push((clip.clone(), 0));
        }
    }

    fn is_idle(&self) -> bool {
        self.voices.is_empty()
    }

    /// The next `out.len()` mono samples.
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        for (clip, pos) in &mut self.voices {
            let n = out.len().min(clip.len() - *pos);
            for (o, s) in out[..n].iter_mut().zip(&clip[*pos..*pos + n]) {
                *o += s;
            }
            *pos += n;
        }
        self.voices.retain(|(clip, pos)| *pos < clip.len());
        for o in out {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::Duration;

    use super::Mixer;

    const RATE: u32 = 44100;
    /// Frames mixed at a time (about 12 ms).
    const PERIOD: usize = 512;
    /// ALSA's buffer, in microseconds.
    const LATENCY: c_uint = 40_000;
    /// Close the device after this long without sounds.
    const IDLE: Duration = Duration::from_secs(5);

    const SND_PCM_STREAM_PLAYBACK: c_int = 0;
    const SND_PCM_FORMAT_S16_LE: c_int = 2;
    const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

    type Pcm = *mut c_void;

    /// The few libasound functions needed, looked up at runtime.
    struct Alsa {
        open: unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int,
        set_params: unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
        writei: unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long,
        recover: unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int,
        drain: unsafe extern "C" fn(Pcm) -> c_int,
        prepare: unsafe extern "C" fn(Pcm) -> c_int,
        close: unsafe extern "C" fn(Pcm) -> c_int,
        _lib: libloading::Library,
    }

    impl Alsa {
        fn load() -> Option<Alsa> {
            // SAFETY: libasound's initialisers have no special requirements,
            // and the signatures match alsa/pcm.h.
            unsafe {
                let lib = libloading::Library::new("libasound.so.2").ok()?;
                Some(Alsa {
                    open: *lib.get(b"snd_pcm_open\0").ok()?,
                    set_params: *lib.get(b"snd_pcm_set_params\0").ok()?,
                    writei: *lib.get(b"snd_pcm_writei\0").ok()?,
                    recover: *lib.get(b"snd_pcm_recover\0").ok()?,
                    drain: *lib.get(b"snd_pcm_drain\0").ok()?,
                    prepare: *lib.get(b"snd_pcm_prepare\0").ok()?,
                    close: *lib.get(b"snd_pcm_close\0").ok()?,
                    _lib: lib,
                })
            }
        }

        fn open_default(&self) -> Option<Pcm> {
            let mut pcm: Pcm = std::ptr::null_mut();
            // SAFETY: pcm is written by snd_pcm_open and only used while open.
            unsafe {
                if (self.open)(&mut pcm, c"default".as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) < 0 {
                    return None;
                }
                let format = SND_PCM_FORMAT_S16_LE;
                if (self.set_params)(pcm, format, SND_PCM_ACCESS_RW_INTERLEAVED, 1, RATE, 1, LATENCY) < 0 {
                    (self.close)(pcm);
                    return None;
                }
            }
            Some(pcm)
        }

        /// Write all of `samples`; false if the device failed.
        fn write(&self, pcm: Pcm, mut samples: &[i16]) -> bool {
            while !samples.is_empty() {
                // SAFETY: the buffer holds samples.len() mono frames.
                let n = unsafe { (self.writei)(pcm, samples.as_ptr().cast(), samples.len() as c_ulong) };
                if n < 0 {
                    // Underrun or suspend: recover and try again.
                    if unsafe { (self.recover)(pcm, n as c_int, 1) } < 0 {
                        return false;
                    }
                } else {
                    samples = &samples[n as usize..];
                }
            }
            true
        }
    }

    pub fn run(rx: Receiver<&'static str>) {
        let Some(alsa) = Alsa::load() else {
            eprintln!("sound: libasound.so.2 not found, sounds are off");
            for _ in rx {}
            return;
        };
        let mut mixer = Mixer::new(RATE);
        let mut pcm: Option<Pcm> = None;
        let mut mixed = vec![0.0f32; PERIOD];
        let mut samples = vec![0i16; PERIOD];
        loop {
            let next = match pcm {
                Some(_) => rx.recv_timeout(IDLE),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            let name = match next {
                Ok(name) => name,
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(p) = pcm.take() {
                        unsafe { (alsa.close)(p) };
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
            mixer.start(name);
            if pcm.is_none() {
                pcm = alsa.open_default();
            }
            let Some(p) = pcm else {
                mixer.voices.clear();
                continue;
            };
            let mut ok = true;
            while ok && !mixer.is_idle() {
                while let Ok(name) = rx.try_recv() {
                    mixer.start(name);
                }
                mixer.fill(&mut mixed);
                for (s, m) in samples.iter_mut().zip(&mixed) {
                    *s = (m * 32767.0) as i16;
                }
                ok = alsa.write(p, &samples);
            }
            // Playback starts once the buffer is full: drain plays the rest.
            unsafe {
                if !ok || (alsa.drain)(p) < 0 || (alsa.prepare)(p) < 0 {
                    (alsa.close)(p);
                    pcm = None;
                    mixer.voices.clear();
                }
            }
        }
        if let Some(p) = pcm {
            unsafe { (alsa.close)(p) };
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod backend {
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    use super::Mixer;

    /// Pause the stream after this long without sounds.
    const IDLE: Duration = Duration::from_secs(2);

    type Shared = Arc<Mutex<Option<Mixer>>>;

    pub fn run(rx: Receiver<&'static str>) {
        let shared: Shared = Arc::new(Mutex::new(None));
        let mut stream: Option<cpal::Stream> = None;
        let mut playing = false;
        loop {
            match rx.recv_timeout(IDLE) {
                Ok(name) => {
                    if stream.is_none() {
                        stream = open(&shared);
                    }
                    let Some(s) = &stream else { continue };
                    if let Some(mixer) = shared.lock().unwrap().as_mut() {
                        mixer.start(name);
                    }
                    if !playing {
                        playing = s.play().is_ok();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let idle = shared.lock().unwrap().as_ref().is_none_or(Mixer::is_idle);
                    if let (true, true, Some(s)) = (playing, idle, &stream) {
                        let _ = s.pause();
                        playing = false;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn open(shared: &Shared) -> Option<cpal::Stream> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        *shared.lock().unwrap() = Some(Mixer::new(config.sample_rate as u32));
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, shared),
            SampleFormat::I16 => build::<i16>(&device, config, shared),
            SampleFormat::U16 => build::<u16>(&device, config, shared),
            SampleFormat::I32 => build::<i32>(&device, config, shared),
            SampleFormat::F64 => build::<f64>(&device, config, shared),
            _ => None,
        };
        if stream.is_none() {
            eprintln!("sound: no usable audio output, sounds are off");
        }
        stream
    }

    fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        shared: &Shared,
    ) -> Option<cpal::Stream> {
        let channels = (config.channels as usize).max(1);
        let shared = shared.clone();
        let mut mono = Vec::new();
        let callback = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            mono.resize(data.len() / channels, 0.0);
            match shared.lock().ok().as_deref_mut().and_then(Option::as_mut) {
                Some(mixer) => mixer.fill(&mut mono),
                None => mono.fill(0.0),
            }
            for (frame, &value) in data.chunks_mut(channels).zip(&mono) {
                frame.fill(T::from_sample(value));
            }
        };
        device.build_output_stream(config, callback, |e| eprintln!("sound: {e}"), None).ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod backend {
    pub fn run(rx: std::sync::mpsc::Receiver<&'static str>) {
        for _ in rx {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sounds_decode() {
        for (name, _) in SOUNDS {
            let clip = decode(name, 48000).unwrap_or_else(|| panic!("{name} does not decode"));
            // Each is between a few hundredths of a second and a few seconds.
            assert!(clip.len() > 1000 && clip.len() < 48000 * 5, "{name}: {} samples", clip.len());
            assert!(clip.iter().any(|s| s.abs() > 0.05), "{name} is silent");
        }
    }

    /// Plays through the real audio output. Run with
    /// cargo test --release plays_sound -- --ignored
    #[test]
    #[ignore]
    fn plays_sound() {
        let sound = Sound::new();
        sound.play("click");
        sound.play("victory");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    #[test]
    fn mixer_sums_and_finishes() {
        let mut mixer = Mixer::new(8000);
        mixer.start("click");
        mixer.start("click");
        let mut out = vec![0.0; 256];
        let mut frames = 0;
        while !mixer.is_idle() {
            mixer.fill(&mut out);
            frames += out.len();
        }
        assert!(frames > 0);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
    }
}
