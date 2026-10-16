use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

const ALL: u8 = (1 << OPERATORS) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Routing {
    prepared: [u8; OPERATORS],
    pub(super) sampled: [u8; OPERATORS],
    prepared_valid: u8,
    sampled_valid: u8,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            prepared: [0; OPERATORS],
            sampled: [0; OPERATORS],
            prepared_valid: ALL,
            sampled_valid: ALL,
        }
    }
}

impl Routing {
    pub(super) fn write_state(&self, w: &mut Writer) {
        w.write_bytes(&self.prepared);
        w.write_bytes(&self.sampled);
        w.write_u8(self.prepared_valid);
        w.write_u8(self.sampled_valid);
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut routing = Self::default();
        r.read_exact(&mut routing.prepared)?;
        r.read_exact(&mut routing.sampled)?;
        routing.prepared_valid = r.read_u8()?;
        routing.sampled_valid = r.read_u8()?;
        ensure!(
            routing
                .prepared
                .iter()
                .chain(&routing.sampled)
                .all(|mode| *mode <= 63)
                && routing.prepared_valid <= ALL
                && routing.sampled_valid <= ALL,
            "invalid FM modulation routing"
        );
        Ok(routing)
    }
}

#[cfg(test)]
mod tests;
