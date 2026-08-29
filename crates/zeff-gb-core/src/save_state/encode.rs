use anyhow::Result;

use super::{
    SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, SaveStateRef, StateWriter, StateWriterGbExt,
};
use crate::hardware::types::hardware_mode::HardwareModePreference;

pub fn encode_state_bytes(state: &SaveStateRef<'_>) -> Result<Vec<u8>> {
    encode_state_bytes_with_version(state, SAVE_STATE_FORMAT_VERSION)
}

fn encode_state_bytes_with_version(
    state: &SaveStateRef<'_>,
    format_version: u32,
) -> Result<Vec<u8>> {
    let mut writer = StateWriter::new();
    writer.write_bytes(&SAVE_STATE_MAGIC);
    writer.write_u32(format_version);

    writer.write_u32(state.version);
    writer.write_bytes(&state.rom_hash);
    state.cpu.write_state(&mut writer);
    writer.write_u8(encode_mode_preference(state.hardware_mode_preference));
    writer.write_hardware_mode(state.hardware_mode);
    writer.write_u64(state.cycle_count);
    writer.write_u8(state.last_opcode);
    writer.write_u16(state.last_opcode_pc);
    state.bus.write_state(&mut writer);
    writer.write_bool(state.boot_rom_enabled);

    super::bess::append_bess_with_optional_zeff_extension(
        &mut writer,
        state.cpu,
        state.bus,
        state.hardware_mode,
        (format_version >= 13).then_some(state.frame_count),
    )?;

    Ok(writer.into_bytes())
}

#[cfg(test)]
pub(super) fn encode_legacy_v12_state_bytes(state: &SaveStateRef<'_>) -> Result<Vec<u8>> {
    encode_state_bytes_with_version(state, 12)
}

pub(super) fn encode_mode_preference(pref: HardwareModePreference) -> u8 {
    match pref {
        HardwareModePreference::Auto => 0,
        HardwareModePreference::ForceDmg => 1,
        HardwareModePreference::ForceCgb => 2,
        HardwareModePreference::ForceSgb => 3,
    }
}
