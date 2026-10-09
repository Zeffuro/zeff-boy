use std::time::{Duration, Instant};

use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use zeff_netplay::rollback::InputDelay;

pub(in crate::app::netplay::proof) struct Trace {
    target: u64,
    delay: u64,
    inputs: Vec<Instant>,
    confirmed: Vec<Instant>,
    handed_off: Vec<Instant>,
    last_at: Option<Instant>,
    error: Option<String>,
}

impl Trace {
    pub(in crate::app::netplay::proof) fn new(target: u64, delay: InputDelay) -> Result<Self> {
        ensure!(
            (1..=1000).contains(&target),
            "latency target must be 1..1000"
        );
        Ok(Self {
            target,
            delay: delay.frames(),
            inputs: Vec::with_capacity(target as usize),
            confirmed: Vec::with_capacity(target as usize),
            handed_off: Vec::with_capacity(target as usize),
            last_at: None,
            error: None,
        })
    }

    pub(in crate::app::netplay::proof) fn record_input(
        &mut self,
        frame: u64,
        at: Instant,
    ) -> Result<()> {
        self.check_at(at)?;
        let next = self.inputs.len() as u64;
        if frame >= self.target || (frame != next && frame.checked_add(1) != Some(next)) {
            return self.fail("input timestamp frame is stale, gapped or out of bounds");
        }
        if frame == next {
            self.inputs.push(at);
        }
        self.last_at = Some(at);
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn record_confirmed(
        &mut self,
        frame: u64,
        at: Instant,
    ) -> Result<()> {
        self.check_at(at)?;
        if frame > self.target || frame != self.confirmed.len() as u64 + 1 {
            return self.fail("confirmed timestamp frame is duplicate, gapped or out of bounds");
        }
        self.confirmed.push(at);
        self.last_at = Some(at);
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn record_handoff(
        &mut self,
        frame: u64,
        at: Instant,
    ) -> Result<()> {
        self.check_at(at)?;
        if frame <= self.handed_off.len() as u64 || frame > self.confirmed.len() as u64 {
            return self.fail("audio handoff is duplicate or precedes confirmation");
        }
        self.handed_off.resize(frame as usize, at);
        self.last_at = Some(at);
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn report(
        &self,
        presented: &[(u64, Instant)],
        warmup: u64,
    ) -> Result<Value> {
        self.check_error()?;
        for count in [
            self.inputs.len(),
            self.confirmed.len(),
            self.handed_off.len(),
            presented.len(),
        ] {
            ensure!(count as u64 == self.target, "latency observations missing");
        }
        ensure!(
            warmup < self.target.saturating_sub(self.delay),
            "no paired latency samples"
        );
        for (index, &(frame, at)) in presented.iter().enumerate() {
            ensure!(frame == index as u64 + 1, "latency presentation frame gap");
            if index != 0 {
                ensure!(at >= presented[index - 1].1, "presentation clock regressed");
            }
        }
        let mut predicted = Vec::new();
        let mut confirmed = Vec::new();
        let mut handoff = Vec::new();
        let mut prebuffer = Vec::new();
        for (index, &input) in self
            .inputs
            .iter()
            .enumerate()
            .take((self.target - self.delay) as usize)
        {
            // A sample at f takes effect in the completed frame f + delay + 1.
            let effect = index + self.delay as usize;
            let prediction = elapsed(input, presented[effect].1)?;
            let confirmation = elapsed(input, self.confirmed[effect])?;
            let output = elapsed(input, self.handed_off[effect])?;
            let buffering = elapsed(self.confirmed[effect], self.handed_off[effect])?;
            if index as u64 >= warmup {
                predicted.push(prediction);
                confirmed.push(confirmation);
                handoff.push(output);
                prebuffer.push(buffering);
            }
        }
        Ok(json!({
            "boundary": "successful App command enqueue to effect-frame responses and PCM handoff",
            "effect_completed_frame": "sample_frame + input_delay + 1",
            "warmup_input_samples": warmup,
            "tail_input_samples_without_effect": self.delay.min(self.target),
            "paired_samples": predicted.len(),
            "input_to_predicted_response": summary(predicted),
            "input_to_confirmed_pcm": summary(confirmed),
            "input_to_audio_handoff": summary(handoff),
            "confirmed_pcm_to_audio_handoff": summary(prebuffer),
            "limits": "headless App polling and proof work included, OS input, rendering, audio backend queues and physical audio excluded",
        }))
    }

    fn check_at(&mut self, at: Instant) -> Result<()> {
        self.check_error()?;
        if self.last_at.is_some_and(|last| at < last) {
            return self.fail("observation clock regressed");
        }
        Ok(())
    }

    fn check_error(&self) -> Result<()> {
        if let Some(error) = &self.error {
            bail!("latency trace: {error}");
        }
        Ok(())
    }

    fn fail(&mut self, error: &str) -> Result<()> {
        self.error.get_or_insert_with(|| error.to_owned());
        self.check_error()
    }
}

fn elapsed(from: Instant, to: Instant) -> Result<Duration> {
    to.checked_duration_since(from)
        .ok_or_else(|| anyhow::anyhow!("effect observation precedes input or confirmation"))
}

fn summary(samples: Vec<Duration>) -> Value {
    let mut sorted: Vec<_> = samples
        .iter()
        .map(|value| value.as_secs_f64() * 1000.0)
        .collect();
    sorted.sort_by(f64::total_cmp);
    json!({
        "samples": sorted.len(),
        "p50_ms": super::quantile(&sorted, 50),
        "p95_ms": super::quantile(&sorted, 95),
        "p99_ms": super::quantile(&sorted, 99),
        "max_ms": sorted.last().unwrap(),
    })
}

#[cfg(test)]
mod tests;
