//! The playback seam. `AudioSink` is the ONE device boundary: tests (and
//! CI runners with no sound card) use [`NullSink`]; production uses the
//! rodio-backed sink behind the `audio` feature. The LISTEN gate's wav
//! renderer implements the same trait — everything above this line is
//! device-free.

use std::sync::Arc;

use pixtuoid_scene::audio::mixer::LoopStem;

pub(crate) trait AudioSink: Send {
    /// Start `stem` looping `samples` (mono f32 @ 44_100) at gain 0.
    fn start_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>);
    /// Replace a looping stem's buffer (the #644 mood-track switch) — the
    /// caller guarantees the stem is at gain 0, so the cut is inaudible.
    fn swap_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>);
    /// Set a looping stem's gain (0..=1, already master-scaled).
    fn set_loop_gain(&mut self, stem: LoopStem, gain: f32);
    /// Fire-and-forget one-shot at `gain`.
    fn play_once(&mut self, samples: Arc<Vec<f32>>, gain: f32);
}

/// Records calls instead of making sound — the CI/test double. Keeps the
/// registered BUFFERS (not just the stem tags) so tests can pin that each
/// stem got the RIGHT bed — a `bed()` arm swap must not pass (review
/// finding: tag-only recording was blind to it).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct NullSink {
    pub(crate) loops_started: Vec<LoopStem>,
    pub(crate) loop_samples: std::collections::HashMap<LoopStem, Arc<Vec<f32>>>,
    /// (stem, new buffer length) per swap — the #644 switch-machine pin.
    pub(crate) swaps: Vec<(LoopStem, usize)>,
    pub(crate) last_gain: std::collections::HashMap<LoopStem, f32>,
    pub(crate) one_shots: usize,
}

#[cfg(test)]
impl AudioSink for NullSink {
    fn start_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>) {
        self.loops_started.push(stem);
        self.loop_samples.insert(stem, samples);
    }
    fn swap_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>) {
        self.swaps.push((stem, samples.len()));
        self.loop_samples.insert(stem, samples);
    }
    fn set_loop_gain(&mut self, stem: LoopStem, gain: f32) {
        self.last_gain.insert(stem, gain);
    }
    fn play_once(&mut self, _samples: Arc<Vec<f32>>, gain: f32) {
        if gain > 0.0 {
            self.one_shots += 1;
        }
    }
}

/// The real device sink — rodio/cpal construction glue (winit-class:
/// needs real audio hardware, codecov-excluded, no unit tests; the LISTEN
/// gate + dogfood are its verification).
#[cfg(feature = "audio")]
pub(crate) mod rodio_sink {
    use super::*;
    use std::collections::HashMap;

    pub(crate) struct RodioSink {
        // field order = drop order: players release before the device sink
        loops: HashMap<LoopStem, rodio::Player>,
        music: Option<rodio::Player>,
        stream: rodio::MixerDeviceSink,
    }

    impl RodioSink {
        /// `None` when no output device is available (headless boxes) —
        /// callers degrade to silence, never error the office.
        pub(crate) fn open() -> Option<Self> {
            // `open_default_sink` keeps rodio's full open FALLBACK: the default
            // device+config first, then — on failure — every other non-"null"
            // output device, each retried across its supported configs. A
            // hand-rolled `from_default_device().open_stream()` would silently drop
            // that `.or_else` and go silent on hardware the fallback would have
            // recovered. rodio's `tracing` feature (Cargo.toml) routes a MID-SESSION
            // stream error (device unplugged, sample-rate change) — fired on the
            // audio thread — to `tracing::error!` instead of the default callback's
            // `eprintln!`, which mid-altscreen would corrupt the TUI. rodio 0.22 has
            // no reconnect, so this is observability, not recovery — audio just goes
            // silent, now logged. `with_stderr_silenced` still wraps the call for
            // ALSA's C-level fd-2 chatter (below rodio's Rust logging).
            let opened = with_stderr_silenced(rodio::DeviceSinkBuilder::open_default_sink);
            match opened {
                Ok(mut stream) => {
                    stream.log_on_drop(false);
                    Some(Self {
                        loops: HashMap::new(),
                        music: None,
                        stream,
                    })
                }
                Err(e) => {
                    tracing::warn!("audio: no output device, running silent: {e}");
                    None
                }
            }
        }

