//! Deterministic placeholder audio generator for the RTS interaction slice.
//!
//! Integer-only synthesis (phase-accumulator triangle DDS, integer linear
//! envelopes) — no float trig, no RNG, no host clock — so output is
//! bit-identical across hosts and regenerations. Same shape as
//! `placeholder_art.rs`: a spec table drives generation, manifest and
//! `--check`.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::digest::sha256_hex;

/// Generator id, stamped into the manifest.
pub const GENERATOR_ID: &str = "mmd-audio-placeholder-v1";
pub const MANIFEST_VERSION: u32 = 1;
pub const LICENSE: &str = "MIT-0";

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const BITS_PER_SAMPLE: u16 = 16;
pub const BLOCK_ALIGN: u16 = CHANNELS * (BITS_PER_SAMPLE / 8);
pub const BYTE_RATE: u32 = SAMPLE_RATE * BLOCK_ALIGN as u32;

/// 5ms integer attack/release, shared by every generated asset.
pub const ENVELOPE_FRAMES: usize = (SAMPLE_RATE as u64 * 5 / 1000) as usize; // 240

pub const MUSIC_FRAMES: usize = 384_000; // 8s
pub const MUSIC_STEP_FRAMES: usize = 24_000; // 0.5s
pub const MUSIC_STEPS: usize = 16;
pub const MUSIC_BASS_PATTERN: [u32; 8] = [110, 110, 147, 147, 98, 98, 131, 131];
pub const MUSIC_LEAD_PATTERN: [u32; 8] = [220, 262, 330, 262, 196, 247, 294, 247];
pub const MUSIC_BASS_AMPLITUDE: i32 = 3072;
pub const MUSIC_LEAD_AMPLITUDE: i32 = 3072;
pub const MUSIC_PEAK_MAX: i32 = 6144;

pub const CUE_PEAK_MAX: i32 = 1536;

/// `assets/audio/generated`
pub fn generated_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("assets/audio/generated")
}

/// Generation / check failures.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("io error: {0}")]
    Io(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("wav error: {0}")]
    Wav(String),
    #[error("hash mismatch for {file}: expected {expected}, got {actual}")]
    HashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("check failed: {0}")]
    Check(String),
}

/// One tracked audio asset entry in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioEntry {
    pub id: u32,
    pub file: String,
    pub role: String,
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
    pub bits: u32,
    pub peak: i32,
    pub sha256: String,
}

/// Tracked audio manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioManifest {
    pub version: u32,
    pub generator: String,
    pub license: String,
    pub assets: Vec<AudioEntry>,
}

/// A single-frequency or two-frequency (sequential) placeholder cue.
enum CueTone {
    One(u32),
    Two(u32, u32),
}

struct CueSpec {
    id: u32,
    file: &'static str,
    role: &'static str,
    frames: usize,
    tone: CueTone,
}

/// The six locked placeholder cues, in manifest order (ids 1..=6).
const CUES: [CueSpec; 6] = [
    CueSpec {
        id: 1,
        file: "voice_select.wav",
        role: "voice",
        frames: 5_760,
        tone: CueTone::One(660),
    },
    CueSpec {
        id: 2,
        file: "voice_move.wav",
        role: "voice",
        frames: 5_760,
        tone: CueTone::One(520),
    },
    CueSpec {
        id: 3,
        file: "voice_gather.wav",
        role: "voice",
        frames: 5_760,
        tone: CueTone::One(740),
    },
    CueSpec {
        id: 4,
        file: "voice_build.wav",
        role: "voice",
        frames: 5_760,
        tone: CueTone::One(440),
    },
    CueSpec {
        id: 5,
        file: "voice_reject.wav",
        role: "voice",
        frames: 8_640,
        tone: CueTone::Two(220, 165),
    },
    CueSpec {
        id: 6,
        file: "ui_click.wav",
        role: "ui",
        frames: 2_880,
        tone: CueTone::One(880),
    },
];

