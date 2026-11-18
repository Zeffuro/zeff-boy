use crate::hardware::vdp::control_core::PhaseLevels;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Capture {
    lead: bool,
    delay: [u8; 2],
    pub bytes: [u8; 4],
}

impl Capture {
    pub fn sample(&mut self, slave: bool, serial: u8) {
        if slave {
            let gates = (self.delay[1] << 1) | u8::from(self.lead);
            for (index, byte) in self.bytes.iter_mut().enumerate() {
                if gates & (1 << index) != 0 {
                    *byte = serial;
                }
            }
        }
    }

    pub fn clock(&mut self, phases: PhaseLevels, trigger: bool, stages: u8) {
        if phases.data_master {
            self.delay[1] = self.delay[0];
            self.lead = phases.horizontal_master && trigger;
        }
        if phases.data_slave {
            self.delay[0] = ((self.delay[1] << 1) | u8::from(self.lead)) & ((1 << stages) - 1);
        }
    }

    pub fn complete(self) -> bool {
        self.delay[1] & 8 != 0
    }

    pub fn gate(self, index: usize) -> bool {
        if index == 0 {
            self.lead
        } else {
            self.delay[1] & (1 << (index - 1)) != 0
        }
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_bool(self.lead);
        writer.write_bytes(&self.delay);
        writer.write_bytes(&self.bytes);
    }

    pub fn read_state(reader: &mut StateReader<'_>, stages: u8) -> Result<Self> {
        let lead = reader.read_bool()?;
        let delay = [reader.read_u8()?, reader.read_u8()?];
        ensure!(
            delay.into_iter().all(|value| value < 1 << stages),
            "invalid render serial capture delay"
        );
        let mut bytes = [0; 4];
        reader.read_exact(&mut bytes)?;
        Ok(Self { lead, delay, bytes })
    }
}
