use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;

pub const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBCOLCO\0";
pub const SAVE_STATE_FORMAT_VERSION: u32 = 1;
pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-coleco-determinism-v1";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-coleco-native-state-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateIdentity {
    pub bios_sha256: [u8; 32],
    pub cartridge_sha256: [u8; 32],
}

pub fn inspect_current_native_tas_state_identity(
    data: &[u8],
) -> anyhow::Result<CurrentNativeTasStateIdentity> {
    if data.len() < 76 || data[..8] != SAVE_STATE_MAGIC {
        bail!("TAS requires a native ColecoVision save-state");
    }
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("TAS requires ColecoVision save-state format {SAVE_STATE_FORMAT_VERSION}");
    }
    Ok(CurrentNativeTasStateIdentity {
        bios_sha256: data[12..44].try_into().expect("length checked"),
        cartridge_sha256: data[44..76].try_into().expect("length checked"),
    })
}

pub fn encode_state(emulator: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut writer = StateWriter::with_capacity(0x40_000);
    writer.write_bytes(&SAVE_STATE_MAGIC);
    writer.write_u32(SAVE_STATE_FORMAT_VERSION);
    writer.write_bytes(&emulator.bios_hash);
    writer.write_bytes(&emulator.cartridge_hash);
    writer.write_u64(emulator.effective_cycles);
    emulator.cpu.write_state(&mut writer);
    emulator.bus.write_state(&mut writer);
    Ok(writer.into_bytes())
}

pub fn decode_state(emulator: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if magic != SAVE_STATE_MAGIC {
        bail!("not a valid ColecoVision save-state");
    }
    let version = reader.read_u32()?;
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("unsupported ColecoVision save-state version {version}");
    }

    let mut bios_hash = [0; 32];
    reader.read_exact(&mut bios_hash)?;
    if bios_hash != emulator.bios_hash {
        bail!("ColecoVision save-state belongs to a different BIOS");
    }
    let mut cartridge_hash = [0; 32];
    reader.read_exact(&mut cartridge_hash)?;
    if cartridge_hash != emulator.cartridge_hash {
        bail!("ColecoVision save-state belongs to a different cartridge");
    }

    let effective_cycles = reader.read_u64()?;
    let mut cpu = emulator.cpu.clone();
    cpu.read_state(&mut reader)?;
    let mut bus = emulator.bus.clone();
    bus.read_state(&mut reader)?;
    if !reader.is_exhausted() {
        bail!("ColecoVision save-state has unexpected trailing data");
    }
    emulator.effective_cycles = effective_cycles;
    emulator.cpu = cpu;
    emulator.bus = bus;
    emulator.debug.clear_hits();
    emulator.opcode_log.clear();
    emulator.instruction_trace.clear();
    Ok(())
}

pub fn validate_and_load_current_native_tas_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeTasStateProjection> {
    inspect_current_native_tas_state_identity(data)?;
    decode_state(emulator, data)?;
    Ok(CurrentNativeTasStateProjection {
        replay_state_bytes: data.to_vec(),
        frame_count: emulator.frame_count(),
        framebuffer: emulator.framebuffer().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BIOS_SIZE;

    fn emulator(cartridge_fill: u8) -> Emulator {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..6].copy_from_slice(&[0x3E, 0x5A, 0x32, 0x00, 0x60, 0x76]);
        let mut cartridge = vec![cartridge_fill; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        Emulator::new(&cartridge, &bios, 48_000).unwrap()
    }

    #[test]
    fn roundtrips_cpu_ram_video_and_timing_state() {
        let mut saved = emulator(0x11);
        saved.step_instruction();
        saved.step_instruction();
        saved.bus_mut().io_write(0xA1, 0x00);
        saved.bus_mut().io_write(0xA1, 0x40);
        saved.bus_mut().io_write(0xA0, 0xA5);
        let state = encode_state(&saved).unwrap();

        let mut restored = emulator(0x11);
        decode_state(&mut restored, &state).unwrap();
        assert_eq!(restored.bus().work_ram()[0], 0x5A);
        assert_eq!(restored.bus().vdp().vram()[0], 0xA5);
        assert_eq!(restored.effective_cycles(), saved.effective_cycles());
        assert_eq!(restored.cpu().regs(), saved.cpu().regs());
    }

    #[test]
    fn rejects_cross_cartridge_and_trailing_data_without_mutation() {
        let mut source = emulator(0x11);
        source.step_instruction();
        let state = encode_state(&source).unwrap();

        let mut other = emulator(0x22);
        let before = encode_state(&other).unwrap();
        assert!(decode_state(&mut other, &state).is_err());
        assert_eq!(encode_state(&other).unwrap(), before);

        let mut trailing = encode_state(&other).unwrap();
        trailing.push(0);
        assert!(decode_state(&mut other, &trailing).is_err());
        assert_eq!(encode_state(&other).unwrap(), before);
    }

    #[test]
    fn truncated_state_is_transactional() {
        let mut emulator = emulator(0x11);
        emulator.step_instruction();
        let state = encode_state(&emulator).unwrap();
        let before = state.clone();

        assert!(decode_state(&mut emulator, &state[..state.len() - 7]).is_err());
        assert_eq!(encode_state(&emulator).unwrap(), before);
    }

    #[test]
    fn state_load_preserves_debug_configuration_and_clears_history() {
        let mut emulator = emulator(0x11);
        emulator.add_breakpoint(4);
        emulator.set_opcode_log_enabled(true);
        emulator.set_instruction_trace_enabled(true);
        emulator.step_instruction();
        let state = encode_state(&emulator).unwrap();

        decode_state(&mut emulator, &state).unwrap();

        assert_eq!(emulator.iter_breakpoints().collect::<Vec<_>>(), vec![4]);
        assert!(emulator.recent_opcodes(1).is_empty());
        assert!(emulator.instruction_trace().is_enabled());
        assert!(emulator.instruction_trace().is_empty());
    }

    #[test]
    fn current_native_tas_state_restores_exact_frame_output() {
        let mut source = emulator(0x11);
        source.step_frame();
        source.bus_mut().vdp_mut().write_register(7, 0x02);
        source.step_frame();
        let state = source.save_state().unwrap();

        let mut target = emulator(0x11);
        let projection = validate_and_load_current_native_tas_state(&mut target, &state).unwrap();

        assert_eq!(projection.replay_state_bytes, state);
        assert_eq!(projection.frame_count, source.frame_count());
        assert_eq!(projection.framebuffer.as_ref(), source.framebuffer());
        assert_eq!(target.save_state().unwrap(), state);
        assert_eq!(target.framebuffer(), source.framebuffer());
    }

    #[test]
    fn current_native_tas_state_rejects_wrong_schema_or_identity_transactionally() {
        let source = emulator(0x11);
        let current = source.save_state().unwrap();
        let mut target = emulator(0x11);
        target.step_instruction();
        let before = target.save_state().unwrap();

        let mut wrong_version = current.clone();
        wrong_version[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());
        let mut wrong_magic = current.clone();
        wrong_magic[0] ^= 1;
        let wrong_cartridge = emulator(0x22).save_state().unwrap();
        for invalid in [&wrong_version, &wrong_magic, &wrong_cartridge] {
            assert!(validate_and_load_current_native_tas_state(&mut target, invalid).is_err());
            assert_eq!(target.save_state().unwrap(), before);
        }
    }
}