/// Phase-accumulator increment for `freq_hz` at `SAMPLE_RATE`, u0.32 fixed point.
fn phase_increment(freq_hz: u32) -> u32 {
    (((freq_hz as u64) << 32) / SAMPLE_RATE as u64) as u32
}

/// One integer triangle sample at `phase` (u0.32 turns), peak `amplitude`.
/// `value(t) = amplitude * (1 - 4*|t - 0.5|)`, all integer arithmetic.
fn triangle_sample(phase: u32, amplitude: i32) -> i32 {
    let half = 1u32 << 31;
    let diff = phase.abs_diff(half);
    amplitude - ((4 * amplitude as i64 * diff as i64) / (1i64 << 32)) as i32
}

/// Generate `frames` integer triangle samples starting at phase 0.
fn triangle_wave(freq_hz: u32, frames: usize, amplitude: i32) -> Vec<i32> {
    let inc = phase_increment(freq_hz);
    let mut phase: u32 = 0;
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        out.push(triangle_sample(phase, amplitude));
        phase = phase.wrapping_add(inc);
    }
    out
}

/// Apply a 5ms integer linear attack/release in place.
fn apply_envelope(samples: &mut [i32]) {
    let n = samples.len();
    let attack = ENVELOPE_FRAMES.min(n);
    for (i, s) in samples.iter_mut().take(attack).enumerate() {
        *s = (*s as i64 * i as i64 / attack as i64) as i32;
    }
    let release = ENVELOPE_FRAMES.min(n);
    for i in 0..release {
        let idx = n - 1 - i;
        samples[idx] = (samples[idx] as i64 * i as i64 / release as i64) as i32;
    }
}

/// Sum two per-sample sequences (music bass + lead).
fn mix(a: &[i32], b: &[i32]) -> Vec<i32> {
    a.iter().zip(b.iter()).map(|(x, y)| x + y).collect()
}

fn generate_music_mono() -> Vec<i32> {
    let mut out = Vec::with_capacity(MUSIC_FRAMES);
    for step in 0..MUSIC_STEPS {
        let bass_freq = MUSIC_BASS_PATTERN[step % 8];
        let lead_freq = MUSIC_LEAD_PATTERN[step % 8];
        let bass = triangle_wave(bass_freq, MUSIC_STEP_FRAMES, MUSIC_BASS_AMPLITUDE);
        let lead = triangle_wave(lead_freq, MUSIC_STEP_FRAMES, MUSIC_LEAD_AMPLITUDE);
        out.extend(mix(&bass, &lead));
    }
    debug_assert_eq!(out.len(), MUSIC_FRAMES);
    apply_envelope(&mut out);
    out
}

fn generate_cue_mono(spec: &CueSpec) -> Vec<i32> {
    let mut out = match spec.tone {
        CueTone::One(freq) => triangle_wave(freq, spec.frames, CUE_PEAK_MAX),
        CueTone::Two(a, b) => {
            let half = spec.frames / 2;
            let first = triangle_wave(a, half, CUE_PEAK_MAX);
            let second = triangle_wave(b, spec.frames - half, CUE_PEAK_MAX);
            let mut v = first;
            v.extend(second);
            v
        }
    };
    apply_envelope(&mut out);
    out
}

fn to_i16(samples: &[i32]) -> Vec<i16> {
    samples.iter().map(|&s| s as i16).collect()
}

fn peak_of(samples: &[i32]) -> i32 {
    samples.iter().map(|&s| s.abs()).max().unwrap_or(0)
}

