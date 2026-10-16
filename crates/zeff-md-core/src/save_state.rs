use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const STATE_MAGIC: &[u8; 8] = b"ZMDSTATE";
pub(crate) const MIN_STATE_VERSION: u32 = 1;
pub(crate) const STATE_VERSION_WITH_SERIAL_IO: u32 = 2;
pub(crate) const STATE_VERSION_WITH_FM_PIPELINE: u32 = 3;
pub(crate) const STATE_VERSION_WITH_ENVELOPE_CLOCK: u32 = 4;
pub(crate) const STATE_VERSION_WITH_ENVELOPE_CARRY: u32 = 5;
pub(crate) const STATE_VERSION_WITH_SLOT_SCHEDULER: u32 = 6;
pub(crate) const STATE_VERSION_WITH_MODULATION_ROUTING: u32 = 7;
pub(crate) const STATE_VERSION_WITH_LFO_LATCH: u32 = 8;
pub(crate) const STATE_VERSION_WITH_FM_PUBLICATION: u32 = 9;
pub(crate) const STATE_VERSION_WITH_SSG_EG: u32 = 10;
pub(crate) const STATE_VERSION_WITH_CSM_LEVEL: u32 = 11;
pub(crate) const STATE_VERSION_WITH_CSM_TIMER: u32 = 12;
pub(crate) const STATE_VERSION_WITH_TIMER_B_PHASE: u32 = 13;
pub(crate) const STATE_VERSION_WITH_INTERLACE: u32 = 14;
pub(crate) const STATE_VERSION_WITH_SAVE_RAM: u32 = 15;
pub(crate) const STATE_VERSION: u32 = STATE_VERSION_WITH_SAVE_RAM;
pub(crate) use crate::constants::MAX_STATE_BYTES;

pub(crate) fn write_section(
    writer: &mut StateWriter,
    encode: impl FnOnce(&mut StateWriter) -> Result<()>,
) -> Result<()> {
    let mut result = Ok(());
    writer.write_section(|writer| result = encode(writer));
    result
}

pub(crate) fn read_section(
    reader: &mut StateReader<'_>,
    decode: impl FnOnce(&mut StateReader<'_>) -> Result<()>,
) -> Result<()> {
    let mut section = StateReader::new(reader.read_slice(MAX_STATE_BYTES)?);
    decode(&mut section)?;
    ensure!(section.is_exhausted(), "trailing MD state section data");
    Ok(())
}
