//! SDL audio runtime for the `rts` subcommand (T16).
//!
//! Turns T15's semantic [`AudioEvent`]s into sound through pinned core SDL
//! only — no mixer/codec dependency, no callback thread: every stream is a
//! plain queue this module fills from pre-loaded, pre-validated PCM.
//!
//! # Layers
//!
//! - [`load_assets`] — loads every T14 WAV via `AudioSpecWAV::load_wav` and
//!   independently checks the tracked manifest's hash plus the locked PCM
//!   contract (48kHz/stereo/i16) before anything touches a device.
//! - [`StreamOps`] — the four stream primitives this module needs
//!   (`put_data`/`queued_bytes`/`clear`/`set_gain`), narrow enough that
//!   [`AudioEngine`]'s whole event/watermark/lane policy is generic over it
//!   and unit-tested against [`FakeStream`] with no SDL device.
//! - [`AudioEngine`] — the policy: music watermark refill, voice-batch
//!   lane replacement, dedicated reject lane, UI round robin, per-bus gain.
//! - [`SdlAudioSink`] — `AudioEngine<SdlStream>`, built by [`SdlAudioSink::open`]:
//!   the one place that actually opens a device and creates/binds streams.
//! - [`BufferedAudioSink`] — records `set_gains`/`emit` calls made before a
//!   window exists (ADR 018/020 startup ordering), replayed once into
//!   whichever real sink [`crate::rts_run::run`] ends up building.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use sdl3::Sdl;
use sdl3::audio::{
    AudioDevice, AudioFormat, AudioSpec, AudioSpecWAV, AudioStream, AudioStreamOwner,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::rts_feedback::{AudioBus, AudioError, AudioEvent, AudioSink, EffectiveGains, VoiceCue};

/// Locked PCM contract (T14): every tracked asset must match exactly.
const SAMPLE_RATE: i32 = 48_000;
const CHANNELS: i32 = 2;

/// Fixed manifest asset ids (T14 `xtask audio`), stable across regeneration.
const MUSIC_ID: u32 = 0;
const CUE_COUNT: usize = 6;
const ASSET_COUNT: usize = 1 + CUE_COUNT;

fn cue_asset_id(cue: VoiceCue) -> u32 {
    match cue {
        VoiceCue::Select => 1,
        VoiceCue::Move => 2,
        VoiceCue::Gather => 3,
        VoiceCue::Build => 4,
    }
}
const REJECT_ASSET_ID: u32 = 5;
const UI_CLICK_ASSET_ID: u32 = 6;

/// Own-stream counts (ADR 020): music1 + unit voice8 + reject1 + UI SFX4.
const MUSIC_STREAMS: usize = 1;
const VOICE_STREAMS: usize = 8;
const REJECT_STREAMS: usize = 1;
const SFX_STREAMS: usize = 4;
const TOTAL_STREAMS: usize = MUSIC_STREAMS + VOICE_STREAMS + REJECT_STREAMS + SFX_STREAMS;

fn sdl_err(e: sdl3::Error) -> AudioError {
    AudioError(e.to_string())
}

// ---------------------------------------------------------------------------
// Asset loading
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ManifestEntry {
    id: u32,
    file: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    assets: Vec<ManifestEntry>,
}

/// Validated PCM buffers, indexed by manifest asset id (`0..=6`).
pub struct AssetBank {
    pcm: Vec<Vec<u8>>,
}

impl AssetBank {
    fn music(&self) -> &[u8] {
        &self.pcm[MUSIC_ID as usize]
    }

    fn cue(&self, id: u32) -> &[u8] {
        &self.pcm[id as usize]
    }

    #[cfg(test)]
    fn synthetic(lens: [usize; ASSET_COUNT]) -> Self {
        Self {
            pcm: lens.into_iter().map(|len| vec![0u8; len]).collect(),
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Load and validate every T14 tracked WAV under `dir` (normally
/// `<workspace root>/assets/audio/generated`): manifest hash first (catches
/// tamper before any format assumption), then the exact locked PCM contract
/// via the actual `AudioSpecWAV::load_wav` parse — never the manifest's own
/// declared fields, which is what "independently" means here.
pub fn load_assets(dir: &Path) -> Result<AssetBank, AudioError> {
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|e| AudioError(format!("audio manifest {}: {e}", manifest_path.display())))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        AudioError(format!(
            "audio manifest {}: parse failed: {e}",
            manifest_path.display()
        ))
    })?;

    if manifest.assets.len() != ASSET_COUNT {
        return Err(AudioError(format!(
            "audio manifest {}: expected {ASSET_COUNT} assets, found {}",
            manifest_path.display(),
            manifest.assets.len()
        )));
    }

    let mut by_id: Vec<Option<Vec<u8>>> = vec![None; ASSET_COUNT];
    for entry in &manifest.assets {
        let Some(slot) = by_id.get_mut(entry.id as usize) else {
            return Err(AudioError(format!(
                "audio manifest {}: asset id {} out of range",
                manifest_path.display(),
                entry.id
            )));
        };
        if slot.is_some() {
            return Err(AudioError(format!(
                "audio manifest {}: duplicate asset id {}",
                manifest_path.display(),
                entry.id
            )));
        }

        let path = dir.join(&entry.file);
        let bytes = fs::read(&path)
            .map_err(|e| AudioError(format!("audio asset {}: {e}", path.display())))?;

        let actual_hash = sha256_hex(&bytes);
        if actual_hash != entry.sha256 {
            return Err(AudioError(format!(
                "audio asset {}: hash mismatch (manifest {}, on-disk {actual_hash}) — \
                 tampered or stale tracked asset",
                path.display(),
                entry.sha256
            )));
        }

        let wav = AudioSpecWAV::load_wav(&path).map_err(|e| {
            AudioError(format!(
                "audio asset {}: wav load failed: {e}",
                path.display()
            ))
        })?;
        if wav.freq != SAMPLE_RATE
            || i32::from(wav.channels) != CHANNELS
            || wav.format != AudioFormat::S16LE
        {
            return Err(AudioError(format!(
                "audio asset {}: wav spec {}Hz/{}ch/{:?} does not match the locked \
                 48000Hz/stereo/S16LE contract",
                path.display(),
                wav.freq,
                wav.channels,
                wav.format
            )));
        }

        *slot = Some(wav.buffer().to_vec());
    }

    let pcm = by_id
        .into_iter()
        .enumerate()
        .map(|(id, slot)| {
            slot.ok_or_else(|| {
                AudioError(format!(
                    "audio manifest {}: missing asset id {id}",
                    manifest_path.display()
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(AssetBank { pcm })
}

// ---------------------------------------------------------------------------
// Stream abstraction
// ---------------------------------------------------------------------------

/// The four stream primitives [`AudioEngine`] needs. Narrow enough that the
/// whole event/watermark/lane policy below is generic over it — real SDL
/// ([`SdlStream`]) and a fake ([`FakeStream`], test-only) are the only two
/// implementations.
pub trait StreamOps {
    fn put_data(&self, bytes: &[u8]) -> Result<(), AudioError>;
    fn queued_bytes(&self) -> Result<i32, AudioError>;
    fn clear(&self) -> Result<(), AudioError>;
    fn set_gain(&self, gain: f32) -> Result<(), AudioError>;
}

/// The real [`StreamOps`], over one bound `AudioStreamOwner`. A newtype
/// (rather than implementing the trait directly on `AudioStreamOwner`) so
/// method lookup inside the impl unambiguously reaches the real SDL
/// `AudioStream` methods through `Deref`, not this trait's own.
pub struct SdlStream(AudioStreamOwner);

impl SdlStream {
    fn as_stream(&self) -> &AudioStream {
        &self.0
    }
}

impl StreamOps for SdlStream {
    fn put_data(&self, bytes: &[u8]) -> Result<(), AudioError> {
        self.0.put_data(bytes).map_err(sdl_err)
    }
    fn queued_bytes(&self) -> Result<i32, AudioError> {
        self.0.queued_bytes().map_err(sdl_err)
    }
    fn clear(&self) -> Result<(), AudioError> {
        self.0.clear().map_err(sdl_err)
    }
    fn set_gain(&self, gain: f32) -> Result<(), AudioError> {
        self.0.set_gain(gain).map_err(sdl_err)
    }
}

// ---------------------------------------------------------------------------
// Policy engine — generic over StreamOps, SDL-free for tests
// ---------------------------------------------------------------------------

/// The whole event/watermark/lane/gain policy (ADR 020), generic over
/// [`StreamOps`] so it is unit-tested against [`FakeStream`] with no SDL
/// device, and reused unchanged as [`SdlAudioSink`].
pub struct AudioEngine<S: StreamOps> {
    assets: AssetBank,
    /// Kept only to close on drop (its `Drop` calls `SDL_CloseAudioDevice`);
    /// never read once the streams are bound.
    #[allow(dead_code)]
    device: Option<AudioDevice>,
    music: S,
    voice: Vec<S>,
    reject: S,
    sfx: Vec<S>,
    sfx_next: usize,
    music_active: bool,
    gains: EffectiveGains,
}

impl<S: StreamOps> AudioEngine<S> {
    fn new(
        assets: AssetBank,
        device: Option<AudioDevice>,
        music: S,
        voice: Vec<S>,
        reject: S,
        sfx: Vec<S>,
    ) -> Self {
        debug_assert_eq!(voice.len(), VOICE_STREAMS);
        debug_assert_eq!(sfx.len(), SFX_STREAMS);
        Self {
            assets,
            device,
            music,
            voice,
            reject,
            sfx,
            sfx_next: 0,
            music_active: false,
            gains: EffectiveGains {
                music_basis_points: 0,
                voice_basis_points: 0,
                sfx_basis_points: 0,
            },
        }
    }

    #[cfg(test)]
    fn gains(&self) -> EffectiveGains {
        self.gains
    }
}

fn basis_points_to_gain(bp: u16) -> f32 {
    bp as f32 / 10_000.0
}

impl<S: StreamOps> AudioSink for AudioEngine<S> {
    /// Per-bus stream gains only (basis points `/10000`); the device gain
    /// itself is never touched and stays at its default `1.0` (ADR 020).
    fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError> {
        self.gains = gains;
        let music = basis_points_to_gain(gains.for_bus(AudioBus::Music));
        let voice = basis_points_to_gain(gains.for_bus(AudioBus::Voice));
        let sfx = basis_points_to_gain(gains.for_bus(AudioBus::Sfx));
        self.music.set_gain(music)?;
        for stream in &self.voice {
            stream.set_gain(voice)?;
        }
        self.reject.set_gain(voice)?;
        for stream in &self.sfx {
            stream.set_gain(sfx)?;
        }
        Ok(())
    }

    fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError> {
        match event {
            // Marks active only; `maintain` (below) owns every buffer
            // queued from here — a muted or not-yet-maintained stream still
            // advances, it just plays silence.
            AudioEvent::StartMusic => self.music_active = true,
            AudioEvent::Voice(batch) => {
                // Latest action replaces the whole batch: clear all eight
                // lanes first, then fill only `0..len` — the rest stays
                // silent until the next batch.
                for stream in &self.voice {
                    stream.clear()?;
                }
                for (lane, cue) in batch.cues().enumerate() {
                    let bytes = self.assets.cue(cue_asset_id(cue.cue));
                    self.voice[lane].put_data(bytes)?;
                }
            }
            AudioEvent::Reject => {
                // Dedicated lane: never touches (or is touched by) the unit
                // voice lanes above.
                self.reject.clear()?;
                self.reject.put_data(self.assets.cue(REJECT_ASSET_ID))?;
            }
            AudioEvent::Ui(_) => {
                let lane = self.sfx_next;
                self.sfx_next = (self.sfx_next + 1) % SFX_STREAMS;
                self.sfx[lane].clear()?;
                self.sfx[lane].put_data(self.assets.cue(UI_CLICK_ASSET_ID))?;
            }
        }
        Ok(())
    }

    /// Music-only upkeep: while active and queued bytes are below two full
    /// buffers, queue another — never clear/flush, so a still-playing tail
    /// is never cut. The asset's own zero first/last frame makes every loop
    /// boundary click-free.
    fn maintain(&mut self) -> Result<(), AudioError> {
        if !self.music_active {
            return Ok(());
        }
        let music = self.assets.music();
        let watermark = 2 * music.len() as i64;
        while (self.music.queued_bytes()? as i64) < watermark {
            self.music.put_data(music)?;
        }
        Ok(())
    }
}

/// `AudioEngine<SdlStream>`: the only sink T16 ever hands a real device to.
pub type SdlAudioSink = AudioEngine<SdlStream>;

impl SdlAudioSink {
    /// Load assets from `assets_dir`, open the default playback device,
    /// create and bind all 14 streams, then resume the device. Any failure
    /// — load, open, create, bind, or resume — is an actionable
    /// [`AudioError`] naming the asset or device step at fault; the caller
    /// (`rts_run::run`) turns that into an interactive-startup
    /// `RunError::Failed` (exit 1) with no offscreen fallback.
    pub fn open(sdl: &Sdl, assets_dir: &Path) -> Result<Self, AudioError> {
        let assets = load_assets(assets_dir)?;

        let audio = sdl
            .audio()
            .map_err(|e| AudioError(format!("audio subsystem init failed: {e}")))?;
        let spec = AudioSpec {
            freq: Some(SAMPLE_RATE),
            channels: Some(CHANNELS),
            format: Some(AudioFormat::S16LE),
        };

        let device = audio
            .open_playback_device(&spec)
            .map_err(|e| AudioError(format!("open playback device failed: {e}")))?;

        let mut streams = Vec::with_capacity(TOTAL_STREAMS);
        for _ in 0..TOTAL_STREAMS {
            let stream = audio
                .new_playback_stream(&spec, None)
                .map_err(|e| AudioError(format!("create audio stream failed: {e}")))?;
            streams.push(SdlStream(stream));
        }

        {
            let refs: Vec<&AudioStream> = streams.iter().map(SdlStream::as_stream).collect();
            device
                .bind_streams(&refs)
                .map_err(|e| AudioError(format!("bind audio streams failed: {e}")))?;
        }

        if !device.resume() {
            return Err(AudioError(format!(
                "audio device resume failed: {}",
                sdl3::get_error()
            )));
        }

        let mut iter = streams.into_iter();
        let music = iter.next().expect("14 streams were just created");
        let voice: Vec<SdlStream> = (&mut iter).take(VOICE_STREAMS).collect();
        let reject = iter.next().expect("14 streams were just created");
        let sfx: Vec<SdlStream> = (&mut iter).take(SFX_STREAMS).collect();
        debug_assert!(iter.next().is_none());

        // Kept alive (not dropped) for the sink's lifetime: the bound
        // streams depend on the device staying open.
        Ok(AudioEngine::new(
            assets,
            Some(device),
            music,
            voice,
            reject,
            sfx,
        ))
    }
}

// ---------------------------------------------------------------------------
// Buffered pre-window sink (ADR 018/020 startup ordering)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum BufferedOp {
    Gains(EffectiveGains),
    Event(AudioEvent),
}

struct BufferedInner {
    ops: Vec<BufferedOp>,
}

/// Records every `set_gains`/`emit` call before frame 1's window-vs-offscreen
/// decision is made. [`Self::handle`] hands out another owner of the same
/// log (`Rc<RefCell<_>>`, the same pattern `FakeSinkHandle` uses) so the run
/// loop can box one clone into [`crate::rts_run::RtsSession`] and keep the
/// other to replay once the real sink is known — exactly once, in order,
/// into whichever of [`SdlAudioSink`] or `FakeAudioSink` the run picked.
#[derive(Clone)]
pub struct BufferedAudioSink {
    inner: Rc<RefCell<BufferedInner>>,
}

impl BufferedAudioSink {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(BufferedInner { ops: Vec::new() })),
        }
    }

    /// Another owner of the same recorded log.
    pub fn handle(&self) -> Self {
        self.clone()
    }

    /// Replay every recorded `set_gains`/`emit` call, in order, into `sink`,
    /// then one `maintain()` to prime the initial music watermark before the
    /// first present. A replay failure is the caller's cue to treat this as
    /// the same fatal interactive-startup error a live SDL failure would be.
    pub fn replay_into(&self, sink: &mut dyn AudioSink) -> Result<(), AudioError> {
        for op in &self.inner.borrow().ops {
            match op {
                BufferedOp::Gains(gains) => sink.set_gains(*gains)?,
                BufferedOp::Event(event) => sink.emit(*event)?,
            }
        }
        sink.maintain()
    }
}

impl Default for BufferedAudioSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for BufferedAudioSink {
    fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError> {
        self.inner.borrow_mut().ops.push(BufferedOp::Gains(gains));
        Ok(())
    }

    fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError> {
        self.inner.borrow_mut().ops.push(BufferedOp::Event(event));
        Ok(())
    }

    /// Never emits or records: the buffered stage predates any device, so
    /// there is nothing for it to prime yet — [`Self::replay_into`] does
    /// that once, on the real sink.
    fn maintain(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts_feedback::{UiCue, UnitCue, VoiceBatch};
    use mmd_engine::rts::EntityId;
    use std::cell::Cell;

    fn unit(index: u32, cue: VoiceCue) -> UnitCue {
        UnitCue {
            entity: EntityId {
                index,
                generation: 1,
            },
            cue,
        }
    }

    fn batch(cues: &[UnitCue]) -> VoiceBatch {
        let mut b = VoiceBatch::new();
        for &c in cues {
            b.push_sorted(c);
        }
        b
    }

    // -- Fake stream: records every call, test-only, no SDL -------------

    #[derive(Default)]
    struct FakeStream {
        log: RefCell<Vec<String>>,
        queued: Cell<i32>,
        gain: Cell<f32>,
    }

    impl FakeStream {
        fn log(&self) -> Vec<String> {
            self.log.borrow().clone()
        }
    }

    impl StreamOps for FakeStream {
        fn put_data(&self, bytes: &[u8]) -> Result<(), AudioError> {
            self.queued.set(self.queued.get() + bytes.len() as i32);
            self.log
                .borrow_mut()
                .push(format!("put_data({})", bytes.len()));
            Ok(())
        }
        fn queued_bytes(&self) -> Result<i32, AudioError> {
            Ok(self.queued.get())
        }
        fn clear(&self) -> Result<(), AudioError> {
            self.queued.set(0);
            self.log.borrow_mut().push("clear".to_string());
            Ok(())
        }
        fn set_gain(&self, gain: f32) -> Result<(), AudioError> {
            self.gain.set(gain);
            self.log.borrow_mut().push(format!("set_gain({gain})"));
            Ok(())
        }
    }

    fn test_engine(music_len: usize) -> AudioEngine<FakeStream> {
        let assets = AssetBank::synthetic([music_len, 8, 8, 8, 8, 8, 8]);
        AudioEngine::new(
            assets,
            None,
            FakeStream::default(),
            (0..VOICE_STREAMS).map(|_| FakeStream::default()).collect(),
            FakeStream::default(),
            (0..SFX_STREAMS).map(|_| FakeStream::default()).collect(),
        )
    }

    // -- Loader -----------------------------------------------------------

    #[test]
    fn loader_rejects_wrong_wav_spec_or_hash() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/audio/generated");
        let tmp = tempfile::tempdir().expect("tempdir");
        for entry in fs::read_dir(&src).expect("read generated dir") {
            let entry = entry.expect("dir entry");
            fs::copy(entry.path(), tmp.path().join(entry.file_name())).expect("copy asset");
        }

        // Tamper the audio bytes without touching the manifest's hash.
        let path = tmp.path().join("ui_click.wav");
        let mut bytes = fs::read(&path).expect("read ui_click.wav");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&path, bytes).expect("write tampered wav");

        let err = match load_assets(tmp.path()) {
            Ok(_) => panic!("tampered asset must be rejected"),
            Err(e) => e,
        };
        assert!(err.0.contains("hash"), "{}", err.0);
        assert!(err.0.contains("ui_click.wav"), "{}", err.0);
    }

    #[test]
    fn loader_accepts_the_tracked_assets() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/audio/generated");
        let bank = load_assets(&dir).expect("tracked assets load and validate");
        assert_eq!(bank.pcm.len(), ASSET_COUNT);
        assert!(!bank.music().is_empty());
        for id in 1..=CUE_COUNT as u32 {
            assert!(!bank.cue(id).is_empty(), "cue {id}");
        }
    }

    // -- Music watermark ---------------------------------------------------

    #[test]
    fn music_watermark_queues_two_buffers() {
        let mut engine = test_engine(100);
        engine.emit(AudioEvent::StartMusic).expect("start music");
        engine.maintain().expect("maintain");

        assert_eq!(
            engine.music.log(),
            vec!["put_data(100)".to_string(), "put_data(100)".to_string()],
            "queues until queued bytes >= 2x music length"
        );
        assert_eq!(engine.music.queued_bytes().unwrap(), 200);

        // Steady state: nothing further queues while already at watermark.
        engine.maintain().expect("maintain again");
        assert_eq!(engine.music.log().len(), 2, "already at 2x, nothing queued");
    }

    #[test]
    fn maintain_before_start_music_queues_nothing() {
        let mut engine = test_engine(100);
        engine.maintain().expect("maintain with no active music");
        assert!(engine.music.log().is_empty());
    }

    // -- Voice batch lanes ---------------------------------------------------

    #[test]
    fn voice_batch_replaces_eight_lanes() {
        let mut engine = test_engine(100);
        let first = batch(&[unit(1, VoiceCue::Select), unit(2, VoiceCue::Move)]);
        engine.emit(AudioEvent::Voice(first)).expect("first batch");

        for stream in &engine.voice {
            assert!(stream.log().contains(&"clear".to_string()));
        }
        assert!(
            engine.voice[0]
                .log()
                .iter()
                .any(|l| l.starts_with("put_data"))
        );
        assert!(
            engine.voice[1]
                .log()
                .iter()
                .any(|l| l.starts_with("put_data"))
        );
        assert!(
            !engine.voice[2]
                .log()
                .iter()
                .any(|l| l.starts_with("put_data"))
        );

        let second = batch(&[unit(9, VoiceCue::Build)]);
        engine
            .emit(AudioEvent::Voice(second))
            .expect("second batch");

        // Every lane cleared again; only lane 0 has new content — the old
        // batch's lane 1 content is gone (cleared, not appended to).
        assert_eq!(
            engine.voice[0].log().last(),
            Some(&"put_data(8)".to_string())
        );
        assert_eq!(engine.voice[1].log().last(), Some(&"clear".to_string()));
    }

    // -- Reject dedicated lane ----------------------------------------------

    #[test]
    fn reject_uses_dedicated_voice_lane() {
        let mut engine = test_engine(100);
        let voiced = batch(&[unit(1, VoiceCue::Select)]);
        engine.emit(AudioEvent::Voice(voiced)).expect("voice batch");
        let accepted_log_before = engine.voice[0].log();

        engine.emit(AudioEvent::Reject).expect("reject");

        assert_eq!(
            engine.voice[0].log(),
            accepted_log_before,
            "an accepted unit's lane is untouched by a reject"
        );
        assert!(engine.reject.log().contains(&"clear".to_string()));
        assert!(
            engine
                .reject
                .log()
                .iter()
                .any(|l| l.starts_with("put_data"))
        );
    }

    // -- UI round robin -------------------------------------------------------

    #[test]
    fn fifth_ui_click_steals_oldest_sfx_lane() {
        let mut engine = test_engine(100);
        for _ in 0..5 {
            engine.emit(AudioEvent::Ui(UiCue::Menu)).expect("ui click");
        }
        // Lanes 0..4 each got exactly one clear+put from the first four
        // clicks; the fifth wraps back to lane 0, adding a second
        // clear+put there — deterministic round robin, not random eviction.
        assert_eq!(
            engine.sfx[0]
                .log()
                .iter()
                .filter(|l| l.starts_with("put_data"))
                .count(),
            2,
            "lane 0 stolen by the fifth (wrapped) click"
        );
        for lane in 1..SFX_STREAMS {
            assert_eq!(
                engine.sfx[lane]
                    .log()
                    .iter()
                    .filter(|l| l.starts_with("put_data"))
                    .count(),
                1
            );
        }
    }

    // -- Gain ------------------------------------------------------------

    #[test]
    fn live_gain_change_updates_all_bus_streams() {
        let mut engine = test_engine(100);
        let gains = EffectiveGains {
            music_basis_points: 2800,
            voice_basis_points: 5600,
            sfx_basis_points: 4800,
        };
        engine.set_gains(gains).expect("set gains");

        assert_eq!(engine.gains(), gains);
        assert_eq!(engine.music.gain.get(), 0.28);
        for stream in &engine.voice {
            assert_eq!(stream.gain.get(), 0.56);
        }
        assert_eq!(
            engine.reject.gain.get(),
            0.56,
            "reject shares the voice bus"
        );
        for stream in &engine.sfx {
            assert_eq!(stream.gain.get(), 0.48);
        }
    }

    /// Mute is gain-only: queued PCM + stream count stay put across set_gains.
    #[test]
    fn mute_gain_update_does_not_clear_streams() {
        let mut engine = test_engine(100);
        engine.emit(AudioEvent::StartMusic).expect("start music");
        engine.maintain().expect("prime music queue");
        let music_queued_before = engine.music.queued_bytes().unwrap();
        assert!(music_queued_before > 0, "music must have queued bytes");

        // Seed one voice lane so we can prove mute doesn't clear it either.
        engine.voice[0].put_data(&[1, 2, 3, 4]).expect("seed voice");
        let voice_queued_before = engine.voice[0].queued_bytes().unwrap();
        let clears = |s: &FakeStream| s.log().iter().filter(|l| *l == "clear").count();
        let music_clears_before = clears(&engine.music);
        let voice_clears_before = clears(&engine.voice[0]);

        engine
            .set_gains(EffectiveGains {
                music_basis_points: 0,
                voice_basis_points: 0,
                sfx_basis_points: 0,
            })
            .expect("mute via gains");

        assert_eq!(engine.music.queued_bytes().unwrap(), music_queued_before);
        assert_eq!(engine.voice[0].queued_bytes().unwrap(), voice_queued_before);
        assert_eq!(engine.music.gain.get(), 0.0);
        assert_eq!(engine.voice[0].gain.get(), 0.0);
        assert_eq!(
            clears(&engine.music),
            music_clears_before,
            "mute must not clear music"
        );
        assert_eq!(
            clears(&engine.voice[0]),
            voice_clears_before,
            "mute must not clear voice"
        );
        // Stream inventory unchanged (engine still owns the same lanes).
        assert_eq!(engine.voice.len(), VOICE_STREAMS);
        assert_eq!(engine.sfx.len(), SFX_STREAMS);
    }

    // -- Buffered replay ---------------------------------------------------

    #[test]
    fn buffered_frame1_events_replay_once() {
        let buffered = BufferedAudioSink::new();
        let handle = buffered.handle();
        let mut buffered = buffered;
        buffered
            .set_gains(EffectiveGains {
                music_basis_points: 100,
                voice_basis_points: 200,
                sfx_basis_points: 300,
            })
            .expect("buffered set_gains never fails");
        buffered
            .emit(AudioEvent::StartMusic)
            .expect("buffered emit never fails");

        let mut engine = test_engine(100);
        handle.replay_into(&mut engine).expect("replay");

        assert_eq!(
            engine.gains(),
            EffectiveGains {
                music_basis_points: 100,
                voice_basis_points: 200,
                sfx_basis_points: 300,
            }
        );
        assert!(engine.music_active, "the buffered StartMusic replayed");
        // Replay ends with exactly one `maintain()`, priming the watermark.
        let puts = engine
            .music
            .log()
            .iter()
            .filter(|l| l.starts_with("put_data"))
            .count();
        assert_eq!(puts, 2, "one maintain queues 2x buffers");

        // A second replay of the same handle must not duplicate anything
        // beyond what a second explicit replay call does — the log itself
        // is the single source of truth, not consumed by replaying it.
        let mut engine2 = test_engine(100);
        handle.replay_into(&mut engine2).expect("replay again");
        assert_eq!(engine2.gains(), engine.gains());
    }

    // -- Offscreen / dummy driver integration ------------------------------

    /// `SDL_AUDIODRIVER` is process-global; these two tests are the only
    /// ones in this binary that touch it, but cargo still runs them
    /// concurrently by default — serialize them so one's env mutation can
    /// never leak into the other's `sdl3::init()`.
    static AUDIO_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The offscreen path never even calls [`SdlAudioSink::open`] — proven
    /// at the CLI level in `tests/rts_cli_contract.rs`
    /// (`audio_offscreen_survives_invalid_audio_driver`). This test proves
    /// the other half: under `SDL_AUDIODRIVER=dummy`, opening a real device
    /// and running one full maintain cycle succeeds — the interactive path
    /// this module exists for.
    #[test]
    fn dummy_driver_starts_and_maintains() {
        let _guard = AUDIO_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Never asserted on a shared/default driver: forcing `dummy` here
        // is what keeps this test host-independent and silent.
        // Safety: serialized by `AUDIO_ENV_LOCK` above, env restored before
        // the guard drops.
        let previous = std::env::var_os("SDL_AUDIODRIVER");
        unsafe {
            std::env::set_var("SDL_AUDIODRIVER", "dummy");
        }
        let sdl = sdl3::init().expect("sdl init");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/audio/generated");

        let mut sink = SdlAudioSink::open(&sdl, &dir).expect("dummy device opens");
        sink.set_gains(EffectiveGains {
            music_basis_points: 2800,
            voice_basis_points: 5600,
            sfx_basis_points: 4800,
        })
        .expect("set gains on dummy device");
        sink.emit(AudioEvent::StartMusic).expect("start music");
        sink.maintain().expect("maintain on dummy device");
        sink.emit(AudioEvent::Ui(UiCue::Menu)).expect("ui click");

        unsafe {
            match previous {
                Some(v) => std::env::set_var("SDL_AUDIODRIVER", v),
                None => std::env::remove_var("SDL_AUDIODRIVER"),
            }
        }
    }

    #[test]
    fn audio_device_open_failure_is_actionable() {
        let _guard = AUDIO_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("SDL_AUDIODRIVER");
        unsafe {
            std::env::set_var("SDL_AUDIODRIVER", "mmd-nonexistent-driver");
        }
        let sdl = sdl3::init().expect("sdl init");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/audio/generated");

        let err = match SdlAudioSink::open(&sdl, &dir) {
            Ok(_) => panic!("nonexistent driver must fail"),
            Err(e) => e,
        };
        assert!(!err.0.is_empty());

        unsafe {
            match previous {
                Some(v) => std::env::set_var("SDL_AUDIODRIVER", v),
                None => std::env::remove_var("SDL_AUDIODRIVER"),
            }
        }
    }
}
