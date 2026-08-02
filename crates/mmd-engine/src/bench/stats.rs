//! Streaming / trial stats: Hyndman–Fan type-7 percentiles, median, MAD.

/// Noise rejection threshold: normalized MAD > 3% → inconclusive.
pub const NMAD_LIMIT: f64 = 0.03;

/// Consistency constant for normal MAD → σ estimate.
pub const MAD_SCALE: f64 = 1.4826;

/// Absolute gate: median trial p95 frame service (ms).
pub const GATE_P95_MS: f64 = 16.67;

/// Absolute gate: median trial p99 frame service (ms).
pub const GATE_P99_MS: f64 = 25.0;

/// Hyndman–Fan type 7 percentile on sorted ascending samples.
///
/// Continuous index `h = (n - 1) * p` (0-based). Linear interpolate.
/// Empty → NaN. Single sample → that value for any p in [0,1].
pub fn percentile_type7(sorted: &[f64], p: f64) -> f64 {
    assert!((0.0..=1.0).contains(&p), "percentile p must be in [0,1]");
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    if n == 1 {
        return sorted[0];
    }
    let h = (n as f64 - 1.0) * p;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = h - lo as f64;
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

/// Sort copy then type-7 percentile.
pub fn percentile_type7_unsorted(samples: &[f64], p: f64) -> f64 {
    let mut v = samples.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    percentile_type7(&v, p)
}

/// Median of samples (sorts a copy). Empty → NaN.
pub fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    let mut v = samples.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        // Even: average of two central — for 7 trial scalars n odd always.
        // Keep even path for general use.
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// Median absolute deviation about `med`.
pub fn mad(samples: &[f64], med: f64) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    let devs: Vec<f64> = samples.iter().map(|x| (x - med).abs()).collect();
    median(&devs)
}

/// Normalized MAD = `1.4826 × MAD / median`. NaN if median == 0 or empty.
pub fn normalized_mad(samples: &[f64]) -> f64 {
    let med = median(samples);
    if !med.is_finite() || med == 0.0 {
        return f64::NAN;
    }
    MAD_SCALE * mad(samples, med) / med
}

/// True when normalized MAD exceeds 3% noise gate.
pub fn is_noisy(samples: &[f64]) -> bool {
    let n = normalized_mad(samples);
    n.is_finite() && n > NMAD_LIMIT
}

/// Per-trial p95/p99 from frame service samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrialPercentiles {
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub sample_count: usize,
}

impl TrialPercentiles {
    pub fn from_samples(samples: &[f64]) -> Self {
        let mut v = samples.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Self {
            p95_ms: percentile_type7(&v, 0.95),
            p99_ms: percentile_type7(&v, 0.99),
            sample_count: samples.len(),
        }
    }
}

/// Aggregate of 7 (or N) trial percentile scalars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrialAggregate {
    pub median_p95_ms: f64,
    pub median_p99_ms: f64,
    pub nmad_p95: f64,
    pub nmad_p99: f64,
    pub noisy: bool,
}

impl TrialAggregate {
    /// Median of trial p95s / p99s + noise flags.
    pub fn from_trials(trials: &[TrialPercentiles]) -> Self {
        let p95s: Vec<f64> = trials.iter().map(|t| t.p95_ms).collect();
        let p99s: Vec<f64> = trials.iter().map(|t| t.p99_ms).collect();
        let median_p95_ms = median(&p95s);
        let median_p99_ms = median(&p99s);
        let nmad_p95 = normalized_mad(&p95s);
        let nmad_p99 = normalized_mad(&p99s);
        let noisy = is_noisy(&p95s) || is_noisy(&p99s);
        Self {
            median_p95_ms,
            median_p99_ms,
            nmad_p95,
            nmad_p99,
            noisy,
        }
    }
}

/// Collect frame service samples for one trial.
#[derive(Debug, Default, Clone)]
pub struct SampleBuffer {
    pub frame_service_ms: Vec<f64>,
    pub sim_ms: Vec<f64>,
    pub upload_ms: Vec<f64>,
    /// Async submit→fence completion proxy (ms). Never labeled true GPU time.
    pub gpu_queue_latency_ms: Vec<f64>,
}

impl SampleBuffer {
    /// Pre-size for measured trial (push must not alloc after reserve).
    pub fn with_capacity(frames: usize) -> Self {
        Self {
            frame_service_ms: Vec::with_capacity(frames),
            sim_ms: Vec::with_capacity(frames),
            upload_ms: Vec::with_capacity(frames),
            // Queue latency samples ≤ frames (often fewer).
            gpu_queue_latency_ms: Vec::with_capacity(frames),
        }
    }

    pub fn reserve(&mut self, frames: usize) {
        self.frame_service_ms.reserve(frames);
        self.sim_ms.reserve(frames);
        self.upload_ms.reserve(frames);
        self.gpu_queue_latency_ms.reserve(frames);
    }

    pub fn push_frame(&mut self, frame_service_ms: f64, sim_ms: f64, upload_ms: f64) {
        self.frame_service_ms.push(frame_service_ms);
        self.sim_ms.push(sim_ms);
        self.upload_ms.push(upload_ms);
    }

    pub fn push_queue_latency(&mut self, ms: f64) {
        self.gpu_queue_latency_ms.push(ms);
    }

    pub fn clear(&mut self) {
        self.frame_service_ms.clear();
        self.sim_ms.clear();
        self.upload_ms.clear();
        self.gpu_queue_latency_ms.clear();
    }

    pub fn trial_frame_service(&self) -> TrialPercentiles {
        TrialPercentiles::from_samples(&self.frame_service_ms)
    }

    pub fn component_median_ms(samples: &[f64]) -> f64 {
        median(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type7_p50_ten_samples() {
        let s: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        assert!((percentile_type7(&s, 0.5) - 5.5).abs() < 1e-12);
    }

    #[test]
    fn type7_p95_ten_samples() {
        let s: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        // h = 9 * 0.95 = 8.55 → 9 + 0.55*(10-9) = 9.55
        assert!((percentile_type7(&s, 0.95) - 9.55).abs() < 1e-12);
    }
}
