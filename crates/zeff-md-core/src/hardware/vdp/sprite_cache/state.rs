use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpSpriteCache {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        for words in self.entries {
            for word in words {
                write_optional(w, word, StateWriter::write_u16);
            }
        }
    }

    pub(in crate::hardware::vdp) fn read_state_version(
        r: &mut StateReader<'_>,
        version: u32,
    ) -> Result<Self> {
        let mut result = Self::default();
        for words in &mut result.entries {
            for (index, word) in words.iter_mut().enumerate() {
                *word = read_optional(r, StateReader::read_u16)?;
                let mask = if index == 0 {
                    if version >= crate::save_state::STATE_VERSION_WITH_INTERLACE {
                        SPRITE_CACHED_POSITION_MASK
                    } else {
                        SPRITE_POSITION_MASK
                    }
                } else {
                    SPRITE_LAYOUT_MASK
                };
                ensure!(
                    word.is_none_or(|value| value & !mask == 0),
                    "invalid VDP sprite cache word"
                );
            }
        }
        Ok(result)
    }
}
