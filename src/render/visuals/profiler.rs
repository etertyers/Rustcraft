//! The GPU profiler (M54): a timestamp at the start and end of every
//! render pass (Apple GPUs can't time inside a pass, so each effect gets
//! a pass of its own), read back a couple of frames later without
//! stalling. Needs wgpu's `TIMESTAMP_QUERY`; without it the profiler is
//! off and effects are measured by turning them off and comparing
//! `--bench` frame times.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

/// Passes timed in one frame, at most.
pub const MAX_PASSES: u32 = 32;
/// Read-back buffers in flight: results arrive two or three frames late.
const SLOTS: usize = 3;
/// Frames each average in F3 covers.
pub const AVERAGE_FRAMES: usize = 60;

const IDLE: u8 = 0;
const MAPPING: u8 = 1;
const MAPPED: u8 = 2;

struct Slot {
    buffer: wgpu::Buffer,
    /// The passes written into it, in order.
    names: Vec<&'static str>,
    state: Arc<AtomicU8>,
}

pub struct GpuProfiler {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    /// Nanoseconds per timestamp tick.
    period: f64,
    /// This frame's slot and passes, while recording.
    current: Option<usize>,
    names: Vec<&'static str>,
    /// The last `AVERAGE_FRAMES` times of each pass, in ms.
    history: BTreeMap<&'static str, VecDeque<f64>>,
    /// Since `reset_totals`: each pass's total ms and frames, for
    /// `--perf-test`.
    totals: BTreeMap<&'static str, (f64, u64)>,
    /// The latest frame read back: (pass, ms) in draw order.
    latest: Vec<(&'static str, f64)>,
    /// The GPU time of each of the last frames from its first pass's start
    /// to its last pass's end (passes overlap on a tile-based GPU, so the
    /// passes' own times add up to more than this), and since
    /// `reset_totals`: their sum and count.
    spans: VecDeque<f64>,
    span_total: (f64, u64),
}

impl GpuProfiler {
    /// The profiler, if the device has timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let count = MAX_PASSES * 2;
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu profiler"),
            ty: wgpu::QueryType::Timestamp,
            count,
        });
        let size = u64::from(count) * 8;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu profiler resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gpu profiler read-back"),
                    size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                names: Vec::new(),
                state: Arc::new(AtomicU8::new(IDLE)),
            })
            .collect();
        Some(Self {
            query_set,
            resolve,
            slots,
            period: f64::from(queue.get_timestamp_period()),
            current: None,
            names: Vec::new(),
            history: BTreeMap::new(),
            totals: BTreeMap::new(),
            latest: Vec::new(),
            spans: VecDeque::new(),
            span_total: (0.0, 0),
        })
    }

    /// Starts a frame: picks a free read-back slot (if none is free this
    /// frame isn't timed).
    pub fn begin_frame(&mut self) {
        self.names.clear();
        self.current = self
            .slots
            .iter()
            .position(|slot| slot.state.load(Ordering::Acquire) == IDLE);
    }

    /// The timestamp writes for a pass named `name`, if this frame is timed.
    pub fn pass(&mut self, name: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.current?;
        let index = self.names.len() as u32;
        if index >= MAX_PASSES {
            return None;
        }
        self.names.push(name);
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(index * 2),
            end_of_pass_write_index: Some(index * 2 + 1),
        })
    }

    /// Resolves this frame's timestamps into its read-back slot.
    pub fn end_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(slot) = self.current else {
            return;
        };
        let count = self.names.len() as u32 * 2;
        if count == 0 {
            self.current = None;
            return;
        }
        encoder.resolve_query_set(&self.query_set, 0..count, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve,
            0,
            &self.slots[slot].buffer,
            0,
            Some(u64::from(count) * 8),
        );
        self.slots[slot].names = std::mem::take(&mut self.names);
    }

    /// After the frame's commands are submitted: starts reading them back.
    pub fn after_submit(&mut self) {
        let Some(slot) = self.current.take() else {
            return;
        };
        let slot = &self.slots[slot];
        let state = slot.state.clone();
        state.store(MAPPING, Ordering::Release);
        let bytes = slot.names.len() as u64 * 16;
        slot.buffer
            .slice(..bytes)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(
                    if result.is_ok() { MAPPED } else { IDLE },
                    Ordering::Release,
                );
            });
    }

    /// Takes in every frame that has been read back (polling the device
    /// without waiting).
    pub fn collect(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        for i in 0..self.slots.len() {
            if self.slots[i].state.load(Ordering::Acquire) != MAPPED {
                continue;
            }
            let (times, span) = {
                let slot = &self.slots[i];
                let bytes = slot.names.len() as u64 * 16;
                let Ok(data) = slot.buffer.slice(..bytes).get_mapped_range() else {
                    continue;
                };
                let stamps: &[u64] = bytemuck::cast_slice(&data);
                // First start to last end.
                let first = stamps.iter().step_by(2).copied().min().unwrap_or(0);
                let last = stamps.iter().skip(1).step_by(2).copied().max().unwrap_or(0);
                let span = last.saturating_sub(first) as f64 * self.period / 1.0e6;
                let times = slot
                    .names
                    .iter()
                    .enumerate()
                    .map(|(n, name)| {
                        let ticks = stamps[n * 2 + 1].saturating_sub(stamps[n * 2]);
                        (*name, ticks as f64 * self.period / 1.0e6)
                    })
                    .collect::<Vec<_>>();
                (times, span)
            };
            self.record_span(span);
            self.slots[i].buffer.unmap();
            self.slots[i].state.store(IDLE, Ordering::Release);
            self.record(&times);
        }
    }

    fn record_span(&mut self, ms: f64) {
        self.spans.push_back(ms);
        if self.spans.len() > AVERAGE_FRAMES {
            self.spans.pop_front();
        }
        self.span_total.0 += ms;
        self.span_total.1 += 1;
    }

    /// The frame's GPU time over the last `AVERAGE_FRAMES` frames, in ms.
    pub fn span_average(&self) -> f64 {
        self.spans.iter().sum::<f64>() / self.spans.len().max(1) as f64
    }

    fn record(&mut self, times: &[(&'static str, f64)]) {
        // Passes with the same name in one frame (shadow cascades) add up.
        let mut frame: Vec<(&'static str, f64)> = Vec::new();
        for (name, ms) in times {
            match frame.iter_mut().find(|(n, _)| n == name) {
                Some((_, total)) => *total += ms,
                None => frame.push((name, *ms)),
            }
        }
        for (name, ms) in &frame {
            let history = self.history.entry(name).or_default();
            history.push_back(*ms);
            if history.len() > AVERAGE_FRAMES {
                history.pop_front();
            }
            let total = self.totals.entry(name).or_default();
            total.0 += ms;
            total.1 += 1;
        }
        // Passes that stopped being drawn (an effect turned off) drop out.
        let names: Vec<&'static str> = frame.iter().map(|(n, _)| *n).collect();
        self.history.retain(|name, _| names.contains(name));
        self.latest = frame;
    }

    /// Each pass drawn in the latest timed frame with its average over the
    /// last `AVERAGE_FRAMES` frames, in ms, in draw order.
    pub fn averages(&self) -> Vec<(&'static str, f64)> {
        self.latest
            .iter()
            .map(|(name, _)| {
                let h = &self.history[name];
                (*name, h.iter().sum::<f64>() / h.len().max(1) as f64)
            })
            .collect()
    }

    /// Forgets the totals (the end of `--perf-test`'s warm-up).
    pub fn reset_totals(&mut self) {
        self.totals.clear();
        self.span_total = (0.0, 0);
    }

    /// Each pass's mean ms per frame since `reset_totals`, and the mean
    /// GPU time of a whole frame (first pass start to last pass end).
    pub fn means(&self) -> (BTreeMap<String, f64>, f64) {
        let means: BTreeMap<String, f64> = self
            .totals
            .iter()
            .map(|(name, (ms, n))| (name.to_string(), ms / (*n).max(1) as f64))
            .collect();
        (means, self.span_total.0 / self.span_total.1.max(1) as f64)
    }
}

/// The F3 lines for `averages`.
pub fn overlay_lines(averages: &[(&'static str, f64)], frame: f64) -> Vec<String> {
    let mut lines = vec![format!(
        "GPU {frame:.2} ms (avg of {AVERAGE_FRAMES} frames)"
    )];
    lines.extend(
        averages
            .iter()
            .map(|(name, ms)| format!("  {name}: {ms:.2} ms")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_shows_the_frame_and_each_pass() {
        let lines = overlay_lines(&[("world", 2.0), ("post", 0.25)], 2.1);
        assert_eq!(lines[0], "GPU 2.10 ms (avg of 60 frames)");
        assert_eq!(lines[2], "  post: 0.25 ms");
    }
}
