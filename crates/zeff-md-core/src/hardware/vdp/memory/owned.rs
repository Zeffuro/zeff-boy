use super::*;
use crate::hardware::vdp::owner::runtime::MemoryWrite;
use anyhow::{Result, ensure};

impl VdpMemory {
    pub(in crate::hardware::vdp) fn validate_owned_writes(
        &self,
        writes: &[Option<MemoryWrite>],
    ) -> Result<()> {
        for write in writes.iter().flatten() {
            match *write {
                MemoryWrite::Vram { .. } => {}
                MemoryWrite::Cram { index, mask, value } => {
                    ensure!(
                        index < 64 && mask & !0xeee == 0 && value & !mask == 0,
                        "invalid physical CRAM write"
                    );
                    ensure!(
                        self.cram_is_known(usize::from(index)),
                        "unknown physical CRAM write base"
                    );
                }
                MemoryWrite::Vsram { index, mask, value } => {
                    ensure!(
                        index < 40 && mask & !0x7ff == 0 && value & !mask == 0,
                        "invalid physical VSRAM write"
                    );
                }
            }
        }
        Ok(())
    }

    pub(in crate::hardware::vdp) fn commit_owned_writes(&mut self, writes: &[Option<MemoryWrite>]) {
        for write in writes.iter().flatten() {
            match *write {
                MemoryWrite::Vram { address, value } => self.vram[usize::from(address)] = value,
                MemoryWrite::Cram { index, mask, value } => {
                    let word = &mut self.cram[usize::from(index)];
                    *word = (*word & !mask) | value;
                }
                MemoryWrite::Vsram { index, mask, value } => {
                    let word = &mut self.vsram[usize::from(index)];
                    *word = (*word & !mask) | value;
                }
            }
        }
    }
}
