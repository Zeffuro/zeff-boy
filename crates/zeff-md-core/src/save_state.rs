use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const STATE_MAGIC: &[u8; 8] = b"ZMDRAW01";
pub(crate) const STATE_VERSION: u32 = 1;

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