/// Canonical WAV bytes: RIFF/fmt(16)/data only, PCM, 48kHz s16le stereo
/// (both channels duplicated from `mono`), block align 4, byte rate 192000.
fn wav_bytes(mono: &[i16]) -> Vec<u8> {
    let num_frames = mono.len() as u32;
    let data_size = num_frames * BLOCK_ALIGN as u32;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&BYTE_RATE.to_le_bytes());
    out.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for &s in mono {
        out.extend_from_slice(&s.to_le_bytes());
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Parsed WAV header + interleaved stereo samples, for validation.
pub struct ParsedWav {
    pub audio_format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub byte_rate: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub frames: Vec<(i16, i16)>,
}

/// Parse a WAV, asserting the chunk sequence is exactly RIFF/fmt(16)/data.
pub fn parse_wav(bytes: &[u8]) -> Result<ParsedWav, AudioError> {
    let mut c = Cursor::new(bytes);
    let mut buf4 = [0u8; 4];
    read_exact(&mut c, &mut buf4, "riff tag")?;
    if &buf4 != b"RIFF" {
        return Err(AudioError::Wav("missing RIFF tag".into()));
    }
    let mut buf_u32 = [0u8; 4];
    read_exact(&mut c, &mut buf_u32, "riff size")?;
    read_exact(&mut c, &mut buf4, "wave tag")?;
    if &buf4 != b"WAVE" {
        return Err(AudioError::Wav("missing WAVE tag".into()));
    }
    read_exact(&mut c, &mut buf4, "fmt tag")?;
    if &buf4 != b"fmt " {
        return Err(AudioError::Wav(format!(
            "expected fmt chunk first, got {buf4:?}"
        )));
    }
    read_exact(&mut c, &mut buf_u32, "fmt size")?;
    if u32::from_le_bytes(buf_u32) != 16 {
        return Err(AudioError::Wav("fmt chunk is not 16 bytes (PCM)".into()));
    }
    let mut buf2 = [0u8; 2];
    read_exact(&mut c, &mut buf2, "audio format")?;
    let audio_format = u16::from_le_bytes(buf2);
    read_exact(&mut c, &mut buf2, "channels")?;
    let channels = u16::from_le_bytes(buf2);
    read_exact(&mut c, &mut buf_u32, "sample rate")?;
    let sample_rate = u32::from_le_bytes(buf_u32);
    read_exact(&mut c, &mut buf_u32, "byte rate")?;
    let byte_rate = u32::from_le_bytes(buf_u32);
    read_exact(&mut c, &mut buf2, "block align")?;
    let block_align = u16::from_le_bytes(buf2);
    read_exact(&mut c, &mut buf2, "bits per sample")?;
    let bits_per_sample = u16::from_le_bytes(buf2);
    read_exact(&mut c, &mut buf4, "data tag")?;
    if &buf4 != b"data" {
        return Err(AudioError::Wav(format!(
            "expected data chunk after fmt, got {buf4:?} (only RIFF/fmt/data allowed)"
        )));
    }
    read_exact(&mut c, &mut buf_u32, "data size")?;
    let data_size = u32::from_le_bytes(buf_u32) as usize;
    let pos = c.position() as usize;
    let end = pos + data_size;
    let data = bytes
        .get(pos..end)
        .ok_or_else(|| AudioError::Wav("data chunk shorter than declared size".into()))?;
    if end != bytes.len() {
        return Err(AudioError::Wav(
            "trailing bytes after data chunk (extra chunk present)".into(),
        ));
    }
    if !data_size.is_multiple_of(4) {
        return Err(AudioError::Wav(
            "data size not a multiple of block align".into(),
        ));
    }
    let frames = data
        .chunks_exact(4)
        .map(|f| {
            let l = i16::from_le_bytes([f[0], f[1]]);
            let r = i16::from_le_bytes([f[2], f[3]]);
            (l, r)
        })
        .collect();
    Ok(ParsedWav {
        audio_format,
        channels,
        sample_rate,
        byte_rate,
        block_align,
        bits_per_sample,
        frames,
    })
}

fn read_exact(c: &mut Cursor<&[u8]>, buf: &mut [u8], what: &str) -> Result<(), AudioError> {
    std::io::Read::read_exact(c, buf).map_err(|_| AudioError::Wav(format!("truncated: {what}")))
}

/// Assert the WAV matches the locked PCM contract.
pub fn assert_locked_format(file: &str, parsed: &ParsedWav) -> Result<(), AudioError> {
    if parsed.audio_format != 1
        || parsed.channels != CHANNELS
        || parsed.sample_rate != SAMPLE_RATE
        || parsed.byte_rate != BYTE_RATE
        || parsed.block_align != BLOCK_ALIGN
        || parsed.bits_per_sample != BITS_PER_SAMPLE
    {
        return Err(AudioError::Wav(format!(
            "{file}: wav format does not match locked PCM contract"
        )));
    }
    Ok(())
}

fn write_asset(
    out_dir: &Path,
    id: u32,
    file: &str,
    role: &str,
    mono: &[i32],
) -> Result<AudioEntry, AudioError> {
    let peak = peak_of(mono);
    let i16s = to_i16(mono);
    let bytes = wav_bytes(&i16s);
    let path = out_dir.join(file);
    fs::write(&path, &bytes).map_err(|e| AudioError::Io(e.to_string()))?;
    Ok(AudioEntry {
        id,
        file: file.to_string(),
        role: role.to_string(),
        frames: mono.len() as u32,
        rate: SAMPLE_RATE,
        channels: CHANNELS as u32,
        bits: BITS_PER_SAMPLE as u32,
        peak,
        sha256: sha256_hex(&bytes),
    })
}

fn write_manifest(out_dir: &Path, manifest: &AudioManifest) -> Result<(), AudioError> {
    let path = out_dir.join("manifest.json");
    let mut body =
        serde_json::to_string_pretty(manifest).map_err(|e| AudioError::Manifest(e.to_string()))?;
    body.push('\n');
    fs::write(path, body).map_err(|e| AudioError::Io(e.to_string()))
}

fn load_manifest(out_dir: &Path) -> Result<AudioManifest, AudioError> {
    let path = out_dir.join("manifest.json");
    let raw = fs::read_to_string(&path).map_err(|e| AudioError::Io(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| AudioError::Manifest(e.to_string()))
}

/// Generate all seven tracked assets + manifest under `out_dir`.
pub fn generate_audio(out_dir: &Path) -> Result<AudioManifest, AudioError> {
    fs::create_dir_all(out_dir).map_err(|e| AudioError::Io(e.to_string()))?;
    let mut assets = Vec::with_capacity(1 + CUES.len());
    assets.push(write_asset(
        out_dir,
        0,
        "music_placeholder.wav",
        "music",
        &generate_music_mono(),
    )?);
    for spec in &CUES {
        assets.push(write_asset(
            out_dir,
            spec.id,
            spec.file,
            spec.role,
            &generate_cue_mono(spec),
        )?);
    }
    let manifest = AudioManifest {
        version: MANIFEST_VERSION,
        generator: GENERATOR_ID.to_string(),
        license: LICENSE.to_string(),
        assets,
    };
    write_manifest(out_dir, &manifest)?;
    Ok(manifest)
}

fn scratch_dir() -> Result<PathBuf, AudioError> {
    let dir = std::env::temp_dir().join(format!(
        "mmd-audio-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).map_err(|e| AudioError::Io(e.to_string()))?;
    Ok(dir)
}

/// Verify tracked audio assets match a clean regeneration, plus per-file
/// format/duration/headroom/loop/hash invariants.
pub fn check_audio(out_dir: &Path) -> Result<(), AudioError> {
    let on_disk = load_manifest(out_dir)?;

    // Reject missing/unexpected tracked WAVs before comparing content.
    let expected_files: std::collections::BTreeSet<String> =
        on_disk.assets.iter().map(|a| a.file.clone()).collect();
    let mut found_files = std::collections::BTreeSet::new();
    for entry in fs::read_dir(out_dir).map_err(|e| AudioError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| AudioError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".wav") {
            found_files.insert(name);
        }
    }
    if found_files != expected_files {
        return Err(AudioError::Check(format!(
            "tracked wav set mismatch: on disk {found_files:?}, manifest expects {expected_files:?}"
        )));
    }

    let tmp = scratch_dir()?;
    let fresh = generate_audio(&tmp)?;
    if fresh != on_disk {
        return Err(AudioError::Check(
            "regenerated manifest differs from tracked manifest.json".into(),
        ));
    }

    let mut hashes = std::collections::HashSet::new();
    for entry in &on_disk.assets {
        let tracked =
            fs::read(out_dir.join(&entry.file)).map_err(|e| AudioError::Io(e.to_string()))?;
        let expected = sha256_hex(&tracked);
        if expected != entry.sha256 {
            return Err(AudioError::HashMismatch {
                file: entry.file.clone(),
                expected: entry.sha256.clone(),
                actual: expected,
            });
        }
        let regenerated =
            fs::read(tmp.join(&entry.file)).map_err(|e| AudioError::Io(e.to_string()))?;
        if tracked != regenerated {
            return Err(AudioError::Check(format!(
                "wav bytes drift for {}",
                entry.file
            )));
        }

        let parsed = parse_wav(&tracked)?;
        assert_locked_format(&entry.file, &parsed)?;
        if parsed.frames.len() as u32 != entry.frames {
            return Err(AudioError::Check(format!(
                "{}: frame count {} != manifest {}",
                entry.file,
                parsed.frames.len(),
                entry.frames
            )));
        }
        if !parsed.frames.iter().all(|(l, r)| l == r) {
            return Err(AudioError::Check(format!(
                "{}: left/right channels are not duplicated",
                entry.file
            )));
        }
        let peak = parsed
            .frames
            .iter()
            .map(|(l, _)| i32::from(*l).abs())
            .max()
            .unwrap_or(0);
        let peak_limit = if entry.role == "music" {
            MUSIC_PEAK_MAX
        } else {
            CUE_PEAK_MAX
        };
        if peak > peak_limit {
            return Err(AudioError::Check(format!(
                "{}: peak {peak} exceeds locked max {peak_limit}",
                entry.file
            )));
        }
        if entry.role == "music" {
            let (first_l, first_r) = parsed.frames[0];
            let (last_l, last_r) = *parsed.frames.last().expect("non-empty");
            if (first_l, first_r, last_l, last_r) != (0, 0, 0, 0) {
                return Err(AudioError::Check(format!(
                    "{}: loop endpoints are not zero",
                    entry.file
                )));
            }
        } else {
            hashes.insert(entry.sha256.clone());
        }
    }
    if hashes.len() != CUES.len() {
        return Err(AudioError::Check("cue hashes are not all distinct".into()));
    }
    Ok(())
}

/// CLI entry: write or check generated placeholder audio.
pub fn run_audio(check: bool, workspace_root: &Path) -> Result<(), AudioError> {
    let out = generated_dir(workspace_root);
    if check {
        check_audio(&out)?;
        println!("audio: ok (7 wav + manifest)");
    } else {
        let manifest = generate_audio(&out)?;
        println!(
            "audio: wrote {} wav + manifest -> {}",
            manifest.assets.len(),
            out.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_out() -> PathBuf {
        tempfile::tempdir().expect("tempdir").keep()
    }

    #[test]
    fn generated_wavs_use_locked_pcm_format() {
        let out = temp_out();
        let manifest = generate_audio(&out).expect("generate");
        for entry in &manifest.assets {
            let bytes = fs::read(out.join(&entry.file)).expect("read");
            let parsed = parse_wav(&bytes).expect("parse");
            assert_locked_format(&entry.file, &parsed).expect("locked format");
        }
    }

    #[test]
    fn generated_durations_are_exact() {
        let out = temp_out();
        let manifest = generate_audio(&out).expect("generate");
        let expect = |file: &str, frames: u32| {
            let entry = manifest
                .assets
                .iter()
                .find(|a| a.file == file)
                .expect("entry");
            assert_eq!(entry.frames, frames, "{file}");
        };
        expect("music_placeholder.wav", MUSIC_FRAMES as u32);
        expect("voice_select.wav", 5_760);
        expect("voice_move.wav", 5_760);
        expect("voice_gather.wav", 5_760);
        expect("voice_build.wav", 5_760);
        expect("voice_reject.wav", 8_640);
        expect("ui_click.wav", 2_880);
    }

    #[test]
    fn music_loop_endpoints_are_zero() {
        let out = temp_out();
        generate_audio(&out).expect("generate");
        let bytes = fs::read(out.join("music_placeholder.wav")).expect("read");
        let parsed = parse_wav(&bytes).expect("parse");
        assert_eq!(parsed.frames[0], (0, 0));
        assert_eq!(*parsed.frames.last().unwrap(), (0, 0));
    }

    #[test]
    fn cue_hashes_are_distinct() {
        let out = temp_out();
        let manifest = generate_audio(&out).expect("generate");
        let cue_hashes: std::collections::HashSet<_> = manifest
            .assets
            .iter()
            .filter(|a| a.role != "music")
            .map(|a| a.sha256.clone())
            .collect();
        assert_eq!(cue_hashes.len(), CUES.len());
    }

    #[test]
    fn source_mix_has_headroom() {
        // Worst-case concurrent playback: music + 9 voice + 4 SFX peaks.
        let sum = MUSIC_PEAK_MAX + 9 * CUE_PEAK_MAX + 4 * CUE_PEAK_MAX;
        assert!(sum <= 26_112, "sum={sum}");
    }

    #[test]
    fn double_generation_is_byte_identical() {
        let out1 = temp_out();
        let out2 = temp_out();
        let m1 = generate_audio(&out1).expect("gen1");
        let m2 = generate_audio(&out2).expect("gen2");
        assert_eq!(m1, m2);
        for entry in &m1.assets {
            let a = fs::read(out1.join(&entry.file)).expect("a");
            let b = fs::read(out2.join(&entry.file)).expect("b");
            assert_eq!(a, b, "{}", entry.file);
        }
    }

    #[test]
    fn audio_check_detects_tamper() {
        let out = temp_out();
        generate_audio(&out).expect("generate");
        check_audio(&out).expect("clean check");

        let path = out.join("ui_click.wav");
        let mut bytes = fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&path, &bytes).expect("write");
        match check_audio(&out) {
            Err(AudioError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }

        // Restore, then prove an unexpected extra wav file is also rejected.
        generate_audio(&out).expect("regenerate");
        fs::write(out.join("extra.wav"), b"not a real wav").expect("write extra");
        match check_audio(&out) {
            Err(AudioError::Check(_)) => {}
            other => panic!("expected Check error for unexpected wav, got {other:?}"),
        }
    }

    #[test]
    fn manifest_hashes_match_files() {
        let out = temp_out();
        let manifest = generate_audio(&out).expect("generate");
        for entry in &manifest.assets {
            let bytes = fs::read(out.join(&entry.file)).expect("read");
            assert_eq!(sha256_hex(&bytes), entry.sha256);
        }
    }

    #[test]
    fn triangle_wave_peak_matches_amplitude() {
        let wave = triangle_wave(440, 2_000, 1_000);
        assert_eq!(peak_of(&wave), 1_000);
    }

    #[test]
    fn stereo_channels_are_duplicated() {
        let out = temp_out();
        generate_audio(&out).expect("generate");
        let bytes = fs::read(out.join("voice_select.wav")).expect("read");
        let parsed = parse_wav(&bytes).expect("parse");
        assert!(parsed.frames.iter().all(|(l, r)| l == r));
    }
}
