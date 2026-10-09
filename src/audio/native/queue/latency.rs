use super::*;
use cpal::{OutputStreamTimestamp, StreamInstant};
use serde_json::{Value, json};

const MARKERS: usize = 120;

#[derive(Clone, Copy)]
struct Span {
    id: usize,
    frames: usize,
    callback: StreamInstant,
    playback: StreamInstant,
}

struct Observer {
    output: rtrb::Producer<Span>,
    active: Option<Span>,
    previous_callback: Option<StreamInstant>,
    errors: Arc<AtomicU64>,
    rate: u32,
}

impl Observer {
    fn observe(&mut self, pcm: &[f32], channels: usize, stamp: OutputStreamTimestamp) {
        if channels == 0
            || !pcm.len().is_multiple_of(channels)
            || stamp
                .playback
                .checked_duration_since(stamp.callback)
                .is_none()
            || self
                .previous_callback
                .is_some_and(|previous| stamp.callback < previous)
        {
            self.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.previous_callback = Some(stamp.callback);
        for (offset, frame) in pcm.chunks_exact(channels).enumerate() {
            let value = frame[0];
            if !value.is_finite()
                || !(0.0..=MARKERS as f32).contains(&value)
                || value.fract() != 0.0
                || frame.iter().any(|&channel| channel != value)
            {
                self.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let id = value as usize;
            if self.active.is_some_and(|span| span.id != id)
                && self.output.push(self.active.take().unwrap()).is_err()
            {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
            if id == 0 {
                continue;
            }
            if self.active.is_none() {
                let Some(playback) = frames_duration(offset, self.rate)
                    .and_then(|delta| stamp.playback.checked_add(delta))
                else {
                    self.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                self.active = Some(Span {
                    id,
                    frames: 0,
                    callback: stamp.callback,
                    playback,
                });
            }
            self.active.as_mut().unwrap().frames += 1;
        }
    }
}

fn frames_duration(frames: usize, rate: u32) -> Option<Duration> {
    (rate != 0).then_some(())?;
    let nanos = (frames as u64).checked_mul(1_000_000_000)? / u64::from(rate);
    Some(Duration::from_nanos(nanos))
}

fn marker_frames(index: usize, rate: u32, hz: u32) -> usize {
    ((index + 1) * rate as usize / hz as usize) - (index * rate as usize / hz as usize)
}

fn observer(rate: u32, records: usize) -> (Observer, rtrb::Consumer<Span>, Arc<AtomicU64>) {
    let (output, records) = rtrb::RingBuffer::new(records);
    let errors = Arc::new(AtomicU64::new(0));
    (
        Observer {
            output,
            active: None,
            previous_callback: None,
            errors: Arc::clone(&errors),
            rate,
        },
        records,
        errors,
    )
}

fn summarize(
    queued: &[StreamInstant],
    records: &[Span],
    rate: u32,
    hz: u32,
    warmup: usize,
) -> anyhow::Result<Value> {
    anyhow::ensure!(
        queued.len() == MARKERS && records.len() == MARKERS,
        "marker capture incomplete"
    );
    anyhow::ensure!(warmup < MARKERS, "marker warmup excludes every sample");
    let mut callback = Vec::new();
    let mut playback = Vec::new();
    let mut overlaps = 0;
    for (index, span) in records.iter().enumerate() {
        anyhow::ensure!(span.id == index + 1, "marker order duplicated or gapped");
        anyhow::ensure!(
            span.frames == marker_frames(index, rate, hz),
            "marker PCM frame count differs"
        );
        if index != 0 {
            anyhow::ensure!(
                queued[index] >= queued[index - 1],
                "enqueue clock regressed"
            );
            anyhow::ensure!(
                span.callback >= records[index - 1].callback,
                "callback clock regressed"
            );
            anyhow::ensure!(
                span.playback >= records[index - 1].playback,
                "playback clock regressed"
            );
        }
        let predicted = span
            .playback
            .checked_duration_since(queued[index])
            .ok_or_else(|| anyhow::anyhow!("predicted playback precedes enqueue"))?;
        // The callback clock is sampled before a concurrently admitted marker can be read.
        let callback_ms = match span.callback.checked_duration_since(queued[index]) {
            Some(delta) => delta.as_secs_f64() * 1000.0,
            None => {
                overlaps += 1;
                -queued[index]
                    .checked_duration_since(span.callback)
                    .unwrap()
                    .as_secs_f64()
                    * 1000.0
            }
        };
        if index >= warmup {
            callback.push(callback_ms);
            playback.push(predicted.as_secs_f64() * 1000.0);
        }
    }
    Ok(json!({
        "markers": MARKERS, "warmup_markers": warmup, "sample_rate": rate, "cadence_hz": hz,
        "all_marker_callback_timestamp_overlaps": overlaps,
        "queue_to_callback_clock": quantiles(callback),
        "queue_to_host_predicted_playback": quantiles(playback),
    }))
}

fn quantiles(mut sorted: Vec<f64>) -> Value {
    sorted.sort_by(f64::total_cmp);
    let at = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    json!({"samples": sorted.len(), "p50_ms": at(50), "p95_ms": at(95), "p99_ms": at(99), "min_ms": sorted[0], "max_ms": sorted.last().unwrap()})
}

mod host;
mod tests;
