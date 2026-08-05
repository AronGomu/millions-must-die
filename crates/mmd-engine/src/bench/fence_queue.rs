//! Bounded frames-in-flight fence queue (cap = 2).
//!
//! Waits oldest fence before admitting frame when at capacity.
//! Tracks submit→complete latency as `gpu_queue_latency` proxy — never true GPU time.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use thiserror::Error;

/// Production / phase-0 allowed frames in flight.
pub const MAX_FRAMES_IN_FLIGHT: usize = 2;

/// One submitted frame awaiting fence completion.
#[derive(Debug)]
pub struct InflightFrame<F> {
    pub fence: F,
    pub submit_at: Instant,
    pub frame_index: u64,
}

/// Result of beginning a frame (may have waited on oldest).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeginFrame {
    /// Wall time spent waiting on oldest fence (ms). 0 if no wait.
    pub backpressure_wait_ms: f64,
    /// In-flight count after any wait, before this frame submits.
    pub in_flight_before_submit: usize,
}

/// Completed fence observation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompletedFrame {
    pub frame_index: u64,
    /// Submit → fence-complete duration (ms). Async queue latency proxy.
    pub gpu_queue_latency_ms: f64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FenceQueueError {
    #[error("drain incomplete: submitted={submitted} completed={completed}")]
    DrainIncomplete { submitted: u64, completed: u64 },
    #[error("in-flight exceeded cap {cap}: observed {observed}")]
    ExceededCap { cap: usize, observed: usize },
}

/// Bounded fence queue. Generic over fence token `F`.
#[derive(Debug)]
pub struct FenceQueue<F> {
    cap: usize,
    pending: VecDeque<InflightFrame<F>>,
    submitted: u64,
    completed: u64,
    max_observed_in_flight: usize,
}

impl<F> FenceQueue<F> {
    pub fn new(cap: usize) -> Self {
        assert!(cap >= 1, "cap >= 1");
        Self {
            cap,
            pending: VecDeque::with_capacity(cap),
            submitted: 0,
            completed: 0,
            max_observed_in_flight: 0,
        }
    }

    pub fn production() -> Self {
        Self::new(MAX_FRAMES_IN_FLIGHT)
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    pub fn submitted(&self) -> u64 {
        self.submitted
    }

    pub fn completed(&self) -> u64 {
        self.completed
    }

    pub fn max_observed_in_flight(&self) -> usize {
        self.max_observed_in_flight
    }

    /// If at cap, pop oldest pending frame for caller to wait.
    ///
    /// Caller must `complete_waited` after the fence signals.
    pub fn take_oldest_if_full(&mut self) -> Option<InflightFrame<F>> {
        if self.pending.len() >= self.cap {
            self.pending.pop_front()
        } else {
            None
        }
    }

    /// Record completion after caller waited on a frame from `take_oldest_if_full` or drain.
    ///
    /// Latency is returned, never accumulated here: a per-complete Vec inside
    /// the queue grows (and reallocs) on the measured frame path.
    pub fn complete_waited(&mut self, frame: InflightFrame<F>, done_at: Instant) -> CompletedFrame {
        let latency = duration_ms(frame.submit_at, done_at);
        self.completed += 1;
        CompletedFrame {
            frame_index: frame.frame_index,
            gpu_queue_latency_ms: latency,
        }
    }

    /// Begin frame accounting after any required oldest wait.
    pub fn begin_after_backpressure(&self, backpressure_wait_ms: f64) -> BeginFrame {
        BeginFrame {
            backpressure_wait_ms,
            in_flight_before_submit: self.pending.len(),
        }
    }

    /// After GPU submit: push fence. Never allows pending > cap.
    pub fn submit(&mut self, fence: F, submit_at: Instant) -> Result<(), FenceQueueError> {
        if self.pending.len() >= self.cap {
            return Err(FenceQueueError::ExceededCap {
                cap: self.cap,
                observed: self.pending.len() + 1,
            });
        }
        let frame_index = self.submitted;
        self.submitted += 1;
        self.pending.push_back(InflightFrame {
            fence,
            submit_at,
            frame_index,
        });
        self.max_observed_in_flight = self.max_observed_in_flight.max(self.pending.len());
        if self.pending.len() > self.cap {
            return Err(FenceQueueError::ExceededCap {
                cap: self.cap,
                observed: self.pending.len(),
            });
        }
        Ok(())
    }

    /// Non-blocking poll: complete any ready fences from front (in order).
    ///
    /// Prefer [`Self::poll_ready_into`] on measured paths (avoids Vec alloc).
    pub fn poll_ready<Q>(&mut self, is_ready: Q, now: Instant) -> Vec<CompletedFrame>
    where
        Q: FnMut(&F) -> bool,
    {
        let mut out = Vec::new();
        self.poll_ready_into(is_ready, now, &mut out);
        out
    }

    /// Non-blocking poll into caller buffer (`clear` then push; capacity reused).
    pub fn poll_ready_into<Q>(
        &mut self,
        mut is_ready: Q,
        now: Instant,
        out: &mut Vec<CompletedFrame>,
    ) where
        Q: FnMut(&F) -> bool,
    {
        out.clear();
        while let Some(front) = self.pending.front() {
            if !is_ready(&front.fence) {
                break;
            }
            let done = self.pending.pop_front().expect("front");
            out.push(self.complete_waited(done, now));
        }
    }

    /// Pop all remaining frames for caller drain-wait. After waits, call `finish_drain`.
    pub fn take_all_pending(&mut self) -> Vec<InflightFrame<F>> {
        self.pending.drain(..).collect()
    }

    /// After draining all pending via waits + `complete_waited`, assert counts match.
    pub fn finish_drain(&self) -> Result<(), FenceQueueError> {
        if self.submitted != self.completed {
            return Err(FenceQueueError::DrainIncomplete {
                submitted: self.submitted,
                completed: self.completed,
            });
        }
        if !self.pending.is_empty() {
            return Err(FenceQueueError::DrainIncomplete {
                submitted: self.submitted,
                completed: self.completed,
            });
        }
        Ok(())
    }

    /// Assert never exceeded cap (for tests / post-run).
    pub fn assert_cap_held(&self) -> Result<(), FenceQueueError> {
        if self.max_observed_in_flight > self.cap {
            Err(FenceQueueError::ExceededCap {
                cap: self.cap,
                observed: self.max_observed_in_flight,
            })
        } else {
            Ok(())
        }
    }
}

fn duration_ms(start: Instant, end: Instant) -> f64 {
    end.saturating_duration_since(start).as_secs_f64() * 1000.0
}

/// Helper used by tests with synthetic delayed fences.
#[derive(Debug, Clone)]
pub struct MockFence {
    pub ready_after: Instant,
}

impl MockFence {
    pub fn new_delay(from: Instant, delay: Duration) -> Self {
        Self {
            ready_after: from + delay,
        }
    }

    pub fn is_ready(&self, now: Instant) -> bool {
        now >= self.ready_after
    }

    pub fn wait(&self) {
        let now = Instant::now();
        if now < self.ready_after {
            std::thread::sleep(self.ready_after.saturating_duration_since(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_is_two() {
        assert_eq!(MAX_FRAMES_IN_FLIGHT, 2);
        assert_eq!(FenceQueue::<()>::production().cap(), 2);
    }
}
