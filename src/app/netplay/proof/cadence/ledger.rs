use anyhow::{Result, bail, ensure};
use zeff_netplay::wire::Message;

const MAX_FRAMES: u64 = 1000;
const MAX_PCM_SAMPLES: usize = 4096;

pub(in crate::app::netplay::proof) struct Confirmed {
    pub(in crate::app::netplay::proof) checkpoint: Message,
    pub(in crate::app::netplay::proof) ports: [u16; 2],
    pub(in crate::app::netplay::proof) pcm_bits: Vec<u32>,
}

impl Confirmed {
    pub(in crate::app::netplay::proof) fn check(
        &self,
        checkpoint: &Message,
        ports: [u16; 2],
        audio: &[f32],
    ) -> Result<()> {
        ensure!(self.checkpoint == *checkpoint, "cadence checkpoint differs");
        ensure!(self.ports == ports, "cadence delayed ports differ");
        ensure!(
            self.pcm_bits
                .iter()
                .copied()
                .eq(audio.iter().map(|sample| sample.to_bits())),
            "cadence raw PCM bits differ"
        );
        Ok(())
    }
}

pub(in crate::app::netplay::proof) struct Ledger {
    target: u64,
    inputs: Vec<u16>,
    records: Vec<Confirmed>,
    error: Option<String>,
}

impl Ledger {
    pub(in crate::app::netplay::proof) fn new(target: u64) -> Result<Self> {
        ensure!(
            (1..=MAX_FRAMES).contains(&target),
            "cadence ledger target must be 1..1000"
        );
        Ok(Self {
            target,
            inputs: Vec::with_capacity(target as usize),
            records: Vec::with_capacity(target as usize),
            error: None,
        })
    }

    pub(in crate::app::netplay::proof) fn record_input(
        &mut self,
        frame: u64,
        raw: u16,
    ) -> Result<()> {
        self.check_error()?;
        if frame >= self.target {
            return self.fail(format!("local sample frame {frame} exceeds target"));
        }
        let next = self.inputs.len() as u64;
        if frame == next {
            self.inputs.push(raw);
        } else if frame.checked_add(1) == Some(next) {
            if self.inputs.last().copied() != Some(raw) {
                return self.fail(format!("local sample retry changed at frame {frame}"));
            }
        } else {
            return self.fail(format!(
                "local sample frame {frame} is stale or gapped; next is {next}"
            ));
        }
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn record_frame(
        &mut self,
        checkpoint: &Message,
        ports: [u16; 2],
        audio: &[f32],
    ) -> Result<()> {
        self.check_error()?;
        let Message::Checkpoint { frame, .. } = checkpoint else {
            return self.fail("confirmed response lacks a checkpoint".into());
        };
        if *frame == 0 || *frame > self.target {
            return self.fail(format!("confirmed frame {frame} exceeds ledger bounds"));
        }
        let next = self.records.len() as u64 + 1;
        if *frame != next {
            return self.fail(format!(
                "confirmed frame {frame} is duplicate or gapped; next is {next}"
            ));
        }
        if audio.len() > MAX_PCM_SAMPLES {
            return self.fail(format!("confirmed frame {frame} exceeds PCM sample limit"));
        }
        self.records.push(Confirmed {
            checkpoint: checkpoint.clone(),
            ports,
            pcm_bits: audio.iter().map(|sample| sample.to_bits()).collect(),
        });
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn record_audio(&mut self) -> Result<()> {
        self.check_error()?;
        self.fail("audio-only confirmation lacks a checkpoint".into())
    }

    pub(in crate::app::netplay::proof) fn check_complete(&self) -> Result<()> {
        self.check_error()?;
        ensure!(
            self.inputs.len() as u64 == self.target,
            "cadence local samples missing"
        );
        ensure!(
            self.records.len() as u64 == self.target,
            "cadence confirmed records missing"
        );
        Ok(())
    }

    pub(in crate::app::netplay::proof) fn inputs(&self) -> &[u16] {
        &self.inputs
    }

    pub(in crate::app::netplay::proof) fn records(&self) -> &[Confirmed] {
        &self.records
    }

    pub(in crate::app::netplay::proof) fn check_error(&self) -> Result<()> {
        if let Some(error) = &self.error {
            bail!("cadence ledger: {error}");
        }
        Ok(())
    }

    fn fail(&mut self, error: String) -> Result<()> {
        if self.error.is_none() {
            self.error = Some(error);
        }
        self.check_error()
    }
}

#[cfg(test)]
mod tests;