        fn source_of(samples: &Arc<Vec<f32>>) -> rodio::buffer::SamplesBuffer {
            let mono = std::num::NonZero::new(1u16).expect("1 != 0");
            let rate = std::num::NonZero::new(pixtuoid_scene::audio::dsp::SAMPLE_RATE)
                .expect("44100 != 0");
            rodio::buffer::SamplesBuffer::new(mono, rate, samples.as_slice())
        }

        /// Decode and loop a user-selected local file without buffering the
        /// entire track in memory. This is the only music player in local-file
        /// mode, so it replaces the synthesized loop bank rather than mixing
        /// over it.
        pub(crate) fn start_music_file(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
            let source = decode_looped_file(path)?;
            let player = rodio::Player::connect_new(self.stream.mixer());
            player.set_volume(0.0);
            player.append(source);
            self.music = Some(player);
            Ok(())
        }

        pub(crate) fn set_music_gain(&mut self, gain: f32) {
            if let Some(player) = &self.music {
                player.set_volume(gain.clamp(0.0, 1.0));
            }
        }
    }

    fn decode_looped_file(
        path: &std::path::Path,
    ) -> anyhow::Result<rodio::decoder::LoopedDecoder<std::fs::File>> {
        let file = std::fs::File::open(path)?;
        Ok(rodio::Decoder::new_looped(file)?)
    }

    /// Run `f` with fd 2 pointed at /dev/null (Unix): ALSA and friends
    /// print raw diagnostics to stderr during device open, and with the
    /// lazy spawn that happens MID-ALTSCREEN — one stray line corrupts the
    /// TUI (lowfi's first-ever issue was exactly this). rodio's own logs
    /// are already off via `log_on_drop(false)`.
    #[cfg(unix)]
    fn with_stderr_silenced<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: plain dup/dup2 fd shuffling; restored before returning.
        // A panic inside `f` would leak the redirect — acceptable for a
        // device-open that must not unwind (rodio returns Result).
        unsafe {
            let saved = libc::dup(2);
            if saved >= 0 {
                let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
                if devnull >= 0 {
                    libc::dup2(devnull, 2);
                    libc::close(devnull);
                }
            }
            let out = f();
            if saved >= 0 {
                libc::dup2(saved, 2);
                libc::close(saved);
            }
            out
        }
    }

    #[cfg(not(unix))]
    fn with_stderr_silenced<T>(f: impl FnOnce() -> T) -> T {
        f()
    }

    impl AudioSink for RodioSink {
        fn start_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>) {
            use rodio::Source;
            let player = rodio::Player::connect_new(self.stream.mixer());
            player.set_volume(0.0);
            player.append(Self::source_of(&samples).repeat_infinite());
            self.loops.insert(stem, player);
        }

        fn swap_loop(&mut self, stem: LoopStem, samples: Arc<Vec<f32>>) {
            // dropping the old Player stops it (the caller holds the stem
            // at gain 0 across the swap, so nothing audible is cut)
            if let Some(old) = self.loops.remove(&stem) {
                old.stop();
            }
            self.start_loop(stem, samples);
        }

        fn set_loop_gain(&mut self, stem: LoopStem, gain: f32) {
            if let Some(player) = self.loops.get(&stem) {
                player.set_volume(gain);
            }
        }

        fn play_once(&mut self, samples: Arc<Vec<f32>>, gain: f32) {
            if gain <= 0.0 {
                return;
            }
            let player = rodio::Player::connect_new(self.stream.mixer());
            player.set_volume(gain);
            player.append(Self::source_of(&samples));
            player.detach();
        }
    }

    #[cfg(test)]
    mod local_file_tests {
        use super::*;
        use rodio::Source;

        #[test]
        fn a_valid_wav_decodes_as_a_loop_without_an_audio_device() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("fixture.wav");
            let samples = [0i16, 1000, -1000, 0];
            let data_len = (samples.len() * std::mem::size_of::<i16>()) as u32;
            let mut wav = Vec::with_capacity(44 + data_len as usize);
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&8_000u32.to_le_bytes());
            wav.extend_from_slice(&16_000u32.to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            for sample in samples {
                wav.extend_from_slice(&sample.to_le_bytes());
            }
            std::fs::write(&path, wav).unwrap();

            let mut decoded = decode_looped_file(&path).expect("valid WAV decodes");
            assert_eq!(decoded.channels().get(), 1);
            assert_eq!(decoded.sample_rate().get(), 8_000);
            assert!(
                decoded.by_ref().take(samples.len() * 3).count() > samples.len(),
                "looped decoder continues beyond one pass"
            );
        }
    }
}
