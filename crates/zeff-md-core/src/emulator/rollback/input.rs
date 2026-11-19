use super::{Emulator, MdRollbackConfig};
use crate::{ControllerKind, ControllerPort};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdRollbackInputs([u16; 2]);

impl MdRollbackInputs {
    pub fn word_mask(kind: ControllerKind) -> u16 {
        match kind {
            ControllerKind::ThreeButton => 0x0f4b,
            ControllerKind::SixButton => 0x0fff,
        }
    }

    pub fn new(words: [u16; 2], config: &MdRollbackConfig) -> Result<Self> {
        let inputs = Self(words);
        inputs.validate(config)?;
        Ok(inputs)
    }

    pub fn words(self) -> [u16; 2] {
        self.0
    }

    pub(super) fn validate(self, config: &MdRollbackConfig) -> Result<()> {
        for (index, (word, kind)) in self.0.into_iter().zip(config.pads).enumerate() {
            let mask = Self::word_mask(kind);
            ensure!(
                word & !mask == 0,
                "MD port {} has unsupported input bits",
                index + 1
            );
        }
        Ok(())
    }

    pub(super) fn apply(self, core: &mut Emulator) {
        let mut input = core.input_mut();
        for (port, word) in [ControllerPort::One, ControllerPort::Two]
            .into_iter()
            .zip(self.0)
        {
            input.set_host_pad(port, word as u8, (word >> 8) as u8);
        }
    }
}
