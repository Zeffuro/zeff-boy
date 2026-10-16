use super::*;
use anyhow::{Result, bail, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Operator {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        self.write_prefix(w);
        w.write_bool(self.keyed);
        self.pipeline.write_state(w);
        self.ssg.write_state(w);
    }

    fn write_prefix(&self, w: &mut Writer) {
        w.write_bytes(&self.parameters);
        w.write_u32(self.phase);
        w.write_u16(self.attenuation);
        w.write_u8(match self.envelope {
            Envelope::Off => 0,
            Envelope::Attack => 1,
            Envelope::Decay => 2,
            Envelope::Sustain => 3,
            Envelope::Release => 4,
        });
    }

    #[cfg(test)]
    pub(crate) fn write_state_version(&self, w: &mut Writer, version: u32) {
        self.write_prefix(w);
        if version < crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK {
            w.write_u32(0);
            w.write_u8(0);
        }
        w.write_bool(self.keyed);
        if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
            self.pipeline.write_state_version(w, version);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_SSG_EG {
            self.ssg.write_state(w);
        }
    }

    #[cfg(test)]
    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        Self::read_state_version(r, crate::save_state::STATE_VERSION)
    }

    pub(crate) fn read_state_version(r: &mut Reader<'_>, version: u32) -> Result<Self> {
        let mut parameters = [0; 7];
        r.read_exact(&mut parameters)?;
        let phase = r.read_u32()?;
        let attenuation = r.read_u16()?;
        let envelope = match r.read_u8()? {
            0 => Envelope::Off,
            1 => Envelope::Attack,
            2 => Envelope::Decay,
            3 => Envelope::Sustain,
            4 => Envelope::Release,
            _ => bail!("invalid FM envelope"),
        };
        if version < crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK {
            ensure!(r.read_u32()? < 1 << 14, "invalid FM envelope rate phase");
            ensure!(r.read_u8()? < ENVELOPE_DIVIDER, "invalid FM envelope phase");
        }
        let keyed = r.read_bool()?;
        let pipeline = if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
            Pipeline::read_state_version(r, version)?
        } else {
            Pipeline::migrated(keyed)
        };
        let ssg = if version >= crate::save_state::STATE_VERSION_WITH_SSG_EG {
            ssg::Ssg::read_state(r)?
        } else {
            ssg::Ssg::default()
        };
        ensure!(phase <= PHASE_MASK, "invalid FM operator phase");
        ensure!(attenuation <= ENVELOPE_MAX, "invalid FM attenuation");
        Ok(Self {
            parameters,
            phase,
            attenuation,
            envelope,
            keyed,
            pipeline,
            ssg,
        })
    }
}

#[cfg(test)]
mod tests;
