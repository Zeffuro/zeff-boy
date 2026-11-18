use crate::hardware::audio::Audio;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioEffect {
    Psg { tick: u64, value: u8 },
    Opn2 { tick: u64, port: u8, value: u8 },
    Reset { tick: u64 },
}

impl AudioEffect {
    pub fn tick(self) -> u64 {
        match self {
            Self::Psg { tick, .. } | Self::Opn2 { tick, .. } | Self::Reset { tick } => tick,
        }
    }
    pub fn apply(self, audio: &mut Audio) -> Result<()> {
        ensure!(
            audio.state_now() <= self.tick(),
            "physical SOUND audio effect is late"
        );
        match self {
            Self::Psg { tick, value } => audio.write_psg(tick, value),
            Self::Opn2 { tick, port, value } => audio.write_opn2(tick, port, value),
            Self::Reset { tick } => audio.reset_opn2(tick),
        }
        Ok(())
    }
    pub fn write_state(self, w: &mut StateWriter) {
        match self {
            Self::Psg { tick, value } => {
                w.write_u8(0);
                w.write_u64(tick);
                w.write_u8(value);
            }
            Self::Opn2 { tick, port, value } => {
                w.write_u8(1);
                w.write_u64(tick);
                w.write_u8(port);
                w.write_u8(value);
            }
            Self::Reset { tick } => {
                w.write_u8(2);
                w.write_u64(tick);
            }
        }
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(match r.read_u8()? {
            0 => Self::Psg {
                tick: r.read_u64()?,
                value: r.read_u8()?,
            },
            1 => {
                let tick = r.read_u64()?;
                let port = r.read_u8()?;
                ensure!(port < 4, "invalid SOUND audio port");
                Self::Opn2 {
                    tick,
                    port,
                    value: r.read_u8()?,
                }
            }
            2 => Self::Reset {
                tick: r.read_u64()?,
            },
            _ => anyhow::bail!("invalid SOUND audio effect"),
        })
    }
}
