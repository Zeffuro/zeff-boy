use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Channel {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        for operator in self.operators {
            operator.write_state(w);
        }
        self.write_tail(w, crate::save_state::STATE_VERSION);
    }

    #[cfg(test)]
    pub(crate) fn write_state_version(&self, w: &mut Writer, version: u32) {
        for operator in self.operators {
            operator.write_state_version(w, version);
        }
        self.write_tail(w, version);
    }

    fn write_tail(&self, w: &mut Writer, version: u32) {
        w.write_u16(self.frequency.0);
        w.write_u8(self.frequency.1);
        w.write_u8(self.algorithm);
        w.write_u8(self.feedback);
        w.write_u8(self.stereo);
        for value in self.history {
            w.write_u32(value as u32);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PIPELINE {
            w.write_u32(self.previous_s2 as u32);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            self.render.write_state(w);
        } else if version >= crate::save_state::STATE_VERSION_WITH_MODULATION_ROUTING {
            Routing::default().write_state(w);
        }
    }

    pub(crate) fn read_state(r: &mut Reader<'_>, version: u32) -> Result<Self> {
        let mut operators = [Operator::default(); OPERATORS];
        for operator in &mut operators {
            *operator = Operator::read_state_version(r, version)?;
        }
        let frequency = (r.read_u16()?, r.read_u8()?);
        let algorithm = r.read_u8()?;
        let feedback = r.read_u8()?;
        let stereo = r.read_u8()?;
        let history = [r.read_u32()? as i32, r.read_u32()? as i32];
        let previous_s2 = if version >= crate::save_state::STATE_VERSION_WITH_FM_PIPELINE {
            r.read_u32()? as i32
        } else {
            0
        };
        let render = if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            Render::read_state(r)?
        } else {
            if version >= crate::save_state::STATE_VERSION_WITH_MODULATION_ROUTING {
                Routing::read_state(r)?;
            }
            Render::migrated()
        };
        ensure!(
            frequency.0 <= 0x7ff && frequency.1 <= 7,
            "invalid FM frequency"
        );
        ensure!(
            algorithm <= 7 && feedback <= 7 && stereo & !0xf7 == 0,
            "invalid FM channel parameters"
        );
        let peak = super::super::constants::OPERATOR_PEAK;
        ensure!(
            history
                .iter()
                .chain([&previous_s2])
                .all(|v| (-peak..=peak).contains(v)),
            "invalid FM feedback history"
        );
        Ok(Self {
            operators,
            frequency,
            algorithm,
            feedback,
            stereo,
            history,
            previous_s2,
            render,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_operator_pipeline_memory_rejects() {
        let channel = Channel {
            previous_s2: super::super::super::constants::OPERATOR_PEAK + 1,
            ..Channel::default()
        };
        let mut writer = Writer::new();
        channel.write_state(&mut writer);
        assert!(
            Channel::read_state(
                &mut Reader::new(&writer.into_bytes()),
                crate::save_state::STATE_VERSION
            )
            .is_err()
        );
    }
}
