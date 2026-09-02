use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{Rtc, RtcSaveState};

pub(super) const MAGIC: [u8; 8] = *b"ZBWSRTC1";
pub(super) const STATE_LEN: usize = 16;
pub(in crate::hardware::bus) const EXTENSION_LEN: usize = MAGIC.len() + STATE_LEN;

pub(in crate::hardware::bus) fn encode_state(rtc: &Rtc) -> Vec<u8> {
    let state = rtc.save_state();
    let mut writer = StateWriter::with_capacity(STATE_LEN);
    writer.write_u8(state.command);
    writer.write_bytes(&state.payload);
    writer.write_u8(state.payload_index);
    writer.write_u8(state.payload_len);
    writer.write_u8(state.ready_delay_reads);
    writer.write_bool(state.invalid_command);
    writer.write_u32(state.subsecond_cycles);
    let bytes = writer.into_bytes();
    debug_assert_eq!(bytes.len(), STATE_LEN);
    bytes
}

pub(in crate::hardware::bus) fn encode_extension(rtc: &Rtc) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(EXTENSION_LEN);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&encode_state(rtc));
    bytes
}

pub(in crate::hardware::bus) fn decode_extension(bytes: &[u8]) -> Result<RtcSaveState> {
    ensure!(
        bytes.len() == EXTENSION_LEN && bytes[..MAGIC.len()] == MAGIC,
        "invalid WonderSwan RTC persistence extension"
    );
    let mut reader = StateReader::new(&bytes[MAGIC.len()..]);
    let command = reader.read_u8()?;
    let mut payload = [0; 7];
    reader.read_exact(&mut payload)?;
    let state = RtcSaveState {
        command,
        payload,
        payload_index: reader.read_u8()?,
        payload_len: reader.read_u8()?,
        ready_delay_reads: reader.read_u8()?,
        invalid_command: reader.read_bool()?,
        subsecond_cycles: reader.read_u32()?,
    };
    ensure!(
        reader.is_exhausted(),
        "trailing WonderSwan RTC persistence bytes"
    );
    ensure!(
        super::valid_datetime(state.payload),
        "invalid RTC date/time"
    );
    let mut candidate = Rtc::new();
    candidate.load_state(state)?;
    Ok(state)
}
