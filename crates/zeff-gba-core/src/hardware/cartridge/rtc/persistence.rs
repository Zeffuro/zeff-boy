use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::RtcGpio;

pub(in crate::hardware::cartridge) const MAGIC: [u8; 8] = *b"ZBGARTC1";
pub(in crate::hardware::cartridge) const EXTENSION_LEN: usize = 40;

pub(in crate::hardware::cartridge) fn encode_state(rtc: &RtcGpio) -> Vec<u8> {
    let mut writer = StateWriter::with_capacity(EXTENSION_LEN - MAGIC.len());
    rtc.write_state(&mut writer);
    let bytes = writer.into_bytes();
    debug_assert_eq!(bytes.len(), EXTENSION_LEN - MAGIC.len());
    bytes
}

pub(in crate::hardware::cartridge) fn encode_extension(rtc: &RtcGpio) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(EXTENSION_LEN);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&encode_state(rtc));
    bytes
}

pub(in crate::hardware::cartridge) fn decode_extension(bytes: &[u8]) -> Result<RtcGpio> {
    ensure!(
        bytes.len() == EXTENSION_LEN && bytes[..MAGIC.len()] == MAGIC,
        "invalid GBA RTC persistence extension"
    );
    let mut reader = StateReader::new(&bytes[MAGIC.len()..]);
    let rtc = RtcGpio::read_state(&mut reader)?;
    ensure!(reader.is_exhausted(), "trailing GBA RTC persistence bytes");
    Ok(rtc)
}
