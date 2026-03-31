use anyhow::{Context, Result, anyhow, bail};

use super::{
    SAVE_STATE_DECODE_STACK_SIZE, SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, SAVE_STATE_VERSION,
    SaveState, StateReader, StateReaderGbExt,
};
use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::types::hardware_mode::HardwareModePreference;

pub fn decode_on_thread(bytes: Vec<u8>) -> Result<SaveState> {
    let decode_thread = std::thread::Builder::new()
        .name("save-state-decode".to_string())
        .stack_size(SAVE_STATE_DECODE_STACK_SIZE)
        .spawn(move || decode_state(&bytes))
        .context("failed to spawn save-state decode thread")?;

    decode_thread
        .join()
        .map_err(|_| anyhow!("save-state decode thread panicked"))?
}

pub(super) fn decode_state(bytes: &[u8]) -> Result<SaveState> {
    let mut reader = StateReader::new(bytes);

    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;
    if magic != SAVE_STATE_MAGIC {
        bail!("invalid save-state file header");
    }

    let format_version = reader.read_u32()?;
    if format_version != SAVE_STATE_FORMAT_VERSION {
        bail!(
            "unsupported save-state file format {} (expected {})",
            format_version,
            SAVE_STATE_FORMAT_VERSION
        );
    }

    let version = reader.read_u32()?;
    let mut rom_hash = [0u8; 32];
    reader.read_exact(&mut rom_hash)?;
    let cpu = Cpu::read_state(&mut reader)?;
    let hardware_mode_preference = decode_mode_preference(reader.read_u8()?)?;
    let hardware_mode = reader.read_hardware_mode()?;
    let cycle_count = reader.read_u64()?;
    let last_opcode = reader.read_u8()?;
    let last_opcode_pc = reader.read_u16()?;
    let bus = Bus::read_state(&mut reader)?;

    if !reader.is_exhausted() && !super::bess::has_bess_footer(bytes) {
        bail!("save-state file has unexpected trailing data");
    }

    Ok(SaveState {
        version,
        rom_hash,
        cpu,
        bus,
        hardware_mode_preference,
        hardware_mode,
        cycle_count,
        last_opcode,
        last_opcode_pc,
    })
}

fn decode_mode_preference(tag: u8) -> Result<HardwareModePreference> {
    match tag {
        0 => Ok(HardwareModePreference::Auto),
        1 => Ok(HardwareModePreference::ForceDmg),
        2 => Ok(HardwareModePreference::ForceCgb),
        _ => bail!("invalid hardware mode preference tag in save-state file: {tag}"),
    }
}

pub fn validate_compatibility(state: &SaveState, expected_rom_hash: [u8; 32]) -> Result<()> {
    if state.version != SAVE_STATE_VERSION {
        return Err(anyhow!(
            "unsupported save-state version {} (expected {})",
            state.version,
            SAVE_STATE_VERSION
        ));
    }

    if state.rom_hash != expected_rom_hash {
        return Err(anyhow!(
            "save state ROM hash does not match currently loaded ROM"
        ));
    }

    Ok(())
}
